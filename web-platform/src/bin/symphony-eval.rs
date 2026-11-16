//! `symphony-eval`: evaluate an organization rules checkout on fixed
//! drafting tasks (P3 design §5 E1). Exit code 1 on a regression against the
//! baseline, 2 on usage or setup errors.

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use web_platform::eval::{self, Options, Report, DEFAULT_TOLERANCE};

const USAGE: &str = "\
usage: symphony-eval [--org-config DIR] [--tasks DIR] [--codex CMD]
                     [--baseline FILE] [--tolerance PP] [--write-baseline]
                     [--repeat N] [--turn-timeout SECS] [--out FILE]

  --org-config DIR    organization rules checkout (default .)
  --tasks DIR         task files (default <org-config>/evals/tasks)
  --codex CMD         Codex app-server command (default \"codex app-server\")
  --baseline FILE     compare with this report; exit 1 on a regression
  --tolerance PP      allowed pass-rate drop in percentage points (default 5)
  --write-baseline    write the report to --baseline instead of comparing
  --repeat N          runs per task (default 1)
  --turn-timeout SECS per draft (default 1200)
  --out FILE          report path (default eval-report.json)";

struct Args {
    org_config: PathBuf,
    tasks: Option<PathBuf>,
    codex: String,
    baseline: Option<PathBuf>,
    tolerance: f64,
    write_baseline: bool,
    repeat: usize,
    turn_timeout: u64,
    out: PathBuf,
}

fn parse(mut it: impl Iterator<Item = String>) -> Result<Args, String> {
    let mut a = Args {
        org_config: PathBuf::from("."),
        tasks: None,
        codex: "codex app-server".into(),
        baseline: None,
        tolerance: DEFAULT_TOLERANCE,
        write_baseline: false,
        repeat: 1,
        turn_timeout: 1200,
        out: PathBuf::from("eval-report.json"),
    };
    while let Some(flag) = it.next() {
        let mut value = || it.next().ok_or_else(|| format!("{flag} 需要参数"));
        match flag.as_str() {
            "--org-config" => a.org_config = value()?.into(),
            "--tasks" => a.tasks = Some(value()?.into()),
            "--codex" => a.codex = value()?,
            "--baseline" => a.baseline = Some(value()?.into()),
            "--tolerance" => a.tolerance = value()?.parse().map_err(|_| "--tolerance 需要数字")?,
            "--write-baseline" => a.write_baseline = true,
            "--repeat" => {
                a.repeat = value()?
                    .parse()
                    .ok()
                    .filter(|n| *n > 0)
                    .ok_or("--repeat 需要正整数")?
            }
            "--turn-timeout" => {
                a.turn_timeout = value()?.parse().map_err(|_| "--turn-timeout 需要秒数")?
            }
            "--out" => a.out = value()?.into(),
            "-h" | "--help" => return Err(String::new()),
            other => return Err(format!("未知参数：{other}")),
        }
    }
    if a.write_baseline && a.baseline.is_none() {
        return Err("--write-baseline 需要 --baseline".into());
    }
    Ok(a)
}

fn print_summary(r: &Report) {
    println!(
        "org {} · codex {} · pass rate {:.1}%",
        &r.org_commit[..r.org_commit.len().min(8)],
        r.codex_version,
        r.pass_rate
    );
    for t in &r.tasks {
        println!(
            "  {} {} ({})",
            if t.passed { "PASS" } else { "FAIL" },
            t.name,
            t.doc
        );
        for run in t.runs.iter().filter(|r| !r.passed) {
            for reason in &run.reasons {
                println!("      - {reason}");
            }
        }
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    let args = match parse(std::env::args().skip(1)) {
        Ok(a) => a,
        Err(e) => {
            if !e.is_empty() {
                eprintln!("{e}");
            }
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    let tasks_dir = args
        .tasks
        .clone()
        .unwrap_or_else(|| args.org_config.join("evals/tasks"));
    let tasks = match eval::load_tasks(&tasks_dir) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(2);
        }
    };
    let opts = Options {
        org_config: args.org_config.clone(),
        codex: args.codex.clone(),
        repeat: args.repeat,
        turn_timeout: Duration::from_secs(args.turn_timeout),
        base_home: web_platform::sdlc::org_config::default_base_home(),
    };
    let report = match eval::run(&opts, &tasks).await {
        Ok(r) => r,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(2);
        }
    };
    let json = serde_json::to_string_pretty(&report).unwrap_or_default();
    if let Err(e) = std::fs::write(&args.out, &json) {
        eprintln!("无法写入 {}：{e}", args.out.display());
        return ExitCode::from(2);
    }
    print_summary(&report);
    let Some(baseline) = &args.baseline else {
        return ExitCode::SUCCESS;
    };
    if args.write_baseline {
        if let Err(e) = std::fs::write(baseline, &json) {
            eprintln!("无法写入 {}：{e}", baseline.display());
            return ExitCode::from(2);
        }
        println!("baseline written to {}", baseline.display());
        return ExitCode::SUCCESS;
    }
    let base: Report = match std::fs::read_to_string(baseline)
        .map_err(|e| e.to_string())
        .and_then(|t| serde_json::from_str(&t).map_err(|e| e.to_string()))
    {
        Ok(b) => b,
        Err(e) => {
            eprintln!("无法读取基线 {}：{e}", baseline.display());
            return ExitCode::from(2);
        }
    };
    let regressions = eval::compare(&report, &base, args.tolerance);
    if regressions.is_empty() {
        println!("no regression against {}", baseline.display());
        ExitCode::SUCCESS
    } else {
        for r in &regressions {
            println!("REGRESSION: {r}");
        }
        ExitCode::from(1)
    }
}
