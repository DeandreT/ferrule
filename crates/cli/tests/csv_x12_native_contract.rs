//! Authored complete native CSV rows feeding one singular X12 mapping.
use ir::{Instance, Value};
use mapping::Project;
use serde_json::{Value as Json, json};
use std::error::Error;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const PROJECT: &[u8] = include_bytes!("csv_x12_native/fixtures/project.json");
const CONTEXT: &[u8] = include_bytes!("csv_x12_native/fixtures/context.json");
const CASES: &[u8] = include_bytes!("csv_x12_native/fixtures/cases.json");
const FIXTURES: &[(&str, &[u8])] = &[
    ("project.json", PROJECT),
    ("context.json", CONTEXT),
    ("cases.json", CASES),
    (
        "zero.csv",
        include_bytes!("csv_x12_native/fixtures/zero.csv"),
    ),
    ("one.csv", include_bytes!("csv_x12_native/fixtures/one.csv")),
    (
        "multiple.csv",
        include_bytes!("csv_x12_native/fixtures/multiple.csv"),
    ),
    (
        "zero-source-tags.json",
        include_bytes!("csv_x12_native/fixtures/zero-source-tags.json"),
    ),
    (
        "one-source-tags.json",
        include_bytes!("csv_x12_native/fixtures/one-source-tags.json"),
    ),
    (
        "multiple-source-tags.json",
        include_bytes!("csv_x12_native/fixtures/multiple-source-tags.json"),
    ),
    (
        "zero-typed-target-tags.json",
        include_bytes!("csv_x12_native/fixtures/zero-typed-target-tags.json"),
    ),
    (
        "one-typed-target-tags.json",
        include_bytes!("csv_x12_native/fixtures/one-typed-target-tags.json"),
    ),
    (
        "multiple-typed-target-tags.json",
        include_bytes!("csv_x12_native/fixtures/multiple-typed-target-tags.json"),
    ),
    (
        "zero-expected.x12",
        include_bytes!("csv_x12_native/fixtures/zero-expected.x12"),
    ),
    (
        "one-expected.x12",
        include_bytes!("csv_x12_native/fixtures/one-expected.x12"),
    ),
    (
        "multiple-expected.x12",
        include_bytes!("csv_x12_native/fixtures/multiple-expected.x12"),
    ),
];

fn fixture(name: &str) -> Result<&'static [u8], Box<dyn Error>> {
    FIXTURES
        .iter()
        .find(|(path, _)| *path == name)
        .map(|(_, body)| *body)
        .ok_or_else(|| format!("missing authored fixture {name}").into())
}

fn tagged(instance: &Instance) -> Json {
    match instance {
        Instance::Repeated(items) => {
            json!({"kind":"Repeated","items":items.iter().map(tagged).collect::<Vec<_>>()})
        }
        Instance::Group(fields) => {
            let origin = match fields.xml_type_origin() {
                ir::XmlTypeOrigin::Unknown => {
                    json!({"kind":"Unknown","identity":null,"literal":null})
                }
                ir::XmlTypeOrigin::Absent => {
                    json!({"kind":"Absent","identity":null,"literal":null})
                }
                ir::XmlTypeOrigin::Explicit(identity) => {
                    json!({"kind":"Explicit","identity":identity,"literal":null})
                }
                ir::XmlTypeOrigin::ExplicitPadded {
                    literal,
                    resolved_identity,
                } => {
                    json!({"kind":"ExplicitPadded","identity":resolved_identity,"literal":literal})
                }
            };
            json!({"kind":"Group","xml_type_origin":origin,"fields":fields.iter().map(|(name,value)|json!({"name":name,"value":tagged(value)})).collect::<Vec<_>>()})
        }
        Instance::Scalar(value) => match value {
            Value::Null => json!({"kind":"Null"}),
            Value::String(value) => json!({"kind":"String","value":value}),
            Value::Int(value) => json!({"kind":"Int","value":value}),
            Value::Float(value) => {
                json!({"kind":"Float","bits":format!("{:016x}",value.to_bits())})
            }
            Value::Bool(value) => json!({"kind":"Bool","value":value}),
            other => json!({"unqualified_scalar":format!("{other:#?}")}),
        },
        other => json!({"unqualified_instance":format!("{other:#?}")}),
    }
}

fn original(root: &Path, label: &str, value: &impl std::fmt::Debug) -> Result<(), Box<dyn Error>> {
    std::fs::write(
        root.join(format!("{label}-ORIGINAL.txt")),
        format!("{value:#?}\n"),
    )?;
    Ok(())
}

