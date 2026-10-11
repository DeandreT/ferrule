fn main() -> std::process::ExitCode {
    std::process::ExitCode::from(pdf_worker::host::worker_entry())
}
