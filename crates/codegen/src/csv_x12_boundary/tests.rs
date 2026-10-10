use super::*;
use crate::{
    Expression, ExpressionNode, IterationPlan, NamedSourceProgram, NamedTargetProgram,
    ScalarFunction, XmlBoundaryProgram, XmlInputPolicy, XmlOutputPolicy,
};
use ir::ScalarType;
use mapping::FormatOptions;
use serde_json::{Value as Json, json};
use std::error::Error;
use std::path::{Path, PathBuf};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src/csv_x12_boundary/fixtures")
}

fn literal(name: &str) -> Result<Json, Box<dyn Error>> {
    Ok(serde_json::from_slice(&std::fs::read(
        fixtures().join(name),
    )?)?)
}

fn evidence(label: &str) -> Result<PathBuf, Box<dyn Error>> {
    let path = std::env::temp_dir().join(format!(
        "ferrule-csv-x12-policy-{label}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos()
    ));
    std::fs::create_dir_all(&path)?;
    Ok(path)
}

fn project() -> Result<Project, Box<dyn Error>> {
    Ok(serde_json::from_value(literal(
        "constant-envelope-project.json",
    )?)?)
}

fn policy(project: &Project) -> Result<CsvX12BoundaryPolicy, Box<dyn Error>> {
    Ok(CsvX12BoundaryPolicy {
        source: CsvInputPolicy::from_format_options(&project.source_options)?,
        target: X12BoundaryOptions::from_format_options(&project.target_options)?,
    })
}

fn error(error: &CsvX12BoundaryError) -> Json {
    match error {
        CsvX12BoundaryError::SourceSchema { field, reason } => {
            json!({"variant":"SourceSchema","field":field,"reason":reason})
        }
        CsvX12BoundaryError::SourceNameBytes { maximum, observed } => {
            json!({"variant":"SourceNameBytes","maximum":maximum,"observed":observed})
        }
        CsvX12BoundaryError::SourceFormatOptions => json!({"variant":"SourceFormatOptions"}),
        CsvX12BoundaryError::SourceRepairRequired => json!({"variant":"SourceRepairRequired"}),
        CsvX12BoundaryError::BadDelimiter(value) => {
            json!({"variant":"BadDelimiter","character":value})
        }
        CsvX12BoundaryError::BadQuote(value) => json!({"variant":"BadQuote","character":value}),
        CsvX12BoundaryError::ConflictingQuoteSettings => {
            json!({"variant":"ConflictingQuoteSettings"})
        }
        CsvX12BoundaryError::DelimiterQuoteConflict => json!({"variant":"DelimiterQuoteConflict"}),
        CsvX12BoundaryError::ProgramField { field } => {
            json!({"variant":"ProgramField","field":field})
        }
        CsvX12BoundaryError::EmbeddedSourceSchema(codegen_schema::CodecError::TooLarge {
            bytes,
            max,
        }) => {
            json!({"variant":"EmbeddedSourceSchema","cause":{"variant":"TooLarge","bytes":bytes,"max":max}})
        }
        CsvX12BoundaryError::TargetX12(cause) => match cause.as_ref() {
            X12BoundaryPolicyError::Schema { side, path, reason } => {
                json!({"variant":"TargetX12","cause":{"variant":"Schema","side":format!("{side:?}"),"path":path,"reason":reason}})
            }
            other => json!({"variant":"TargetX12","unexpected":format!("{other:#?}")}),
        },
        CsvX12BoundaryError::Validation(ProgramValidationError::MissingDependency {
            node,
            dependency,
        }) => {
            json!({"variant":"Validation","cause":{"variant":"MissingDependency","node":node,"dependency":dependency}})
        }
        other => json!({"unexpected_typed_error":format!("{other:#?}")}),
    }
}

fn source_policy(policy: &CsvInputPolicy) -> Json {
    json!({"delimiter":policy.delimiter,"quote":policy.quote,"quote_disabled":policy.quote_disabled,
        "has_headers":policy.has_headers,"preserve_empty_strings":policy.preserve_empty_strings,
        "utf8_bom":policy.utf8_bom})
}

