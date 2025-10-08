use std::env;
use std::process::Command;

fn main() {
    // Capture git commit hash (short)
    let git_commit = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .and_then(|o| {
            if o.status.success() {
                Some(String::from_utf8_lossy(&o.stdout).trim().to_string())
            } else {
                None
            }
        })
        .unwrap_or_else(|| "unknown".to_string());

    // Capture git dirty state
    let git_status_dirty = Command::new("git")
        .args(["diff", "--quiet"])
        .status()
        .ok()
        .map(|s| if s.success() { "clean" } else { "dirty" })
        .unwrap_or("unknown");

    // Build timestamp
    let build_time = chrono::Utc::now().to_rfc3339();

    println!("cargo:rustc-env=GIT_COMMIT={}", git_commit);
    println!("cargo:rustc-env=GIT_STATE={}", git_status_dirty);
    println!("cargo:rustc-env=BUILD_TIME={}", build_time);

    // Re-run if HEAD or index changes
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/index");
}
