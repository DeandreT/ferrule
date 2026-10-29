//! Shell completion scripts for `ferrule completions`.

use std::io::Write;

use anyhow::Context;
use clap::ValueEnum;
use clap_complete::Shell;

/// Shells with a generated completion script.
#[derive(Clone, Copy, ValueEnum)]
pub(crate) enum CompletionShell {
    Bash,
    Zsh,
    Fish,
    #[value(name = "powershell")]
    PowerShell,
}

impl From<CompletionShell> for Shell {
    fn from(shell: CompletionShell) -> Self {
        match shell {
            CompletionShell::Bash => Self::Bash,
            CompletionShell::Zsh => Self::Zsh,
            CompletionShell::Fish => Self::Fish,
            CompletionShell::PowerShell => Self::PowerShell,
        }
    }
}

/// Writes the completion script for `command` to `out`.
///
/// The script is rendered into memory first because clap_complete panics on
/// write failures; a closed stdout is then reported as an ordinary error.
pub(crate) fn write(
    shell: CompletionShell,
    mut command: clap::Command,
    out: &mut dyn Write,
) -> anyhow::Result<()> {
    let mut script = Vec::new();
    let bin_name = command.get_name().to_owned();
    clap_complete::generate(Shell::from(shell), &mut command, bin_name, &mut script);
    out.write_all(&script)
        .context("writing completion script to stdout")?;
    out.flush().context("flushing completion script to stdout")
}
