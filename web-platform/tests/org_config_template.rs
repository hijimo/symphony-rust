//! `deploy/org-config-template/` (P3 design §5 E2) stays usable: it passes
//! the organization repository export checks, its templates and eval tasks
//! are valid, nothing in its `config.toml` is dropped and the CI workflows
//! download the asset the release publishes.

use std::path::{Path, PathBuf};

use web_platform::sdlc::org_config::{build_home, export_commit, filter_config};
use web_platform::sdlc::templates::{validate_template, TemplateKey};
use web_platform::sdlc::workspace::WorkspaceService;

const ASSET: &str = "symphony-eval-x86_64-unknown-linux-gnu.tar.gz";

fn template_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../deploy/org-config-template")
}

fn read(rel: &str) -> String {
    std::fs::read_to_string(template_dir().join(rel)).unwrap()
}

fn copy(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        let dest = to.join(e.file_name());
        if e.file_type().unwrap().is_dir() {
            copy(&e.path(), &dest);
        } else {
            std::fs::copy(e.path(), dest).unwrap();
        }
    }
}

fn git(dir: &Path, args: &[&str]) -> String {
    let out = std::process::Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@t"])
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

#[tokio::test]
async fn template_passes_the_export_checks_and_builds_a_codex_home() {
    let tmp = tempfile::TempDir::new().unwrap();
    let repo = tmp.path().join("org");
    copy(&template_dir(), &repo);
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-q", "-m", "template"]);
    let sha = git(&repo, &["rev-parse", "HEAD"]);
    let git_dir = PathBuf::from(git(&repo, &["rev-parse", "--absolute-git-dir"]));

    let root = tmp.path().join("org-config");
    let ws = WorkspaceService::new(tmp.path().to_str().unwrap());
    export_commit(&ws, &root, &git_dir, &sha).await.unwrap();
    let home = build_home(&root, &sha, &tmp.path().join("base")).unwrap();
    for f in ["AGENTS.md", "REVIEW.md", "config.toml", "skills/security-policy/SKILL.md"] {
        assert!(home.join(f).is_file(), "{f}");
    }
}

#[test]
fn templates_tasks_and_config_are_valid() {
    let (_, dropped) = filter_config(&read("config.toml")).unwrap();
    assert!(dropped.is_empty(), "{dropped:?}");

    let dir = template_dir().join("templates");
    let mut n = 0;
    for e in std::fs::read_dir(&dir).unwrap() {
        let path = e.unwrap().path();
        let key = TemplateKey::parse(path.file_stem().unwrap().to_str().unwrap()).unwrap();
        let errors = validate_template(key, &std::fs::read_to_string(&path).unwrap());
        assert!(errors.is_empty(), "{}: {errors:?}", path.display());
        n += 1;
    }
    assert!(n >= 1);

    let tasks = web_platform::eval::load_tasks(&template_dir().join("evals/tasks")).unwrap();
    let docs: Vec<&str> = tasks.iter().map(|t| t.doc.as_str()).collect();
    assert_eq!(docs, ["intent.incident", "spec"]);
}

#[test]
fn ci_workflows_download_the_released_asset() {
    let release = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../.github/workflows/release.yml"),
    )
    .unwrap();
    assert!(release.contains(ASSET), "release publishes {ASSET}");
    assert!(release.contains(&format!("{ASSET}.sha256")));
    for wf in [".github/workflows/eval.yml", ".gitlab-ci.yml"] {
        let text = read(wf);
        let _: serde_yaml::Value = serde_yaml::from_str(&text).unwrap();
        assert!(text.contains(ASSET), "{wf}");
        assert!(text.contains("sha256sum -c"), "{wf}");
        assert!(text.contains("--baseline evals/baseline.json"), "{wf}");
        assert!(text.contains("OPENAI_API_KEY"), "{wf}");
        assert!(!text.contains("sk-"), "{wf}: no credentials");
    }
}
