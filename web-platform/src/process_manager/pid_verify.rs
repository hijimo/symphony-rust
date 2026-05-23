use tracing::debug;

/// Verify that a PID corresponds to the expected Symphony process.
///
/// Three-factor verification:
/// 1. Process exists
/// 2. Process command line contains "symphony"
/// 3. Process start time is within tolerance of the recorded start time
///
/// Returns true if the PID is verified as belonging to our process.
pub fn verify_pid(pid: u32, _expected_start_secs: i64) -> bool {
    // Factor 1: Check if process exists
    if !process_exists(pid) {
        debug!(pid, "PID verification failed: process does not exist");
        return false;
    }

    // Factor 2: Check command line contains "symphony"
    if !process_is_symphony(pid) {
        debug!(pid, "PID verification failed: not a symphony process");
        return false;
    }

    // Factor 3: Start time check (platform-specific, best-effort)
    // On macOS, getting exact start time is complex; we rely on factors 1+2
    // for now and add start time verification as an enhancement.
    true
}

/// Check if a process with the given PID exists.
fn process_exists(pid: u32) -> bool {
    // Use kill(pid, 0) which checks existence without sending a signal
    unsafe { libc::kill(pid as i32, 0) == 0 }
}

/// Check if the process command line contains "symphony".
#[cfg(target_os = "macos")]
fn process_is_symphony(pid: u32) -> bool {
    use std::process::Command;

    let output = Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "comm="])
        .output();

    match output {
        Ok(out) => {
            let comm = String::from_utf8_lossy(&out.stdout);
            comm.to_lowercase().contains("symphony")
        }
        Err(_) => false,
    }
}

/// Check if the process command line contains "symphony".
#[cfg(target_os = "linux")]
fn process_is_symphony(pid: u32) -> bool {
    use std::fs;
    let cmdline_path = format!("/proc/{}/cmdline", pid);
    match fs::read_to_string(&cmdline_path) {
        Ok(cmdline) => cmdline.to_lowercase().contains("symphony"),
        Err(_) => false,
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn process_is_symphony(_pid: u32) -> bool {
    // Fallback: assume valid if process exists
    true
}

/// Send SIGTERM to a process.
pub fn send_sigterm(pid: u32) -> bool {
    unsafe { libc::kill(pid as i32, libc::SIGTERM) == 0 }
}

/// Send SIGKILL to a process.
pub fn send_sigkill(pid: u32) -> bool {
    unsafe { libc::kill(pid as i32, libc::SIGKILL) == 0 }
}

/// Wait for a process to exit, with a timeout.
/// Returns true if the process exited within the timeout.
pub async fn wait_for_exit(pid: u32, timeout: std::time::Duration) -> bool {
    let start = std::time::Instant::now();
    let check_interval = std::time::Duration::from_millis(200);

    loop {
        if !process_exists(pid) {
            return true;
        }
        if start.elapsed() >= timeout {
            return false;
        }
        tokio::time::sleep(check_interval).await;
    }
}
