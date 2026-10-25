//! The PDF worker entry point that is packaged beside `ferrule-gui` as
//! `ferrule-pdf-worker`. GUI tests copy this build into private package
//! directories, because `cargo test` builds examples but not other packages'
//! binaries.

fn main() -> std::process::ExitCode {
    std::process::ExitCode::from(pdf_worker::host::worker_entry())
}
