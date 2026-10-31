//! `allowit completion --shell SHELL`.
use crate::{ctx::Ctx, error::Result};

pub(crate) const EXAMPLES: &str = "\
Examples:
  allowit completion --shell zsh > ~/.zfunc/_allowit
  allowit completion --shell bash > /etc/bash_completion.d/allowit";

/// Print a shell completion script
#[derive(clap::Args, Debug)]
pub(crate) struct Args {
    /// The shell
    #[arg(long, value_enum)]
    pub shell: clap_complete::Shell,
}

pub(crate) fn run(args: Args, cx: &mut Ctx) -> Result<i32> {
    let mut buffer = Vec::new();
    clap_complete::generate(args.shell, &mut super::command(), "allowit", &mut buffer);
    cx.out.raw = true;
    cx.out.stdout.push_str(&String::from_utf8_lossy(&buffer));
    Ok(0)
}
