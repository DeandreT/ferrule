use std::ffi::OsString;
use std::io::{BufReader, BufWriter, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{Context, bail};
use clap::error::ErrorKind;
use clap::{ArgAction, Parser, Subcommand, ValueEnum};
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

#[derive(Clone, Copy, ValueEnum)]
enum CodegenLanguage {
    Rust,
    Csharp,
}

#[derive(Clone, Copy, ValueEnum)]
enum MfdExportProfile {
    FerruleExtensions,
    NativeMfd,
}

impl From<MfdExportProfile> for mfd::ExportProfile {
    fn from(profile: MfdExportProfile) -> Self {
        match profile {
            MfdExportProfile::FerruleExtensions => Self::FerruleExtensions,
            MfdExportProfile::NativeMfd => Self::NativeMfd,
        }
    }
}

#[derive(Subcommand)]
enum Command {
    /// Run a mapping project, including configured PDF input. Output supports
    /// CSV, XLSX, XML, JSON, SQLite, EDI, FlexText, and Protocol Buffers.
    /// For SQLite the table name is the schema root's name.
    Run {
        /// Project file. `ferrule run PROJECT - -` streams one payload-compatible
        /// primary input to one output artifact without temporary files.
        #[arg(
            value_name = "PROJECT",
            required_unless_present = "project",
            conflicts_with = "project"
        )]
        project_positional: Option<PathBuf>,
        #[arg(long, value_name = "PROJECT", conflicts_with = "project_positional")]
        project: Option<PathBuf>,
        /// Input instance; defaults to the project's source_path.
        #[arg(long, conflicts_with = "input_positional")]
        input: Option<PathBuf>,
        #[arg(
            value_name = "INPUT",
            allow_hyphen_values = true,
            conflicts_with = "input"
        )]
        input_positional: Option<PathBuf>,
        /// Output instance; defaults to the project's target_path.
        #[arg(long, conflicts_with = "output_positional")]
        output: Option<PathBuf>,
        #[arg(
            value_name = "OUTPUT",
            allow_hyphen_values = true,
            conflicts_with = "output"
        )]
        output_positional: Option<PathBuf>,
        /// Evaluate and write only the primary target or one named target.
        #[arg(long, value_name = "primary|NAME")]
        target: Option<String>,
        /// Write a deterministic, versioned JSON Lines execution trace.
        #[arg(long, value_name = "PATH")]
        trace_json: Option<PathBuf>,
        /// Named host scalar in NAME=VALUE form. Repeat for multiple
        /// parameters; declared mapping types apply during execution.
        #[arg(long = "param", value_name = "NAME=VALUE")]
        parameters: Vec<String>,
    },
    /// Run a typed graph of complete mapping stages and publish selected outputs.
    RunPipeline {
        #[arg(long, value_name = "PIPELINE")]
        pipeline: PathBuf,
        /// Host input name and file path. Repeat for each declared host input.
        #[arg(long = "input", action = ArgAction::Append, num_args = 2, value_names = ["NAME", "PATH"])]
        inputs: Vec<String>,
        /// Stage ID and destination for its primary target. Repeat as needed.
        #[arg(long = "output", action = ArgAction::Append, num_args = 2, value_names = ["STAGE", "PATH"])]
        outputs: Vec<String>,
        /// Stage ID, named target, and destination. Repeat as needed.
        #[arg(long = "named-output", action = ArgAction::Append, num_args = 3, value_names = ["STAGE", "TARGET", "PATH"])]
        named_outputs: Vec<String>,
        /// Named host scalar in NAME=VALUE form, shared by all stages.
        #[arg(long = "param", value_name = "NAME=VALUE")]
        parameters: Vec<String>,
    },
    /// Check project graph, scope, and schema references without reading data.
    Validate {
        #[arg(long)]
        project: PathBuf,
    },
    /// Generate a standalone Rust or C# mapping library.
    Generate {
        #[arg(long)]
        project: PathBuf,
        #[arg(long, value_enum)]
        language: CodegenLanguage,
        #[arg(long)]
        out: PathBuf,
        /// Local `codegen-runtime` crate used by generated Rust projects.
        /// Required for Rust generation until the runtime is published.
        #[arg(long)]
        rust_runtime_path: Option<PathBuf>,
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
    /// Convert an .mfd design into a Ferrule project or runnable pipeline.
    ImportMfd {
        #[arg(long)]
        mfd: PathBuf,
        #[arg(long)]
        out: PathBuf,
        /// Import a connected serial XML design as a typed pipeline.
        #[arg(long)]
        pipeline: bool,
        /// Trusted root containing the mapping and all referenced resources.
        #[arg(long, conflicts_with = "package_manifest")]
        package_root: Option<PathBuf>,
        /// Explicitly trusted portable package manifest. Its directory is the
        /// package root and its relative catalogs are searched after direct
        /// catalog flags.
        #[arg(long, value_name = "FILE", conflicts_with = "package_root")]
        package_manifest: Option<PathBuf>,
        /// Trusted EDI configuration catalog. Repeat to search multiple
        /// catalogs in declaration order after the mapping package.
        #[arg(long = "edi-catalog-root", value_name = "DIR")]
        edi_catalog_roots: Vec<PathBuf>,
        /// Trusted JSON Schema catalog. Repeat to search multiple catalogs in
        /// declaration order after the mapping package.
        #[arg(long = "json-schema-root", value_name = "DIR")]
        json_schema_catalog_roots: Vec<PathBuf>,
    },
    /// Convert a Ferrule project file into an .mfd design
    /// (generated XSDs are written next to it).
    ExportMfd {
        #[arg(long)]
        project: PathBuf,
        #[arg(long)]
        out: PathBuf,
        /// Export a typed serial XML pipeline instead of a single project.
        #[arg(long)]
        pipeline: bool,
        /// Preserve Ferrule extensions or require a native MFD export.
        #[arg(long, value_enum, default_value_t = MfdExportProfile::FerruleExtensions)]
        profile: MfdExportProfile,
        /// Inspect the exact export without creating files or directories.
        #[arg(long)]
        check: bool,
        /// Print one versioned JSON report on stdout.
        #[arg(long)]
        report_json: bool,
    },
}