fn apply_metadata(node: &mut SchemaNode, field: &str, value: &Json) -> Result<(), Box<dyn Error>> {
    macro_rules! boolean {
        ($field:ident) => {
            node.$field = value.as_bool().ok_or("expected boolean metadata")?
        };
    }
    macro_rules! optional {
        ($field:ident) => {
            node.$field = Some(serde_json::from_value(value.clone())?)
        };
    }
    match field {
        "attribute" => boolean!(attribute),
        "text" => boolean!(text),
        "nullable" => boolean!(nullable),
        "container_nullable" => boolean!(container_nullable),
        "nillable" => boolean!(nillable),
        "xml_optional" => boolean!(xml_optional),
        "xml_attribute_required" => boolean!(xml_attribute_required),
        "json_any" => boolean!(json_any),
        "fixed" => optional!(fixed),
        "default" => optional!(default),
        "recursive_ref" => optional!(recursive_ref),
        "xml_namespace" => optional!(xml_namespace),
        "xml_wildcard_process_contents" => {
            node.xml_wildcard_process_contents = serde_json::from_value(value.clone())?
        }
        "xml_default_type" => optional!(xml_default_type),
        "xml_type_alternatives" => boolean!(xml_type_alternatives),
        "xml_alternative_kind" => {
            node.xml_alternative_kind = serde_json::from_value(value.clone())?
        }
        "alternative_mode" => node.alternative_mode = serde_json::from_value(value.clone())?,
        "json_unique_items" => boolean!(json_unique_items),
        "string_length_range" => optional!(string_length_range),
        "numeric_range" => optional!(numeric_range),
        "database_relation" => optional!(database_relation),
        _ => return Err(format!("unrecognized authored metadata case {field}").into()),
    }
    Ok(())
}

fn schema(case: &Json) -> Result<SchemaNode, Box<dyn Error>> {
    let input = literal(case["schema"].as_str().ok_or("missing schema literal")?)?;
    if case["construction"] == "ordinary_schema" {
        return Ok(serde_json::from_value(input)?);
    }
    let mut schema = project()?.source;
    let name = case["id"].as_str().ok_or("missing id")?;
    if let Some(field) = name.strip_prefix("root-metadata-") {
        apply_metadata(&mut schema, field, &input[field])?;
    } else if let Some(field) = name.strip_prefix("column-metadata-") {
        let SchemaKind::Group { children, .. } = &mut schema.kind else {
            return Err("authored root must be a group".into());
        };
        apply_metadata(
            &mut children[0],
            field,
            &input["kind"]["children"][0][field],
        )?;
    } else {
        // Kind-only decoding leaves cross-field invalid metadata controls to
        // the selected admission boundary rather than the Project loader.
        schema.kind = serde_json::from_value(input["kind"].clone())?;
    }
    Ok(schema)
}

#[test]
fn csv_x12_complete_source_and_option_admission_oracles() -> Result<(), Box<dyn Error>> {
    let root = evidence("schemas-options")?;
    let base = project()?;
    let selected = policy(&base)?;
    let source_cases = literal("source-cases.json")?;
    let option_cases = literal("option-cases.json")?;
    let mut sources = Vec::new();
    for case in source_cases
        .as_array()
        .ok_or("source cases must be an array")?
    {
        let input = schema(case);
        std::fs::write(
            root.join(format!(
                "{}-INPUT.original.txt",
                case["id"].as_str().unwrap()
            )),
            format!("{input:#?}"),
        )?;
        let input = input?;
        let result = prepare(&input, &base.target, &selected);
        std::fs::write(
            root.join(format!(
                "{}-OUTCOME.original.txt",
                case["id"].as_str().unwrap()
            )),
            format!("{result:#?}"),
        )?;
        sources.push((case.clone(), result));
    }
    let mut options = Vec::new();
    for case in option_cases
        .as_array()
        .ok_or("option cases must be an array")?
    {
        let input: FormatOptions = serde_json::from_value(case["input"].clone())?;
        let result = CsvInputPolicy::from_format_options(&input);
        std::fs::write(
            root.join(format!(
                "{}-OPTIONS.original.txt",
                case["id"].as_str().unwrap()
            )),
            format!("{input:#?}\n{result:#?}"),
        )?;
        options.push((case.clone(), result));
    }
    assert_eq!(sources.len(), 61);
    assert_eq!(options.len(), 18);
    for (case, result) in sources {
        if !case["expected"]["error"].is_null() {
            assert_eq!(
                error(&result.unwrap_err()),
                case["expected"]["error"],
                "{}",
                case["id"]
            );
        } else {
            let actual = result?;
            let fields: Vec<_> = actual
                .fields
                .iter()
                .map(|field| json!({"name":field.name,"ty":field.ty}))
                .collect();
            assert_eq!(json!(fields), case["expected"]["fields"]);
            assert_eq!(
                source_policy(&actual.source),
                case["expected"]["source_policy"]
            );
            let descriptor: Json = serde_json::from_str(&actual.target_descriptor)?;
            let decoded = codegen_schema::decode(
                descriptor["schema"]
                    .as_str()
                    .ok_or("missing owning target schema")?,
                MAX_EMBEDDED_JSON_SCHEMA_BYTES,
            )?;
            assert_eq!(decoded, base.target);
        }
    }
    for (case, result) in options {
        if case["expected_error"].is_null() {
            assert_eq!(source_policy(&result?), case["expected_policy"]);
        } else {
            assert_eq!(error(&result.unwrap_err()), case["expected_error"]);
        }
    }
    Ok(())
}

