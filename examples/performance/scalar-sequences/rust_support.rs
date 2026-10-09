//! Untimed typed/byte checks and sparse measured boundary originals.
use std::error::Error;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::time::Instant;

use ir::{Instance, Value, XmlTypeOrigin};
use serde_json::{Value as Json, json};
use sha2::{Digest, Sha256};

pub type Result<T> = std::result::Result<T, Box<dyn Error>>;
pub const CEILING: usize = 64 * 1024 * 1024;

pub fn require(ok: bool, message: &str) -> Result<()> {
    if ok {
        Ok(())
    } else {
        Err(std::io::Error::other(message).into())
    }
}

pub fn mode(mode: &str) -> Result<()> {
    require(
        matches!(mode, "numeric" | "capture"),
        "numeric or capture required",
    )
}

pub fn dimension(count: usize, width: usize) -> Result<()> {
    require(
        matches!(
            (count, width),
            (1, 32) | (16, 32) | (128, 32) | (16, 4096) | (16, 262_144) | (128, 262_144) | (3, 4)
        ),
        "outside frozen six dimensions/two small controls",
    )
}

pub fn fresh_dir(path: &Path) -> Result<()> {
    require(path.is_absolute(), "absolute fresh directory required")?;
    fs::create_dir(path)?;
    Ok(())
}

pub fn write(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    file.write_all(bytes)?;
    file.flush()?;
    file.sync_all()?;
    Ok(())
}

pub fn retain(path: &Path, value: &Json) -> Result<()> {
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    serde_json::to_writer_pretty(&mut file, value)?;
    file.write_all(b"\n")?;
    file.flush()?;
    file.sync_all()?;
    Ok(())
}

pub fn read_text(path: &Path) -> Result<String> {
    require(path.is_absolute(), "absolute input required")?;
    let mut bytes = Vec::new();
    File::open(path)?
        .take(CEILING as u64)
        .read_to_end(&mut bytes)?;
    require(bytes.len() < CEILING, "study document must be below 64 MiB")?;
    Ok(String::from_utf8(bytes)?)
}

