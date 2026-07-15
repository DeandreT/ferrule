use std::ffi::OsString;
use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::bail;
use clap::error::ErrorKind;
use clap::{Parser, Subcommand, ValueEnum};
use serde_json::json;

#[derive(Parser)]
#[command(name = "ferrule", version, about = "Run ferrule data mapping projects")]
struct Cli {
    /// Select human-readable or JSON Lines diagnostics on stderr.
    #[arg(long, global = true, value_enum, default_value_t = DiagnosticFormat::Human)]
    diagnostics: DiagnosticFormat,

    #[command(subcommand)]
    command: Command,
}

#[derive(Clone, Copy, ValueEnum)]
enum DiagnosticFormat {
    Human,
    Json,
}

#[derive(Subcommand)]
enum Command {
    /// Run a mapping project, including configured PDF input. Output supports
    /// CSV, XLSX, XML, JSON, SQLite, EDI, FlexText, and Protocol Buffers.
    /// For SQLite the table name is the schema root's name.
    Run {
        #[arg(long)]
        project: PathBuf,
        /// Input instance; defaults to the project's source_path.
        #[arg(long)]
        input: Option<PathBuf>,
        /// Output instance; defaults to the project's target_path.
        #[arg(long)]
        output: Option<PathBuf>,
    },
    /// Check project graph, scope, and schema references without reading data.
    Validate {
        #[arg(long)]
        project: PathBuf,
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

impl Command {
    fn name(&self) -> &'static str {
        match self {
            Self::Run { .. } => "run",
            Self::Validate { .. } => "validate",
            Self::ImportXsd { .. } => "import-xsd",
            Self::ImportJsonSchema { .. } => "import-json-schema",
            Self::ImportDb { .. } => "import-db",
            Self::ImportMfd { .. } => "import-mfd",
            Self::ExportMfd { .. } => "export-mfd",
        }
    }
}

impl DiagnosticFormat {
    fn warning(self, command: &str, message: &str) {
        self.emit(command, "warning", None, message);
    }

    fn validation_error(self, command: &str, issue: &engine::ValidationIssue) {
        self.emit(command, "error", Some(&issue.location), &issue.message);
    }

    fn error(self, command: &str, error: &anyhow::Error) {
        match self {
            Self::Human => eprintln!("Error: {error:?}"),
            Self::Json => {
                let message = format!("{error:#}");
                self.emit(command, "error", None, &message);
            }
        }
    }

    fn emit(self, command: &str, severity: &str, location: Option<&str>, message: &str) {
        match self {
            Self::Human => match severity {
                "warning" => eprintln!("warning: {message}"),
                _ => match location {
                    Some(location) => eprintln!("error: {location}: {message}"),
                    None => eprintln!("Error: {message}"),
                },
            },
            Self::Json => {
                let mut diagnostic = json!({
                    "schema_version": 1,
                    "command": command,
                    "severity": severity,
                    "message": message,
                });
                if let Some(location) = location {
                    diagnostic["location"] = json!(location);
                }
                eprintln!("{diagnostic}");
            }
        }
    }
}

fn main() -> ExitCode {
    let args = std::env::args_os().collect::<Vec<_>>();
    let json_diagnostics = json_diagnostics_requested(&args);
    let cli = match Cli::try_parse_from(&args) {
        Ok(cli) => cli,
        Err(error)
            if json_diagnostics
                && !matches!(
                    error.kind(),
                    ErrorKind::DisplayHelp | ErrorKind::DisplayVersion
                ) =>
        {
            let exit_code = ExitCode::from(error.exit_code() as u8);
            let command = command_name_from_args(&args).unwrap_or("cli");
            DiagnosticFormat::Json.emit(command, "error", None, &error.to_string());
            return exit_code;
        }
        Err(error) => error.exit(),
    };
    let command = cli.command.name();
    let diagnostics = cli.diagnostics;
    match execute(cli) {
        Ok(exit_code) => exit_code,
        Err(error) => {
            diagnostics.error(command, &error);
            ExitCode::FAILURE
        }
    }
}

fn json_diagnostics_requested(args: &[OsString]) -> bool {
    args.iter().any(|arg| arg == "--diagnostics=json")
        || args
            .windows(2)
            .any(|pair| pair[0] == "--diagnostics" && pair[1] == "json")
}

fn command_name_from_args(args: &[OsString]) -> Option<&'static str> {
    const COMMANDS: [&str; 7] = [
        "run",
        "validate",
        "import-xsd",
        "import-json-schema",
        "import-db",
        "import-mfd",
        "export-mfd",
    ];
    let mut args = args.iter().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--diagnostics" {
            args.next();
            continue;
        }
        if arg
            .to_str()
            .is_some_and(|arg| arg.starts_with("--diagnostics="))
        {
            continue;
        }
        if arg.to_str().is_some_and(|arg| arg.starts_with('-')) {
            continue;
        }
        return COMMANDS.into_iter().find(|command| arg == command);
    }
    None
}

fn execute(cli: Cli) -> anyhow::Result<ExitCode> {
    let diagnostics = cli.diagnostics;
    match cli.command {
        Command::Run {
            project,
            input,
            output,
        } => {
            let outcome =
                cli::run_project_with_paths(&project, input.as_deref(), output.as_deref())?;
            println!(
                "wrote {} record(s) to {}",
                outcome.records_written,
                outcome.output_path.display()
            );
            for output in outcome.extra_outputs {
                println!(
                    "wrote {} record(s) for {} to {}",
                    output.records_written,
                    output.name,
                    output.path.display()
                );
            }
            Ok(ExitCode::SUCCESS)
        }
        Command::Validate { project } => {
            let issues = cli::validate_project(&project)?;
            if issues.is_empty() {
                println!("{} is valid", project.display());
                return Ok(ExitCode::SUCCESS);
            }
            for issue in &issues {
                diagnostics.validation_error("validate", issue);
            }
            if matches!(diagnostics, DiagnosticFormat::Json) {
                return Ok(ExitCode::FAILURE);
            }
            bail!("project has {} validation issue(s)", issues.len())
        }
        Command::ImportXsd { xsd } => {
            println!("{}", cli::import_xsd(&xsd)?);
            Ok(ExitCode::SUCCESS)
        }
        Command::ImportJsonSchema { schema } => {
            println!("{}", cli::import_json_schema(&schema)?);
            Ok(ExitCode::SUCCESS)
        }
        Command::ImportDb { db, table } => {
            println!("{}", cli::import_db(&db, &table)?);
            Ok(ExitCode::SUCCESS)
        }
        Command::ImportMfd { mfd, out } => {
            let warnings = cli::import_mfd(&mfd, &out)?;
            for warning in &warnings {
                diagnostics.warning("import-mfd", warning);
            }
            println!("wrote {} ({} warning(s))", out.display(), warnings.len());
            Ok(ExitCode::SUCCESS)
        }
        Command::ExportMfd { project, out } => {
            let warnings = cli::export_mfd(&project, &out)?;
            for warning in &warnings {
                diagnostics.warning("export-mfd", warning);
            }
            println!("wrote {} ({} warning(s))", out.display(), warnings.len());
            Ok(ExitCode::SUCCESS)
        }
    }
}
