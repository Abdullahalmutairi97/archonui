//! Smoke tests that run the real binary headlessly: no window, no agent CLIs needed.
use std::process::Command;

fn archonui(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_archonui")).args(args).output().expect("run archonui")
}

#[test]
fn version_matches_cargo() {
    let out = archonui(&["--version"]);
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), concat!("archonui ", env!("CARGO_PKG_VERSION")));
}

#[test]
fn help_lists_usage() {
    let out = archonui(&["--help"]);
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains("Usage: archonui"));
}

#[test]
fn unknown_flag_is_rejected_not_opened_as_a_file() {
    let out = archonui(&["--definitely-not-a-flag"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("unknown option"));
}

#[test]
fn detect_runs_with_an_empty_path() {
    let home = std::env::temp_dir().join(format!("archonui-detect-{}", std::process::id()));
    std::fs::create_dir_all(&home).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_archonui"))
        .arg("--detect")
        .env("PATH", "")
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", home.join("cfg"))
        .output()
        .unwrap();
    let _ = std::fs::remove_dir_all(&home);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(String::from_utf8_lossy(&out.stdout).trim().is_empty(), "no agents expected on an empty PATH");
}

/// Drives a whole turn through the real binary against a stand-in `claude`
/// that speaks stream-json, so changes to spawning, stdin and parsing are caught
/// without a real account.
#[test]
fn ask_round_trips_through_a_fake_claude() {
    use std::os::unix::fs::PermissionsExt;
    let root = std::env::temp_dir().join(format!("archonui-ask-{}", std::process::id()));
    let bin = root.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let script = bin.join("claude");
    std::fs::write(
        &script,
        r#"#!/bin/sh
case "$1" in --version) echo "2.0.0 (Claude Code)"; exit 0;; esac
cat >/dev/null
echo '{"type":"system","subtype":"init","session_id":"s1"}'
echo '{"type":"assistant","message":{"content":[{"type":"text","text":"pong"}]}}'
echo '{"type":"result","subtype":"success","result":"pong","session_id":"s1"}'
"#,
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_archonui"))
        .args(["--ask", "claude", "ping"])
        .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
        .env("HOME", root.join("home"))
        .env("XDG_CONFIG_HOME", root.join("cfg"))
        .output()
        .unwrap();
    let _ = std::fs::remove_dir_all(&root);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}{}", String::from_utf8_lossy(&out.stderr));
    assert!(stdout.contains("pong") && stdout.contains("finished: Ok"), "{stdout}");
}
