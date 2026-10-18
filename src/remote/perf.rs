//! Timings of what sharing does each round with large data. Ignored by default; run with
//! `ARCHON_PERF_DIR=<scratch> cargo test --release remote::perf -- --ignored --nocapture --test-threads=1`.
//! The repositories are made under `ARCHON_PERF_DIR` once and reused.

use super::{proto, snapshot};
use crate::store::Session;
use std::{
    path::{Path, PathBuf},
    process::Command,
    time::Instant,
};

fn scratch() -> PathBuf {
    PathBuf::from(std::env::var_os("ARCHON_PERF_DIR").expect("set ARCHON_PERF_DIR"))
}

fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["-c", "user.name=t", "-c", "user.email=t@t"])
        .args(args)
        .output()
        .expect("git");
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
}

/// Peak resident memory of this process so far, in MiB.
fn peak_mib() -> u64 {
    let status = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    let kb = status.lines().find(|l| l.starts_with("VmHWM:")).and_then(|l| l.split_whitespace().nth(1)).and_then(|v| v.parse::<u64>().ok());
    kb.unwrap_or(0) / 1024
}

/// A repository with 50 000 small committed files.
fn many_files() -> PathBuf {
    let dir = scratch().join("perf-50k");
    if dir.join(".git").exists() {
        return dir;
    }
    std::fs::create_dir_all(&dir).unwrap();
    git(&dir, &["init", "-q"]);
    for d in 0..500 {
        let sub = dir.join(format!("src/d{d}"));
        std::fs::create_dir_all(&sub).unwrap();
        for f in 0..100 {
            std::fs::write(sub.join(format!("f{f}.ts")), format!("export const v{d}_{f} = {};\n", d * f).repeat(20)).unwrap();
        }
    }
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-qm", "init"]);
    dir
}

/// A repository whose history holds `mib` MiB of incompressible data, in 100 MiB commits.
fn big_history(mib: usize) -> PathBuf {
    let dir = scratch().join(format!("perf-hist-{mib}"));
    if dir.join(".git").exists() {
        return dir;
    }
    std::fs::create_dir_all(&dir).unwrap();
    git(&dir, &["init", "-q"]);
    let mut seed = 0x9e37_79b9_7f4a_7c15u64;
    for i in 0..mib.div_ceil(100) {
        let mut data = vec![0u8; 100 << 20];
        for chunk in data.chunks_mut(8) {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            chunk.copy_from_slice(&seed.to_le_bytes()[..chunk.len()]);
        }
        let name = format!("blob{i}.bin");
        std::fs::write(dir.join(&name), &data).unwrap();
        git(&dir, &["add", &name]);
        git(&dir, &["commit", "-qm", &name]);
        git(&dir, &["rm", "-q", &name]);
    }
    std::fs::write(dir.join("README"), "hi\n").unwrap();
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-qm", "last"]);
    dir
}

/// `n` chats of `messages` messages of about `bytes` bytes each.
fn chats(n: usize, messages: usize, bytes: usize) -> Vec<Session> {
    (0..n)
        .map(|i| {
            let mut s = Session::new("claude", PathBuf::from("/work/repo"));
            s.title = format!("chat {i}");
            for m in 0..messages {
                let role = if m % 2 == 0 { "user" } else { "agent" };
                let text = format!("message {m} of chat {i} ").repeat(bytes / 24 + 1);
                s.messages.push(serde_json::from_value(serde_json::json!({ "role": role, "text": text })).unwrap());
            }
            s
        })
        .collect()
}

fn time<T>(label: &str, f: impl FnOnce() -> T) -> T {
    let start = Instant::now();
    let out = f();
    eprintln!("perf: {label}: {:.1} ms", start.elapsed().as_secs_f64() * 1000.0);
    out
}

#[test]
#[ignore]
fn chats_each_round() {
    let sessions = chats(500, 100, 1000);
    let values: Vec<serde_json::Value> = time("serialize 500 chats (UI thread, every backup)", || {
        sessions.iter().filter_map(|s| serde_json::to_value(s).ok()).collect()
    });
    let msg = proto::ClientMsg::PutChats { device: "n1".into(), sessions: values };
    let text = time("encode PutChats", || proto::encode(&msg));
    eprintln!("perf: PutChats frame: {} MiB", text.len() >> 20);
    let one = time("serialize 1 chat", || serde_json::to_value(&sessions[0]).ok());
    eprintln!("perf: one chat: {} KiB", one.map_or(0, |v| v.to_string().len() >> 10));
    let list: Vec<proto::SessionSummary> = sessions
        .iter()
        .map(|s| proto::SessionSummary {
            id: s.id.clone(),
            title: s.title.clone(),
            agent: s.agent.clone(),
            project: s.project.clone(),
            cwd: s.cwd.clone(),
            updated: s.updated,
            busy: false,
            messages: s.messages.len(),
        })
        .collect();
    let sessions_frame = proto::encode(&proto::HostMsg::Sessions { sessions: list });
    eprintln!("perf: Sessions reply for 500 chats: {} KiB", sessions_frame.len() >> 10);
}

#[test]
#[ignore]
fn snapshot_of_many_files() {
    let dir = many_files();
    let project = format!("p{}", std::process::id());
    time("first snapshot, 50k files", || snapshot::take(&dir, &project, "n1", false).unwrap());
    time("snapshot, nothing changed", || snapshot::take(&dir, &project, "n1", false).unwrap());
    time("current", || snapshot::current(&dir, &project, "n1"));
    let full = time("full bundle", || snapshot::bundle(&dir, &project, "n1", None, proto::MAX_BUNDLE_BYTES).unwrap());
    if let snapshot::Bundle::Ready(b) = full {
        eprintln!("perf: full bundle {} MiB", b.len() >> 20);
    }
}

#[test]
#[ignore]
fn first_copy_of_a_big_history() {
    let mib = std::env::var("ARCHON_PERF_MIB").ok().and_then(|v| v.parse().ok()).unwrap_or(1000);
    let max = std::env::var("ARCHON_PERF_MAX").ok().and_then(|v| v.parse().ok()).unwrap_or(proto::MAX_BUNDLE_BYTES);
    let dir = big_history(mib);
    let project = format!("p{}", std::process::id());
    let before = peak_mib();
    time("snapshot", || snapshot::take(&dir, &project, "n1", false).unwrap());
    let commit = snapshot::current(&dir, &project, "n1").unwrap();
    // What `backup` does for a project the cloud has never confirmed.
    let made = time("full bundle", || snapshot::bundle(&dir, &project, "n1", None, max));
    match made {
        Ok(snapshot::Bundle::TooBig(n)) => eprintln!("perf: too big: {} MiB", n >> 20),
        Ok(snapshot::Bundle::Ready(bytes)) => {
            eprintln!("perf: bundle {} MiB, fits: {}", bytes.len() >> 20, proto::fits(bytes.len().div_ceil(3) * 4));
            if proto::fits(bytes.len().div_ceil(3) * 4) {
                use base64::Engine as _;
                let bundle = time("base64", || base64::engine::general_purpose::STANDARD.encode(&bytes));
                drop(bytes);
                let msg = proto::ClientMsg::PutProject { project: project.clone(), name: "big".into(), device: "n1".into(), commit, bundle };
                let text = time("encode PutProject (UI thread)", || proto::encode(&msg));
                eprintln!("perf: frame {} MiB", text.len() >> 20);
            }
        }
        Err(e) => eprintln!("perf: bundle refused: {e}"),
    }
    eprintln!("perf: peak RSS {} MiB (was {} MiB before)", peak_mib(), before);
}
