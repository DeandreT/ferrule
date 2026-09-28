use std::path::PathBuf;

use clap::Parser;

#[derive(Parser)]
#[command(about = "Validate a MapForce capability ledger and its local evidence")]
struct Args {
    /// Ledger path relative to the repository root.
    #[arg(default_value = "conformance/mapforce-2026r2.json")]
    ledger: PathBuf,
    /// Root containing repository-relative evidence files.
    #[arg(long, default_value = concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))]
    root: PathBuf,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    let input = std::fs::read_to_string(args.root.join(&args.ledger))?;
    let ledger = mapforce_conformance::parse(&input)?;
    ledger.validate_files(&args.root)?;
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
    Ok(())
}
