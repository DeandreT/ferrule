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
enum CodegenX12EnvelopeProfile {
    SingleTransaction,
    GroupedTransactions,
    #[value(name = "mapforce-interchanges")]
    MapForceInterchanges,
}

impl From<CodegenX12EnvelopeProfile> for codegen::X12EnvelopeProfile {
    fn from(profile: CodegenX12EnvelopeProfile) -> Self {
        match profile {
            CodegenX12EnvelopeProfile::SingleTransaction => Self::SingleTransaction,
            CodegenX12EnvelopeProfile::GroupedTransactions => Self::GroupedTransactions,
            CodegenX12EnvelopeProfile::MapForceInterchanges => Self::MapForceInterchanges,
        }
    }
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
    /// Export a project and explicitly declared local resources as an offline bundle.
    BundleProject {
        /// Explicit source root. Every selected file path is relative to this directory.
        #[arg(long)]
        root: PathBuf,
        #[arg(long, value_name = "RELATIVE_PROJECT")]
        project: String,
        /// One resource file relative to the source root; repeat as needed.
        #[arg(long = "resource", value_name = "RELATIVE_FILE")]
        resources: Vec<String>,
        #[arg(long = "edi-catalog", value_name = "RELATIVE_DIRECTORY")]
        edi_catalogs: Vec<String>,
        #[arg(long = "json-schema-catalog", value_name = "RELATIVE_DIRECTORY")]
        json_schema_catalogs: Vec<String>,
        /// Include configured static local input files. Instance bytes are omitted by default.
        #[arg(long)]
        include_inputs: bool,
        /// A directory that does not already exist.
        #[arg(long)]
        out: PathBuf,
    },
    /// Fetch one explicitly approved JSON response and map it to stdout.
    RunRestJson {
        #[arg(long, value_name = "PROJECT")]
        project: PathBuf,
        /// Host-owned JSON request description, separate from the project.
        #[arg(long, value_name = "PATH")]
        request: PathBuf,
        /// Host-owned header values; credentials never belong in argv or projects.
        #[arg(long, value_name = "PATH")]
        header_values: Option<PathBuf>,
        /// Grant one live request for this invocation. Default is denied.
        #[arg(long)]
        allow_live_rest: bool,
        /// Permit cleartext HTTP explicitly; use HTTPS for protected data.
        #[arg(long, requires = "allow_live_rest")]
        allow_insecure_http: bool,
        #[arg(long, value_name = "LOGICAL_JSON_PATH")]
        response_identity: PathBuf,
        #[arg(long, value_name = "LOGICAL_JSON_PATH")]
        output_identity: PathBuf,
        #[arg(long = "param", value_name = "NAME=VALUE")]
        parameters: Vec<String>,
    },
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
        /// Extract configured filesystem PDF sources in the packaged Linux worker.
        #[arg(long)]
        pdf_worker: bool,
        /// Retain closed successful PDF worker evidence folders.
        #[arg(long, requires = "pdf_worker")]
        keep_pdf_evidence: bool,
        /// Write a deterministic, versioned JSON Lines execution trace.
        #[arg(long, value_name = "PATH")]
        trace_json: Option<PathBuf>,
        /// Named host scalar in NAME=VALUE form. Repeat for multiple
        /// parameters; declared mapping types apply during execution.
        #[arg(long = "param", value_name = "NAME=VALUE")]
        parameters: Vec<String>,
        /// Run under a restricted policy file: allowed roots, HTTP origins and
        /// explicit budgets. Policy enforcement, not a sandbox. Default: trusted.
        #[arg(long, value_name = "FILE")]
        restricted_policy: Option<PathBuf>,
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
        /// Run every stage under a restricted policy file. Default: trusted.
        #[arg(long, value_name = "FILE")]
        restricted_policy: Option<PathBuf>,
    },
    /// Check project graph, scope, and schema references without reading data.
    Validate {
        #[arg(long)]
        project: PathBuf,
    },
    /// Generate a standalone Rust or C# mapping library.
    #[command(group(
        clap::ArgGroup::new("x12_envelope_host")
            .args(["x12_adapters", "static_document_adapters"])
            .multiple(true)
    ))]
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
        /// Include bounded CSV output methods for a flat primary target.
        #[arg(long)]
        csv_output: bool,
        /// Include singular JSON5 text and UTF-8 byte companion methods.
        #[arg(long, conflicts_with = "csv_output")]
        json5_adapters: bool,
        /// Include supported raw X12 text and strict UTF-8 document methods.
        #[arg(long, conflicts_with_all = ["csv_output", "json5_adapters"])]
        x12_adapters: bool,
        /// Explicit ownership grammar for the primary X12 source. Requires
        /// --x12-adapters or --static-document-adapters.
        #[arg(long, value_enum, requires = "x12_envelope_host")]
        x12_source_envelope_profile: Option<CodegenX12EnvelopeProfile>,
        /// Explicit ownership grammar for the primary X12 target. Requires
        /// --x12-adapters or --static-document-adapters.
        #[arg(long, value_enum, requires = "x12_envelope_host")]
        x12_target_envelope_profile: Option<CodegenX12EnvelopeProfile>,
        /// Include static JSON/X12 inputs and explicitly selected C# document outputs.
        #[arg(long, conflicts_with_all = ["csv_output", "json5_adapters", "x12_adapters"])]
        static_document_adapters: bool,
        /// Include all-row flat CSV input and singular X12 C# output methods.
        #[arg(long, conflicts_with_all = ["csv_output", "json5_adapters", "x12_adapters", "static_document_adapters"])]
        csv_x12_adapters: bool,
        /// Explicit ownership grammar for the CSV companion's X12 target.
        #[arg(long, value_enum, requires = "csv_x12_adapters")]
        csv_x12_target_envelope_profile: Option<CodegenX12EnvelopeProfile>,
        /// Include full-row CSV input and explicitly selected strict JSON C# outputs.
        #[arg(long, conflicts_with_all = ["csv_output", "json5_adapters", "x12_adapters", "static_document_adapters", "csv_x12_adapters"])]
        csv_json_adapters: bool,
        /// Include complete single-document CSV input/output methods in Rust or C#.
        #[arg(long, conflicts_with_all = ["csv_output", "json5_adapters", "x12_adapters", "static_document_adapters", "csv_x12_adapters", "csv_json_adapters"])]
        csv_document_adapters: bool,
        /// Write unescaped target fields and CRLF records; target quoting must be disabled.
        #[arg(long, requires = "csv_document_adapters")]
        raw_delimited_crlf: bool,
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
        /// Refuse incomplete single-project imports before writing the project.
        #[arg(long, conflicts_with = "pipeline")]
        require_executable: bool,
        /// Preserve supported row error order; refuse unsupported exception shapes.
        #[arg(long)]
        item_ordered_exceptions: bool,
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
            Self::BundleProject { .. } => "bundle-project",
            Self::RunRestJson { .. } => "run-rest-json",
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
                "info" => eprintln!("{message}"),
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
    const COMMANDS: [&str; 11] = [
        "bundle-project",
        "run",
        "run-rest-json",
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
        Command::BundleProject {
            root,
            project,
            resources,
            edi_catalogs,
            json_schema_catalogs,
            include_inputs,
            out,
        } => {
            let catalogs = edi_catalogs
                .into_iter()
                .map(|root| cli::BundleCatalog {
                    kind: cli::BundleCatalogKind::EdiConfig,
                    root,
                })
                .chain(
                    json_schema_catalogs
                        .into_iter()
                        .map(|root| cli::BundleCatalog {
                            kind: cli::BundleCatalogKind::JsonSchema,
                            root,
                        }),
                )
                .collect();
            let outcome = cli::bundle_project(
                &root,
                &out,
                &cli::BundleOptions {
                    project,
                    resources,
                    catalogs,
                    include_inputs,
                },
            )?;
            println!(
                "wrote {} file(s), including {} input file(s), to {}",
                outcome.files_written,
                outcome.input_files,
                outcome.output_directory.display()
            );
            Ok(ExitCode::SUCCESS)
        }
        Command::RunRestJson {
            project,
            request,
            header_values,
            allow_live_rest,
            allow_insecure_http,
            response_identity,
            output_identity,
            parameters,
        } => {
            let policy = if allow_live_rest {
                cli::RestExecutionPolicy::AllowSingleRequest {
                    allow_insecure_http,
                }
            } else {
                cli::RestExecutionPolicy::Deny
            };
            // Denial precedes request-file reads and parameter diagnostics.
            if policy == cli::RestExecutionPolicy::Deny {
                return Err(cli::RestJsonError::Disabled.into());
            }
            let parameters = parse_runtime_parameters(&parameters)?;
            let mut options =
                cli::RestJsonMappingOptions::new(&response_identity, &output_identity);
            options.runtime_parameters = Some(&parameters);
            let outcome = cli::run_project_rest_json_request_file_payloads(
                &project,
                &request,
                header_values.as_deref(),
                policy,
                &options,
            )?;
            let artifact = outcome
                .artifacts
                .first()
                .context("REST mapping returned no artifact")?;
            let mut stdout = std::io::stdout().lock();
            stdout.write_all(&artifact.bytes)?;
            stdout.flush()?;
            Ok(ExitCode::SUCCESS)
        }
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
            pdf_worker,
            keep_pdf_evidence,
            restricted_policy,
        } => {
            let project = project
                .or(project_positional)
                .context("missing project path")?;
            let input = input.or(input_positional);
            let output = output.or(output_positional);
            // Route arguments are refused before the policy, design, or any
            // input is read and before the trace stage is created.
            let route = cli::select_run_route(&cli::RunRouteArgs {
                input_path: input.as_deref(),
                output_path: output.as_deref(),
                pdf_worker,
                trace_path: trace_json.as_deref(),
            })?;
            let execution_policy = restricted_policy
                .as_deref()
                .map(cli::load_execution_policy_file)
                .transpose()?;
            let parameters = parse_runtime_parameters(&parameters)?;
            let protected_output_paths = trace_json.as_deref().into_iter().collect::<Vec<_>>();
            let target = target.as_deref().map(|target| {
                if target == "primary" {
                    cli::TargetSelection::Primary
                } else {
                    cli::TargetSelection::Named(target)
                }
            });
            let design = cli::load_project(&project)?;
            let request = cli::RunRequest {
                input_path: input.as_deref(),
                output_path: output.as_deref(),
                target,
                runtime_parameters: Some(&parameters),
                protected_output_paths: &protected_output_paths,
                execution_policy: execution_policy.as_ref(),
                observers: cli::SourceOwnershipObservers {
                    tracing: trace_json.is_some(),
                    debugging: false,
                },
            };
            if route == cli::HostRoute::StandardStreams {
                let prepared = cli::PreparedStreamRun::prepare(&design, &project, request)?;
                let trace = trace_json
                    .as_deref()
                    .map(cli::JsonTraceFile::create)
                    .transpose()?;
                let mut stdin = BufReader::new(std::io::stdin().lock());
                let mut stdout = BufWriter::new(std::io::stdout().lock());
                let outcome = prepared.execute(
                    &mut stdin,
                    trace.as_ref().map(|trace| trace as &dyn cli::TraceSink),
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
                let prepared = cli::PreparedFileRun::prepare(&design, &project, request)?;
                let trace = trace_json
                    .as_deref()
                    .map(cli::JsonTraceFile::create)
                    .transpose()?;
                let observers = cli::RunObservers {
                    trace_sink: trace.as_ref().map(|trace| trace as &dyn cli::TraceSink),
                    ..cli::RunObservers::default()
                };
                let outcome = if pdf_worker {
                    let policy = if keep_pdf_evidence {
                        cli::FilesystemRunPolicy::with_retained_pdf_evidence()
                    } else {
                        cli::FilesystemRunPolicy::new()
                    };
                    let result = prepared.execute_with_pdf_reader(&observers, &policy);
                    for path in policy.retained_pdf_evidence() {
                        diagnostics.emit(
                            "run",
                            "info",
                            None,
                            &format!(
                                "successful PDF worker evidence retained at {}",
                                path.display()
                            ),
                        );
                    }
                    result?
                } else {
                    prepared.execute(&observers)?
                };
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
            restricted_policy,
        } => {
            let execution_policy = restricted_policy
                .as_deref()
                .map(cli::load_execution_policy_file)
                .transpose()?;
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
                    execution_policy: execution_policy.as_ref(),
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
            csv_output,
            json5_adapters,
            x12_adapters,
            x12_source_envelope_profile,
            x12_target_envelope_profile,
            static_document_adapters,
            csv_x12_adapters,
            csv_x12_target_envelope_profile,
            csv_json_adapters,
            csv_document_adapters,
            raw_delimited_crlf,
        } => {
            if csv_json_adapters && matches!(language, CodegenLanguage::Rust) {
                bail!("--csv-json-adapters requires --language csharp");
            }
            if csv_x12_adapters && matches!(language, CodegenLanguage::Rust) {
                bail!("--csv-x12-adapters requires --language csharp");
            }
            if static_document_adapters && matches!(language, CodegenLanguage::Rust) {
                bail!("--static-document-adapters currently requires --language csharp");
            }
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
            let outcome = if csv_document_adapters {
                if raw_delimited_crlf {
                    cli::generate_project_with_raw_delimited_document_adapters(
                        &project, &out, target,
                    )?
                } else {
                    cli::generate_project_with_csv_document_adapters(&project, &out, target)?
                }
            } else if csv_json_adapters {
                cli::generate_project_with_csv_json_adapters(&project, &out, target)?
            } else if csv_x12_adapters {
                cli::generate_project_with_csv_x12_envelope_profile(
                    &project,
                    &out,
                    target,
                    csv_x12_target_envelope_profile.map(Into::into),
                )?
            } else if static_document_adapters {
                cli::generate_project_with_static_document_envelope_profiles(
                    &project,
                    &out,
                    target,
                    x12_source_envelope_profile.map(Into::into),
                    x12_target_envelope_profile.map(Into::into),
                )?
            } else if x12_adapters {
                cli::generate_project_with_x12_envelope_profiles(
                    &project,
                    &out,
                    target,
                    x12_source_envelope_profile.map(Into::into),
                    x12_target_envelope_profile.map(Into::into),
                )?
            } else if json5_adapters {
                cli::generate_project_with_json5_adapters(&project, &out, target)?
            } else if csv_output {
                cli::generate_project_with_csv_output(&project, &out, target)?
            } else {
                cli::generate_project(&project, &out, target)?
            };
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
            require_executable,
            item_ordered_exceptions,
            package_root,
            package_manifest,
            edi_catalog_roots,
            json_schema_catalog_roots,
        } => {
            let warnings = if require_executable && !item_ordered_exceptions {
                cli::import_mfd_with_profile(
                    &mfd,
                    &out,
                    package_root.as_deref(),
                    package_manifest.as_deref(),
                    &edi_catalog_roots,
                    &json_schema_catalog_roots,
                    mfd::ImportProfile::Executable,
                )?
            } else {
                let import = if pipeline {
                    cli::import_mfd_pipeline_with_exception_order
                } else {
                    cli::import_mfd_with_exception_order
                };
                import(
                    &mfd,
                    &out,
                    package_root.as_deref(),
                    package_manifest.as_deref(),
                    &edi_catalog_roots,
                    &json_schema_catalog_roots,
                    item_ordered_exceptions,
                )?
            };
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

#[cfg(test)]
mod x12_envelope_flag_tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn explicit_envelope_flags_require_the_x12_companion_and_known_values() {
        for flag in [
            "--x12-source-envelope-profile",
            "--x12-target-envelope-profile",
        ] {
            let base = [
                "ferrule",
                "generate",
                "--project",
                "unused.json",
                "--language",
                "csharp",
                "--out",
                "unused-output",
            ];
            let mut absent = base.to_vec();
            absent.extend([flag, "grouped-transactions"]);
            let result = Cli::try_parse_from(&absent);
            eprintln!(
                "arguments={absent:?}; original outcome={:?}",
                result.as_ref().err()
            );
            assert_eq!(
                result.err().unwrap().kind(),
                ErrorKind::MissingRequiredArgument
            );
            for value in [
                "single-transaction",
                "grouped-transactions",
                "mapforce-interchanges",
            ] {
                let mut accepted = base.to_vec();
                accepted.extend(["--x12-adapters", flag, value]);
                let result = Cli::try_parse_from(&accepted);
                eprintln!(
                    "arguments={accepted:?}; original outcome={:?}",
                    result.as_ref().err()
                );
                assert!(result.is_ok());
            }
            let mut invalid = base.to_vec();
            invalid.extend(["--x12-adapters", flag, "automatic"]);
            let result = Cli::try_parse_from(&invalid);
            eprintln!(
                "arguments={invalid:?}; original outcome={:?}",
                result.as_ref().err()
            );
            assert_eq!(result.err().unwrap().kind(), ErrorKind::InvalidValue);
        }
    }

    #[test]
    fn mapforce_envelope_value_selects_mapforce_interchanges_on_each_side() {
        for (flag, source) in [
            ("--x12-source-envelope-profile", true),
            ("--x12-target-envelope-profile", false),
        ] {
            let arguments = [
                "ferrule",
                "generate",
                "--project",
                "unused.json",
                "--language",
                "csharp",
                "--out",
                "unused-output",
                "--x12-adapters",
                flag,
                "mapforce-interchanges",
            ];
            let parsed = Cli::try_parse_from(arguments);
            eprintln!(
                "arguments={arguments:?}; original outcome={:?}",
                parsed.as_ref().err()
            );
            let Command::Generate {
                x12_source_envelope_profile,
                x12_target_envelope_profile,
                ..
            } = parsed.unwrap().command
            else {
                panic!("generate subcommand");
            };
            let (selected, other) = if source {
                (x12_source_envelope_profile, x12_target_envelope_profile)
            } else {
                (x12_target_envelope_profile, x12_source_envelope_profile)
            };
            assert_eq!(
                selected.map(codegen::X12EnvelopeProfile::from),
                Some(codegen::X12EnvelopeProfile::MapForceInterchanges)
            );
            assert!(other.is_none());
            let mut misspelled = arguments;
            misspelled[10] = "map-force-interchanges";
            let result = Cli::try_parse_from(misspelled);
            assert_eq!(result.err().unwrap().kind(), ErrorKind::InvalidValue);
        }
    }

    #[test]
    fn rust_grouped_flag_requires_an_explicit_runtime_path() {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "ferrule-grouped-x12-flag-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&root).unwrap();
        let output = root.join("absent-parent/output");
        let args = vec![
            OsString::from("ferrule"),
            OsString::from("generate"),
            OsString::from("--project"),
            root.join("absent-project.json").into_os_string(),
            OsString::from("--language"),
            OsString::from("rust"),
            OsString::from("--out"),
            output.clone().into_os_string(),
            OsString::from("--x12-adapters"),
            OsString::from("--x12-source-envelope-profile"),
            OsString::from("grouped-transactions"),
        ];
        std::fs::write(root.join("ARGUMENTS.original.txt"), format!("{args:#?}")).unwrap();
        let result = execute(Cli::try_parse_from(args).unwrap());
        std::fs::write(root.join("OUTCOME.original.txt"), format!("{result:#?}")).unwrap();
        assert_eq!(
            result.unwrap_err().to_string(),
            "--rust-runtime-path is required for --language rust until the runtime crate is published"
        );
        assert!(!output.parent().unwrap().exists());
    }
}

#[cfg(test)]
mod static_document_flag_tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn static_document_flag_conflicts_with_other_optional_profiles() {
        for option in ["--csv-output", "--json5-adapters", "--x12-adapters"] {
            let args = [
                "ferrule",
                "generate",
                "--project",
                "unused.json",
                "--language",
                "csharp",
                "--out",
                "unused-output",
                "--static-document-adapters",
                option,
            ];
            let result = Cli::try_parse_from(args);
            let error = result
                .err()
                .expect("mutually exclusive optional profiles were admitted");
            eprintln!("arguments={args:?}; original clap error={error}");
            assert_eq!(error.kind(), ErrorKind::ArgumentConflict);
        }
    }

    #[test]
    fn envelope_profiles_require_an_x12_or_static_document_adapter() {
        let base = [
            "ferrule",
            "generate",
            "--project",
            "unused.json",
            "--language",
            "csharp",
            "--out",
            "unused-output",
            "--x12-target-envelope-profile",
            "mapforce-interchanges",
        ];
        let result = Cli::try_parse_from(base);
        assert_eq!(
            result.err().unwrap().kind(),
            ErrorKind::MissingRequiredArgument
        );
        let mut static_documents = base.to_vec();
        static_documents.push("--static-document-adapters");
        let Command::Generate {
            x12_target_envelope_profile,
            static_document_adapters,
            ..
        } = Cli::try_parse_from(static_documents).unwrap().command
        else {
            panic!("generate subcommand");
        };
        assert!(static_document_adapters);
        assert_eq!(
            x12_target_envelope_profile.map(codegen::X12EnvelopeProfile::from),
            Some(codegen::X12EnvelopeProfile::MapForceInterchanges)
        );
    }

    #[test]
    fn static_document_flag_backend_refusal_precedes_project_and_runtime_loading() {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "ferrule-static-document-flag-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&root).unwrap();
        let input = root.join("absent-project.json");
        let output = root.join("absent-parent/output");
        let args = vec![
            OsString::from("ferrule"),
            OsString::from("generate"),
            OsString::from("--project"),
            input.into_os_string(),
            OsString::from("--language"),
            OsString::from("rust"),
            OsString::from("--out"),
            output.clone().into_os_string(),
            OsString::from("--static-document-adapters"),
        ];
        std::fs::write(root.join("ARGUMENTS.original.txt"), format!("{args:#?}")).unwrap();
        let result = execute(Cli::try_parse_from(args).unwrap());
        std::fs::write(root.join("OUTCOME.original.txt"), format!("{result:#?}")).unwrap();
        assert_eq!(
            result.unwrap_err().to_string(),
            "--static-document-adapters currently requires --language csharp"
        );
        assert!(!output.parent().unwrap().exists());
    }
}

#[cfg(test)]
mod csv_x12_flag_tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn csv_x12_target_profile_is_explicit_required_and_value_checked() {
        let base = [
            "ferrule",
            "generate",
            "--project",
            "absent.json",
            "--language",
            "csharp",
            "--out",
            "absent-output",
        ];
        for (value, wanted) in [
            (
                "single-transaction",
                codegen::X12EnvelopeProfile::SingleTransaction,
            ),
            (
                "grouped-transactions",
                codegen::X12EnvelopeProfile::GroupedTransactions,
            ),
        ] {
            let mut missing = base.to_vec();
            missing.extend(["--csv-x12-target-envelope-profile", value]);
            assert_eq!(
                Cli::try_parse_from(&missing)
                    .err()
                    .expect("required CSV companion")
                    .kind(),
                ErrorKind::MissingRequiredArgument
            );
            let mut selected = base.to_vec();
            selected.extend([
                "--csv-x12-adapters",
                "--csv-x12-target-envelope-profile",
                value,
            ]);
            let parsed = Cli::try_parse_from(selected).unwrap();
            let Command::Generate {
                csv_x12_target_envelope_profile,
                ..
            } = parsed.command
            else {
                panic!("generate");
            };
            assert_eq!(
                csv_x12_target_envelope_profile.map(codegen::X12EnvelopeProfile::from),
                Some(wanted)
            );
        }
        let mut absent = base.to_vec();
        absent.push("--csv-x12-adapters");
        let parsed = Cli::try_parse_from(absent).unwrap();
        let Command::Generate {
            csv_x12_target_envelope_profile,
            ..
        } = parsed.command
        else {
            panic!("generate");
        };
        assert!(csv_x12_target_envelope_profile.is_none());
        let mut invalid = base.to_vec();
        invalid.extend([
            "--csv-x12-adapters",
            "--csv-x12-target-envelope-profile",
            "automatic",
        ]);
        assert_eq!(
            Cli::try_parse_from(invalid)
                .err()
                .expect("invalid profile")
                .kind(),
            ErrorKind::InvalidValue
        );
    }

    #[test]
    fn csv_x12_grouped_target_flag_keeps_early_backend_refusal() {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "ferrule-csv-x12-explicit-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&root).unwrap();
        let output = root.join("absent-parent/output");
        let args = vec![
            OsString::from("ferrule"),
            OsString::from("generate"),
            OsString::from("--project"),
            root.join("absent-project.json").into_os_string(),
            OsString::from("--language"),
            OsString::from("rust"),
            OsString::from("--out"),
            output.clone().into_os_string(),
            OsString::from("--csv-x12-adapters"),
            OsString::from("--csv-x12-target-envelope-profile"),
            OsString::from("grouped-transactions"),
        ];
        let result = execute(Cli::try_parse_from(args).unwrap());
        assert_eq!(
            result.unwrap_err().to_string(),
            "--csv-x12-adapters requires --language csharp"
        );
        assert!(!output.parent().unwrap().exists());
    }

    #[test]
    fn csv_x12_flag_conflicts_before_project_loading() {
        for option in [
            "--csv-output",
            "--json5-adapters",
            "--x12-adapters",
            "--static-document-adapters",
        ] {
            let args = [
                "ferrule",
                "generate",
                "--project",
                "absent.json",
                "--language",
                "csharp",
                "--out",
                "absent-output",
                "--csv-x12-adapters",
                option,
            ];
            let result = Cli::try_parse_from(args);
            let error = result
                .err()
                .expect("mutually exclusive adapters were admitted");
            eprintln!("arguments={args:?}; full original clap error={error}");
            assert_eq!(error.kind(), ErrorKind::ArgumentConflict);
        }
    }

    #[test]
    fn csv_x12_flag_backend_refusal_precedes_project_and_runtime_loading() {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "ferrule-csv-x12-flag-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&root).unwrap();
        let output = root.join("absent-parent/output");
        let args = vec![
            OsString::from("ferrule"),
            OsString::from("generate"),
            OsString::from("--project"),
            root.join("absent-project.json").into_os_string(),
            OsString::from("--language"),
            OsString::from("rust"),
            OsString::from("--out"),
            output.clone().into_os_string(),
            OsString::from("--csv-x12-adapters"),
        ];
        std::fs::write(root.join("ARGUMENTS.original.txt"), format!("{args:#?}")).unwrap();
        let result = execute(Cli::try_parse_from(args).unwrap());
        std::fs::write(root.join("OUTCOME.original.txt"), format!("{result:#?}")).unwrap();
        assert_eq!(
            result.unwrap_err().to_string(),
            "--csv-x12-adapters requires --language csharp"
        );
        assert!(!output.parent().unwrap().exists());
    }
}