fn runtime_feature(error: &(dyn Error + 'static)) -> Option<Json> {
    use codegen_runtime::RuntimeError;
    match error.downcast_ref::<RuntimeError>()? {
        RuntimeError::FilterMapRuntime { boundary, .. } => Some(json!({
            "type":"RuntimeError", "kind":"FilterMapRuntime", "boundary":{
                "item":boundary.item, "phase":format!("{:?}", boundary.phase),
                "capture_index":boundary.capture_index, "source_position":boundary.source_position,
                "function":boundary.function, "node":boundary.node, "kind":format!("{:?}", boundary.kind)}})),
        RuntimeError::FilterMapValueType { expected, found } => Some(json!({
            "type":"RuntimeError", "kind":"FilterMapValueType",
            "expected":format!("{expected:?}"), "found":scalar(found)})),
        RuntimeError::FilterMapNonFinite { bits } => Some(json!({
            "type":"RuntimeError", "kind":"FilterMapNonFinite", "bits":format!("{bits:016x}")})),
        RuntimeError::FilterMapBudget {
            kind,
            used,
            requested,
            max,
        } => Some(json!({
            "type":"RuntimeError", "kind":"FilterMapBudget", "budget_kind":format!("{kind:?}"),
            "used":used.to_string(), "requested":requested.to_string(), "max":max.to_string()})),
        RuntimeError::FilterMapCancelled => {
            Some(json!({"type":"RuntimeError", "kind":"FilterMapCancelled"}))
        }
        _ => None,
    }
}

pub fn error_with(
    error: &(dyn Error + 'static),
    native: fn(&(dyn Error + 'static)) -> Option<Json>,
) -> Json {
    let mut chain = Vec::new();
    let mut current = Some(error);
    while let Some(cause) = current {
        chain.push(
            json!({"debug":format!("{cause:#?}"), "display":cause.to_string(),
            "known_feature":native(cause).or_else(|| runtime_feature(cause))}),
        );
        current = cause.source();
    }
    json!({"outcome":"Err", "ordered_cause_chain":chain})
}

pub fn error(error: &(dyn Error + 'static)) -> Json {
    error_with(error, |_| None)
}

pub fn outcome<T: std::fmt::Debug, E: Error + 'static>(result: &std::result::Result<T, E>) -> Json {
    match result {
        Ok(value) => json!({"outcome":"Ok", "debug":format!("{value:#?}")}),
        Err(cause) => error(cause),
    }
}

pub fn scalar(value: &Value) -> Json {
    match value {
        Value::Null => json!({"kind":"Scalar", "tag":"Null"}),
        Value::JsonNull(_) => json!({"kind":"Scalar", "tag":"JsonNull"}),
        Value::XmlNil(_) => json!({"kind":"Scalar", "tag":"XmlNil"}),
        Value::Bool(v) => json!({"kind":"Scalar", "tag":"Bool", "value":v}),
        Value::Int(v) => json!({"kind":"Scalar", "tag":"Int", "value":v}),
        Value::Float(v) => {
            json!({"kind":"Scalar", "tag":"Float", "bits":format!("{:016x}", v.to_bits())})
        }
        Value::String(v) => json!({"kind":"Scalar", "tag":"String", "value":v}),
    }
}

pub fn instance(value: &Instance) -> Json {
    match value {
        Instance::Scalar(value) => scalar(value),
        Instance::Group(fields) => {
            let origin = match fields.xml_type_origin() {
                XmlTypeOrigin::Unknown => json!({"tag":"Unknown"}),
                XmlTypeOrigin::Absent => json!({"tag":"Absent"}),
                XmlTypeOrigin::Explicit(identity) => json!({"tag":"Explicit", "identity":identity}),
                XmlTypeOrigin::ExplicitPadded {
                    literal,
                    resolved_identity,
                } => {
                    json!({"tag":"ExplicitPadded", "literal":literal, "identity":resolved_identity})
                }
            };
            json!({"kind":"Group", "origin":origin, "fields":fields.iter().map(|(name, v)|
                json!({"name":name, "value":instance(v)})).collect::<Vec<_>>()})
        }
        Instance::Repeated(values) => json!({"kind":"Repeated",
            "items":values.iter().map(instance).collect::<Vec<_>>()}),
        Instance::MappedSequence(values) => json!({"kind":"MappedSequence",
            "items":values.iter().map(instance).collect::<Vec<_>>()}),
        Instance::DocumentSet(documents) => json!({"kind":"DocumentSet",
            "documents":documents.iter().map(|d| json!({"path":d.path(),
                "effective_source_path":d.source_path(), "value":instance(d.value())})).collect::<Vec<_>>()}),
    }
}

pub fn group(fields: Vec<(&str, Instance)>) -> Instance {
    Instance::Group(
        fields
            .into_iter()
            .map(|(name, value)| (name.to_owned(), value))
            .collect::<Vec<_>>()
            .into(),
    )
}

pub fn expected_source(count: usize, width: usize) -> Instance {
    group(vec![
        ("Count", Instance::Scalar(Value::Int(count as i64))),
        (
            "Capture",
            Instance::Scalar(Value::String("x".repeat(width))),
        ),
    ])
}

pub fn expected_output(mode: &str, count: usize, width: usize) -> Instance {
    group(vec![(
        "Rows",
        Instance::Repeated(
            (1..=count)
                .map(|index| {
                    let value = if mode == "numeric" {
                        Value::Int(index as i64)
                    } else {
                        Value::String("x".repeat(width))
                    };
                    group(vec![("Value", Instance::Scalar(value))])
                })
                .collect(),
        ),
    )])
}

pub fn phase(label: &str, started: Instant) -> Result<()> {
    println!(
        "{}",
        json!({"phase":label, "pid":std::process::id(),
        "elapsed_ns":started.elapsed().as_nanos().to_string(),
        "proc_status_original":fs::read_to_string("/proc/self/status")?,
        "proc_stat_original":fs::read_to_string("/proc/self/stat")?})
    );
    std::io::stdout().flush()?;
    Ok(())
}

pub fn stat(path: &Path) -> Result<Json> {
    let s = fs::metadata(path)?;
    Ok(
        json!({"dev":s.dev(), "ino":s.ino(), "mode":s.mode(), "uid":s.uid(),
        "gid":s.gid(), "nlink":s.nlink(), "bytes":s.len(), "mtime":s.mtime(),
        "mtime_nsec":s.mtime_nsec(), "ctime":s.ctime(), "ctime_nsec":s.ctime_nsec()}),
    )
}

pub fn retain_file(path: &Path, destination: &Path) -> Result<Json> {
    let before = stat(path)?;
    let mut input = File::open(path)?;
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)?;
    let mut hash = Sha256::new();
    let mut length = 0_u64;
    let mut buffer = [0_u8; 65_536];
    loop {
        let n = input.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        output.write_all(&buffer[..n])?;
        hash.update(&buffer[..n]);
        length += n as u64;
    }
    output.flush()?;
    output.sync_all()?;
    let after = stat(path)?;
    Ok(
        json!({"path":path, "retained":destination, "before":before, "after":after,
        "bytes":length, "sha256":format!("{:x}", hash.finalize()),
        "stable":before == after && before["bytes"].as_u64() == Some(length)}),
    )
}

pub fn verify_bytes(expected: &Path, actual: &Path, directory: &Path) -> Result<bool> {
    // Full originals are retained before any equality assertion, including unequal files.
    let expected_row = retain_file(expected, &directory.join("expected-bytes.original.json"))?;
    let actual_row = retain_file(actual, &directory.join("timed-bytes.original.json"))?;
    retain(
        &directory.join("byte-identities.original.json"),
        &json!({"expected":expected_row, "actual":actual_row}),
    )?;
    let mut a = File::open(directory.join("expected-bytes.original.json"))?;
    let mut b = File::open(directory.join("timed-bytes.original.json"))?;
    let mut equal = true;
    let mut left = [0_u8; 65_536];
    let mut right = [0_u8; 65_536];
    loop {
        // read_exact-like filling avoids unequal read chunk sizes being mistaken for byte mismatches.
        let fill = |file: &mut File, buffer: &mut [u8]| -> std::io::Result<usize> {
            let mut n = 0;
            while n < buffer.len() {
                let read = file.read(&mut buffer[n..])?;
                if read == 0 {
                    break;
                }
                n += read;
            }
            Ok(n)
        };
        let na = fill(&mut a, &mut left)?;
        let nb = fill(&mut b, &mut right)?;
        equal &= na == nb && left[..na] == right[..nb];
        if na == 0 && nb == 0 {
            break;
        }
    }
    equal &= expected_row["stable"] == true && actual_row["stable"] == true;
    retain(
        &directory.join("byte-comparison.original.json"),
        &json!({"complete_eof_equal":equal}),
    )?;
    Ok(equal)
}

pub fn paths(args: &[String]) -> Result<Vec<PathBuf>> {
    let paths: Vec<_> = args.iter().map(PathBuf::from).collect();
    require(
        paths.iter().all(|p| p.is_absolute()),
        "absolute file arguments required",
    )?;
    Ok(paths)
}
