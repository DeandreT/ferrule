//! Fixture352: real owned process/file/codec controls; synthetic payloads are explicit.
use pdf_worker::*;
use std::io::{self, Read, Write};
use std::os::unix::fs::PermissionsExt;
#[path = "protocol_boundary_wire.rs"]
mod wire;
use wire::mutate;

const NULL: &[u8] = b"{\"kind\":\"null\"}";

fn frozen(path: &str, bytes: &[u8]) -> io::Result<()> {
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o444))
}
fn replace_owned(path: &str, bytes: &[u8]) -> io::Result<()> {
    // Only this disposable job's known frozen leaf, never a caller source path.
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o444))
}
fn filled(path: &str, prefix: &[u8], count: usize, byte: u8) -> io::Result<()> {
    if count < prefix.len() {
        return Err(io::Error::other("fixed352 fill size"));
    }
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    file.write_all(prefix)?;
    let block = [byte; 16384];
    let mut left = count - prefix.len();
    while left != 0 {
        let n = left.min(block.len());
        file.write_all(&block[..n])?;
        left -= n;
    }
    file.sync_all()?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o444))
}
fn envelope(id: RequestIdentity, bytes: &[u8]) -> io::Result<Vec<u8>> {
    let descriptor = InstanceDescriptor::new(bytes.len() as u64, Sha256Digest::of(bytes))
        .map_err(io::Error::other)?;
    let result = ResultEnvelope::new(id, Outcome::Ok(descriptor)).map_err(io::Error::other)?;
    let mut raw = Vec::new();
    encode_result(&result, &mut raw).map_err(io::Error::other)?;
    Ok(raw)
}
fn run_selected(name: &str) -> io::Result<u8> {
    let mode = name
        .strip_prefix("fixture352-")
        .ok_or_else(|| io::Error::other("fixed352 name"))?;
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.len() != 3 || args[0] != "linux-x86_64-v1" {
        return Err(io::Error::other("fixed352 argv"));
    }
    let parent = args[1].parse::<u32>().map_err(io::Error::other)?;
    let id = RequestIdentity::new(
        RequestId::parse_hex(&args[2]).map_err(io::Error::other)?,
        Profile::LinuxX86_64V1,
    );
    pdf_worker::host::fixture_install_policy(parent)?;
    if let Some(change) = mode.strip_prefix("request-") {
        let raw = std::fs::read("request.json")?;
        replace_owned("request.json", &mutate(&raw, change)?)?;
        return Ok(pdf_worker::host::worker_entry());
    }
    if mode == "staged-count" || mode == "staged-hash" {
        let mut bytes = std::fs::read("input.pdf")?;
        if mode == "staged-count" {
            bytes.push(0);
        } else {
            bytes[0] ^= 1;
        }
        replace_owned("input.pdf", &bytes)?;
        return Ok(pdf_worker::host::worker_entry());
    }
    let mut ready = Vec::new();
    encode_ready(&ReadyFrame::new(id), &mut ready).map_err(io::Error::other)?;
    let ready_change = mode.strip_prefix("ready-");
    if let Some(change) = ready_change {
        ready = mutate(&ready, change)?;
    }
    ready.push(b'\n');
    std::io::stdout().write_all(&ready)?;
    std::io::stdout().flush()?;
    if unsafe { libc::close(libc::STDOUT_FILENO) } != 0 {
        return Err(io::Error::last_os_error());
    }
    if ready_change.is_some_and(|s| !matches!(s, "order" | "exact" | "valid")) {
        // Deliberately stay alive for actual owned TERM/reap, with no fake handler.
        loop {
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }
    let mut start = Vec::new();
    std::io::stdin()
        .take(MAX_CONTROL_FRAME_BYTES as u64 + 2)
        .read_to_end(&mut start)?;
    let body = start
        .strip_suffix(b"\n")
        .filter(|b| !b.contains(&b'\n'))
        .ok_or_else(|| io::Error::other("exact352 Start"))?;
    decode_start(body, &id.request_id()).map_err(io::Error::other)?;
    match mode {
        "error-exit0" => {
            // A complete declared error whose exit contradicts its required 2.
            let result = ResultEnvelope::new(
                id,
                Outcome::HostError {
                    error: HostError::SnapshotMismatch,
                    diagnostics: Diagnostics::empty(),
                },
            )
            .map_err(io::Error::other)?;
            let mut raw = Vec::new();
            encode_result(&result, &mut raw).map_err(io::Error::other)?;
            frozen("result.json", &raw)?;
            return Ok(0);
        }
        "partial-exit3" => {
            frozen("instance.part", &NULL[..NULL.len() / 2])?;
            return Ok(3);
        }
        "instance-over" => {
            // The installed 64 MiB file-size limit stops this write itself.
            filled("instance.json", NULL, MAX_INSTANCE_BYTES + 1, b' ')?;
            return Err(io::Error::other(
                "file-size limit admitted a one-over instance",
            ));
        }
        _ => {}
    }
    if mode == "instance-encode-over" {
        let overhead = b"{\"kind\":\"group\",\"fields\":[[\"\",{\"kind\":\"null\"}]]}".len();
        let instance = ir::Instance::Group(
            vec![(
                "x".repeat(MAX_INSTANCE_BYTES - overhead + 1),
                ir::Instance::Scalar(ir::Value::Null),
            )]
            .into(),
        );
        let mut part = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open("instance.part")?;
        let encoded = encode_instance(&instance, &mut part);
        part.sync_all()?;
        drop(part);
        eprintln!("actual352 encode refusal: {encoded:#?}");
        match encoded {
            Err(error) if error.kind() == CodecErrorKind::InstanceTooLarge => {
                let result = ResultEnvelope::new(
                    id,
                    Outcome::HostError {
                        error: HostError::InstanceTooLarge,
                        diagnostics: Diagnostics::empty(),
                    },
                )
                .map_err(io::Error::other)?;
                let mut raw = Vec::new();
                encode_result(&result, &mut raw).map_err(io::Error::other)?;
                frozen("result.json", &raw)?;
                return Ok(2);
            }
            _ => {
                return Err(io::Error::other(
                    "actual352 encode did not refuse InstanceTooLarge",
                ));
            }
        }
    }
    let instance_size = if mode == "instance-exact" {
        MAX_INSTANCE_BYTES
    } else if mode == "job-exact" || mode == "job-over" {
        16 * 1024 * 1024
    } else {
        NULL.len()
    };
    filled("instance.json", NULL, instance_size, b' ')?;
    if mode == "both-leaves" {
        filled("instance.part", NULL, NULL.len(), b' ')?;
    }
    let bytes = std::fs::read("instance.json")?;
    let mut result = envelope(id, &bytes)?;
    drop(bytes);
    if let Some(change) = mode.strip_prefix("result-") {
        result = mutate(&result, change)?;
    }
    frozen("result.json", &result)?;
    if mode == "complete-then-kill" {
        // A complete valid result does not outrank an abnormal termination.
        unsafe { libc::raise(libc::SIGKILL) };
    }
    if mode == "receipt-exact" || mode == "receipt-over" {
        filled(
            "receipt.part",
            b"",
            65536 + usize::from(mode == "receipt-over"),
            b'R',
        )?;
    }
    if mode == "job-exact" || mode == "job-over" {
        let subtotal = [
            "input.pdf",
            "request.json",
            "control.bin",
            "stderr.bin",
            "instance.json",
            "result.json",
        ]
        .iter()
        .try_fold(0u64, |sum, name| {
            std::fs::metadata(name).map(|m| sum + m.len())
        })?;
        let total = 80 * 1024 * 1024 + u64::from(mode == "job-over");
        let part = total
            .checked_sub(subtotal)
            .ok_or_else(|| io::Error::other("fixed352 job subtotal"))?;
        if part > MAX_INSTANCE_BYTES as u64 {
            return Err(io::Error::other("fixed352 part per-file cap"));
        }
        filled("instance.part", b"", part as usize, 0)?;
    }
    Ok(0)
}
pub(super) fn run(name: &str) -> Option<io::Result<u8>> {
    if name.starts_with("fixture352-") {
        Some(run_selected(name))
    } else {
        None
    }
}