#[cfg(test)]
mod csv_json_flag_tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn csv_json_flag_conflicts_before_project_loading() {
        for option in [
            "--csv-output",
            "--json5-adapters",
            "--x12-adapters",
            "--static-document-adapters",
            "--csv-x12-adapters",
        ] {
            let args = [
                "ferrule",
                "generate",
                "--project",
                "absent.json",
                "--language",
                "csharp",
                "--out",
                "absent-output",
                "--csv-json-adapters",
                option,
            ];
            let result = Cli::try_parse_from(args);
            let error = result
                .err()
                .expect("mutually exclusive profiles were admitted");
            eprintln!("arguments={args:?}; full original clap error={error}");
            assert_eq!(error.kind(), ErrorKind::ArgumentConflict);
        }
    }

    #[test]
    fn csv_json_flag_backend_refusal_precedes_loading() {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "ferrule-csv-json-flag-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&root).unwrap();
        let output = root.join("absent-parent/output");
        let args = vec![
            OsString::from("ferrule"),
            OsString::from("generate"),
            OsString::from("--project"),
            root.join("absent-project.json").into_os_string(),
            OsString::from("--language"),
            OsString::from("rust"),
            OsString::from("--out"),
            output.clone().into_os_string(),
            OsString::from("--csv-json-adapters"),
        ];
        std::fs::write(root.join("ARGUMENTS.original.txt"), format!("{args:#?}")).unwrap();
        let result = execute(Cli::try_parse_from(args).unwrap());
        std::fs::write(root.join("OUTCOME.original.txt"), format!("{result:#?}")).unwrap();
        assert_eq!(
            result.unwrap_err().to_string(),
            "--csv-json-adapters requires --language csharp"
        );
        assert!(!output.parent().unwrap().exists());
    }
}

