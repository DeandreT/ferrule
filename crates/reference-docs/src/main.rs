use std::path::Path;
use std::process::ExitCode;

const USAGE: &str = "usage: reference-docs [--check]";

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let check = match (args.next().as_deref(), args.next()) {
        (None, _) => false,
        (Some("--check"), None) => true,
        _ => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    let root = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."));
    let result = if check {
        reference_docs::stale(root)
    } else {
        reference_docs::write(root)
    };
    match result {
        Ok(paths) if check && !paths.is_empty() => {
            for path in paths {
                eprintln!("stale: {path}");
            }
            eprintln!("regenerate with `{}`", reference_docs::REGENERATE);
            ExitCode::FAILURE
        }
        Ok(paths) => {
            for path in paths {
                println!("updated {path}");
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}
