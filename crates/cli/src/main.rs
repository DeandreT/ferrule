use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "ferrule", version, about = "Run ferrule data mapping projects")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run a mapping project against a CSV input, producing a CSV output.
    Run {
        #[arg(long)]
        project: PathBuf,
        #[arg(long)]
        input: PathBuf,
        #[arg(long)]
        output: PathBuf,
    },
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Run {
            project,
            input,
            output,
        } => {
            let rows = cli::run_project(&project, &input, &output)?;
            println!("wrote {rows} row(s) to {}", output.display());
            Ok(())
        }
    }
}
