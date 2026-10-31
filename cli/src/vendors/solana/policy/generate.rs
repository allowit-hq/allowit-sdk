//! `allowit policy new PROMPT`: compiles and saves a new local policy.
//! Local only; never signs. Prints the generated Rust source. The prompt must
//! read "Spend up to N tokens per day" or "Spend up to N test tokens per day",
//! optionally followed by " with PaySH discovery"; other rules exit 20.
use super::*;

pub(super) fn run(local: &Local, pos: &[String], json: bool, stdout: &mut String) -> Result<i32> {
    let policy = native(Policy::generate(&local.network, &pos[0]))?;
    native(
        FileJournal::new(local.directory.join("journal"))
            .locked(|| local.save_policy(&policy).map_err(to_native)),
    )?;
    if json {
        stdout.push_str(&serde_json::to_string(&policy).unwrap());
        stdout.push('\n');
    } else {
        // Text output is the generated Rust source alone.
        stdout.push_str(&policy.rust);
        if !policy.rust.ends_with('\n') {
            stdout.push('\n');
        }
    }
    Ok(0)
}
