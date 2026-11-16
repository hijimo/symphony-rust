//! Organization rules repository sync (P3 design §4 O1): settings, export of
//! a commit without running anything from it, safety checks, keeping the
//! last good version, project pins.

mod sdlc_support;

use std::path::{Path, PathBuf};

use reqwest::StatusCode;
use sdlc_support::*;
use serde_json::{json, Value};

const URL: &str = "/api/admin/org-config";
const PLAN_TPL: &str = "---\nsymphony_template:\n  prompt: |\n    Write a short plan.\n---\n# Plan: <title>\n\n## Files\n\n## Steps\n\n## Tests\n\n## Risks\n\n## Alternatives considered\n";

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

fn codex_homes(app: &SdlcApp) -> Vec<String> {
    std::fs::read_to_string(app.codex_log())
        .unwrap_or_default()
        .lines()
        .filter_map(|l| l.strip_prefix("codex_home ").map(str::to_string))
        .collect()
}

fn home_dir(app: &SdlcApp, sha: &str) -> PathBuf {
    app.workspace_root.join("org-config/homes").join(sha)
}

async fn trial(app: &SdlcApp) {
    let (st, v) = app
        .post(
            &format!("/api/projects/{}/doc-templates/plan/trial", app.project_id),
            json!({"content": PLAN_TPL, "sample": "导出报表"}),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "{v}");
}

