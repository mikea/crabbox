use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/refs");

    set_env("BUILD_TIMESTAMP", timestamp());
    set_env("BUILD_PROFILE", env_var("PROFILE"));
    set_env("BUILD_TARGET", env_var("TARGET"));
    set_env(
        "GIT_COMMIT",
        git_commit().unwrap_or_else(|| "unknown".to_string()),
    );
    set_env(
        "GIT_DIRTY",
        git_dirty()
            .map(|dirty| if dirty { "dirty" } else { "clean" }.to_string())
            .unwrap_or_else(|| "unknown".to_string()),
    );
    set_env(
        "RUSTC_VERSION",
        rustc_version().unwrap_or_else(|| "unknown".to_string()),
    );
}

fn set_env(key: &str, value: String) {
    println!("cargo:rustc-env={key}={value}");
}

fn env_var(key: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| "unknown".to_string())
}

fn timestamp() -> String {
    command_output(&["date", "-u", "+%Y-%m-%dT%H:%M:%SZ"]).unwrap_or_else(|| "unknown".to_string())
}

fn git_commit() -> Option<String> {
    command_output(&["git", "rev-parse", "--short=12", "HEAD"])
}

fn git_dirty() -> Option<bool> {
    let output = Command::new("git")
        .args(["status", "--porcelain"])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    Some(!output.stdout.is_empty())
}

fn rustc_version() -> Option<String> {
    command_output(&["rustc", "--version"])
}

fn command_output(cmd: &[&str]) -> Option<String> {
    let (program, args) = cmd.split_first()?;

    let output = Command::new(program).args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }

    let text = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if text.is_empty() { None } else { Some(text) }
}
