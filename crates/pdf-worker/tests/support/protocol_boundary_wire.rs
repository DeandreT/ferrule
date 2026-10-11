//! Shared fixture352 wire stimulus transformations; independent expectations live in the frozen catalog.
use pdf_worker::{MAX_CONTROL_FRAME_BYTES, MAX_REQUEST_BYTES, MAX_RESULT_BYTES};
use serde_json::{Value, json};
use std::io;

pub(super) fn mutate(base: &[u8], change: &str) -> io::Result<Vec<u8>> {
    if change == "malformed" {
        return Ok(b"{invalid".to_vec());
    }
    if change == "trailing" {
        let mut b = base.to_vec();
        b.extend_from_slice(b"{}");
        return Ok(b);
    }
    if change == "duplicate" {
        let mut b = b"{\"protocol\":\"ferrule.pdf-worker/1\",".to_vec();
        b.extend_from_slice(&base[1..]);
        return Ok(b);
    }
    let mut value: Value = serde_json::from_slice(base).map_err(io::Error::other)?;
    match change {
        "valid" => return Ok(base.to_vec()),
        "version" => value["protocol"] = json!("ferrule.pdf-worker/9"),
        "wrong_id" => value["request_id"] = json!("ffffffffffffffffffffffffffffffff"),
        "profile" => value["profile"] = json!("unsupported-profile"),
        "unknown" => {
            value
                .as_object_mut()
                .unwrap()
                .insert("unexpected".into(), json!(true));
        }
        "limits" => value["effective_limits"]["address_space_bytes"] = json!(536870911u64),
        "false_start" => value["start"] = json!(false),
        "order" => {
            let mut entries = value.as_object().unwrap().iter().collect::<Vec<_>>();
            entries.reverse();
            let text = entries
                .into_iter()
                .map(|(k, v)| {
                    format!(
                        "{}:{}",
                        serde_json::to_string(k).unwrap(),
                        serde_json::to_string(v).unwrap()
                    )
                })
                .collect::<Vec<_>>()
                .join(",");
            return Ok(format!("{{{text}}}").into_bytes());
        }
        "exact" | "over" => {
            let cap = if value.get("effective_limits").is_some() {
                MAX_CONTROL_FRAME_BYTES
            } else if value.get("layout").is_some() {
                MAX_REQUEST_BYTES
            } else {
                MAX_RESULT_BYTES
            };
            let mut bytes = base.to_vec();
            bytes.resize(cap + usize::from(change == "over"), b' ');
            return Ok(bytes);
        }
        _ => return Err(io::Error::other("unknown fixed352 mutation")),
    }
    serde_json::to_vec(&value).map_err(io::Error::other)
}
