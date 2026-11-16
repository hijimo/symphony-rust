//! Organization rules repository sync (P3 design §4 O1): settings, export of
//! a commit without running anything from it, safety checks, keeping the
//! last good version, project pins.

mod sdlc_support;

use std::path::{Path, PathBuf};

use reqwest::StatusCode;
use sdlc_support::*;
use serde_json::{json, Value};

const URL: &str = "/api/admin/org-config";

fn org_files() -> Vec<(&'static str, &'static str)> {
    vec![
        ("AGENTS.md", "# Org rules\nUse structured logging.\n"),
        ("config.toml", "model = \"gpt-5.5\"\n"),
        (
            "skills/security-policy/SKILL.md",
            "---\nname: security-policy\n---\nMask PII.\n",
        ),
        (
            "templates/spec.md",
            "# Spec: <title>\n\n## Requirements\n\n## Design\n\n## Concerns\n",
        ),
    ]
}

/// Commit `files` to `main` of the org repository and return the new sha.
fn commit(bare: &Path, root: &Path, files: &[(&str, &str)], tag: Option<&str>) -> String {
    let seed = root.join("org-push");
    let _ = std::fs::remove_dir_all(&seed);
    git(
        root,
        &[
            "clone",
            "-q",
            bare.to_str().unwrap(),
            seed.to_str().unwrap(),
        ],
    )
    .unwrap();
    for (p, c) in files {
        let f = seed.join(p);
        std::fs::create_dir_all(f.parent().unwrap()).unwrap();
        std::fs::write(f, c).unwrap();
    }
    git(&seed, &["add", "-A"]).unwrap();
    git(
        &seed,
        &[
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "commit",
            "-q",
            "-m",
            "c",
        ],
    )
    .unwrap();
    if let Some(t) = tag {
        git(&seed, &["tag", t]).unwrap();
        git(&seed, &["push", "-q", "origin", t]).unwrap();
    }
    git(&seed, &["push", "-q", "origin", "main"]).unwrap();
    git(&seed, &["rev-parse", "HEAD"])
        .unwrap()
        .trim()
        .to_string()
}

async fn configure(app: &SdlcApp, url: &str) {
    let (st, v) = app
        .put(
            "/api/admin/config",
            json!({"configs": [{"key": "org_config.url", "value": url}]}),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "{v}");
}

async fn sync(app: &SdlcApp) -> Value {
    let (st, v) = app.post(&format!("{URL}/sync"), json!({})).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    v["data"].clone()
}

fn export_dir(app: &SdlcApp, sha: &str) -> PathBuf {
    app.workspace_root.join("org-config").join(sha)
}

#[tokio::test]
async fn disabled_until_a_repository_is_configured() {
    let app = SdlcApp::new().await;
    let (st, v) = app.get(URL).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(v["data"]["enabled"], false, "{v}");
    assert_eq!(v["data"]["ref"], "main");
    assert_eq!(v["data"]["intervalSecs"], 300);
    let s = sync(&app).await;
    assert_eq!(s["enabled"], false);
    assert!(s["currentSha"].is_null());

    configure(&app, "--upload-pack=touch /tmp/x").await;
    let s = sync(&app).await;
    assert!(s["lastError"].as_str().unwrap().contains("仓库地址"), "{s}");
}