fn invalid_expression(program: &mut Program) {
    program.expressions.push(ExpressionNode {
        id: 999,
        expression: Expression::Call {
            function: ScalarFunction::Upper,
            args: vec![998],
        },
    });
}

#[test]
fn csv_x12_program_order_preserves_full_original_causes() -> Result<(), Box<dyn Error>> {
    let root = evidence("programs")?;
    let base = project()?;
    let selected = policy(&base)?;
    let lowered = crate::lower(&base);
    std::fs::write(
        root.join("BASE-LOWER.original.txt"),
        format!("{lowered:#?}"),
    )?;
    let lowered = lowered?;
    let cases = literal("program-cases.json")?;
    let mut outcomes = Vec::new();
    for case in cases.as_array().ok_or("program cases must be an array")? {
        let name = case["id"].as_str().ok_or("missing case id")?;
        let mut program = lowered.clone();
        match name {
            "all-row-body-loop" => {
                let project: Project = serde_json::from_value(literal("all-row-project.json")?)?;
                let result = crate::lower(&project);
                std::fs::write(
                    root.join("ALL-ROW-LOWER.original.txt"),
                    format!("{result:#?}"),
                )?;
                program = result?;
            }
            "constant-envelope" => {}
            "refuse-xml_boundary" => {
                program.xml_boundary = Some(XmlBoundaryProgram {
                    input: XmlInputPolicy {
                        allow_inactive_root_type_members: false,
                        root_view_policy: false,
                    },
                    extra_inputs: Vec::new(),
                    output: XmlOutputPolicy::default(),
                    extra_outputs: Vec::new(),
                })
            }
            "refuse-extra_sources" => program.extra_sources.push(NamedSourceProgram {
                name: "Reference".into(),
                source: program.source.clone(),
                dynamic: None,
            }),
            "refuse-extra_targets" => program.extra_targets.push(NamedTargetProgram {
                name: "Other".into(),
                target: program.target.clone(),
                root: program.root.clone(),
            }),
            "refuse-primary_output" => program.root.repeating = true,
            "refuse-dynamic_target" => {
                program.root.children[0].iteration =
                    Some(IterationPlan::dynamic_documents(Vec::new(), 999))
            }
            "deep-target-before-invalid-expression" => {
                let mut target = SchemaNode::group("Leaf", Vec::new());
                for _ in 0..65 {
                    target = SchemaNode::group("Wrapper", vec![target]);
                }
                program.target = target;
                invalid_expression(&mut program);
            }
            "wide-target-before-invalid-expression" => {
                program.target = SchemaNode::group(
                    "Root",
                    (0..10_000)
                        .map(|index| SchemaNode::scalar(format!("Cell{index}"), ScalarType::String))
                        .collect(),
                );
                invalid_expression(&mut program);
            }
            "source-shape-before-invalid-expression" => {
                program.source = serde_json::from_value(literal("schemas/nested-column.json")?)?;
                invalid_expression(&mut program);
            }
            "ordinary-validation-cause" => invalid_expression(&mut program),
            _ => return Err(format!("unknown authored program control {name}").into()),
        }
        let result = prepare_csv_x12_boundary(&program, &selected);
        std::fs::write(
            root.join(format!("{name}-COMPLETE.original.txt")),
            format!("{program:#?}\n{result:#?}"),
        )?;
        outcomes.push((case.clone(), result));
    }
    assert_eq!(outcomes.len(), 11);
    for (case, result) in outcomes {
        if case["expected_error"].is_null() {
            result?;
        } else {
            assert_eq!(
                error(&result.unwrap_err()),
                case["expected_error"],
                "{}",
                case["id"]
            );
        }
    }
    let mut deep = base;
    deep.target = SchemaNode::group("Leaf", Vec::new());
    for _ in 0..65 {
        deep.target = SchemaNode::group("Wrapper", vec![deep.target]);
    }
    // This is constructed IR, not JSON loader recursion; target bounds must
    // precede graph validation and lower's recursive schema copies.
    let result = prepare_csv_x12_project_boundary(&deep, &selected);
    std::fs::write(
        root.join("PROJECT-DEEP.original.txt"),
        format!("{result:#?}"),
    )?;
    assert_eq!(
        error(&result.unwrap_err()),
        json!({"variant":"TargetX12","cause":{"variant":"Schema","side":"Target","path":[],"reason":"schema depth or node limit exceeded"}})
    );
    Ok(())
}
