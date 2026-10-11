//! Startup-only fixture behavior, selected by three disjoint executable names.
//! Re-exec preserves the manager's direct child PID and sole wait4 authority.
use std::io::{self, Write};
use std::os::unix::process::CommandExt;

const REEXEC_MARKER: &str = "FERRULE_PDF_STARTUP350_REEXEC";

fn limit(resource: libc::__rlimit_resource_t) -> io::Result<[u64; 2]> {
    let mut value = std::mem::MaybeUninit::<libc::rlimit>::uninit();
    if unsafe { libc::getrlimit(resource, value.as_mut_ptr()) } != 0 {
        return Err(io::Error::last_os_error());
    }
    let value = unsafe { value.assume_init() };
    Ok([value.rlim_cur, value.rlim_max])
}

fn marker(stage: &str, cpu: [u64; 2], installed: Option<[[u64; 2]; 4]>) -> io::Result<()> {
    let record = serde_json::json!({
        "fixture": "startup350",
        "stage": stage,
        "pid": std::process::id(),
        "parent": unsafe { libc::getppid() },
        "cpu": cpu,
        "installed_limits": installed,
    });
    let mut stderr = io::stderr().lock();
    serde_json::to_writer(&mut stderr, &record).map_err(io::Error::other)?;
    stderr.write_all(b"\n")?;
    stderr.flush()
}

fn selected(name: &str, cpu: [u64; 2], supported: bool) -> io::Result<u8> {
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    if args.len() != 3 || args[0] != "linux-x86_64-v1" {
        return Err(io::Error::other("fixed startup fixture argv"));
    }
    let parent = args[1]
        .to_str()
        .ok_or_else(|| io::Error::other("startup parent argv"))?
        .parse::<u32>()
        .map_err(io::Error::other)?;
    if parent == 0 || parent > i32::MAX as u32 {
        return Err(io::Error::other("startup parent argv"));
    }
    pdf_worker::RequestId::parse_hex(
        args[2]
            .to_str()
            .ok_or_else(|| io::Error::other("startup request argv"))?,
    )
    .map_err(io::Error::other)?;
    let executable = std::env::current_exe()?;
    if executable.file_name().and_then(std::ffi::OsStr::to_str) != Some(name) {
        return Err(io::Error::other("startup fixture executable identity"));
    }
    match std::env::var_os(REEXEC_MARKER) {
        None => {
            // Change only this disposable direct child. Never raise an existing
            // limit or alter the test parent, and never launch another child.
            let inherited = limit(libc::RLIMIT_CPU)?;
            if inherited[0] < 30 || inherited[1] < 31 {
                return Err(io::Error::other(
                    "startup fixture initial CPU limits incompatible",
                ));
            }
            let desired = libc::rlimit {
                rlim_cur: cpu[0],
                rlim_max: cpu[1],
            };
            if unsafe { libc::setrlimit(libc::RLIMIT_CPU, &desired) } != 0 {
                return Err(io::Error::last_os_error());
            }
            let actual = limit(libc::RLIMIT_CPU)?;
            if actual != cpu {
                return Err(io::Error::other("startup fixture CPU setup mismatch"));
            }
            marker("prepared", actual, None)?;
            // This replaces the same process. No spawn/wait/signal helper is
            // added, and the original argv/cwd/stdio remain unchanged.
            Err(std::process::Command::new(&executable)
                .args(&args)
                .env_clear()
                .env(REEXEC_MARKER, "yes")
                .exec())
        }
        Some(value) if value == "yes" => {
            let inherited = limit(libc::RLIMIT_CPU)?;
            if inherited != cpu {
                return Err(io::Error::other(
                    "startup fixture re-exec inheritance mismatch",
                ));
            }
            marker("inherited", inherited, None)?;
            if !supported {
                // Real production setup attempts to install CPU 30/31, refuses
                // the actual inherited low soft/hard limit, and publishes its
                // complete typed ResourceSetup result before Ready or PDF access.
                return Ok(pdf_worker::host::worker_entry());
            }
            pdf_worker::host::fixture_install_policy(parent)?;
            let installed = [
                limit(libc::RLIMIT_AS)?,
                limit(libc::RLIMIT_CPU)?,
                limit(libc::RLIMIT_FSIZE)?,
                limit(libc::RLIMIT_CORE)?,
            ];
            marker("installed", limit(libc::RLIMIT_CPU)?, Some(installed))?;
            // Intentional exit before Ready: this qualifies actual supported
            // policy installation, not extraction, request parsing or success.
            Ok(0)
        }
        Some(_) => Err(io::Error::other("startup fixture re-exec marker")),
    }
}

pub(super) fn run(name: &str) -> Option<io::Result<u8>> {
    match name {
        "fixture-startup-low-soft" => Some(selected(name, [29, 31], false)),
        "fixture-startup-low-hard" => Some(selected(name, [30, 30], false)),
        "fixture-startup-supported" => Some(selected(name, [30, 31], true)),
        _ => None,
    }
}
