use super::*;
use std::error::Error;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

fn project() -> Result<Project, Box<dyn Error>> {
    Ok(serde_json::from_str(include_str!(
        "../../../../codegen/src/csv_x12_boundary/fixtures/constant-envelope-project.json"
    ))?)
}

fn evidence(label: &str) -> Result<PathBuf, Box<dyn Error>> {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let path = std::env::temp_dir().join(format!(
        "ferrule-csv-x12-cli-{label}-{}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&path)?;
    Ok(path)
}

#[test]
fn csv_x12_identity_is_owned_explicit_and_never_schema_sniffed() -> Result<(), Box<dyn Error>> {
    let root = evidence("identity")?;
    let cases: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../codegen/src/csv_x12_boundary/fixtures/cli-cases.json"
    ))?;
    let mut outcomes = Vec::new();
    for case in cases.as_array().ok_or("CLI cases must be an array")? {
        if case["changes"].is_null() {
            continue;
        }
        let mut input = serde_json::to_value(project()?)?;
        for (key, value) in case["changes"]
            .as_object()
            .ok_or("changes must be an object")?
        {
            input[key] = value.clone();
        }
        let input: Project = serde_json::from_value(input)?;
        let result = policy(&input);
        std::fs::write(
            root.join(format!(
                "{}-COMPLETE.original.txt",
                case["id"].as_str().unwrap()
            )),
            format!("{input:#?}\n{result:#?}"),
        )?;
        outcomes.push((case.clone(), result));
    }
    assert_eq!(outcomes.len(), 7);
    for (case, result) in outcomes {
        if let Some(expected) = case["expected_error"].as_str() {
            assert_eq!(result.unwrap_err().to_string(), expected);
        } else {
            result?;
        }
    }
    // A foreign explicit family cannot be overwritten by a matching suffix.
    let mut foreign = project()?;
    foreign.source_options.tabular_kind = Some(TabularBoundaryKind::Xlsx);
    let result = policy(&foreign);
    std::fs::write(
        root.join("FOREIGN-CSV.original.txt"),
        format!("{foreign:#?}\n{result:#?}"),
    )?;
    assert_eq!(
        result.unwrap_err().to_string(),
        "generated CSV-to-X12 source requires an explicit CSV format identity"
    );
    Ok(())
}

#[test]
fn csv_x12_backend_and_atomic_refusals_preserve_destination() -> Result<(), Box<dyn Error>> {
    let root = evidence("atomic")?;
    let missing = root.join("absent-project.json");
    let absent_output = root.join("absent-parent/output");
    let backend = crate::generate_project_with_csv_x12_adapters(
        &missing,
        &absent_output,
        crate::GenerateTarget::Rust {
            runtime_path: root.join("absent-runtime"),
        },
    );
    std::fs::write(root.join("BACKEND.original.txt"), format!("{backend:#?}"))?;
    assert_eq!(
        backend.unwrap_err().to_string(),
        "generated CSV-to-X12 adapters require the C# backend"
    );
    assert!(!absent_output.parent().unwrap().exists());
    let sentinel = b"AUTHORED EXISTING OUTPUT\n";
    let mut outcomes = Vec::new();
    for (label, expected) in [
        (
            "source-identity",
            "generated CSV-to-X12 source requires an explicit CSV format identity",
        ),
        (
            "target-identity",
            "generated CSV-to-X12 target requires an explicit X12 format identity",
        ),
        (
            "foreign-options",
            "generated CSV input cannot use conflicting format options",
        ),
        (
            "source-shape",
            "generated CSV source schema at column Some(0): each column must have one non-repeating scalar type",
        ),
    ] {
        let mut input = project()?;
        match label {
            "source-identity" => {
                input.source_path = None;
                input.source_options = Default::default();
            }
            "target-identity" => {
                input.target_path = None;
                input.target_options = Default::default();
            }
            "foreign-options" => input.source_options.json_document = true,
            "source-shape" => {
                input.source =
                    ir::SchemaNode::group("Row", vec![ir::SchemaNode::group("Nested", Vec::new())])
            }
            _ => unreachable!(),
        }
        let directory = root.join(label);
        std::fs::create_dir(&directory)?;
        let file = directory.join("project.json");
        std::fs::write(&file, serde_json::to_vec_pretty(&input)?)?;
        let output = directory.join("output");
        std::fs::create_dir(&output)?;
        std::fs::write(output.join("existing-output.sentinel"), sentinel)?;
        let result = crate::generate_project_with_csv_x12_adapters(
            &file,
            &output,
            crate::GenerateTarget::CSharp,
        );
        let after = std::fs::read(output.join("existing-output.sentinel"));
        let entries: Vec<_> = std::fs::read_dir(&output)?
            .map(|entry| entry.map(|entry| entry.file_name()))
            .collect();
        std::fs::write(
            directory.join("COMPLETE.original.txt"),
            format!("{input:#?}\n{result:#?}\n{after:#?}\n{entries:#?}"),
        )?;
        outcomes.push((result, after, entries, expected));
    }
    for (result, after, entries, expected) in outcomes {
        assert_eq!(result.unwrap_err().to_string(), expected);
        assert_eq!(after?, sentinel);
        assert_eq!(entries.len(), 1);
    }
    Ok(())
}

