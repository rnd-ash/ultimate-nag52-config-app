use std::process::Command;
fn main() {
    let output = Command::new("git").args(&["describe", "--always", "--long", "--dirty"]).output().unwrap();
    let mut build = String::from_utf8(output.stdout).unwrap_or("UNKNOWN-BUILD".into());
    if build.is_empty() {
        build = "UNKNOWN".into()
    }
    println!("cargo:rustc-env=GIT_BUILD={}", build);
}