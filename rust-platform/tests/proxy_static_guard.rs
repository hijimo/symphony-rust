use std::path::{Path, PathBuf};

fn rust_files_under(path: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut stack = vec![path.to_path_buf()];

    while let Some(current) = stack.pop() {
        for entry in std::fs::read_dir(&current).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                files.push(path);
            }
        }
    }

    files
}

#[test]
fn production_code_does_not_add_bare_network_commands() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let src = manifest_dir.join("src");
    let allowlist = [src.join("proxy.rs")];

    let offenders: Vec<String> = rust_files_under(&src)
        .into_iter()
        .filter(|path| !allowlist.iter().any(|allowed| allowed == path))
        .filter_map(|path| {
            let content = std::fs::read_to_string(&path).unwrap();
            content.contains("Command::new").then(|| {
                path.strip_prefix(&manifest_dir)
                    .unwrap()
                    .display()
                    .to_string()
            })
        })
        .collect();

    assert!(
        offenders.is_empty(),
        "network-related process launches must go through proxy::proxy_command; offenders: {offenders:?}"
    );
}

#[test]
fn production_code_does_not_add_bare_reqwest_clients() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let src = manifest_dir.join("src");
    let allowlist = [src.join("proxy.rs")];

    let offenders: Vec<String> = rust_files_under(&src)
        .into_iter()
        .filter(|path| !allowlist.iter().any(|allowed| allowed == path))
        .filter_map(|path| {
            let content = std::fs::read_to_string(&path).unwrap();
            (content.contains("reqwest::Client::new()")
                || content.contains("reqwest::Client::builder()")
                || content.contains("Client::builder()"))
            .then(|| {
                path.strip_prefix(&manifest_dir)
                    .unwrap()
                    .display()
                    .to_string()
            })
        })
        .collect();

    assert!(
        offenders.is_empty(),
        "reqwest clients must use proxy::proxy_aware_client_builder; offenders: {offenders:?}"
    );
}
