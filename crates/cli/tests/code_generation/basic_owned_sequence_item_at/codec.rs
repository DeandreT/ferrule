//! Local test recipe codec: lossless scalar tags, float bits and ordered fields.
use codegen_runtime::{Instance, Value, XmlTypeOrigin};
use serde_json::{Value as Json, json};

/// Construct only the independently authored finite function-error recipes.
pub fn function_error(recipe: &Json) -> codegen_runtime::FunctionError {
    let function = match recipe["function"].as_str().unwrap() {
        "tokenize" => "tokenize",
        "tokenize-by-length" => "tokenize-by-length",
        "tokenize-regexp" => "tokenize-regexp",
        "generate-sequence" => "generate-sequence",
        _ => panic!("closed authored function"),
    };
    if recipe["kind"] == "FunctionType" {
        let got = match recipe["detail"].as_str().unwrap() {
            "xml nil" => "xml nil",
            "int" => "int",
            "float" => "float",
            "bool" => "bool",
            "string" => "string",
            "null" => "null",
            "json null" => "json null",
            _ => panic!("closed authored type"),
        };
        codegen_runtime::FunctionError::TypeMismatch { function, got }
    } else {
        let message = match recipe["detail"].as_str().unwrap() {
            "requires a non-empty delimiter" => "requires a non-empty delimiter",
            "requires a positive integer length" => "requires a positive integer length",
            _ => panic!("closed authored argument detail"),
        };
        codegen_runtime::FunctionError::InvalidArgument { function, message }
    }
}

/// Retain Debug/cause diagnostics and the exhaustive product-specific raw tree
/// before the deliberately closed recipe observer runs. Debug alone is lossy.
/// Observation panics fail this call, not the remaining cohort. Evidence I/O is
/// outside that catch and keeps its concrete error type.
#[allow(dead_code)] // Standalone Starting hosts use the opt-in entry instead.
pub fn retain_outcome<T, E>(
    invocation: std::thread::Result<Result<T, E>>,
    write: impl FnMut(&str, &[u8]) -> std::io::Result<()>,
    capture: impl FnOnce(&T) -> Json,
    observe: impl FnOnce(&Result<T, E>) -> Json,
) -> Result<Option<Json>, Box<dyn std::error::Error>>
where
    T: std::fmt::Debug,
    E: std::error::Error + 'static,
{
    retain_outcome_inner(invocation, write, capture, None::<fn() -> Json>, observe)
}

/// Opt-in post-call retention. The hook reads the actual caller variables after
/// invocation, independently of output projection/observation, including panic.
/// Existing callers keep their exact output format through retain_outcome.
#[allow(dead_code)] // Older standalone hosts deliberately stay non-opt-in.
pub fn retain_outcome_with_input_after<T, E>(
    invocation: std::thread::Result<Result<T, E>>,
    write: impl FnMut(&str, &[u8]) -> std::io::Result<()>,
    capture: impl FnOnce(&T) -> Json,
    input_after: impl FnOnce() -> Json,
    observe: impl FnOnce(&Result<T, E>) -> Json,
) -> Result<Option<Json>, Box<dyn std::error::Error>>
where
    T: std::fmt::Debug,
    E: std::error::Error + 'static,
{
    retain_outcome_inner(invocation, write, capture, Some(input_after), observe)
}

fn retain_input_after(
    captured: Option<std::thread::Result<Json>>,
    write: &mut impl FnMut(&str, &[u8]) -> std::io::Result<()>,
) -> std::io::Result<Option<(bool, bool)>> {
    match captured {
        None => Ok(None),
        Some(Ok(state)) => {
            let unchanged = state["unchanged"] == true;
            let bytes = serde_json::to_vec_pretty(&state).map_err(std::io::Error::other)?;
            write("COMPLETE-INPUT-AFTER.json", &bytes)?;
            Ok(Some((true, unchanged)))
        }
        Some(Err(payload)) => {
            write(
                "COMPLETE-INPUT-AFTER-FAILURE.txt",
                panic_record(&payload).as_bytes(),
            )?;
            Ok(Some((false, false)))
        }
    }
}

fn retain_outcome_inner<T, E>(
    invocation: std::thread::Result<Result<T, E>>,
    mut write: impl FnMut(&str, &[u8]) -> std::io::Result<()>,
    capture: impl FnOnce(&T) -> Json,
    input_after: Option<impl FnOnce() -> Json>,
    observe: impl FnOnce(&Result<T, E>) -> Json,
) -> Result<Option<Json>, Box<dyn std::error::Error>>
where
    T: std::fmt::Debug,
    E: std::error::Error + 'static,
{
    let opt_in = input_after.is_some();
    // Snapshot actual post-call state BEFORE any product encoder or observer.
    // Hold the result while the original raw return/error/panic is retained.
    let after =
        input_after.map(|hook| std::panic::catch_unwind(std::panic::AssertUnwindSafe(hook)));
    let product = match invocation {
        Ok(product) => product,
        Err(payload) => {
            let raw = write(
                "COMPLETE-INVOCATION-PANIC.txt",
                panic_record(&payload).as_bytes(),
            );
            let state = retain_input_after(after, &mut write);
            raw?;
            state?;
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
    let raw_text = write("COMPLETE-RAW-PRODUCT.txt", raw.as_bytes());
    // Attempt both reached records; a hook/capture failure cannot erase the raw
    // return, and a raw write failure cannot suppress the reached post-call hook.
    let after_record = retain_input_after(after, &mut write);
    raw_text?;
    let encode = || match &product {
        Ok(value) => json!({"Ok":capture(value)}),
        Err(error) => json!({"Err":{"debug":format!("{error:#?}"),"display":error.to_string()}}),
    };
    let lossless = if opt_in {
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(encode)) {
            Ok(value) => value,
            Err(payload) => {
                let failure = write(
                    "COMPLETE-RAW-CAPTURE-FAILURE.txt",
                    panic_record(&payload).as_bytes(),
                );
                after_record?;
                failure?;
                return Ok(None);
            }
        }
    } else {
        encode()
    };
    let raw_json = write(
        "COMPLETE-RAW-PRODUCT.json",
        &serde_json::to_vec_pretty(&lossless)?,
    );
    let retained_after = after_record?;
    raw_json?;
    let observation = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| observe(&product)));
    match observation {
        Ok(mut record) => {
            if let Some((complete, unchanged)) = retained_after {
                record["input_after_complete"] = json!(complete);
                record["input_after_unchanged"] = json!(unchanged);
                record["matched"] = json!(record["matched"] == true && complete && unchanged);
            }
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
