use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Args as ClapArgs, Parser, Subcommand, ValueEnum};
use mapforce_conformance::{GateReport, Selection, Summary, gate, summarize};

#[derive(Parser)]
#[command(about = "Validate, summarize and gate a MapForce capability ledger")]
struct Args {
    /// Ledger path relative to the repository root.
    #[arg(default_value = "conformance/mapforce-2026r2.json")]
    ledger: PathBuf,
    /// Root containing repository-relative evidence files.
    #[arg(long, global = true, default_value = concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))]
    root: PathBuf,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Count every status by profile and dimension.
    Summary(ReportArgs),
    /// Fail unless every selected applicable cell has full evidence-backed support.
    Gate(ReportArgs),
}

#[derive(ClapArgs)]
struct ReportArgs {
    /// Include this profile. Repeat for more profiles; omit for all profiles.
    #[arg(long = "profile")]
    profiles: Vec<String>,
    /// Include this capability. Repeat for more capabilities; omit for all.
    #[arg(long = "capability")]
    capabilities: Vec<String>,
    /// Include this exact dimension. Repeat for more dimensions; omit for all.
    #[arg(long = "dimension")]
    dimensions: Vec<String>,
    /// Output format for this report.
    #[arg(long, value_enum, default_value_t = OutputFormat::Human)]
    format: OutputFormat,
}

impl ReportArgs {
    fn selection(&self) -> Selection {
        Selection {
            profiles: self.profiles.clone(),
            capabilities: self.capabilities.clone(),
            dimensions: self.dimensions.clone(),
        }
    }
}

#[derive(Clone, Copy, ValueEnum)]
enum OutputFormat {
    Human,
    Json,
}

fn main() -> Result<ExitCode, Box<dyn std::error::Error>> {
    let args = Args::parse();
    let input = std::fs::read_to_string(args.root.join(&args.ledger))?;
    let ledger = mapforce_conformance::parse(&input)?;
    ledger.validate_files(&args.root)?;
    match args.command {
        None => {
            println!(
                "Validated {} {} {}: {} capabilities, {} profiles, {} evidence references; inventory complete: {}",
                ledger.target.product,
                ledger.target.release,
                ledger.target.edition,
                ledger.capabilities.len(),
                ledger.profiles.len(),
                ledger.evidence.len(),
                ledger.inventory_complete,
            );
            Ok(ExitCode::SUCCESS)
        }
        Some(Command::Summary(options)) => {
            let summary = summarize(&ledger, &options.selection())?;
            emit_summary(&summary, options.format)?;
            Ok(ExitCode::SUCCESS)
        }
        Some(Command::Gate(options)) => {
            let report = gate(&ledger, &options.selection())?;
            emit_gate(&report, options.format)?;
            Ok(if report.passed {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            })
        }
    }
}

fn emit_summary(summary: &Summary, format: OutputFormat) -> Result<(), serde_json::Error> {
    match format {
        OutputFormat::Human => print_summary(summary),
        OutputFormat::Json => println!("{}", serde_json::to_string_pretty(summary)?),
    }
    Ok(())
}

fn emit_gate(report: &GateReport, format: OutputFormat) -> Result<(), serde_json::Error> {
    match format {
        OutputFormat::Human => {
            print_summary(&report.summary);
            println!(
                "Gate {}: {} in-scope cells, {} evidenced not-applicable cells, {} failures",
                if report.passed { "PASS" } else { "FAIL" },
                report.in_scope_cells,
                report.excluded_not_applicable_cells,
                report.failures.len(),
            );
            for failure in &report.failures {
                let location = match (&failure.profile, &failure.capability, &failure.dimension) {
                    (Some(profile), Some(capability), Some(dimension)) => {
                        format!("{profile}/{capability}/{dimension}")
                    }
                    _ => "inventory".into(),
                };
                println!("  {location}: {}", failure.detail);
            }
        }
        OutputFormat::Json => println!("{}", serde_json::to_string_pretty(report)?),
    }
    Ok(())
}

fn print_summary(summary: &Summary) {
    println!(
        "{} {} {}: {} selected cells; inventory complete: {}",
        summary.target.product,
        summary.target.release,
        summary.target.edition,
        summary.selected_cells,
        summary.inventory_complete,
    );
    for row in &summary.by_profile_dimension {
        let counts = row
            .statuses
            .iter()
            .map(|(status, count)| format!("{}={count}", status.as_str()))
            .collect::<Vec<_>>()
            .join(", ");
        println!(
            "  {}/{}: {} [{}]; not_applicable evidenced={}, unevidenced={}",
            row.profile,
            row.dimension,
            row.capabilities,
            counts,
            row.evidenced_not_applicable,
            row.unevidenced_not_applicable,
        );
    }
}