#[test]
fn complete_csv_rows_feed_one_x12_document_without_dropping_rows() -> Result<(), Box<dyn Error>> {
    let root: PathBuf = std::env::temp_dir().join(format!(
        "ferrule-csv-x12-275-native-{}-{}",
        std::process::id(),
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
    ));
    std::fs::create_dir_all(root.join("fixtures"))?;
    for (name, body) in FIXTURES {
        std::fs::write(root.join("fixtures").join(name), body)?;
    }
    eprintln!("CSV_X12_275_NATIVE_ORIGINALS={}", root.display());
    let project: Project = serde_json::from_slice(PROJECT)?;
    original(&root, "PROJECT", &project)?;
    let validation = engine::validate(&project);
    original(&root, "VALIDATION", &validation)?;
    if !validation.is_empty() {
        return Err("authored all-row native project did not validate".into());
    }
    let lowered = codegen::lower(&project);
    original(&root, "ORDINARY-LOWERING", &lowered)?;
    let program = lowered?;
    let compiled_validation = codegen::validate_program(&program);
    original(&root, "ORDINARY-PROGRAM-VALIDATION", &compiled_validation)?;
    compiled_validation?;
    let context: Json = serde_json::from_slice(CONTEXT)?;
    let current_datetime = context["current_date_time"]
        .as_str()
        .ok_or("authored context")?;
    let execution = engine::ExecutionContext::with_main_mapping_file_path(
        Path::new("authored/rows.json"),
        Path::new("authored/main.json"),
    )
    .with_current_datetime(current_datetime);
    let cases: Json = serde_json::from_slice(CASES)?;
    let cases = cases["cases"].as_array().ok_or("authored cases")?;
    let options = format_csv::CsvReadOptions::from(&project.source_options);
    let syntax = project
        .target_options
        .x12_separators
        .ok_or("authored target separators")?;
    let mut comparisons = Vec::new();
    for case in cases {
        let id = case["id"].as_str().ok_or("authored case id")?;
        let input = fixture(case["input"].as_str().ok_or("authored input")?)?;
        let parsed = format_csv::from_str_with_options(
            std::str::from_utf8(input)?,
            &project.source,
            &options,
        );
        original(&root, &format!("{id}-COMPLETE-PARSE"), &parsed)?;
        let source = Instance::Repeated(parsed?);
        let before = tagged(&source);
        let expected_source: Json = serde_json::from_slice(fixture(
            case["source"].as_str().ok_or("authored source tags")?,
        )?)?;
        let outcome = engine::run_with_context(&project, &source, &execution);
        original(&root, &format!("{id}-COMPLETE-TYPED-OUTCOME"), &outcome)?;
        let target = outcome?;
        let target_before = tagged(&target);
        let expected_target: Json = serde_json::from_slice(fixture(
            case["typed_target"]
                .as_str()
                .ok_or("authored target tags")?,
        )?)?;
        let wire = format_edi::x12::to_string_with_syntax_and_autocomplete(
            &project.target,
            &target,
            format_edi::x12::Separators {
                element: syntax.element,
                component: syntax.component,
                segment: syntax.segment,
                repetition: syntax.repetition,
                release: syntax.release,
            },
            project.target_options.x12_interchange_version.as_deref(),
            format_edi::x12::Autocomplete {
                current_datetime,
                request_acknowledgement: false,
                transaction_set: Some("940"),
            },
        );
        original(&root, &format!("{id}-COMPLETE-WRITE"), &wire)?;
        let wire = wire?;
        std::fs::write(root.join(format!("{id}-ACTUAL.x12")), wire.as_bytes())?;
        let expected_wire = fixture(case["wire"].as_str().ok_or("authored wire")?)?;
        let after_source = tagged(&source);
        let after_target = tagged(&target);
        let full = json!({"expected_source":expected_source,"actual_source":before,"source_after":after_source,"expected_typed_target":expected_target,"actual_typed_target":target_before,"target_after":after_target});
        std::fs::write(
            root.join(format!("{id}-COMPLETE-TAGS.json")),
            serde_json::to_vec_pretty(&full)?,
        )?;
        let matched = full["actual_source"] == full["expected_source"]
            && full["source_after"] == full["actual_source"]
            && full["actual_typed_target"] == full["expected_typed_target"]
            && full["target_after"] == full["actual_typed_target"]
            && wire.as_bytes() == expected_wire;
        comparisons.push(json!({"id":id,"matched":matched}));
    }
    std::fs::write(
        root.join("ALL-COMPLETE-COMPARISONS.json"),
        serde_json::to_vec_pretty(&comparisons)?,
    )?;
    if cases.len() != 3 || comparisons.iter().any(|row| row["matched"] != true) {
        return Err("complete native all-row graph or wire differs from authored literals".into());
    }
    Ok(())
}
