//! `symphony-eval` end to end (P3 design §5 E1) with the fake Codex
//! app-server: organization template and CODEX_HOME, scoring, baseline and
//! exit codes.

mod sdlc_support;

use std::path::Path;
use std::process::{Command, Output};

use sdlc_support::{git, FAKE_CODEX};
use serde_json::Value;

const SPEC_TPL: &str = "---\nsymphony_template:\n  prompt: |\n    ORG-SPEC-PROMPT follow the org rules.\n---\n# Spec: <title>\n\n## Requirements\n\n## Design\n\n## Concerns\n";

fn write(p: &Path, text: &str) {
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, text).unwrap();
}

struct Env {
    dir: tempfile::TempDir,
}

impl Env {
    fn new() -> Self {
        let dir = tempfile::TempDir::new().unwrap();
        let org = dir.path().join("org");
        write(&org.join("AGENTS.md"), "# Org rules\n");
        write(&org.join("templates/spec.md"), SPEC_TPL);
        write(
            &org.join("evals/tasks/spec-concerns.yaml"),
            "doc: spec\nrequirement: 导出报表需要鉴权\ncontext:\n  intent.md: |\n    # Intent\n    export\nexpect:\n  must_include: [\"owner: \\\\w+\"]\n  must_not_include: [\"TODO\"]\n  max_total_tokens: 1000\n",
        );
        write(
            &org.join("evals/tasks/plan-auth.yaml"),
            "doc: plan\nrequirement: 导出报表\nexpect:\n  must_include: [\"鉴权\"]\n",
        );
        git(&org, &["init", "-q", "-b", "main"]).unwrap();
        git(&org, &["add", "-A"]).unwrap();
        git(
            &org,
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
        write(&dir.path().join("fake_codex.py"), FAKE_CODEX);
        std::fs::create_dir_all(dir.path().join("base")).unwrap();
        Self { dir }
    }

    fn org(&self) -> std::path::PathBuf {
        self.dir.path().join("org")
    }

    fn run(&self, extra: &[&str]) -> Output {
        let codex = format!(
            "FAKE_CODEX_LOG={} python3 {}",
            self.dir.path().join("codex.log").display(),
            self.dir.path().join("fake_codex.py").display()
        );
        Command::new(env!("CARGO_BIN_EXE_symphony-eval"))
            .args([
                "--org-config",
                self.org().to_str().unwrap(),
                "--codex",
                &codex,
            ])
            .args(["--turn-timeout", "60"])
            .args(extra)
            .current_dir(self.dir.path())
            .env("CODEX_HOME", self.dir.path().join("base"))
            .output()
            .unwrap()
    }

    fn report(&self, name: &str) -> Value {
        serde_json::from_str(&std::fs::read_to_string(self.dir.path().join(name)).unwrap()).unwrap()
    }
}

fn stdout(o: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

#[test]
fn eval_scores_tasks_and_gates_on_the_baseline() {
    let env = Env::new();
    let sha = git(&env.org(), &["rev-parse", "HEAD"])
        .unwrap()
        .trim()
        .to_string();

    let out = env.run(&["--baseline", "baseline.json", "--write-baseline"]);
    assert_eq!(out.status.code(), Some(0), "{}", stdout(&out));
    let report = env.report("eval-report.json");
    assert_eq!(report["orgCommit"], sha);
    assert_eq!(report["passRate"], 50.0, "{report:#}");
    let tasks = report["tasks"].as_array().unwrap();
    assert_eq!(tasks[0]["name"], "plan-auth");
    assert_eq!(tasks[0]["passed"], false);
    assert_eq!(tasks[0]["runs"][0]["reasons"][0], "缺少：鉴权");
    assert_eq!(tasks[1]["name"], "spec-concerns");
    assert_eq!(tasks[1]["passed"], true, "{report:#}");
    assert_eq!(tasks[1]["runs"][0]["totalTokens"], 120);
    assert_eq!(env.report("baseline.json"), report);
    assert!(stdout(&out).contains("FAIL plan-auth"));

    // The organization template and CODEX_HOME were used.
    let log = std::fs::read_to_string(env.dir.path().join("codex.log")).unwrap();
    assert!(log.contains("ORG-SPEC-PROMPT"), "{log}");
    assert!(
        log.lines().any(
            |l| l.starts_with("codex_home ") && l.ends_with(&format!("org-config/homes/{sha}"))
        ),
        "{log}"
    );

    // Same result: no regression.
    let out = env.run(&["--baseline", "baseline.json", "--repeat", "2"]);
    assert_eq!(out.status.code(), Some(0), "{}", stdout(&out));
    assert_eq!(
        env.report("eval-report.json")["tasks"][1]["runs"]
            .as_array()
            .unwrap()
            .len(),
        2
    );

    // A rule change that breaks a passing task fails the check.
    write(
        &env.org().join("evals/tasks/spec-concerns.yaml"),
        "doc: spec\nrequirement: x\nexpect:\n  must_include: [\"审计日志\"]\n",
    );
    git(&env.org(), &["add", "-A"]).unwrap();
    git(
        &env.org(),
        &[
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "commit",
            "-q",
            "-m",
            "c2",
        ],
    )
    .unwrap();
    let out = env.run(&["--baseline", "baseline.json", "--out", "r2.json"]);
    assert_eq!(out.status.code(), Some(1), "{}", stdout(&out));
    let text = stdout(&out);
    assert!(text.contains("REGRESSION: 通过率 0.0%"), "{text}");
    assert!(text.contains("任务 spec-concerns 失败"), "{text}");
    assert_eq!(env.report("r2.json")["passRate"], 0.0);
}

#[test]
fn usage_and_setup_errors_exit_with_2() {
    let env = Env::new();
    for args in [
        vec!["--bogus"],
        vec!["--write-baseline"],
        vec!["--repeat", "0"],
        vec!["--tasks", "/nonexistent"],
        vec!["--baseline", "missing.json"],
    ] {
        let out = env.run(&args);
        assert_eq!(out.status.code(), Some(2), "{args:?}: {}", stdout(&out));
    }
    // An unsafe organization repository is rejected before any draft.
    write(&env.org().join("auth.json"), "{}");
    git(&env.org(), &["add", "-A"]).unwrap();
    git(
        &env.org(),
        &[
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "commit",
            "-q",
            "-m",
            "bad",
        ],
    )
    .unwrap();
    let out = env.run(&[]);
    assert_eq!(out.status.code(), Some(2));
    assert!(stdout(&out).contains("auth.json"), "{}", stdout(&out));
}
