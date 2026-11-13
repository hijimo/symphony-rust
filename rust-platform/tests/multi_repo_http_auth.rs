//! Regression: with partial clones (`--filter=blob:none`) the *checkout* that
//! follows the clone lazily fetches blobs from the promisor remote. Every git
//! invocation must therefore authenticate via GIT_ASKPASS — not only
//! clone/fetch. The file:// fixtures in `multi_repo_workspace.rs` cannot catch
//! this (no auth, and file:// serves everything), so this test runs a real
//! authenticated smart-HTTP server (git-http-backend behind Basic auth).

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use symphony_platform::config::repos::{CheckoutMode, RepoMode, RepoRole, RepoSpec};
use symphony_platform::config::service_config::HooksConfig;
use symphony_platform::workspace::repos::{RepoCheckoutContext, RepoStatus};
use symphony_platform::workspace::{MultiRepoPrepare, WorkspaceManager};
use tempfile::TempDir;

const TOKEN: &str = "glpat-http-auth-test";

fn git(cwd: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@e")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@e")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn http_backend() -> Option<PathBuf> {
    let exec = Command::new("git").arg("--exec-path").output().ok()?;
    let p = PathBuf::from(String::from_utf8_lossy(&exec.stdout).trim()).join("git-http-backend");
    p.exists().then_some(p)
}