impl Command {
    fn name(&self) -> &'static str {
        match self {
            Self::Run { .. } => "run",
            Self::RunPipeline { .. } => "run-pipeline",
            Self::Validate { .. } => "validate",
            Self::Generate { .. } => "generate",
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

    fn export_issue(self, issue: &mfd::ExportCompatibilityIssue, blocking: bool) {
        let severity = if blocking { "error" } else { "warning" };
        match self {
            Self::Human => {
                let component = match issue.component_uid {
                    Some(uid) => format!("{} (uid {uid})", issue.component),
                    None => issue.component.clone(),
                };
                eprintln!(
                    "{severity}: MFD compatibility in {component}: {}",
                    issue.message
                );
            }
            Self::Json => {
                eprintln!(
                    "{}",
                    json!({
                        "schema_version": 1,
                        "command": "export-mfd",
                        "severity": severity,
                        "feature": issue.feature,
                        "component": issue.component,
                        "component_uid": issue.component_uid,
                        "message": issue.message,
                    })
                );
            }
        }
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
    const COMMANDS: [&str; 9] = [
        "run",
        "run-pipeline",
        "validate",
        "generate",
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
            project_positional,
            input,
            input_positional,
            output,
            output_positional,
            target,
            trace_json,
            parameters,
        } => {
            let project = project
                .or(project_positional)
                .context("missing project path")?;
            let input = input.or(input_positional);
            let output = output.or(output_positional);
            let parameters = parse_runtime_parameters(&parameters)?;
            let trace = trace_json
                .as_deref()
                .map(cli::JsonTraceFile::create)
                .transpose()?;
            let protected_output_paths = trace
                .as_ref()
                .map(|trace| vec![trace.destination()])
                .unwrap_or_default();
            let target = target.as_deref().map(|target| {
                if target == "primary" {
                    cli::TargetSelection::Primary
                } else {
                    cli::TargetSelection::Named(target)
                }
            });
            if output.as_deref() == Some(std::path::Path::new("-")) {
                let mut stdin = BufReader::new(std::io::stdin().lock());
                let mut stdout = BufWriter::new(std::io::stdout().lock());
                let outcome = cli::run_project_with_standard_streams(
                    &project,
                    &cli::StandardIoRunOptions {
                        input_path: input.as_deref(),
                        output_path: output.as_deref(),
                        target,
                        runtime_parameters: Some(&parameters),
                        trace_sink: trace.as_ref().map(|trace| trace as &dyn cli::TraceSink),
                    },
                    &mut stdin,
                )?;
                if let Some(trace) = trace {
                    // Streamed artifacts are never published to their logical paths.
                    trace.finish(&[])?;
                }
                let artifact = outcome
                    .artifacts
                    .first()
                    .context("standard-stream run produced no output artifact")?;
                stdout
                    .write_all(&artifact.bytes)
                    .context("writing mapping artifact to stdout")?;
                stdout
                    .flush()
                    .context("flushing mapping artifact to stdout")?;
            } else {
                if input.as_deref() == Some(std::path::Path::new("-")) {
                    bail!("`--input -` requires `--output -`");
                }
                let outcome = cli::run_project_with_options(
                    &project,
                    &cli::RunOptions {
                        input_path: input.as_deref(),
                        output_path: output.as_deref(),
                        target,
                        runtime_parameters: Some(&parameters),
                        trace_sink: trace.as_ref().map(|trace| trace as &dyn cli::TraceSink),
                        debug_hook: None,
                        before_publish: None,
                        protected_output_paths: &protected_output_paths,
                    },
                )?;
                if let Some(trace) = trace {
                    trace.finish(&outcome.artifacts)?;
                }
                println!(
                    "wrote {} record(s) to {}",
                    outcome.records_written,
                    outcome.output_path.display()
                );
                for output in outcome.primary_outputs {
                    println!(
                        "wrote {} record(s) for {} to {}",
                        output.records_written,
                        output.name,
                        output.path.display()
                    );
                }
                for output in outcome.extra_outputs {
                    println!(
                        "wrote {} record(s) for {} to {}",
                        output.records_written,
                        output.name,
                        output.path.display()
                    );
                }
            }
            Ok(ExitCode::SUCCESS)
        }
        Command::RunPipeline {
            pipeline,
            inputs,
            outputs,
            named_outputs,
            parameters,
        } => {
            let inputs = inputs
                .as_chunks::<2>()
                .0
                .iter()
                .map(|pair| cli::PipelineHostFile {
                    name: pair[0].clone(),
                    path: PathBuf::from(&pair[1]),
                })
                .collect::<Vec<_>>();
            let mut publications = outputs
                .as_chunks::<2>()
                .0
                .iter()
                .map(|pair| cli::PipelineOutputFile {
                    stage: pair[0].clone(),
                    target: None,
                    path: PathBuf::from(&pair[1]),
                })
                .collect::<Vec<_>>();
            publications.extend(named_outputs.as_chunks::<3>().0.iter().map(|triple| {
                cli::PipelineOutputFile {
                    stage: triple[0].clone(),
                    target: Some(triple[1].clone()),
                    path: PathBuf::from(&triple[2]),
                }
            }));
            let parameters = parse_runtime_parameters(&parameters)?;
            let outcome = cli::run_pipeline_file_with_options(
                &pipeline,
                &inputs,
                &publications,
                &cli::PipelineRunOptions {
                    runtime_parameters: Some(&parameters),
                    ..cli::PipelineRunOptions::default()
                },
            )?;
            for artifact in &outcome.artifacts {
                let target = artifact.target.as_deref().unwrap_or("primary");
                println!(
                    "wrote {} record(s) from stage `{}` target `{target}` to {}",
                    artifact.records_written,
                    artifact.stage,
                    artifact.path.display()
                );
            }
            println!(
                "completed {} stage(s); published {} artifact(s)",
                outcome.stages_executed.len(),
                outcome.artifacts.len()
            );
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
        Command::Generate {
            project,
            language,
            out,
            rust_runtime_path,
        } => {
            let target = match language {
                CodegenLanguage::Rust => cli::GenerateTarget::Rust {
                    runtime_path: rust_runtime_path.context(
                        "--rust-runtime-path is required for --language rust until the runtime crate is published",
                    )?,
                },
                CodegenLanguage::Csharp => {
                    if rust_runtime_path.is_some() {
                        bail!("--rust-runtime-path applies only to --language rust");
                    }
                    cli::GenerateTarget::CSharp
                }
            };
            let outcome = cli::generate_project(&project, &out, target)?;
            println!(
                "generated {} file(s) in {}",
                outcome.files_written,
                outcome.output_directory.display()
            );
            Ok(ExitCode::SUCCESS)
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
        Command::ImportMfd {
            mfd,
            out,
            pipeline,
            package_root,
            package_manifest,
            edi_catalog_roots,
            json_schema_catalog_roots,
        } => {
            let import = if pipeline {
                cli::import_mfd_pipeline
            } else {
                cli::import_mfd
            };
            let warnings = import(
                &mfd,
                &out,
                package_root.as_deref(),
                package_manifest.as_deref(),
                &edi_catalog_roots,
                &json_schema_catalog_roots,
            )?;
            for warning in &warnings {
                diagnostics.warning("import-mfd", warning);
            }
            println!("wrote {} ({} warning(s))", out.display(), warnings.len());
            Ok(ExitCode::SUCCESS)
        }
        Command::ExportMfd {
            project,
            out,
            pipeline,
            profile,
            check,
            report_json,
        } => export_mfd_command(
            diagnostics,
            &project,
            &out,
            pipeline,
            profile.into(),
            check,
            report_json,
        ),
    }
}

fn export_mfd_command(
    diagnostics: DiagnosticFormat,
    project: &std::path::Path,
    out: &std::path::Path,
    pipeline: bool,
    profile: mfd::ExportProfile,
    check: bool,
    report_json: bool,
) -> anyhow::Result<ExitCode> {
    let result = if pipeline && check {
        cli::preflight_mfd_pipeline_export(project, out)
    } else if pipeline {
        cli::export_mfd_pipeline_with_profile(project, out, profile)
    } else if check {
        cli::preflight_mfd_export(project, out)
    } else {
        cli::export_mfd_with_profile(project, out, profile)
    };
    let report = match result {
        Ok(report) => report,
        Err(error) => match error.downcast_ref::<mfd::MfdError>() {
            Some(mfd::MfdError::IncompatibleExport(report)) => (**report).clone(),
            _ => return Err(error),
        },
    };
    let accepted = profile != mfd::ExportProfile::NativeMfd || report.is_native_compatible();
    let blocking = !accepted;
    for issue in &report.issues {
        diagnostics.export_issue(issue, blocking);
    }
    for warning in &report.warnings {
        if blocking {
            diagnostics.emit("export-mfd", "error", None, warning);
        } else {
            diagnostics.warning("export-mfd", warning);
        }
    }
    if report_json {
        let output = json!({
            "schema_version": 1,
            "command": "export-mfd",
            "profile": profile,
            "mode": if check { "check" } else { "export" },
            "accepted": accepted,
            "report": report,
        });
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else if check {
        println!(
            "{}: {} ({} compatibility issue(s), {} export warning(s))",
            out.display(),
            export_compatibility_label(report.compatibility),
            report.issues.len(),
            report.warnings.len()
        );
    } else if accepted {
        println!(
            "wrote {} ({} warning(s)); MFD compatibility: {}",
            out.display(),
            report.warnings.len(),
            export_compatibility_label(report.compatibility)
        );
    }
    Ok(if accepted {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}

fn export_compatibility_label(compatibility: mfd::ExportCompatibility) -> &'static str {
    match compatibility {
        mfd::ExportCompatibility::NativeMfd => "native MFD",
        mfd::ExportCompatibility::FerruleExtensions => "Ferrule extensions required",
        mfd::ExportCompatibility::Incomplete => "incomplete",
    }
}

fn parse_runtime_parameters(values: &[String]) -> anyhow::Result<engine::RuntimeParameters> {
    let mut parameters = engine::RuntimeParameters::new();
    for value in values {
        let (name, value) = value
            .split_once('=')
            .with_context(|| format!("runtime parameter `{value}` must use NAME=VALUE"))?;
        parameters
            .insert(name, ir::Value::String(value.to_string()))
            .with_context(|| format!("invalid runtime parameter `{name}`"))?;
    }
    Ok(parameters)
}