#[cfg(test)]
mod pdf_evidence_flag_tests {
    use super::*;
    use std::io::Write;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn retain(path: &std::path::Path, bytes: &[u8]) {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .unwrap();
        file.write_all(bytes).unwrap();
        file.sync_all().unwrap();
    }

    #[test]
    fn retention_requires_the_worker_before_execution() {
        let args = [
            "ferrule",
            "run",
            "--project",
            "absent.json",
            "--input",
            "input.xml",
            "--output",
            "output.xml",
            "--keep-pdf-evidence",
        ];
        let result = Cli::try_parse_from(args);
        eprintln!(
            "arguments={args:?}; full original clap error={:?}",
            result.as_ref().err()
        );
        let error = result
            .err()
            .expect("retention without a worker was admitted");
        assert_eq!(error.kind(), ErrorKind::MissingRequiredArgument);
        assert!(error.to_string().contains("--pdf-worker"));
    }

    #[test]
    fn retention_and_worker_flags_parse_together_without_execution() {
        let args = [
            "ferrule",
            "run",
            "--project",
            "absent.json",
            "--input",
            "input.xml",
            "--output",
            "output.xml",
            "--pdf-worker",
            "--keep-pdf-evidence",
        ];
        let parsed = Cli::try_parse_from(args);
        eprintln!(
            "arguments={args:?}; full original clap error={:?}",
            parsed.as_ref().err()
        );
        let parsed = parsed.unwrap();
        let actual = match parsed.command {
            Command::Run {
                pdf_worker,
                keep_pdf_evidence,
                ..
            } => (pdf_worker, keep_pdf_evidence),
            _ => panic!("run command was not parsed"),
        };
        eprintln!("full original parsed worker/retention tuple={actual:?}");
        assert_eq!(actual, (true, true));
    }

