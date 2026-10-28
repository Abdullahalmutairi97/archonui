//! Embeds the git commit the binary was built from, for the self-update check (src/app/update.rs).
//! A build outside a git checkout (a release tarball) gets an empty commit.
use std::process::Command;

fn git(args: &[&str]) -> Option<String> {
    let out = Command::new("git").args(args).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string()).filter(|s| !s.is_empty())
}

fn main() {
    let commit = git(&["rev-parse", "HEAD"]).unwrap_or_default();
    let branch = git(&["rev-parse", "--abbrev-ref", "HEAD"]).filter(|b| b != "HEAD").unwrap_or_default();
    println!("cargo:rustc-env=ARCHONUI_GIT_COMMIT={commit}");
    println!("cargo:rustc-env=ARCHONUI_GIT_BRANCH={branch}");
    // Rebuild this script's output when HEAD moves (a commit, checkout, pull or reset).
    if let Some(dir) = git(&["rev-parse", "--absolute-git-dir"]) {
        for file in ["HEAD", "logs/HEAD"] {
            let path = std::path::Path::new(&dir).join(file);
            if path.exists() {
                println!("cargo:rerun-if-changed={}", path.display());
            }
        }
    }
    println!("cargo:rerun-if-changed=build.rs");
}