#[tokio::test]
async fn drafting_sessions_run_with_the_org_codex_home() {
    let app = SdlcApp::new().await;
    // Off: the inherited CODEX_HOME is left alone.
    trial(&app).await;
    assert_eq!(codex_homes(&app), ["-"]);

    let base = app.dir.path().join("codex-home");
    std::fs::create_dir_all(base.join("skills/symphony-push")).unwrap();
    std::fs::write(base.join("skills/symphony-push/SKILL.md"), "push\n").unwrap();
    std::fs::write(base.join("auth.json"), "{}").unwrap();
    let mut files = org_files();
    files.push((
        "config.toml",
        "model = \"gpt-5.5\"\n[mcp_servers.x]\ncommand = \"sh\"\n",
    ));
    let bare = make_bare(app.dir.path(), "org", &files);
    configure(&app, bare.to_str().unwrap()).await;
    let s = sync(&app).await;
    let sha = s["currentSha"].as_str().unwrap().to_string();
    assert_eq!(s["droppedConfig"], json!(["mcp_servers"]), "{s}");

    // Template trial (a new session per run).
    trial(&app).await;
    let home = home_dir(&app, &sha);
    assert_eq!(codex_homes(&app)[1], home.to_string_lossy());
    assert_eq!(
        std::fs::read_to_string(home.join("AGENTS.md")).unwrap(),
        "# Org rules\nUse structured logging.\n"
    );
    assert!(home.join("skills/security-policy/SKILL.md").is_file());
    assert!(home.join("skills/symphony-push/SKILL.md").is_file());
    assert_eq!(
        std::fs::read_to_string(home.join("config.toml")).unwrap(),
        "model = \"gpt-5.5\"\n"
    );
    assert!(std::fs::symlink_metadata(home.join("auth.json"))
        .unwrap()
        .file_type()
        .is_symlink());

    // Flow stage session: G1 approval starts the spec draft.
    let (st, v) = app
        .post(
            &app.base(),
            json!({"track": "full", "title": "Org", "initialContent": INTENT_OK}),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let iid = v["data"]["flow"]["issueIid"].as_i64().unwrap();
    let base_url = format!("{}/{iid}", app.base());
    let (st, v) = app
        .post(&format!("{base_url}/docs/intent/submit"), json!({}))
        .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let (st, v) = app
        .post(
            &format!("{base_url}/docs/intent/decision"),
            json!({"action": "approve"}),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    app.wait_flow(iid, |f| SdlcApp::doc_status(f, "spec") == "in_review")
        .await;
    assert_eq!(codex_homes(&app)[2], home.to_string_lossy());

    // A pinned project uses its pinned version.
    let v1 = commit(&bare, app.dir.path(), &[("AGENTS.md", "v1\n")], Some("v1"));
    commit(&bare, app.dir.path(), &[("AGENTS.md", "v2\n")], None);
    rusqlite::Connection::open(app.dir.path().join("test.db"))
        .unwrap()
        .execute(
            "UPDATE projects SET org_config_pin = 'v1' WHERE id = ?1",
            [app.project_id],
        )
        .unwrap();
    sync(&app).await;
    trial(&app).await;
    let pinned = home_dir(&app, &v1);
    assert_eq!(codex_homes(&app)[3], pinned.to_string_lossy());
    assert_eq!(
        std::fs::read_to_string(pinned.join("AGENTS.md")).unwrap(),
        "v1\n"
    );

    // An unresolvable pin falls back to the inherited CODEX_HOME.
    rusqlite::Connection::open(app.dir.path().join("test.db"))
        .unwrap()
        .execute(
            "UPDATE projects SET org_config_pin = 'gone' WHERE id = ?1",
            [app.project_id],
        )
        .unwrap();
    trial(&app).await;
    assert_eq!(codex_homes(&app)[4], "-");
}

/// P3 O3: project template → organization `templates/<doc>.md` → built-in.
#[tokio::test]
async fn organization_templates_sit_between_project_and_builtin() {
    let app = SdlcApp::new().await;
    let bare = make_bare(app.dir.path(), "org", &org_files());
    configure(&app, bare.to_str().unwrap()).await;
    let sha = sync(&app).await["currentSha"].as_str().unwrap().to_string();
    let org_ref = format!("spec@org:{}", &sha[..8]);

    let (st, v) = app
        .get(&format!("/api/projects/{}/doc-templates", app.project_id))
        .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let by_key = |v: &Value, k: &str| {
        v["data"]
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["key"] == k)
            .cloned()
            .unwrap()
    };
    let spec = by_key(&v, "spec");
    assert_eq!(spec["source"], "org", "{spec}");
    assert_eq!(spec["templateRef"], org_ref);
    assert!(spec["content"]
        .as_str()
        .unwrap()
        .contains("# Spec: <title>"));
    assert_eq!(by_key(&v, "plan")["source"], "builtin");

    // A new spec document is created from the organization template.
    let (_, v) = app
        .post(
            &app.base(),
            json!({"track": "full", "title": "Org tpl", "initialContent": INTENT_OK}),
        )
        .await;
    let iid = v["data"]["flow"]["issueIid"].as_i64().unwrap();
    let base = format!("{}/{iid}", app.base());
    app.post(&format!("{base}/docs/intent/submit"), json!({}))
        .await;
    let (st, v) = app
        .post(
            &format!("{base}/docs/intent/decision"),
            json!({"action": "approve"}),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    app.wait_flow(iid, |f| SdlcApp::doc_status(f, "spec") == "in_review")
        .await;
    let spec = std::fs::read_to_string(
        app.primary_ws(iid)
            .join(format!("docs/changes/{iid}/spec.md")),
    )
    .unwrap();
    assert!(spec.contains(&format!("template: {org_ref}")), "{spec}");
    let (_, v) = app.get(&format!("{base}/templates")).await;
    let list = v["data"].as_array().unwrap();
    assert_eq!(list[1]["source"], "org", "{v}");
    assert_eq!(list[0]["source"], "builtin");
}

fn db(app: &SdlcApp) -> rusqlite::Connection {
    rusqlite::Connection::open(app.dir.path().join("test.db")).unwrap()
}

/// P3 O4: documents and token usage record the organization version.
#[tokio::test]
async fn documents_and_token_usage_record_the_org_version() {
    let app = SdlcApp::new().await;
    // Off: nothing is recorded.
    let (_, v) = app
        .post(
            &app.base(),
            json!({"track": "full", "title": "Off", "initialContent": INTENT_OK}),
        )
        .await;
    let off = v["data"]["flow"]["issueIid"].as_i64().unwrap();
    let intent = std::fs::read_to_string(
        app.primary_ws(off)
            .join(format!("docs/changes/{off}/intent.md")),
    )
    .unwrap();
    assert!(intent.contains("org_config_sha: \"\""), "{intent}");

    let bare = make_bare(app.dir.path(), "org", &org_files());
    configure(&app, bare.to_str().unwrap()).await;
    let sha = sync(&app).await["currentSha"].as_str().unwrap().to_string();

    let (_, v) = app
        .post(
            &app.base(),
            json!({"track": "full", "title": "On", "initialContent": INTENT_OK}),
        )
        .await;
    let iid = v["data"]["flow"]["issueIid"].as_i64().unwrap();
    let base = format!("{}/{iid}", app.base());
    let doc = |name: &str| {
        std::fs::read_to_string(
            app.primary_ws(iid)
                .join(format!("docs/changes/{iid}/{name}.md")),
        )
        .unwrap()
    };
    assert!(
        doc("intent").contains(&format!("org_config_sha: \"{sha}\"")),
        "{}",
        doc("intent")
    );
    app.post(&format!("{base}/docs/intent/submit"), json!({}))
        .await;
    let (st, v) = app
        .post(
            &format!("{base}/docs/intent/decision"),
            json!({"action": "approve"}),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let flow = app
        .wait_flow(iid, |f| SdlcApp::doc_status(f, "spec") == "in_review")
        .await;
    assert!(doc("spec").contains(&format!("org_config_sha: \"{sha}\"")));
    let docs = flow["documents"].as_array().unwrap();
    for d in docs {
        assert_eq!(d["orgConfigSha"], sha, "{d}");
    }
    let rows: Vec<Option<String>> = {
        let c = db(&app);
        let mut stmt = c
            .prepare(
                "SELECT u.org_config_sha FROM agent_token_usage u JOIN change_flows f ON f.id = u.flow_id WHERE f.issue_iid = ?1",
            )
            .unwrap();
        let rows = stmt
            .query_map([iid], |r| r.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        rows
    };
    assert!(!rows.is_empty());
    assert!(
        rows.iter().all(|r| r.as_deref() == Some(sha.as_str())),
        "{rows:?}"
    );
}

/// P3 O4: project owners follow the latest version or lock one.
#[tokio::test]
async fn project_owners_lock_or_follow_the_org_version() {
    let app = SdlcApp::new().await;
    let url = format!("/api/projects/{}/org-config", app.project_id);
    let (st, v) = app.get(&url).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(v["data"]["enabled"], false);
    assert_eq!(v["data"]["canEdit"], true);
    let (st, _) = app.put(&url, json!({"pin": "v1"})).await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "not enabled");

    let bare = make_bare(app.dir.path(), "org", &org_files());
    let v1 = commit(&bare, app.dir.path(), &[("AGENTS.md", "v1\n")], Some("v1"));
    let head = commit(&bare, app.dir.path(), &[("AGENTS.md", "v2\n")], None);
    configure(&app, bare.to_str().unwrap()).await;
    sync(&app).await;

    let (_, v) = app.get(&url).await;
    let d = &v["data"];
    assert_eq!(d["enabled"], true, "{v}");
    assert_eq!(d["url"], bare.to_str().unwrap());
    assert_eq!(d["ref"], "main");
    assert_eq!(d["followedSha"], head);
    assert_eq!(d["effectiveSha"], head);
    assert!(d["pin"].is_null() && d["pinError"].is_null());

    let (st, v) = app.put(&url, json!({"pin": "v1"})).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(v["data"]["pin"], "v1");
    assert_eq!(v["data"]["effectiveSha"], v1, "pinned version is exported");
    assert!(export_dir(&app, &v1).is_dir());

    let (st, _) = app.put(&url, json!({"pin": "-x"})).await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
    let (st, v) = app.put(&url, json!({"pin": "no-such-tag"})).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert!(v["data"]["effectiveSha"].is_null());
    assert!(
        v["data"]["pinError"]
            .as_str()
            .unwrap()
            .contains("no-such-tag"),
        "{v}"
    );

    let (st, v) = app.put(&url, json!({"pin": null})).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert!(v["data"]["pin"].is_null());
    assert_eq!(v["data"]["effectiveSha"], head);

    // Members read but cannot change it; others cannot read.
    let hash = web_platform::auth::password::hash_password("pw123456").unwrap();
    use web_platform::repository::{ProjectMemberRepository, UserRepository};
    let bob = app
        .repo
        .create_user("bob", &hash, None, "user")
        .await
        .unwrap();
    let carol = app
        .repo
        .create_user("carol", &hash, None, "user")
        .await
        .unwrap();
    app.repo
        .add_member(app.project_id, bob.id, "member", None)
        .await
        .unwrap();
    let login = |name: &'static str| {
        let client = app.client.clone();
        let url = app.url("/api/auth/login");
        async move {
            let v: Value = client
                .post(url)
                .json(&json!({"username": name, "password": "pw123456"}))
                .send()
                .await
                .unwrap()
                .json()
                .await
                .unwrap();
            v["data"]["token"].as_str().unwrap().to_string()
        }
    };
    let bob_token = login("bob").await;
    let r = app
        .client
        .get(app.url(&url))
        .bearer_auth(&bob_token)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    let v: Value = r.json().await.unwrap();
    assert_eq!(v["data"]["canEdit"], false);
    let r = app
        .client
        .put(app.url(&url))
        .bearer_auth(&bob_token)
        .json(&json!({"pin": "v1"}))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 403);
    let _ = carol;
    let carol_token = login("carol").await;
    let r = app
        .client
        .get(app.url(&url))
        .bearer_auth(&carol_token)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 403);
}
