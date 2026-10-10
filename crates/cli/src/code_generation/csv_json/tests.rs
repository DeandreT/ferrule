use super::*;
use std::error::Error;
use std::path::PathBuf;

fn project() -> Result<Project, Box<dyn Error>> {
    Ok(serde_json::from_str(include_str!(
        "../../../../codegen/src/csv_json_boundary/fixtures/project.json"
    ))?)
}

fn evidence(label: &str) -> Result<PathBuf, Box<dyn Error>> {
    let path = std::env::temp_dir().join(format!(
        "ferrule-csv-json-cli-{label}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos()
    ));
    std::fs::create_dir(&path)?;
    Ok(path)
}

fn mutated(case: &serde_json::Value) -> Result<Project, Box<dyn Error>> {
    let mut value = serde_json::to_value(project()?)?;
    for change in case["typed_mutations"]
        .as_array()
        .ok_or("mutation list required")?
    {
        let (key, change) = change
            .as_object()
            .ok_or("one mutation required")?
            .iter()
            .next()
            .ok_or("empty mutation")?;
        match key.as_str() {
            "source_options_replace" => value["source_options"] = change.clone(),
            "primary_options_replace" => value["target_options"] = change.clone(),
            "source_path" => value["source_path"] = change.clone(),
            "target_path" => value["target_path"] = change.clone(),
            "primary_options_set" => {
                for (name, child) in change.as_object().ok_or("options object required")? {
                    value["target_options"][name] = child.clone();
                }
            }
            "named_options_set" => {
                let index = change["index"].as_u64().ok_or("target index required")? as usize;
                for (name, child) in change["options"]
                    .as_object()
                    .ok_or("options object required")?
                {
                    value["extra_targets"][index]["options"][name] = child.clone();
                }
            }
            "named_path" => {
                let index = change["index"].as_u64().ok_or("target index required")? as usize;
                value["extra_targets"][index]["path"] = change["path"].clone();
            }
            other => return Err(format!("unhandled frozen mutation {other}").into()),
        }
    }
    Ok(serde_json::from_value(value)?)
}

#[test]
fn csv_json_complete_owned_format_policy_oracles() -> Result<(), Box<dyn Error>> {
    let root = evidence("identity")?;
    let cases: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../codegen/src/csv_json_boundary/fixtures/admission-cases.json"
    ))?;
    let mut outcomes = Vec::new();
    for case in cases.as_array().ok_or("case array required")? {
        if case["entry"] != "cli_policy" {
            continue;
        }
        let input = mutated(case)?;
        let result = policy(&input);
        std::fs::write(
            root.join(format!(
                "{}-COMPLETE.original.txt",
                case["id"].as_str().ok_or("case id required")?
            )),
            format!("{input:#?}\n{result:#?}"),
        )?;
        outcomes.push((case.clone(), result));
    }
    assert_eq!(outcomes.len(), 5);
    for (case, result) in outcomes {
        let error = result.expect_err("frozen policy refusal was admitted");
        assert_eq!(
            error.to_string(),
            case["expected"]["message"].as_str().unwrap()
        );
        if case["expected"]["type"] == "CsvJsonBoundaryError" {
            let CsvJsonBoundaryError::JsonFormatOptions { owner, error } = error
                .downcast_ref::<CsvJsonBoundaryError>()
                .ok_or("original typed policy cause missing")?
            else {
                return Err("wrong typed policy error".into());
            };
            assert!(matches!(
                error.as_ref(),
                codegen::X12BoundaryPolicyError::FormatOptions
            ));
            assert_eq!(
                error.to_string(),
                case["expected"]["fields"]["error"]["message"]
                    .as_str()
                    .unwrap()
            );
            match owner {
                CsvJsonBoundaryOwner::Target => {
                    assert_eq!(case["expected"]["fields"]["owner"]["variant"], "Target")
                }
                CsvJsonBoundaryOwner::NamedTarget { index, name } => {
                    assert_eq!(
                        *index as u64,
                        case["expected"]["fields"]["owner"]["index"]
                            .as_u64()
                            .unwrap()
                    );
                    assert_eq!(
                        name,
                        case["expected"]["fields"]["owner"]["name"]
                            .as_str()
                            .unwrap()
                    );
                }
                CsvJsonBoundaryOwner::Source => {
                    return Err("JSON option cause attributed to CSV source".into());
                }
            }
            assert!(error.source().is_none());
        }
    }
    let mut input = project()?;
    input.source_path = Some("authored.txt".into());
    let positive = policy(&input);
    std::fs::write(
        root.join("EXPLICIT-CSV.original.txt"),
        format!("{input:#?}\n{positive:#?}"),
    )?;
    assert_eq!(
        positive?.extra_target_names,
        input
            .extra_targets
            .iter()
            .map(|target| target.name.clone())
            .collect::<Vec<_>>()
    );
    input.source_options.tabular_kind = Some(TabularBoundaryKind::Xlsx);
    input.source_path = Some("authored.csv".into());
    let foreign = policy(&input);
    std::fs::write(
        root.join("FOREIGN-CSV.original.txt"),
        format!("{input:#?}\n{foreign:#?}"),
    )?;
    assert_eq!(
        foreign.unwrap_err().to_string(),
        "generated CSV-to-JSON source requires an explicit CSV format identity"
    );
    Ok(())
}

#[test]
fn csv_json_backend_and_admission_preserve_existing_output() -> Result<(), Box<dyn Error>> {
    let root = evidence("atomic")?;
    let missing = root.join("missing-project.json");
    let output = root.join("absent-parent/output");
    let backend = crate::generate_project_with_csv_json_adapters(
        &missing,
        &output,
        crate::GenerateTarget::Rust {
            runtime_path: root.join("absent-runtime"),
        },
    );
    std::fs::write(root.join("BACKEND.original.txt"), format!("{backend:#?}"))?;
    assert_eq!(
        backend.unwrap_err().to_string(),
        "generated CSV-to-JSON adapters require the C# backend"
    );
    assert!(!output.parent().unwrap().exists());
    let mut input = project()?;
    input.extra_targets[0]
        .options
        .json_schema_unresolved_reference = Some("authored-absent.schema.json".into());
    let file = root.join("project.json");
    std::fs::write(&file, serde_json::to_vec_pretty(&input)?)?;
    let output = root.join("output");
    std::fs::create_dir(&output)?;
    let sentinel = b"AUTHORED EXISTING OUTPUT\n";
    std::fs::write(output.join("sentinel"), sentinel)?;
    let result = crate::generate_project_with_csv_json_adapters(
        &file,
        &output,
        crate::GenerateTarget::CSharp,
    );
    let after = std::fs::read(output.join("sentinel"));
    let entries = std::fs::read_dir(&output)?
        .map(|entry| entry.map(|entry| entry.file_name()))
        .collect::<Result<Vec<_>, _>>()?;
    std::fs::write(
        root.join("COMPLETE.original.txt"),
        format!("{input:#?}\n{result:#?}\n{after:#?}\n{entries:#?}"),
    )?;
    assert_eq!(
        result.unwrap_err().to_string(),
        "generated CSV-to-JSON NamedTarget { index: 0, name: \"Archive\" } requires plain strict JSON format options"
    );
    assert_eq!(after?, sentinel);
    assert_eq!(entries.len(), 1);
    Ok(())
}
