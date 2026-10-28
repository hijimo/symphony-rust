//! API tests of the AI-native SDLC document flow (HTTP level, fake GitLab,
//! real git, fake codex app-server).

mod sdlc_support;

use reqwest::StatusCode;
use sdlc_support::*;
use serde_json::{json, Value};

async fn start_full(app: &SdlcApp, title: &str) -> i64 {
    let (st, v) = app
        .post(
            &app.base(),
            json!({"track": "full", "title": title, "initialContent": INTENT_OK}),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    v["data"]["flow"]["issueIid"].as_i64().unwrap()
}

fn file_path(iid: i64, doc: &str) -> String {
    format!("docs/changes/{iid}/{doc}.md")
}

async fn read(app: &SdlcApp, iid: i64, path: &str) -> Value {
    let (st, v) = app
        .get(&format!(
            "{}/{iid}/file?path={}",
            app.base(),
            urlencoding::encode(path)
        ))
        .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    v["data"].clone()
}

#[tokio::test]
async fn auth_is_required() {
    let app = SdlcApp::new().await;
    let resp = app.client.get(app.url(&app.base())).send().await.unwrap();
    assert_eq!(resp.status().as_u16(), 401);
}

#[tokio::test]
async fn start_validation_errors() {
    let app = SdlcApp::new().await;
    let (st, v) = app
        .post(&app.base(), json!({"track": "fast", "title": "x"}))
        .await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "{v}");
    let (st, _) = app.post(&app.base(), json!({"track": "full"})).await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
    let (st, _) = app.get(&format!("{}/999", app.base())).await;
    assert_eq!(st, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn start_full_creates_issue_branch_mr_and_labels() {
    let app = SdlcApp::new().await;
    let iid = start_full(&app, "Claim status").await;
    let issue = app.gitlab.issue(iid as u64);
    assert!(
        issue.labels.contains(&"track:full".to_string()),
        "{:?}",
        issue.labels
    );
    assert!(issue.labels.contains(&"doc:intent".to_string()));
    assert!(
        !issue.labels.contains(&"Todo".to_string()),
        "orchestrator must not pick it up"
    );

    let mr = app.gitlab.mr(1);
    assert!(mr.title.starts_with("Draft:"), "{}", mr.title);
    assert_eq!(mr.source, format!("symphony/docs-{iid}"));

    // Branch pushed with intent.md (frontmatter + initial body) and AI/human trailer.
    let text = read_branch_file(
        &app.bare,
        &format!("symphony/docs-{iid}"),
        &file_path(iid, "intent"),
    )
    .unwrap();
    assert!(text.starts_with("---\nsymphony:\n  doc: intent"), "{text}");
    assert!(text.contains("users cannot see claim status"));
    let log = git(
        &app.bare,
        &["log", "-1", "--format=%B", &format!("symphony/docs-{iid}")],
    )
    .unwrap();
    assert!(log.contains("Symphony-Actor: human"), "{log}");

    let (_, v) = app.get(&format!("{}/{iid}", app.base())).await;
    assert_eq!(v["data"]["state"], "intent");
    assert_eq!(v["data"]["label"], "doc:intent");
    assert_eq!(v["data"]["mergePrecheck"]["canMerge"], true);

    // Second start on the same issue → conflict.
    let (st, _) = app
        .post(&app.base(), json!({"track": "full", "issueIid": iid}))
        .await;
    assert_eq!(st, StatusCode::CONFLICT);

    let (_, list) = app.get(&app.base()).await;
    assert_eq!(list["data"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn start_plan_track_on_existing_issue() {
    let app = SdlcApp::new().await;
    let iid = app.gitlab.add_issue("Refactor", &[]) as i64;
    let (st, v) = app
        .post(&app.base(), json!({"track": "plan", "issueIid": iid}))
        .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let docs = v["data"]["documents"].as_array().unwrap();
    assert_eq!(docs.len(), 1);
    assert_eq!(docs[0]["docType"], "plan");
    let text = read(&app, iid, &file_path(iid, "plan")).await;
    assert!(text["content"]
        .as_str()
        .unwrap()
        .contains("# Plan: Refactor"));
    assert!(app
        .gitlab
        .issue(iid as u64)
        .labels
        .contains(&"doc:plan".to_string()));
}

#[tokio::test]
async fn tree_read_search_and_write_rules() {
    let app = SdlcApp::new().await;
    let iid = start_full(&app, "T").await;
    let (_, tree) = app.get(&format!("{}/{iid}/tree", app.base())).await;
    let names: Vec<&str> = tree["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["name"].as_str().unwrap())
        .collect();
    assert!(
        names.contains(&"src") && names.contains(&"README.md") && !names.contains(&".git"),
        "{names:?}"
    );
    let (_, sub) = app
        .get(&format!(
            "{}/{iid}/tree?path=docs/changes/{iid}",
            app.base()
        ))
        .await;
    assert_eq!(sub["data"][0]["writable"], true);

    let f = read(&app, iid, "src/main.rs").await;
    assert_eq!(f["writable"], false);
    let (st, _) = app
        .get(&format!("{}/{iid}/file?path=../etc/passwd", app.base()))
        .await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
    let (st, _) = app
        .get(&format!("{}/{iid}/file?path=.git/config", app.base()))
        .await;
    assert_eq!(st, StatusCode::BAD_REQUEST);

    let (_, s) = app
        .get(&format!("{}/{iid}/search?q=fn%20main", app.base()))
        .await;
    assert_eq!(s["data"][0]["path"], "src/main.rs");
    let (_, s) = app
        .get(&format!("{}/{iid}/search?q=srcmain&mode=file", app.base()))
        .await;
    assert_eq!(s["data"][0]["path"], "src/main.rs");

    // Writing outside the whitelist is forbidden.
    let (st, _) = app
        .put(
            &format!("{}/{iid}/file", app.base()),
            json!({"path": "src/main.rs", "content": "x", "baseVersion": f["version"]}),
        )
        .await;
    assert_eq!(st, StatusCode::FORBIDDEN);

    // Optimistic lock.
    let intent = read(&app, iid, &file_path(iid, "intent")).await;
    let (st, w) = app
        .put(&format!("{}/{iid}/file", app.base()), json!({"path": file_path(iid, "intent"), "content": format!("{}\nmore\n", intent["content"].as_str().unwrap()), "baseVersion": intent["version"]}))
        .await;
    assert_eq!(st, StatusCode::OK, "{w}");
    assert!(w["data"]["commitSha"].is_null(), "autosave must not commit");
    let (st, _) = app
        .put(&format!("{}/{iid}/file", app.base()), json!({"path": file_path(iid, "intent"), "content": "stale", "baseVersion": intent["version"]}))
        .await;
    assert_eq!(st, StatusCode::CONFLICT);

    // Explicit save commits and pushes.
    let cur = read(&app, iid, &file_path(iid, "intent")).await;
    let (st, w) = app
        .put(&format!("{}/{iid}/file", app.base()), json!({"path": file_path(iid, "intent"), "content": cur["content"], "baseVersion": cur["version"], "commit": true}))
        .await;
    assert_eq!(st, StatusCode::OK);
    let sha = w["data"]["commitSha"].as_str().unwrap();
    let remote = git(&app.bare, &["rev-parse", &format!("symphony/docs-{iid}")]).unwrap();
    assert_eq!(remote.trim(), sha);

    let (_, d) = app.get(&format!("{}/{iid}/diff", app.base())).await;
    assert!(d["data"]["diff"].as_str().unwrap().is_empty());
    let (_, t) = app.get(&format!("{}/{iid}/templates", app.base())).await;
    assert_eq!(t["data"].as_array().unwrap().len(), 3);
    let (_, t) = app
        .get(&format!("/api/projects/{}/doc-templates", app.project_id))
        .await;
    assert_eq!(t["data"][1]["source"], "builtin");
}

#[tokio::test]
async fn submit_runs_template_validation() {
    let app = SdlcApp::new().await;
    let (_, v) = app
        .post(&app.base(), json!({"track": "full", "title": "Empty"}))
        .await;
    let iid = v["data"]["flow"]["issueIid"].as_i64().unwrap();
    let (_, val) = app
        .post(
            &format!("{}/{iid}/docs/intent/validate", app.base()),
            json!({}),
        )
        .await;
    assert!(!val["data"].as_array().unwrap().is_empty());
    let (st, v) = app
        .post(
            &format!("{}/{iid}/docs/intent/submit", app.base()),
            json!({}),
        )
        .await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "{v}");
    assert!(v["retMsg"].as_str().unwrap().contains("提交校验未通过"));
}

/// Full path G1 → AI spec → G2 → AI plan → G3 → merge → Todo.
#[tokio::test]
async fn full_flow_to_todo() {
    let app = SdlcApp::new().await;
    let iid = start_full(&app, "Claim status").await;
    let base = format!("{}/{iid}", app.base());

    let (st, v) = app
        .post(&format!("{base}/docs/intent/submit"), json!({}))
        .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(v["data"]["state"], "intent_review");
    assert!(app
        .gitlab
        .issue(iid as u64)
        .labels
        .contains(&"doc:intent-review".to_string()));

    // Gate errors.
    let (st, _) = app
        .post(
            &format!("{base}/docs/spec/decision"),
            json!({"action": "approve"}),
        )
        .await;
    assert_eq!(st, StatusCode::NOT_FOUND);
    let (st, _) = app
        .post(
            &format!("{base}/docs/intent/decision"),
            json!({"action": "request_changes"}),
        )
        .await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "comment is required");

    let (st, v) = app
        .post(
            &format!("{base}/docs/intent/decision"),
            json!({"action": "approve"}),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    // AI drafts spec in background and auto-submits.
    let flow = app
        .wait_flow(iid, |f| SdlcApp::doc_status(f, "spec") == "in_review")
        .await;
    assert_eq!(flow["state"], "spec_review");
    let spec = read_branch_file(
        &app.bare,
        &format!("symphony/docs-{iid}"),
        &file_path(iid, "spec"),
    )
    .unwrap();
    assert!(spec.contains("owner: sec"), "{spec}");
    let log = git(
        &app.bare,
        &["log", "--format=%B", &format!("symphony/docs-{iid}")],
    )
    .unwrap();
    assert!(log.contains("Symphony-Actor: ai"), "{log}");
    app.wait_note(iid, "G1 已批准 intent.md").await;
    app.wait_label(iid, "doc:spec-review").await;

    let (st, _) = app
        .post(
            &format!("{base}/docs/spec/decision"),
            json!({"action": "approve"}),
        )
        .await;
    assert_eq!(st, StatusCode::OK);
    app.wait_flow(iid, |f| SdlcApp::doc_status(f, "plan") == "in_review")
        .await;

    let (st, v) = app
        .post(
            &format!("{base}/docs/plan/decision"),
            json!({"action": "approve"}),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let flow = app.wait_flow(iid, |f| f["state"] == "todo").await;
    assert!(flow["flow"]["mergeCommitSha"].is_string());
    assert!(flow["documents"]
        .as_array()
        .unwrap()
        .iter()
        .all(|d| d["status"] == "merged"));

    let mr = app.gitlab.mr(1);
    assert_eq!(mr.state, "merged");
    assert!(
        !mr.title.starts_with("Draft:"),
        "MR must be Ready before merge"
    );
    assert_eq!(
        app.gitlab.merge_tokens.lock().unwrap()[0],
        "admin-gitlab-token",
        "approver token"
    );
    // Merge commit keeps the docs branch commits (no squash).
    let parents = git(&app.bare, &["rev-list", "--parents", "-n", "1", "main"]).unwrap();
    assert_eq!(
        parents.split_whitespace().count(),
        3,
        "merge commit expected: {parents}"
    );
    assert!(read_branch_file(&app.bare, "main", &file_path(iid, "plan")).is_some());

    app.wait_label(iid, "Todo").await;
    app.wait_note(iid, "文档已合入主干").await;
    let issue = app.gitlab.issue(iid as u64);
    assert!(
        !issue.labels.iter().any(|l| l.starts_with("doc:")),
        "{:?}",
        issue.labels
    );
    assert!(issue.notes.iter().any(|n| n.contains("文档已合入主干")));

    // Approvals recorded with actor and blob sha.
    let approvals = flow["approvals"].as_array().unwrap();
    assert_eq!(
        approvals
            .iter()
            .filter(|a| a["action"] == "approve")
            .count(),
        3
    );
    assert!(approvals
        .iter()
        .all(|a| a["actorName"] == "admin" && a["blobSha"].as_str().unwrap().len() == 64));

    // Merged documents are frozen.
    let cur = read(&app, iid, &file_path(iid, "plan")).await;
    let (st, _) = app
        .put(
            &format!("{base}/file"),
            json!({"path": file_path(iid, "plan"), "content": "x", "baseVersion": cur["version"]}),
        )
        .await;
    assert_eq!(st, StatusCode::CONFLICT);
}

#[tokio::test]
async fn reject_intent_closes_issue_and_mr() {
    let app = SdlcApp::new().await;
    let iid = start_full(&app, "Nope").await;
    let base = format!("{}/{iid}", app.base());
    app.post(&format!("{base}/docs/intent/submit"), json!({}))
        .await;
    let (st, v) = app
        .post(
            &format!("{base}/docs/intent/decision"),
            json!({"action": "reject", "comment": "not now"}),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(v["data"]["state"], "rejected");
    assert_eq!(app.gitlab.issue(iid as u64).state, "closed");
    assert_eq!(app.gitlab.mr(1).state, "closed");
    assert!(read_branch_file(&app.bare, "main", &file_path(iid, "intent")).is_none());
}

#[tokio::test]
async fn spec_cannot_be_rejected() {
    let app = SdlcApp::new().await;
    let iid = start_full(&app, "R").await;
    let base = format!("{}/{iid}", app.base());
    app.post(&format!("{base}/docs/intent/submit"), json!({}))
        .await;
    app.post(
        &format!("{base}/docs/intent/decision"),
        json!({"action": "approve"}),
    )
    .await;
    app.wait_flow(iid, |f| SdlcApp::doc_status(f, "spec") == "in_review")
        .await;
    let (st, _) = app
        .post(
            &format!("{base}/docs/spec/decision"),
            json!({"action": "reject"}),
        )
        .await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn request_changes_redrafts_in_place() {
    let app = SdlcApp::new().await;
    let iid = start_full(&app, "RC").await;
    let base = format!("{}/{iid}", app.base());
    app.post(&format!("{base}/docs/intent/submit"), json!({}))
        .await;
    app.post(
        &format!("{base}/docs/intent/decision"),
        json!({"action": "approve"}),
    )
    .await;
    app.wait_flow(iid, |f| SdlcApp::doc_status(f, "spec") == "in_review")
        .await;
    let (st, v) = app
        .post(
            &format!("{base}/docs/spec/decision"),
            json!({"action": "request_changes", "comment": "Design 太粗"}),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    // AI revises the current document (fake marks revisions) and resubmits.
    app.wait_flow(iid, |f| SdlcApp::doc_status(f, "spec") == "in_review")
        .await;
    let spec = read(&app, iid, &file_path(iid, "spec")).await;
    assert!(spec["content"]
        .as_str()
        .unwrap()
        .contains("<!-- revised -->"));
    let issue = app.gitlab.issue(iid as u64);
    assert!(issue
        .notes
        .iter()
        .any(|n| n.contains("要求修改") && n.contains("Design 太粗")));
}

/// Editing an approved document voids its approval; re-approving upstream
/// cascades downstream to stale (§4.2 / §4.3).
#[tokio::test]
async fn content_change_and_cascade() {
    let app = SdlcApp::new().await;
    let iid = start_full(&app, "Cascade").await;
    let base = format!("{}/{iid}", app.base());
    app.post(&format!("{base}/docs/intent/submit"), json!({}))
        .await;
    app.post(
        &format!("{base}/docs/intent/decision"),
        json!({"action": "approve"}),
    )
    .await;
    app.wait_flow(iid, |f| SdlcApp::doc_status(f, "spec") == "in_review")
        .await;
    app.post(
        &format!("{base}/docs/spec/decision"),
        json!({"action": "approve"}),
    )
    .await;
    app.wait_flow(iid, |f| SdlcApp::doc_status(f, "plan") == "in_review")
        .await;

    // Frontmatter-only change does not void approval.
    let cur = read(&app, iid, &file_path(iid, "intent")).await;
    let text = cur["content"]
        .as_str()
        .unwrap()
        .replace("template: intent@1", "template: intent@9");
    let (st, _) = app.put(&format!("{base}/file"), json!({"path": file_path(iid, "intent"), "content": text, "baseVersion": cur["version"]})).await;
    assert_eq!(st, StatusCode::OK);
    let (_, v) = app.get(&base).await;
    assert_eq!(SdlcApp::doc_status(&v["data"], "intent"), "approved");

    // Body change → intent back to review, approval invalidated.
    let cur = read(&app, iid, &file_path(iid, "intent")).await;
    let text = format!("{}\n- new constraint\n", cur["content"].as_str().unwrap());
    app.put(
        &format!("{base}/file"),
        json!({"path": file_path(iid, "intent"), "content": text, "baseVersion": cur["version"]}),
    )
    .await;
    let (_, v) = app.get(&base).await;
    assert_eq!(SdlcApp::doc_status(&v["data"], "intent"), "in_review");
    assert_eq!(v["data"]["state"], "intent_review");
    assert!(v["data"]["approvals"]
        .as_array()
        .unwrap()
        .iter()
        .any(|a| a["invalidatedAt"].is_string()));
    assert!(app
        .gitlab
        .issue(iid as u64)
        .labels
        .contains(&"doc:intent-review".to_string()));

    // Plan cannot be approved while upstream is not approved.
    let (st, _) = app
        .post(
            &format!("{base}/docs/plan/decision"),
            json!({"action": "approve"}),
        )
        .await;
    assert_eq!(st, StatusCode::CONFLICT);

    // Re-approve intent → spec and plan stale.
    let (st, v) = app
        .post(
            &format!("{base}/docs/intent/decision"),
            json!({"action": "approve"}),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(SdlcApp::doc_status(&v["data"], "spec"), "stale");
    assert_eq!(SdlcApp::doc_status(&v["data"], "plan"), "stale");
    assert_eq!(v["data"]["state"], "spec_review");
    // Stale documents are re-submitted for review.
    let (st, v) = app
        .post(&format!("{base}/docs/spec/submit"), json!({}))
        .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(SdlcApp::doc_status(&v["data"], "spec"), "in_review");
}

#[tokio::test]
async fn merge_failure_keeps_plan_review_and_retry_works() {
    let app = SdlcApp::new().await;
    let iid = app.gitlab.add_issue("P", &[]) as i64;
    app.post(&app.base(), json!({"track": "plan", "issueIid": iid}))
        .await;
    let base = format!("{}/{iid}", app.base());
    // Fill a valid plan and save.
    let cur = read(&app, iid, &file_path(iid, "plan")).await;
    let fm_end = cur["content"].as_str().unwrap().find("\n---\n").unwrap() + 5;
    let text = format!("{}# Plan\n\n## Files\n- src/main.rs\n\n## Steps\n1\n\n## Tests\n- t\n\n## Risks\n- r\n\n## Alternatives considered\n- n\n", &cur["content"].as_str().unwrap()[..fm_end]);
    app.put(&format!("{base}/file"), json!({"path": file_path(iid, "plan"), "content": text, "baseVersion": cur["version"], "commit": true})).await;
    let (st, v) = app
        .post(&format!("{base}/docs/plan/submit"), json!({}))
        .await;
    assert_eq!(st, StatusCode::OK, "{v}");

    *app.gitlab.fail_merge.lock().unwrap() = true;
    // Policy rejection (405): no automatic retry, no extra commits (review r2 B3).
    let head_before = git(&app.bare, &["rev-parse", &format!("symphony/docs-{iid}")]).unwrap();
    let (st, v) = app
        .post(
            &format!("{base}/docs/plan/decision"),
            json!({"action": "approve"}),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let flow = v["data"].clone();
    assert_eq!(flow["state"], "plan_approved_pending_merge", "{flow}");
    assert!(flow["flow"]["mergeError"]
        .as_str()
        .unwrap()
        .contains("重试合并"));
    let head_after = git(&app.bare, &["rev-parse", &format!("symphony/docs-{iid}")]).unwrap();
    assert_eq!(head_before, head_after, "no unreviewed merge commit pushed");
    // Manual retry merges.
    let (st, v) = app.post(&format!("{base}/merge"), json!({})).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(v["data"]["state"], "todo");
    assert!(flow["state"] == "plan_approved_pending_merge", "{flow}");

    let app2 = SdlcApp::new().await;
    let iid2 = app2.gitlab.add_issue("P2", &[]) as i64;
    app2.post(&app2.base(), json!({"track": "plan", "issueIid": iid2}))
        .await;
    let base2 = format!("{}/{iid2}", app2.base());
    let cur = read(&app2, iid2, &file_path(iid2, "plan")).await;
    let fm_end = cur["content"].as_str().unwrap().find("\n---\n").unwrap() + 5;
    let text = format!("{}# Plan\n\n## Files\n- src/main.rs\n\n## Steps\n1\n\n## Tests\n- t\n\n## Risks\n- r\n\n## Alternatives considered\n- n\n", &cur["content"].as_str().unwrap()[..fm_end]);
    app2.put(
        &format!("{base2}/file"),
        json!({"path": file_path(iid2, "plan"), "content": text, "baseVersion": cur["version"]}),
    )
    .await;
    app2.post(&format!("{base2}/docs/plan/submit"), json!({}))
        .await;
    // No merge permission → approve is blocked by the precheck.
    *app2.gitlab.access_level.lock().unwrap() = 20;
    let (st, v) = app2
        .post(
            &format!("{base2}/docs/plan/decision"),
            json!({"action": "approve"}),
        )
        .await;
    assert_eq!(st, StatusCode::FORBIDDEN, "{v}");
    assert!(v["retMsg"].as_str().unwrap().contains("合并权限"));
    let (_, g) = app2.get(&base2).await;
    assert_eq!(g["data"]["mergePrecheck"]["canMerge"], false);
    *app2.gitlab.access_level.lock().unwrap() = 40;
    let (st, _) = app2
        .post(
            &format!("{base2}/docs/plan/decision"),
            json!({"action": "approve"}),
        )
        .await;
    assert_eq!(st, StatusCode::OK);
    app2.wait_flow(iid2, |f| f["state"] == "todo").await;
    // Retry on an already merged flow is a conflict.
    let (st, _) = app2.post(&format!("{base2}/merge"), json!({})).await;
    assert_eq!(st, StatusCode::CONFLICT);
}

#[tokio::test]
async fn revert_creates_r2_revision() {
    let app = SdlcApp::new().await;
    let iid = app.gitlab.add_issue("Rev", &[]) as i64;
    app.post(&app.base(), json!({"track": "plan", "issueIid": iid}))
        .await;
    let base = format!("{}/{iid}", app.base());
    // Revert before merge is refused.
    let (st, _) = app
        .post(&format!("{base}/revert"), json!({"to": "plan"}))
        .await;
    assert_eq!(st, StatusCode::CONFLICT);

    let cur = read(&app, iid, &file_path(iid, "plan")).await;
    let fm_end = cur["content"].as_str().unwrap().find("\n---\n").unwrap() + 5;
    let text = format!("{}# Plan\n\n## Files\n- src/main.rs\n\n## Steps\n1\n\n## Tests\n- t\n\n## Risks\n- r\n\n## Alternatives considered\n- n\n", &cur["content"].as_str().unwrap()[..fm_end]);
    app.put(
        &format!("{base}/file"),
        json!({"path": file_path(iid, "plan"), "content": text, "baseVersion": cur["version"]}),
    )
    .await;
    app.post(&format!("{base}/docs/plan/submit"), json!({}))
        .await;
    app.post(
        &format!("{base}/docs/plan/decision"),
        json!({"action": "approve"}),
    )
    .await;
    app.wait_flow(iid, |f| f["state"] == "todo").await;

    // Simulate an implementation branch with a plan revision.
    let seed = app.dir.path().join("impl");
    git(
        app.dir.path(),
        &[
            "clone",
            "-q",
            app.bare.to_str().unwrap(),
            seed.to_str().unwrap(),
        ],
    )
    .unwrap();
    let plan_path = seed.join(file_path(iid, "plan"));
    let mut t = std::fs::read_to_string(&plan_path).unwrap();
    t.push_str("- implementation note\n");
    std::fs::write(&plan_path, t).unwrap();
    git(&seed, &["checkout", "-q", "-b", "symphony/app-impl"]).unwrap();
    git(
        &seed,
        &[
            "-c",
            "user.name=a",
            "-c",
            "user.email=a@a",
            "commit",
            "-qam",
            "impl",
        ],
    )
    .unwrap();
    git(&seed, &["push", "-q", "origin", "symphony/app-impl"]).unwrap();

    let (st, v) = app
        .post(
            &format!("{base}/revert"),
            json!({"to": "plan", "implBranch": "symphony/app-impl"}),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(v["data"]["flow"]["revision"], 2);
    assert_eq!(
        v["data"]["flow"]["branch"],
        format!("symphony/docs-{iid}-r2")
    );
    assert_eq!(v["data"]["state"], "plan_review");
    assert!(!app
        .gitlab
        .issue(iid as u64)
        .labels
        .contains(&"Todo".to_string()));
    let plan = read_branch_file(
        &app.bare,
        &format!("symphony/docs-{iid}-r2"),
        &file_path(iid, "plan"),
    )
    .unwrap();
    assert!(
        plan.contains("implementation note"),
        "docs copied from implementation branch"
    );
    assert_eq!(app.gitlab.mr(2).source, format!("symphony/docs-{iid}-r2"));
}

#[tokio::test]
async fn draft_failure_retry_and_manual() {
    let app = SdlcApp::with(Opts {
        multi_repo: false,
        codex_script: None,
    })
    .await;
    let iid = start_full(&app, "NoCodex").await;
    let base = format!("{}/{iid}", app.base());
    app.post(&format!("{base}/docs/intent/submit"), json!({}))
        .await;
    app.post(
        &format!("{base}/docs/intent/decision"),
        json!({"action": "approve"}),
    )
    .await;
    // `false` exits immediately: three failures → failed.
    let flow = app
        .wait_flow(iid, |f| {
            f["documents"].as_array().unwrap().iter().any(|d| {
                d["docType"] == "spec" && d["draftState"] == "failed" && d["draftFailures"] == 3
            })
        })
        .await;
    assert_eq!(flow["state"], "spec");
    let (st, _) = app
        .post(&format!("{base}/docs/spec/draft/manual"), json!({}))
        .await;
    assert_eq!(st, StatusCode::OK);
    let (_, v) = app.get(&base).await;
    let spec = v["data"]["documents"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["docType"] == "spec")
        .unwrap()
        .clone();
    assert_eq!(spec["draftState"], "idle");
    let (st, _) = app
        .post(&format!("{base}/docs/spec/draft/retry"), json!({}))
        .await;
    assert_eq!(st, StatusCode::OK);
}

#[tokio::test]
async fn multi_repo_workspace_layout() {
    let app = SdlcApp::with(Opts {
        multi_repo: true,
        ..Default::default()
    })
    .await;
    let iid = start_full(&app, "Multi").await;
    let (_, v) = app.get(&format!("{}/{iid}", app.base())).await;
    assert_eq!(v["data"]["multiRepo"], true);
    assert_eq!(
        v["data"]["writableDir"],
        format!("primary/docs/changes/{iid}")
    );
    let (_, tree) = app.get(&format!("{}/{iid}/tree", app.base())).await;
    let names: Vec<&str> = tree["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, vec!["backend", "primary"]);
    let f = read(&app, iid, "backend/src/api.rs").await;
    assert_eq!(f["writable"], false);
    let (_, s) = app
        .get(&format!("{}/{iid}/search?q=pub%20fn%20api", app.base()))
        .await;
    assert_eq!(s["data"][0]["path"], "backend/src/api.rs");
    let intent = read(&app, iid, &format!("primary/{}", file_path(iid, "intent"))).await;
    assert_eq!(intent["writable"], true);
    let (st, _) = app
        .put(
            &format!("{}/{iid}/file", app.base()),
            json!({"path": "backend/src/api.rs", "content": "x", "baseVersion": f["version"]}),
        )
        .await;
    assert_eq!(st, StatusCode::FORBIDDEN);
    // Plan validation flags files in read-only repos (warning, not blocker).
    let (_, ctx) = app
        .post(&format!("{}/{iid}/workspace", app.base()), json!({}))
        .await;
    assert_eq!(ctx["data"]["multiRepo"], true);
    assert!(
        ctx["data"]["context"]["missing"]
            .as_array()
            .unwrap()
            .is_empty(),
        "{ctx}"
    );
}

// ------------------------------------------------- fixes after verification

/// verify C #1/#2: starting on an existing `Todo` issue removes
/// implementation labels and adds the track label.
#[tokio::test]
async fn existing_issue_drops_impl_labels_and_adds_track() {
    let app = SdlcApp::new().await;
    let iid = app.gitlab.add_issue("Existing", &["Todo", "bug"]) as i64;
    let (st, v) = app
        .post(&app.base(), json!({"track": "plan", "issueIid": iid}))
        .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let labels = app.gitlab.issue(iid as u64).labels;
    assert!(!labels.contains(&"Todo".to_string()), "{labels:?}");
    assert!(labels.contains(&"doc:plan".to_string()));
    assert!(labels.contains(&"track:plan".to_string()));
    assert!(labels.contains(&"bug".to_string()), "unrelated labels kept");
}

/// verify A I-7: failed MR creation rolls back branch + workspace + DB so
/// the user can retry.
#[tokio::test]
async fn failed_start_rolls_back_completely() {
    let app = SdlcApp::new().await;
    let iid = app.gitlab.add_issue("Retry me", &[]) as i64;
    *app.gitlab.fail_create_mr.lock().unwrap() = true;
    let (st, _) = app
        .post(&app.base(), json!({"track": "plan", "issueIid": iid}))
        .await;
    assert_ne!(st, StatusCode::OK);
    assert!(
        git(
            &app.bare,
            &[
                "rev-parse",
                "--verify",
                &format!("refs/heads/symphony/docs-{iid}")
            ]
        )
        .is_err(),
        "remote docs branch removed"
    );
    assert!(!app.primary_ws(iid).exists(), "workspace removed");
    *app.gitlab.fail_create_mr.lock().unwrap() = false;
    let (st, v) = app
        .post(&app.base(), json!({"track": "plan", "issueIid": iid}))
        .await;
    assert_eq!(st, StatusCode::OK, "retry works: {v}");
}

/// verify A I-4: commits pushed to the docs branch after approval are not
/// merged silently.
#[tokio::test]
async fn merge_refuses_unreviewed_remote_commits() {
    let app = SdlcApp::new().await;
    let iid = app.gitlab.add_issue("Sneaky", &[]) as i64;
    app.post(&app.base(), json!({"track": "plan", "issueIid": iid}))
        .await;
    let base = format!("{}/{iid}", app.base());
    let cur = read(&app, iid, &file_path(iid, "plan")).await;
    let fm_end = cur["content"].as_str().unwrap().find("\n---\n").unwrap() + 5;
    let text = format!("{}# Plan\n\n## Files\n- src/main.rs\n\n## Steps\n1\n\n## Tests\n- t\n\n## Risks\n- r\n\n## Alternatives considered\n- n\n", &cur["content"].as_str().unwrap()[..fm_end]);
    app.put(&format!("{base}/file"), json!({"path": file_path(iid, "plan"), "content": text, "baseVersion": cur["version"], "commit": true})).await;
    app.post(&format!("{base}/docs/plan/submit"), json!({}))
        .await;
    // Someone pushes directly to the docs branch.
    let seed = app.dir.path().join("sneak");
    git(
        app.dir.path(),
        &[
            "clone",
            "-q",
            "-b",
            &format!("symphony/docs-{iid}"),
            app.bare.to_str().unwrap(),
            seed.to_str().unwrap(),
        ],
    )
    .unwrap();
    std::fs::write(seed.join("src/evil.rs"), "evil\n").unwrap();
    git(&seed, &["add", "-A"]).unwrap();
    git(
        &seed,
        &[
            "-c",
            "user.name=x",
            "-c",
            "user.email=x@x",
            "commit",
            "-qm",
            "evil",
        ],
    )
    .unwrap();
    git(
        &seed,
        &["push", "-q", "origin", &format!("symphony/docs-{iid}")],
    )
    .unwrap();

    let (st, v) = app
        .post(
            &format!("{base}/docs/plan/decision"),
            json!({"action": "approve"}),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let (_, v) = app.get(&base).await;
    assert_eq!(v["data"]["state"], "plan_approved_pending_merge");
    assert!(v["data"]["flow"]["mergeError"]
        .as_str()
        .unwrap()
        .contains("被直接修改"));
    assert!(read_branch_file(&app.bare, "main", "src/evil.rs").is_none());
}

/// verify A I-1: documents containing credentials are never committed.
#[tokio::test]
async fn secrets_are_never_committed() {
    let app = SdlcApp::new().await;
    let iid = start_full(&app, "Secrets").await;
    let base = format!("{}/{iid}", app.base());
    let cur = read(&app, iid, &file_path(iid, "intent")).await;
    let text = format!(
        "{}\nleak: glpat-abcdefghijklmnop\n",
        cur["content"].as_str().unwrap()
    );
    let (st, v) = app
        .put(&format!("{base}/file"), json!({"path": file_path(iid, "intent"), "content": text, "baseVersion": cur["version"], "commit": true}))
        .await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "{v}");
    assert!(v["retMsg"].as_str().unwrap().contains("凭据"));
    let remote = read_branch_file(
        &app.bare,
        &format!("symphony/docs-{iid}"),
        &file_path(iid, "intent"),
    )
    .unwrap();
    assert!(!remote.contains("glpat-"));
}

/// verify C #5: `Symphony-Actor` only ever carries `ai` / `human`.
#[tokio::test]
async fn trailer_is_ai_or_human_only() {
    let app = SdlcApp::new().await;
    let iid = start_full(&app, "Trailer").await;
    let log = git(
        &app.bare,
        &["log", "--format=%B", &format!("symphony/docs-{iid}")],
    )
    .unwrap();
    for line in log.lines().filter(|l| l.starts_with("Symphony-Actor:")) {
        assert!(
            line == "Symphony-Actor: ai" || line == "Symphony-Actor: human",
            "{line}"
        );
    }
    assert!(log.contains("Symphony-User: Administrator"), "{log}");
}

/// verify C #3 / C #4: draft sessions release their quota; a manual
/// takeover stops the retry loop.
#[tokio::test]
async fn draft_releases_quota_for_many_flows() {
    let app = SdlcApp::new().await; // quota = 4 sessions
    let mut iids = vec![];
    for i in 0..5 {
        let iid = start_full(&app, &format!("Q{i}")).await;
        let base = format!("{}/{iid}", app.base());
        app.post(&format!("{base}/docs/intent/submit"), json!({}))
            .await;
        app.post(
            &format!("{base}/docs/intent/decision"),
            json!({"action": "approve"}),
        )
        .await;
        app.wait_flow(iid, |f| SdlcApp::doc_status(f, "spec") == "in_review")
            .await;
        iids.push(iid);
    }
    assert_eq!(iids.len(), 5);
}

// ------------------------------------------------- r2 review fixes

async fn plan_flow_in_review(app: &SdlcApp, title: &str) -> (i64, String) {
    let iid = app.gitlab.add_issue(title, &[]) as i64;
    app.post(&app.base(), json!({"track": "plan", "issueIid": iid}))
        .await;
    let base = format!("{}/{iid}", app.base());
    let cur = read(app, iid, &file_path(iid, "plan")).await;
    let fm_end = cur["content"].as_str().unwrap().find("\n---\n").unwrap() + 5;
    let text = format!("{}# Plan\n\n## Files\n- src/main.rs\n\n## Steps\n1\n\n## Tests\n- t\n\n## Risks\n- r\n\n## Alternatives considered\n- n\n", &cur["content"].as_str().unwrap()[..fm_end]);
    app.put(&format!("{base}/file"), json!({"path": file_path(iid, "plan"), "content": text, "baseVersion": cur["version"], "commit": true})).await;
    let (st, v) = app
        .post(&format!("{base}/docs/plan/submit"), json!({}))
        .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    (iid, base)
}

/// review r2 B2: a human moving the issue to the implementation phase is
/// never reverted; a mismatch notice is reported instead.
#[tokio::test]
async fn manual_todo_is_not_reverted() {
    let app = SdlcApp::new().await;
    let (iid, base) = plan_flow_in_review(&app, "Manual").await;
    // Human sets Todo on the platform while docs are in review.
    {
        let mut m = app.gitlab.issues.lock().unwrap();
        let i = m.get_mut(&(iid as u64)).unwrap();
        i.labels.retain(|l| !l.starts_with("doc:"));
        i.labels.push("Todo".into());
    }
    let (st, v) = app
        .post(
            &format!("{base}/docs/plan/decision"),
            json!({"action": "request_changes", "comment": "x"}),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let labels = app.gitlab.issue(iid as u64).labels;
    assert!(
        labels.contains(&"Todo".to_string()),
        "manual Todo kept: {labels:?}"
    );
    assert!(!labels.iter().any(|l| l.starts_with("doc:")), "{labels:?}");
}

/// review r2 B3: a real content conflict merges main into the docs branch
/// and retries once.
#[tokio::test]
async fn real_conflict_merges_main_then_retries() {
    let app = SdlcApp::new().await;
    let (iid, base) = plan_flow_in_review(&app, "Conflict").await;
    *app.gitlab.mr_merge_status.lock().unwrap() = Some("cannot_be_merged".into());
    let (st, v) = app
        .post(
            &format!("{base}/docs/plan/decision"),
            json!({"action": "approve"}),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let flow = app.wait_flow(iid, |f| f["state"] == "todo").await;
    assert!(flow["flow"]["mergeCommitSha"].is_string());
}

/// review r2 B1: on revert, a document whose content changed on the
/// implementation branch loses its approval and goes back to review.
#[tokio::test]
async fn revert_voids_approval_of_changed_upstream() {
    let app = SdlcApp::new().await;
    let iid = start_full(&app, "Revert B1").await;
    let base = format!("{}/{iid}", app.base());
    app.post(&format!("{base}/docs/intent/submit"), json!({}))
        .await;
    app.post(
        &format!("{base}/docs/intent/decision"),
        json!({"action": "approve"}),
    )
    .await;
    app.wait_flow(iid, |f| SdlcApp::doc_status(f, "spec") == "in_review")
        .await;
    app.post(
        &format!("{base}/docs/spec/decision"),
        json!({"action": "approve"}),
    )
    .await;
    app.wait_flow(iid, |f| SdlcApp::doc_status(f, "plan") == "in_review")
        .await;
    app.post(
        &format!("{base}/docs/plan/decision"),
        json!({"action": "approve"}),
    )
    .await;
    app.wait_flow(iid, |f| f["state"] == "todo").await;
    // Implementation branch edits intent.md (upstream of the plan revert).
    let seed = app.dir.path().join("impl-b1");
    git(
        app.dir.path(),
        &[
            "clone",
            "-q",
            app.bare.to_str().unwrap(),
            seed.to_str().unwrap(),
        ],
    )
    .unwrap();
    let ip = seed.join(file_path(iid, "intent"));
    let mut t = std::fs::read_to_string(&ip).unwrap();
    t.push_str("\n- changed during implementation\n");
    std::fs::write(&ip, t).unwrap();
    git(&seed, &["checkout", "-q", "-b", "impl-b1"]).unwrap();
    git(
        &seed,
        &[
            "-c",
            "user.name=a",
            "-c",
            "user.email=a@a",
            "commit",
            "-qam",
            "impl",
        ],
    )
    .unwrap();
    git(&seed, &["push", "-q", "origin", "impl-b1"]).unwrap();
    let (st, v) = app
        .post(
            &format!("{base}/revert"),
            json!({"to": "plan", "implBranch": "impl-b1"}),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(
        SdlcApp::doc_status(&v["data"], "intent"),
        "in_review",
        "changed upstream re-reviewed"
    );
    assert_eq!(
        SdlcApp::doc_status(&v["data"], "spec"),
        "approved",
        "unchanged spec keeps approval"
    );
    assert_eq!(v["data"]["state"], "intent_review");
}

/// review r2 I3: a failed "新建需求" start closes the issue it created.
#[tokio::test]
async fn failed_new_change_closes_created_issue() {
    let app = SdlcApp::new().await;
    *app.gitlab.fail_create_mr.lock().unwrap() = true;
    let (st, _) = app
        .post(
            &app.base(),
            json!({"track": "full", "title": "Orphan", "initialContent": INTENT_OK}),
        )
        .await;
    assert_ne!(st, StatusCode::OK);
    let issue = app.gitlab.issue(1);
    assert_eq!(issue.state, "closed");
    assert!(issue.notes.iter().any(|n| n.contains("创建失败")));
    // verify r4 B: the closed issue keeps no flow labels.
    assert!(
        !issue
            .labels
            .iter()
            .any(|l| l.starts_with("doc:") || l.starts_with("track:")),
        "{:?}",
        issue.labels
    );
}

/// verify r4 C-4: a manual takeover issued right after G1 cancels the
/// spec draft that G1 scheduled (no retries, no failures).
#[tokio::test]
async fn manual_takeover_right_after_approval_cancels_draft() {
    // Default fake app runs a codex command that does not exist, so a
    // draft that is not cancelled fails and retries.
    let app = SdlcApp::with(Opts {
        multi_repo: false,
        codex_script: None,
    })
    .await;
    let (st, v) = app
        .post(
            &app.base(),
            json!({"track": "full", "title": "M", "initialContent": INTENT_OK}),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let iid = v["data"]["flow"]["issueIid"].as_i64().unwrap();
    let base = format!("{}/{iid}", app.base());
    let (st, v) = app
        .post(&format!("{base}/docs/intent/submit"), json!({}))
        .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let (st, v) = app
        .post(
            &format!("{base}/docs/intent/decision"),
            json!({"action": "approve"}),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let (st, v) = app
        .post(&format!("{base}/docs/spec/draft/manual"), json!({}))
        .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    for _ in 0..12 {
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        let (_, f) = app.get(&base).await;
        let d = f["data"]["documents"]
            .as_array()
            .unwrap()
            .iter()
            .find(|d| d["docType"] == "spec")
            .cloned()
            .unwrap();
        assert_eq!(d["draftState"], "idle", "{d}");
        assert_eq!(d["draftFailures"], 0, "{d}");
    }
}
