use std::process::Command;
fn main() {
    let mut output = Command::new("git").args(&["describe", "--always", "--long", "--dirty"]).output().unwrap();
    let mut build = String::from_utf8(output.stdout).unwrap_or("UNKNOWN-BUILD".into());
    if build.is_empty() {
        build = "UNKNOWN".into()
    }

    output = Command::new("git").args(&["rev-parse", "--abbrev-ref",  "HEAD"]).output().unwrap();
    let mut branch = String::from_utf8(output.stdout).unwrap_or("UNKNOWN-BUILD".into());
    if branch.is_empty() {
        branch = "UNKNOWN".into()
    }

    println!("cargo:rustc-env=GIT_BUILD={}", build);
    println!("cargo:rustc-env=GIT_BRANCH={}", branch);
}