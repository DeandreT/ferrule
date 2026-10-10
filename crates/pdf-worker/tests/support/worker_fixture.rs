//! Feature-gated synthetic protocol child. It is never selected by production
//! sibling resolution. Mode comes only from the root-owned fixture executable
//! name, not from a production project/flag/environment.
#[cfg(all(target_os = "linux", target_arch = "x86_64", target_env = "gnu"))]
fn run() -> std::io::Result<u8> {
    use pdf_worker::*;
    use std::io::{Read, Write};
    let name = std::env::current_exe()?
        .file_name()
        .and_then(std::ffi::OsStr::to_str)
        .unwrap_or("")
        .to_owned();
    if name == "fixture-production" {
        return Ok(pdf_worker::host::worker_entry());
    }
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.len() != 3 || args[0] != "linux-x86_64-v1" {
        return Ok(2);
    }
    let parent = args[1].parse::<u32>().map_err(std::io::Error::other)?;
    let id = RequestIdentity::new(
        RequestId::parse_hex(&args[2]).map_err(std::io::Error::other)?,
        Profile::LinuxX86_64V1,
    );
    pdf_worker::host::fixture_install_policy(parent)?;
    if name == "fixture-no-ready" {
        loop {
            std::thread::sleep(std::time::Duration::from_secs(1));
        }
    }
    if name == "fixture-exit-before-ready" {
        return Ok(7);
    }
    if name == "fixture-resource-setup" {
        // Synthetic declared pre-Ready outcome only; inherited-limit/kernel
        // failure qualification is separate and remains root-owned.
        let result = ResultEnvelope::new(
            id,
            Outcome::HostError {
                error: HostError::ResourceSetup,
                diagnostics: Diagnostics::empty(),
            },
        )
        .map_err(std::io::Error::other)?;
        let mut bytes = Vec::new();
        encode_result(&result, &mut bytes).map_err(std::io::Error::other)?;
        std::fs::write("result.json", bytes)?;
        return Ok(2);
    }
    if name == "fixture-ignore-term" {
        let mut a: libc::sigaction = unsafe { std::mem::zeroed() };
        a.sa_sigaction = libc::SIG_IGN;
        if unsafe { libc::sigemptyset(&mut a.sa_mask) } != 0
            || unsafe { libc::sigaction(libc::SIGTERM, &a, std::ptr::null_mut()) } != 0
        {
            return Err(std::io::Error::last_os_error());
        }
    }
    let mut ready = Vec::new();
    encode_ready(&ReadyFrame::new(id), &mut ready).map_err(std::io::Error::other)?;
    if name == "fixture-bad-ready" {
        ready = b"{}".to_vec();
    }
    ready.push(b'\n');
    if name == "fixture-extra-stdout" {
        ready.extend_from_slice(b"extra\n");
    }
    std::io::stdout().write_all(&ready)?;
    std::io::stdout().flush()?;
    if name == "fixture-ready-no-eof" {
        loop {
            std::thread::sleep(std::time::Duration::from_secs(1));
        }
    }
    if unsafe { libc::close(libc::STDOUT_FILENO) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    let mut start = Vec::new();
    std::io::stdin()
        .take(MAX_CONTROL_FRAME_BYTES as u64 + 2)
        .read_to_end(&mut start)?;
    let body = start
        .strip_suffix(b"\n")
        .ok_or_else(|| std::io::Error::other("start missing"))?;
    decode_start(body, &id.request_id()).map_err(std::io::Error::other)?;
    if name == "fixture-ignore-term" {
        loop {
            std::thread::sleep(std::time::Duration::from_secs(1));
        }
    }
    if name == "fixture-pdf-error" {
        let mut raw = Vec::new();
        encode_pdf_error(&id, &format_pdf::PdfError::InputTooLarge, &mut raw)
            .map_err(std::io::Error::other)?;
        std::fs::write("result.json", raw)?;
        return Ok(2);
    }
    if name == "fixture-missing-result" {
        return Ok(0);
    }
    if name == "fixture-malformed-result" {
        std::fs::write("result.json", b"{invalid")?;
        return Ok(0);
    }
    if name == "fixture-stderr-exact" || name == "fixture-stderr-over" {
        let n = if name == "fixture-stderr-exact" {
            65536
        } else {
            65537
        };
        std::io::stderr().write_all(&vec![b'X'; n])?;
    }
    let instance = ir::Instance::Group(
        vec![(
            "Value".into(),
            ir::Instance::Scalar(ir::Value::String("Hello".into())),
        )]
        .into(),
    );
    let mut bytes = Vec::new();
    let encoded = encode_instance(&instance, &mut bytes).map_err(std::io::Error::other)?;
    if name == "fixture-part-only" {
        std::fs::write("instance.part", &bytes)?;
    } else {
        std::fs::write("instance.json", &bytes)?;
    }
    if name == "fixture-unknown-file" {
        std::fs::write("unknown", b"unknown")?;
    }
    if name == "fixture-alias" {
        std::fs::hard_link("instance.json", "instance.part")?;
    }
    let outcome = if name == "fixture-host-error" {
        Outcome::HostError {
            error: HostError::SnapshotMismatch,
            diagnostics: Diagnostics::empty(),
        }
    } else {
        Outcome::Ok(
            InstanceDescriptor::new(encoded.byte_len, encoded.sha256)
                .map_err(std::io::Error::other)?,
        )
    };
    let result = ResultEnvelope::new(id, outcome).map_err(std::io::Error::other)?;
    let mut bytes = Vec::new();
    encode_result(&result, &mut bytes).map_err(std::io::Error::other)?;
    std::fs::write("result.json", bytes)?;
    Ok(
        if name == "fixture-wrong-exit" || name == "fixture-host-error" {
            2
        } else {
            0
        },
    )
}
fn main() -> std::process::ExitCode {
    #[cfg(all(target_os = "linux", target_arch = "x86_64", target_env = "gnu"))]
    {
        match run() {
            Ok(code) => std::process::ExitCode::from(code),
            Err(error) => {
                eprintln!("fixture original error: {error:#?}");
                std::process::ExitCode::from(2)
            }
        }
    }
    #[cfg(not(all(target_os = "linux", target_arch = "x86_64", target_env = "gnu")))]
    {
        std::process::ExitCode::from(2)
    }
}