#[tokio::test]
async fn sync_exports_the_commit_and_keeps_the_last_good_version() {
    let app = SdlcApp::new().await;
    let bare = make_bare(app.dir.path(), "org", &org_files());
    configure(&app, bare.to_str().unwrap()).await;
    let s = sync(&app).await;
    let sha = s["currentSha"].as_str().unwrap().to_string();
    assert!(s["lastError"].is_null(), "{s}");
    assert!(s["lastSyncAt"].is_string());
    assert_eq!(s["versions"], json!([sha]));
    let dir = export_dir(&app, &sha);
    assert_eq!(
        std::fs::read_to_string(dir.join("AGENTS.md")).unwrap(),
        "# Org rules\nUse structured logging.\n"
    );
    assert!(dir.join("skills/security-policy/SKILL.md").is_file());
    assert!(dir.join("templates/spec.md").is_file());
    assert!(!dir.join(".git").exists(), "exported, not checked out");

    // Each unsafe commit fails the sync and leaves the good version current.
    let cases: Vec<(Vec<(&str, String)>, &str)> = vec![
        (vec![("auth.json", "{}".to_string())], "auth.json"),
        (vec![("skills/x/.env", "A=1".to_string())], ".env"),
        (vec![("certs/key.pem", "x".to_string())], "key.pem"),
        (
            vec![(
                "AGENTS.md",
                "token glpat-abcdefghijklmnopqrst\n".to_string(),
            )],
            "凭据",
        ),
        (vec![("big.bin", "x".repeat(1024 * 1024 + 1))], "1 MB"),
    ];
    for (files, needle) in cases {
        let refs: Vec<(&str, &str)> = files.iter().map(|(p, c)| (*p, c.as_str())).collect();
        commit(&bare, app.dir.path(), &refs, None);
        let s = sync(&app).await;
        let err = s["lastError"].as_str().unwrap_or_default().to_string();
        assert!(err.contains(needle), "{needle}: {s}");
        assert_eq!(s["currentSha"], sha, "{needle}: previous version stays");
        // Undo for the next case.
        let seed = app.dir.path().join("org-push");
        for (p, _) in &refs {
            let _ = std::fs::remove_file(seed.join(p));
        }
        git(&seed, &["add", "-A"]).unwrap();
        git(
            &seed,
            &[
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "commit",
                "-q",
                "-m",
                "undo",
            ],
        )
        .unwrap();
        git(&seed, &["push", "-q", "origin", "main"]).unwrap();
    }

    // A symlink is rejected as well.
    let seed = app.dir.path().join("org-push");
    std::os::unix::fs::symlink("/etc/passwd", seed.join("link")).unwrap();
    git(&seed, &["add", "-A"]).unwrap();
    git(
        &seed,
        &[
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "commit",
            "-q",
            "-m",
            "link",
        ],
    )
    .unwrap();
    git(&seed, &["push", "-q", "origin", "main"]).unwrap();
    let s = sync(&app).await;
    assert!(s["lastError"].as_str().unwrap().contains("符号链接"), "{s}");
    assert_eq!(s["currentSha"], sha);

    // A good commit becomes current; the previous export is kept.
    std::fs::remove_file(seed.join("link")).unwrap();
    git(&seed, &["add", "-A"]).unwrap();
    git(
        &seed,
        &[
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "commit",
            "-q",
            "-m",
            "unlink",
        ],
    )
    .unwrap();
    git(&seed, &["push", "-q", "origin", "main"]).unwrap();
    let next = commit(
        &bare,
        app.dir.path(),
        &[("AGENTS.md", "# Org rules v2\n")],
        None,
    );
    let s = sync(&app).await;
    assert_eq!(s["currentSha"], next, "{s}");
    assert!(s["lastError"].is_null());
    assert!(export_dir(&app, &sha).is_dir() && export_dir(&app, &next).is_dir());
    assert_eq!(
        std::fs::read_to_string(export_dir(&app, &next).join("AGENTS.md")).unwrap(),
        "# Org rules v2\n"
    );
}

#[tokio::test]
async fn pinned_projects_get_their_version_exported() {
    let app = SdlcApp::new().await;
    let bare = make_bare(app.dir.path(), "org", &org_files());
    let v1 = commit(&bare, app.dir.path(), &[("AGENTS.md", "v1\n")], Some("v1"));
    commit(&bare, app.dir.path(), &[("AGENTS.md", "v2\n")], None);
    configure(&app, bare.to_str().unwrap()).await;
    rusqlite::Connection::open(app.dir.path().join("test.db"))
        .unwrap()
        .execute(
            "UPDATE projects SET org_config_pin = 'v1' WHERE id = ?1",
            [app.project_id],
        )
        .unwrap();
    let s = sync(&app).await;
    assert_ne!(s["currentSha"], v1);
    assert_eq!(s["pins"][0]["projectId"], app.project_id, "{s}");
    assert_eq!(s["pins"][0]["pin"], "v1");
    assert_eq!(s["pins"][0]["sha"], v1);
    assert_eq!(
        std::fs::read_to_string(export_dir(&app, &v1).join("AGENTS.md")).unwrap(),
        "v1\n"
    );

    rusqlite::Connection::open(app.dir.path().join("test.db"))
        .unwrap()
        .execute(
            "UPDATE projects SET org_config_pin = 'no-such-tag' WHERE id = ?1",
            [app.project_id],
        )
        .unwrap();
    let s = sync(&app).await;
    assert!(s["pins"][0]["sha"].is_null());
    assert!(
        s["pins"][0]["error"]
            .as_str()
            .unwrap()
            .contains("no-such-tag"),
        "{s}"
    );
}

#[tokio::test]
async fn only_admins_manage_the_org_repository() {
    let app = SdlcApp::new().await;
    let hash = web_platform::auth::password::hash_password("pw123456").unwrap();
    use web_platform::repository::UserRepository;
    app.repo
        .create_user("bob", &hash, None, "user")
        .await
        .unwrap();
    let login: Value = app
        .client
        .post(app.url("/api/auth/login"))
        .json(&json!({"username": "bob", "password": "pw123456"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let token = login["data"]["token"].as_str().unwrap();
    for (m, p) in [
        (reqwest::Method::GET, URL.to_string()),
        (reqwest::Method::POST, format!("{URL}/sync")),
    ] {
        let r = app
            .client
            .request(m, app.url(&p))
            .bearer_auth(token)
            .json(&json!({}))
            .send()
            .await
            .unwrap();
        assert_eq!(r.status().as_u16(), 403);
    }
}
