//! Disjoint #351 modes only. Synthetic protocol output is not PDF qualification.
use pdf_worker::*;
use std::io::{Read, Write};
pub(super) fn run(name: &str, id: RequestIdentity) -> std::io::Result<u8> {
    if name == "fixture351-ignore-term" {
        let mut action: libc::sigaction = unsafe { std::mem::zeroed() };
        action.sa_sigaction = libc::SIG_IGN;
        if unsafe { libc::sigemptyset(&mut action.sa_mask) } != 0
            || unsafe { libc::sigaction(libc::SIGTERM, &action, std::ptr::null_mut()) } != 0
        {
            return Err(std::io::Error::last_os_error());
        }
    } else if name != "fixture351-success" {
        return Err(std::io::Error::other("unknown ownership fixture"));
    }
    let mut ready = Vec::new();
    encode_ready(&ReadyFrame::new(id), &mut ready).map_err(std::io::Error::other)?;
    ready.push(b'\n');
    std::io::stdout().write_all(&ready)?;
    std::io::stdout().flush()?;
    if unsafe { libc::close(libc::STDOUT_FILENO) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    let mut start = Vec::new();
    std::io::stdin()
        .take(MAX_CONTROL_FRAME_BYTES as u64 + 2)
        .read_to_end(&mut start)?;
    let body = start
        .strip_suffix(b"\n")
        .ok_or_else(|| std::io::Error::other("Start LF"))?;
    decode_start(body, &id.request_id()).map_err(std::io::Error::other)?;
    if name == "fixture351-ignore-term" {
        loop {
            std::thread::sleep(std::time::Duration::from_secs(1));
        }
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
    std::fs::write("instance.json", &bytes)?;
    let result = ResultEnvelope::new(
        id,
        Outcome::Ok(
            InstanceDescriptor::new(encoded.byte_len, encoded.sha256)
                .map_err(std::io::Error::other)?,
        ),
    )
    .map_err(std::io::Error::other)?;
    bytes.clear();
    encode_result(&result, &mut bytes).map_err(std::io::Error::other)?;
    std::fs::write("result.json", bytes)?;
    Ok(0)
}