/// Minimal CGI bridge: Basic auth `oauth2:TOKEN` required on every request.
fn serve(listener: TcpListener, root: PathBuf, backend: PathBuf) {
    for stream in listener.incoming() {
        let Ok(mut stream) = stream else { continue };
        let root = root.clone();
        let backend = backend.clone();
        std::thread::spawn(move || {
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            if reader.read_line(&mut line).is_err() || line.is_empty() {
                return;
            }
            let mut parts = line.split_whitespace();
            let method = parts.next().unwrap_or("GET").to_string();
            let target = parts.next().unwrap_or("/").to_string();
            let (path, query) = target.split_once('?').unwrap_or((&target, ""));
            let (mut len, mut ctype, mut auth, mut encoding, mut proto) = (
                0usize,
                String::new(),
                String::new(),
                String::new(),
                String::new(),
            );
            loop {
                let mut h = String::new();
                reader.read_line(&mut h).unwrap();
                let h = h.trim_end();
                if h.is_empty() {
                    break;
                }
                let (k, v) = h.split_once(':').unwrap_or((h, ""));
                let v = v.trim().to_string();
                match k.to_ascii_lowercase().as_str() {
                    "content-length" => len = v.parse().unwrap_or(0),
                    "content-type" => ctype = v,
                    "authorization" => auth = v,
                    "content-encoding" => encoding = v,
                    "git-protocol" => proto = v,
                    _ => {}
                }
            }
            let mut body = vec![0u8; len];
            reader.read_exact(&mut body).unwrap();
            let expected = format!("Basic {}", base64_encode(&format!("oauth2:{TOKEN}")));
            if auth != expected {
                let _ = stream.write_all(b"HTTP/1.1 401 Unauthorized\r\nWWW-Authenticate: Basic realm=\"git\"\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                return;
            }
            let mut child = Command::new(&backend)
                .env_clear()
                .env("GIT_PROJECT_ROOT", &root)
                .env("GIT_HTTP_EXPORT_ALL", "1")
                .env("REQUEST_METHOD", &method)
                .env("PATH_INFO", path)
                .env("QUERY_STRING", query)
                .env("CONTENT_TYPE", &ctype)
                .env("CONTENT_LENGTH", len.to_string())
                .env("HTTP_CONTENT_ENCODING", &encoding)
                .env("GIT_PROTOCOL", &proto)
                .env("REMOTE_USER", "oauth2")
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .spawn()
                .unwrap();
            child.stdin.take().unwrap().write_all(&body).unwrap();
            let out = child.wait_with_output().unwrap();
            let raw = out.stdout;
            let split = raw
                .windows(4)
                .position(|w| w == b"\r\n\r\n")
                .map(|p| (p, 4))
                .or_else(|| raw.windows(2).position(|w| w == b"\n\n").map(|p| (p, 2)))
                .unwrap_or((0, 0));
            let headers = String::from_utf8_lossy(&raw[..split.0]).to_string();
            let content = &raw[split.0 + split.1..];
            let status = headers
                .lines()
                .find_map(|l| l.strip_prefix("Status: "))
                .unwrap_or("200 OK")
                .to_string();
            let mut resp = format!("HTTP/1.1 {status}\r\n");
            for l in headers.lines().filter(|l| !l.starts_with("Status:")) {
                resp.push_str(l.trim_end());
                resp.push_str("\r\n");
            }
            resp.push_str(&format!(
                "Content-Length: {}\r\nConnection: close\r\n\r\n",
                content.len()
            ));
            let _ = stream.write_all(resp.as_bytes());
            let _ = stream.write_all(content);
        });
    }
}

fn base64_encode(s: &str) -> String {
    const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let b = s.as_bytes();
    let mut out = String::new();
    for chunk in b.chunks(3) {
        let n = (chunk[0] as u32) << 16
            | (*chunk.get(1).unwrap_or(&0) as u32) << 8
            | *chunk.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(T[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[tokio::test]
async fn partial_clone_checkout_authenticates_lazy_blob_fetch() {
    let Some(backend) = http_backend() else {
        eprintln!("git-http-backend not available; skipping");
        return;
    };
    let tmp = TempDir::new().unwrap();
    let host = tmp.path().join("host");
    let bare = host.join("g").join("app.git");
    std::fs::create_dir_all(&bare).unwrap();
    git(&bare, &["init", "--bare", "-q", "--initial-branch=dev"]);
    git(&bare, &["config", "uploadpack.allowFilter", "true"]);
    git(&bare, &["config", "uploadpack.allowAnySHA1InWant", "true"]);
    let seed = tmp.path().join("seed");
    std::fs::create_dir_all(&seed).unwrap();
    git(&seed, &["init", "-q", "--initial-branch=dev"]);
    std::fs::write(seed.join("app.ts"), "hello\n").unwrap();
    git(&seed, &["add", "."]);
    git(&seed, &["commit", "-q", "-m", "init"]);
    git(
        &seed,
        &["push", "-q", bare.to_str().unwrap(), "HEAD:refs/heads/dev"],
    );

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let root = host.clone();
    std::thread::spawn(move || serve(listener, root, backend));

    let ws_root = tmp.path().join("ws");
    std::fs::create_dir_all(&ws_root).unwrap();
    let ctx = RepoCheckoutContext {
        platform_host: format!("http://{addr}"),
        token: Some(TOKEN.into()),
        askpass_dir: ws_root.join(".symphony").join("askpass"),
        issue_number: "36".into(),
        clone_timeout: Duration::from_secs(60),
        fetch_timeout: Duration::from_secs(30),
    };
    let specs = vec![RepoSpec {
        alias: "primary".into(),
        slug: "g/app".into(),
        role: RepoRole::Primary,
        default_branch: "dev".into(),
        mode: RepoMode::Write,
        checkout: CheckoutMode::Partial,
        merge_after: vec![],
    }];
    let mgr = WorkspaceManager::new(ws_root.clone(), HooksConfig::default());
    let (lease, state) = mgr
        .prepare_multi_repo_workspace(MultiRepoPrepare {
            issue_id: "36",
            issue_identifier: "#36",
            run_id: "run-1",
            service_instance_id: "svc",
            checkout: &ctx,
            specs: &specs,
            on_progress: &|| {},
        })
        .await
        .expect("partial clone + checkout over authenticated HTTP must succeed");
    assert_eq!(state.repos["primary"].status, RepoStatus::Ready);
    assert_eq!(
        std::fs::read_to_string(lease.workspace.path.join("primary/app.ts")).unwrap(),
        "hello\n"
    );
}