#[test]
fn csv_x12_explicit_target_selection_preserves_every_captured_option() -> Result<(), Box<dyn Error>>
{
    let mut input = project()?;
    input.source_options.delimiter = Some(';');
    input.source_options.csv_preserve_empty_strings = false;
    input.target_options.lenient_segments = true;
    let before = serde_json::to_value(&input)?;
    let ordinary = policy(&input)?;
    assert_eq!(policy_with_envelope_profile(&input, None)?, ordinary);
    assert_eq!(
        policy_with_envelope_profile(&input, Some(codegen::X12EnvelopeProfile::SingleTransaction))?,
        ordinary,
    );
    let grouped = policy_with_envelope_profile(
        &input,
        Some(codegen::X12EnvelopeProfile::GroupedTransactions),
    )?;
    let mut expected = ordinary;
    expected.target.envelope_profile = codegen::X12EnvelopeProfile::GroupedTransactions;
    assert_eq!(grouped, expected);
    assert_eq!(serde_json::to_value(&input)?, before);
    input.source_options.tabular_kind = Some(TabularBoundaryKind::Xlsx);
    assert_eq!(
        policy_with_envelope_profile(
            &input,
            Some(codegen::X12EnvelopeProfile::GroupedTransactions)
        )
        .unwrap_err()
        .to_string(),
        policy(&input).unwrap_err().to_string(),
    );
    Ok(())
}

#[test]
fn csv_x12_explicit_grouped_selection_keeps_complete_borrowed_grammar_guards()
-> Result<(), Box<dyn Error>> {
    let mut input = project()?;
    input.target = serde_json::from_str(include_str!(
        "../../../../codegen-csharp/tests/x12/grouped/fixtures/004010-schema.json"
    ))?;
    input.graph = mapping::Graph::default();
    input.root = mapping::Scope::default();
    let before = serde_json::to_value(&input)?;
    let default = policy_with_envelope_profile(&input, None)?;
    assert!(matches!(
        codegen::prepare_csv_x12_project_boundary(&input, &default),
        Err(codegen::CsvX12BoundaryError::TargetX12(_)),
    ));
    let selected = policy_with_envelope_profile(
        &input,
        Some(codegen::X12EnvelopeProfile::GroupedTransactions),
    )?;
    let profile = codegen::prepare_csv_x12_project_boundary(&input, &selected)?;
    let descriptor: serde_json::Value = serde_json::from_str(&profile.target_descriptor)?;
    assert_eq!(descriptor["envelope_profile"], "grouped_transactions");
    assert_eq!(serde_json::to_value(&input)?, before);
    // A selection cannot reinterpret an extra wrapper as the physical envelope.
    input.target = ir::SchemaNode::group("Extra", vec![input.target]);
    assert!(matches!(
        codegen::prepare_csv_x12_project_boundary(&input, &selected),
        Err(codegen::CsvX12BoundaryError::TargetX12(_)),
    ));
    Ok(())
}

#[test]
fn csv_x12_selected_target_backend_refusal_still_precedes_any_output() -> Result<(), Box<dyn Error>>
{
    let root = evidence("explicit-backend")?;
    for (label, selection) in [
        ("absent", None),
        (
            "single",
            Some(codegen::X12EnvelopeProfile::SingleTransaction),
        ),
        (
            "grouped",
            Some(codegen::X12EnvelopeProfile::GroupedTransactions),
        ),
    ] {
        let output = root.join(label).join("output");
        let result = crate::generate_project_with_csv_x12_envelope_profile(
            &root.join("absent-project.json"),
            &output,
            crate::GenerateTarget::Rust {
                runtime_path: root.join("absent-runtime"),
            },
            selection,
        );
        assert_eq!(
            result.unwrap_err().to_string(),
            "generated CSV-to-X12 adapters require the C# backend"
        );
        assert!(!output.parent().unwrap().exists());
    }
    Ok(())
}