    #[test]
    fn retained_worker_standard_stream_refusals_precede_loading_and_publication() {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "ferrule-pdf-evidence-flags-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&root).unwrap();
        for (key, input_stream) in [("stdin", true), ("stdout", false)] {
            let output = root.join(format!("{key}.xml"));
            std::fs::write(&output, b"sentinel").unwrap();
            let trace = root.join(format!("{key}.trace.json"));
            let args = vec![
                OsString::from("ferrule"),
                OsString::from("run"),
                OsString::from("--project"),
                root.join("absent-project.json").into_os_string(),
                OsString::from("--input"),
                if input_stream {
                    OsString::from("-")
                } else {
                    root.join("absent-input.pdf").into_os_string()
                },
                OsString::from("--output"),
                if input_stream {
                    output.clone().into_os_string()
                } else {
                    OsString::from("-")
                },
                OsString::from("--pdf-worker"),
                OsString::from("--keep-pdf-evidence"),
                OsString::from("--trace-json"),
                trace.clone().into_os_string(),
            ];
            retain(
                &root.join(format!("{key}.arguments.original.txt")),
                format!("{args:#?}").as_bytes(),
            );
            let parsed = Cli::try_parse_from(args);
            retain(
                &root.join(format!("{key}.parse-error.original.txt")),
                format!("{:?}", parsed.as_ref().err()).as_bytes(),
            );
            let result = execute(parsed.unwrap());
            retain(
                &root.join(format!("{key}.outcome.original.txt")),
                format!("{result:#?}").as_bytes(),
            );
            let output_bytes = std::fs::read(&output).unwrap();
            let trace_exists = trace.exists();
            retain(&root.join(format!("{key}.output.original")), &output_bytes);
            retain(
                &root.join(format!("{key}.trace-state.original.txt")),
                format!("{trace_exists}").as_bytes(),
            );
            assert_eq!(
                result.unwrap_err().to_string(),
                "--pdf-worker requires filesystem input and output; standard-stream payloads remain in-process"
            );
            assert_eq!(output_bytes, b"sentinel");
            assert!(!trace_exists);
        }
        // Each retained file is synced above; directory syncing is Unix-specific.
        #[cfg(unix)]
        std::fs::File::open(&root).unwrap().sync_all().unwrap();
    }
}
