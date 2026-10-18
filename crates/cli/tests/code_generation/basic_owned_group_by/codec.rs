//! Local test recipe codec: lossless scalar tags, float bits and ordered fields.
use codegen_runtime::{Instance, Value, XmlTypeOrigin};
use serde_json::{Value as Json, json};

/// Retain Debug/cause diagnostics and the exhaustive product-specific raw tree
/// before the deliberately closed recipe observer runs. Debug alone is lossy.
/// Observation panics fail this call, not the remaining cohort. Evidence I/O is
/// outside that catch and keeps its concrete error type.
pub fn retain_outcome<T, E>(
    invocation: std::thread::Result<Result<T, E>>,
    mut write: impl FnMut(&str, &[u8]) -> std::io::Result<()>,
    capture: impl FnOnce(&T) -> Json,
    observe: impl FnOnce(&Result<T, E>) -> Json,
) -> Result<Option<Json>, Box<dyn std::error::Error>>
where
    T: std::fmt::Debug,
    E: std::error::Error + 'static,
{
    let product = match invocation {
        Ok(product) => product,
        Err(payload) => {
            write(
                "COMPLETE-INVOCATION-PANIC.txt",
                panic_record(&payload).as_bytes(),
            )?;
            return Ok(None);
        }
    };
    let mut raw = format!("RAW_PRODUCT={product:#?}\n");
    if let Err(error) = &product {
        raw.push_str(&format!("Display={error}\n"));
        let mut source = error.source();
        while let Some(error) = source {
            raw.push_str(&format!("CauseDebug={error:#?}\nCauseDisplay={error}\n"));
            source = error.source();
        }
        raw.push_str("CAUSE_CHAIN_TERMINATED=None\n");
    }
    write("COMPLETE-RAW-PRODUCT.txt", raw.as_bytes())?;
    let lossless = match &product {
        Ok(value) => json!({"Ok":capture(value)}),
        Err(error) => json!({"Err":{"debug":format!("{error:#?}"),"display":error.to_string()}}),
    };
    write(
        "COMPLETE-RAW-PRODUCT.json",
        &serde_json::to_vec_pretty(&lossless)?,
    )?;
    let observation = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| observe(&product)));
    match observation {
        Ok(record) => {
            write(
                "COMPLETE-OBSERVATION.json",
                &serde_json::to_vec_pretty(&record)?,
            )?;
            Ok(Some(record))
        }
        Err(payload) => {
            write(
                "COMPLETE-OBSERVATION-FAILURE.txt",
                panic_record(&payload).as_bytes(),
            )?;
            Ok(None)
        }
    }
}

fn panic_record(payload: &Box<dyn std::any::Any + Send>) -> String {
    format!(
        "PANIC={:?}\nPAYLOAD_TYPE={:?}\nBACKTRACE_AT_CAPTURE_AFTER_UNWIND={}\n",
        payload
            .downcast_ref::<String>()
            .map(String::as_str)
            .or_else(|| payload.downcast_ref::<&str>().copied()),
        payload.as_ref().type_id(),
        std::backtrace::Backtrace::force_capture(),
    )
}

fn origin(fields: &codegen_runtime::InstanceGroup) -> Json {
    match fields.xml_type_origin() {
        XmlTypeOrigin::Unknown => json!({"kind":"Unknown","identity":null,"literal":null}),
        XmlTypeOrigin::Absent => json!({"kind":"Absent","identity":null,"literal":null}),
        XmlTypeOrigin::Explicit(identity) => {
            json!({"kind":"Explicit","identity":identity,"literal":null})
        }
        XmlTypeOrigin::ExplicitPadded {
            literal,
            resolved_identity,
        } => {
            json!({"kind":"ExplicitPadded","identity":resolved_identity,"literal":literal})
        }
    }
}

