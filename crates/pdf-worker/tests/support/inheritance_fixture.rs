//! Reports what the worker process inherited before and after the production
//! policy installation, then exits before Ready by design.
use std::io::{self, Write};

fn open_descriptors() -> io::Result<Vec<i32>> {
    let own = format!("/proc/{}/fd", std::process::id());
    let mut open = Vec::new();
    for entry in std::fs::read_dir("/proc/self/fd")? {
        let entry = entry?;
        if std::fs::read_link(entry.path())?.as_os_str() == own.as_str() {
            continue; // The listing's own directory descriptor.
        }
        let fd = entry.file_name().to_str().and_then(|s| s.parse().ok());
        open.push(fd.ok_or_else(|| io::Error::other("descriptor name"))?);
    }
    open.sort_unstable();
    Ok(open)
}

fn report() -> io::Result<u8> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.len() != 3 || args[0] != "linux-x86_64-v1" {
        return Err(io::Error::other("fixed inheritance fixture argv"));
    }
    let parent = args[1].parse::<u32>().map_err(io::Error::other)?;
    let before = open_descriptors()?;
    let environment = std::env::vars_os().count();
    pdf_worker::host::fixture_install_policy(parent)?;
    let after = open_descriptors()?;
    let mut death_signal = 0;
    if unsafe { libc::prctl(libc::PR_GET_PDEATHSIG, &mut death_signal as *mut i32) } != 0 {
        return Err(io::Error::last_os_error());
    }
    let record = serde_json::json!({
        "descriptors_before": before,
        "descriptors_after": after,
        "environment_entries": environment,
        "parent_death_signal": death_signal,
        "parent": unsafe { libc::getppid() },
    });
    let mut stderr = io::stderr().lock();
    serde_json::to_writer(&mut stderr, &record).map_err(io::Error::other)?;
    stderr.write_all(b"\n")?;
    Ok(0)
}

pub(super) fn run(name: &str) -> Option<io::Result<u8>> {
    (name == "fixture353-inherit").then(report)
}
