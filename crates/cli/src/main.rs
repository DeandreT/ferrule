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
    /// Run a mapping project against a CSV or XML input (chosen by
    /// extension), producing a CSV or XML output.
    Run {
        #[arg(long)]
        project: PathBuf,
        #[arg(long)]
        input: PathBuf,
        #[arg(long)]
        output: PathBuf,
    },
    /// Import an XSD file's root element as a SchemaNode, printed as JSON --
    /// a starting point for hand-authoring a project file's schema.
    ImportXsd {
        #[arg(long)]
        xsd: PathBuf,
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
            println!("wrote {rows} record(s) to {}", output.display());
            Ok(())
        }
        Command::ImportXsd { xsd } => {
            println!("{}", cli::import_xsd(&xsd)?);
            Ok(())
        }
    }
}