/// Exhaustive raw capture, deliberately separate from the closed corpus codec.
/// Distinct unexpected kinds retain their data; they never become valid output.
pub fn raw_tree(instance: &Instance) -> Json {
    match instance {
        Instance::Scalar(Value::Null) => json!(["Null"]),
        Instance::Scalar(Value::JsonNull(_)) => json!(["JsonNull"]),
        Instance::Scalar(Value::XmlNil(_)) => json!(["XmlNil"]),
        Instance::Scalar(Value::Bool(value)) => json!(["Bool", value]),
        Instance::Scalar(Value::Int(value)) => json!(["Int", value]),
        Instance::Scalar(Value::String(value)) => json!(["String", value]),
        Instance::Scalar(Value::Float(value)) => {
            json!(["Float", format!("{:016x}", value.to_bits())])
        }
        Instance::Group(fields) => json!([
            "Group",
            fields
                .iter()
                .map(|(name, value)| json!([name, raw_tree(value)]))
                .collect::<Vec<_>>(),
            origin(fields)
        ]),
        Instance::Repeated(items) => {
            json!(["Repeated", items.iter().map(raw_tree).collect::<Vec<_>>()])
        }
        Instance::MappedSequence(items) => json!([
            "MappedSequence",
            items.iter().map(raw_tree).collect::<Vec<_>>()
        ]),
        Instance::DocumentSet(documents) => json!([
            "DocumentSet",
            documents
                .iter()
                .map(|document| json!({
                    "path":document.path(),"source_path":document.source_path(),
                    // The public getter resolves None to the portable path. Preserve
                    // original Some/None metadata too: Serialize omits that private
                    // field, while DocumentMember's derived Debug retains it exactly.
                    "member_debug":format!("{document:#?}"),"value":raw_tree(document.value())
                }))
                .collect::<Vec<_>>()
        ]),
    }
}

pub fn encode(instance: &Instance) -> Json {
    match instance {
        Instance::Scalar(Value::Null) => json!(["Null"]),
        Instance::Scalar(Value::JsonNull(_)) => json!(["JsonNull"]),
        Instance::Scalar(Value::XmlNil(_)) => json!(["XmlNil"]),
        Instance::Scalar(Value::Bool(value)) => json!(["Bool", value]),
        Instance::Scalar(Value::Int(value)) => json!(["Int", value]),
        Instance::Scalar(Value::String(value)) => json!(["String", value]),
        Instance::Scalar(Value::Float(value)) => {
            json!(["Float", format!("{:016x}", value.to_bits())])
        }
        Instance::Group(fields) => {
            json!([
                "Group",
                fields
                    .iter()
                    .map(|(name, value)| json!([name, encode(value)]))
                    .collect::<Vec<_>>(),
                origin(fields)
            ])
        }
        Instance::Repeated(items) => {
            json!(["Repeated", items.iter().map(encode).collect::<Vec<_>>()])
        }
        other => panic!("closed test recipe kind {other:?}"),
    }
}
pub fn decode(value: &Json) -> Instance {
    match value[0].as_str().unwrap() {
        "Null" => Instance::Scalar(Value::Null),
        "JsonNull" => Instance::Scalar(Value::json_null()),
        "XmlNil" => Instance::Scalar(Value::xml_nil()),
        "Bool" => Instance::Scalar(Value::Bool(value[1].as_bool().unwrap())),
        "Int" => Instance::Scalar(Value::Int(value[1].as_i64().unwrap())),
        "String" => Instance::Scalar(Value::String(value[1].as_str().unwrap().into())),
        "Float" => Instance::Scalar(Value::Float(f64::from_bits(
            u64::from_str_radix(value[1].as_str().unwrap(), 16).unwrap(),
        ))),
        "Group" => {
            let fields: codegen_runtime::InstanceGroup = value[1]
                .as_array()
                .unwrap()
                .iter()
                .map(|field| (field[0].as_str().unwrap().to_owned(), decode(&field[1])))
                .collect::<Vec<_>>()
                .into();
            let origin = match value[2]["kind"].as_str().unwrap() {
                "Unknown" => XmlTypeOrigin::Unknown,
                "Absent" => XmlTypeOrigin::Absent,
                "Explicit" => XmlTypeOrigin::Explicit(value[2]["identity"].as_str().unwrap()),
                "ExplicitPadded" => XmlTypeOrigin::ExplicitPadded {
                    literal: value[2]["literal"].as_str().unwrap(),
                    resolved_identity: value[2]["identity"].as_str().unwrap(),
                },
                _ => panic!("closed authored origin"),
            };
            Instance::Group(fields.with_xml_type_origin(origin).unwrap())
        }
        "Repeated" => Instance::Repeated(value[1].as_array().unwrap().iter().map(decode).collect()),
        _ => panic!("closed typed recipe"),
    }
}
