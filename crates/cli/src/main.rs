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
    /// Run a mapping project against a CSV, XML, JSON, or SQLite input
    /// (chosen by extension), producing an output in any of those formats.
    /// For SQLite the table name is the schema root's name.
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
    /// Import a JSON Schema file's root as a SchemaNode, printed as JSON.
    ImportJsonSchema {
        #[arg(long)]
        schema: PathBuf,
    },
    /// Introspect a SQLite table as a SchemaNode, printed as JSON.
    ImportDb {
        #[arg(long)]
        db: PathBuf,
        #[arg(long)]
        table: String,
    },
    /// Convert a MapForce .mfd design into a ferrule project file.
    ImportMfd {
        #[arg(long)]
        mfd: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    /// Convert a ferrule project file into a MapForce .mfd design
    /// (generated XSDs are written next to it).
    ExportMfd {
        #[arg(long)]
        project: PathBuf,
        #[arg(long)]
        out: PathBuf,
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
        Command::ImportJsonSchema { schema } => {
            println!("{}", cli::import_json_schema(&schema)?);
            Ok(())
        }
        Command::ImportDb { db, table } => {
            println!("{}", cli::import_db(&db, &table)?);
            Ok(())
        }
        Command::ImportMfd { mfd, out } => {
            let warnings = cli::import_mfd(&mfd, &out)?;
            for warning in &warnings {
                eprintln!("warning: {warning}");
            }
            println!("wrote {} ({} warning(s))", out.display(), warnings.len());
            Ok(())
        }
        Command::ExportMfd { project, out } => {
            let warnings = cli::export_mfd(&project, &out)?;
            for warning in &warnings {
                eprintln!("warning: {warning}");
            }
            println!("wrote {} ({} warning(s))", out.display(), warnings.len());
            Ok(())
        }
    }
}
