use std::{error::Error, process::Command};
use vergen::EmitBuilder;

fn main() -> Result<(), Box<dyn Error>> {
    EmitBuilder::builder()
        .all_build()
        .all_git()
        .git_sha(true)
        .emit()?;
    Ok(())
}