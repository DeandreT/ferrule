use super::fs_job::Dir;
use crate::{
    Diagnostics, HostError, InstanceDescriptor, Outcome, Profile, RequestId, RequestIdentity,
    ResultEnvelope,
};
use std::io::{Read, Write};
use std::os::fd::AsRawFd;

fn args() -> Result<(RequestIdentity, u32), std::io::Error> {
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    if args.len() != 3 || args[0] != "linux-x86_64-v1" {
        return Err(std::io::Error::other("fixed worker argv"));
    }
    let parent = args[1]
        .to_str()
        .ok_or_else(|| std::io::Error::other("parent argv"))?
        .parse::<u32>()
        .map_err(std::io::Error::other)?;
    if parent == 0 || parent > i32::MAX as u32 {
        return Err(std::io::Error::other("parent argv"));
    }
    let id = RequestId::parse_hex(
        args[2]
            .to_str()
            .ok_or_else(|| std::io::Error::other("request argv"))?,
    )
    .map_err(std::io::Error::other)?;
    Ok((RequestIdentity::new(id, Profile::LinuxX86_64V1), parent))
}
fn host_result(
    dir: &Dir,
    id: RequestIdentity,
    error: HostError,
    diagnostics: Diagnostics,
) -> Result<(), std::io::Error> {
    let result = ResultEnvelope::new(id, Outcome::HostError { error, diagnostics })
        .map_err(std::io::Error::other)?;
    let mut bytes = Vec::new();
    crate::encode_result(&result, &mut bytes).map_err(std::io::Error::other)?;
    dir.write_frozen("result.json", &bytes)
}
fn run(id: RequestIdentity, parent: u32) -> Result<u8, std::io::Error> {
    // All resource/parent-death/descriptor policy precedes request/PDF access.
    if let Err(e) = super::linux::setup_worker(parent) {
        let dir = Dir::current_worker()?;
        host_result(
            &dir,
            id,
            HostError::ResourceSetup,
            crate::error::capture(&e),
        )?;
        return Ok(2);
    }
    let dir = Dir::current_worker()?;
    let raw = dir.read("request.json", crate::MAX_REQUEST_BYTES)?;
    let request = match crate::decode_request(&raw) {
        Ok(r) if *r.identity() == id => r,
        _ => {
            host_result(&dir, id, HostError::ProtocolError, Diagnostics::empty())?;
            return Ok(2);
        }
    };
    let mut ready = Vec::new();
    crate::encode_ready(&crate::ReadyFrame::new(id), &mut ready).map_err(std::io::Error::other)?;
    ready.push(b'\n');
    {
        let mut stdout = std::io::stdout().lock();
        stdout.write_all(&ready)?;
        stdout.flush()?;
    }
    // Closing owned stdout lets the parent reject any extra frame BEFORE Start.
    if unsafe { libc::close(libc::STDOUT_FILENO) } != 0 {
        return Err(super::linux::errno());
    }
    let mut start = Vec::new();
    std::io::stdin()
        .lock()
        .take(crate::MAX_CONTROL_FRAME_BYTES as u64 + 2)
        .read_to_end(&mut start)?;
    let body = start
        .strip_suffix(b"\n")
        .filter(|body| !body.contains(&b'\n'))
        .ok_or_else(|| std::io::Error::other("exact Start frame required"))?;
    if crate::decode_start(body, &id.request_id()).is_err() {
        host_result(&dir, id, HostError::ProtocolError, Diagnostics::empty())?;
        return Ok(2);
    }
    let bytes = dir.read("input.pdf", format_pdf::MAX_INPUT_BYTES)?;
    if bytes.len() as u64 != request.input().byte_len()
        || crate::Sha256Digest::of(&bytes) != request.input().sha256()
    {
        host_result(&dir, id, HostError::SnapshotMismatch, Diagnostics::empty())?;
        return Ok(2);
    }
    // The only PDF parser invocation; never reached before complete Start.
    let instance = match format_pdf::from_bytes(&bytes, request.layout()) {
        Ok(instance) => instance,
        Err(error) => {
            let mut result = Vec::new();
            crate::encode_pdf_error(&id, &error, &mut result).map_err(std::io::Error::other)?;
            dir.write_frozen("result.json", &result)?;
            return Ok(2);
        }
    };
    let mut part = dir.create("instance.part")?;
    let encoded = match crate::encode_instance(&instance, &mut part) {
        Ok(encoded) => encoded,
        Err(error) => {
            part.sync_all()?;
            drop(part);
            host_result(
                &dir,
                id,
                HostError::InstanceTooLarge,
                crate::error::capture(&error),
            )?;
            return Ok(2);
        }
    };
    part.flush()?;
    part.sync_all()?;
    if unsafe { libc::fchmod(part.as_raw_fd(), 0o444) } != 0 {
        return Err(super::linux::errno());
    }
    part.sync_all()?;
    drop(part);
    dir.rename("instance.part", "instance.json")?;
    let result = ResultEnvelope::new(
        id,
        Outcome::Ok(
            InstanceDescriptor::new(encoded.byte_len, encoded.sha256)
                .map_err(std::io::Error::other)?,
        ),
    )
    .map_err(std::io::Error::other)?;
    let mut bytes = Vec::new();
    crate::encode_result(&result, &mut bytes).map_err(std::io::Error::other)?;
    dir.write_frozen("result.json", &bytes)?;
    Ok(0)
}
pub fn entry() -> u8 {
    let original = args().and_then(|(id, parent)| run(id, parent));
    match original {
        Ok(code) => code,
        Err(error) => {
            eprintln!("PDF worker host failure: {error}");
            2
        }
    }
}
