use super::*;
use crate::{
    CsvX12BoundaryError, IterationPlan, NamedSourceProgram, XmlBoundaryProgram, XmlInputPolicy,
    XmlOutputPolicy,
};
use ir::{ScalarType, ScalarTypeSet};
use serde_json::{Value as Json, json};
use std::collections::BTreeSet;
use std::error::Error;
use std::path::{Path, PathBuf};

const SHARED_IDS: [&str; 41] = [
    "repair-before-foreign-options-and-delimiter",
    "foreign-options-before-delimiter",
    "bad-delimiter-nul",
    "bad-delimiter-lf",
    "bad-delimiter-unicode",
    "disabled-with-explicit-quote",
    "bad-quote-space",
    "delimiter-quote-conflict",
    "scalar-source",
    "repeating-row",
    "row-required",
    "columns257",
    "row-xml-optional",
    "column-group",
    "column-repeating",
    "column-union",
    "column-nullability",
    "column-empty-name",
    "column-duplicate-name",
    "source-name-budget-conversion",
    "policy-missing-name",
    "policy-wrong-order",
    "policy-unknown-name",
    "duplicate-declared-and-policy-name",
    "named-input",
    "xml-boundary",
    "primary-repeating-schema",
    "primary-root-iteration",
    "primary-program-root-repeating",
    "named-repeating-schema",
    "named-root-iteration",
    "primary-dynamic-documents",
    "unselected-named-dynamic-documents",
    "source-depth129-before-invalid-program",
    "primary-depth129-before-invalid-program",
    "unselected-depth129-before-invalid-program",
    "unselected-invalid-required-metadata",
    "original-program-error-after-descriptors",
    "explicit-csv-stored-text",
    "singular-child-row-loop",
    "optional-null-and-explicit-json-null",
];
const CLI_IDS: [&str; 5] = [
    "primary-json-options-foreign",
    "unselected-named-json-unresolved",
    "source-family-missing",
    "primary-json-family-missing",
    "named-non-json-family",
];

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src/csv_json_boundary/fixtures")
}

fn literal(name: &str) -> Result<Json, Box<dyn Error>> {
    Ok(serde_json::from_slice(&std::fs::read(
        fixtures().join(name),
    )?)?)
}

fn write_json(path: impl AsRef<Path>, value: &Json) -> Result<(), Box<dyn Error>> {
    std::fs::write(path, serde_json::to_vec_pretty(value)?)?;
    Ok(())
}

fn write_debug(path: impl AsRef<Path>, value: &impl std::fmt::Debug) -> Result<(), Box<dyn Error>> {
    std::fs::write(path, format!("{value:#?}\n"))?;
    Ok(())
}

fn evidence() -> Result<PathBuf, Box<dyn Error>> {
    let path = std::env::temp_dir().join(format!(
        "ferrule-csv-json-policy-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos()
    ));
    std::fs::create_dir(&path)?;
    Ok(path)
}

fn policy(value: &Json) -> Result<CsvJsonBoundaryPolicy, Box<dyn Error>> {
    let source = &value["source"];
    Ok(CsvJsonBoundaryPolicy {
        source: CsvInputPolicy {
            delimiter: serde_json::from_value(source["delimiter"].clone())?,
            quote: serde_json::from_value(source["quote"].clone())?,
            quote_disabled: serde_json::from_value(source["quote_disabled"].clone())?,
            has_headers: serde_json::from_value(source["has_headers"].clone())?,
            preserve_empty_strings: serde_json::from_value(
                source["preserve_empty_strings"].clone(),
            )?,
            utf8_bom: serde_json::from_value(source["utf8_bom"].clone())?,
        },
        extra_target_names: serde_json::from_value(value["extra_target_names"].clone())?,
    })
}

fn owner(value: &CsvJsonBoundaryOwner) -> Json {
    match value {
        CsvJsonBoundaryOwner::Source => json!({"variant":"Source"}),
        CsvJsonBoundaryOwner::Target => json!({"variant":"Target"}),
        CsvJsonBoundaryOwner::NamedTarget { index, name } => {
            json!({"variant":"NamedTarget","index":index,"name":name})
        }
    }
}

fn envelope(kind: &str, variant: &str, fields: Json, error: &(dyn Error + 'static)) -> Json {
    json!({"type":kind,"variant":variant,"fields":fields,"message":error.to_string(),
        "source":error.source().map(cause)})
}

fn cause(error: &(dyn Error + 'static)) -> Json {
    if let Some(error) = error.downcast_ref::<CsvInputBoundaryError>() {
        return source_error(error);
    }
    if let Some(error) = error.downcast_ref::<codegen_schema::CodecError>() {
        return codec_error(error);
    }
    if let Some(error) = error.downcast_ref::<ProgramValidationError>() {
        return validation_error(error);
    }
    if let Some(error) = error.downcast_ref::<X12BoundaryPolicyError>() {
        return match error {
            X12BoundaryPolicyError::FormatOptions => {
                envelope("X12BoundaryPolicyError", "FormatOptions", json!({}), error)
            }
            other => envelope(
                "X12BoundaryPolicyError",
                "Unexpected",
                json!({"debug":format!("{other:#?}")}),
                error,
            ),
        };
    }
    json!({"unexpected_source_debug":format!("{error:?}"),"message":error.to_string(),
        "source":error.source().map(cause)})
}

fn codec_error(error: &codegen_schema::CodecError) -> Json {
    use codegen_schema::CodecError;
    let (variant, fields) = match error {
        CodecError::Serialization(message) => ("Serialization", json!({"message":message})),
        CodecError::Deserialization(message) => ("Deserialization", json!({"message":message})),
        CodecError::InvalidShape(path) => ("InvalidShape", json!({"path":path})),
        CodecError::InvalidFloatMarker(path) => ("InvalidFloatMarker", json!({"path":path})),
        CodecError::UnsupportedVersion => ("UnsupportedVersion", json!({})),
        CodecError::MetadataChanged => ("MetadataChanged", json!({})),
        CodecError::DepthLimit { depth, max } => ("DepthLimit", json!({"depth":depth,"max":max})),
        CodecError::TooLarge { bytes, max } => ("TooLarge", json!({"bytes":bytes,"max":max})),
    };
    envelope("CodecError", variant, fields, error)
}

fn source_error(error: &CsvInputBoundaryError) -> Json {
    let (variant, fields) = match error {
        CsvInputBoundaryError::SourceSchema { field, reason } => {
            ("SourceSchema", json!({"field":field,"reason":reason}))
        }
        CsvInputBoundaryError::SourceNameBytes { maximum, observed } => (
            "SourceNameBytes",
            json!({"maximum":maximum,"observed":observed}),
        ),
        CsvInputBoundaryError::SourceFormatOptions => ("SourceFormatOptions", json!({})),
        CsvInputBoundaryError::SourceRepairRequired => ("SourceRepairRequired", json!({})),
        CsvInputBoundaryError::BadDelimiter(character) => {
            ("BadDelimiter", json!({"character":character}))
        }
        CsvInputBoundaryError::BadQuote(character) => ("BadQuote", json!({"character":character})),
        CsvInputBoundaryError::ConflictingQuoteSettings => ("ConflictingQuoteSettings", json!({})),
        CsvInputBoundaryError::DelimiterQuoteConflict => ("DelimiterQuoteConflict", json!({})),
        CsvInputBoundaryError::EmbeddedSourceSchema(error) => {
            ("EmbeddedSourceSchema", json!({"error":codec_error(error)}))
        }
    };
    envelope("CsvInputBoundaryError", variant, fields, error)
}

fn old_source_error(error: &CsvX12BoundaryError) -> Json {
    let (variant, fields) = match error {
        CsvX12BoundaryError::SourceSchema { field, reason } => {
            ("SourceSchema", json!({"field":field,"reason":reason}))
        }
        CsvX12BoundaryError::SourceNameBytes { maximum, observed } => (
            "SourceNameBytes",
            json!({"maximum":maximum,"observed":observed}),
        ),
        CsvX12BoundaryError::SourceFormatOptions => ("SourceFormatOptions", json!({})),
        CsvX12BoundaryError::SourceRepairRequired => ("SourceRepairRequired", json!({})),
        CsvX12BoundaryError::BadDelimiter(character) => {
            ("BadDelimiter", json!({"character":character}))
        }
        CsvX12BoundaryError::BadQuote(character) => ("BadQuote", json!({"character":character})),
        CsvX12BoundaryError::ConflictingQuoteSettings => ("ConflictingQuoteSettings", json!({})),
        CsvX12BoundaryError::DelimiterQuoteConflict => ("DelimiterQuoteConflict", json!({})),
        CsvX12BoundaryError::EmbeddedSourceSchema(error) => {
            ("EmbeddedSourceSchema", json!({"error":codec_error(error)}))
        }
        other => ("Unexpected", json!({"debug":format!("{other:#?}")})),
    };
    envelope("CsvX12BoundaryError", variant, fields, error)
}

fn validation_error(error: &ProgramValidationError) -> Json {
    match error {
        ProgramValidationError::MissingBindingExpression {
            target_path,
            target_field,
            expression,
        } => envelope(
            "ProgramValidationError",
            "MissingBindingExpression",
            json!({"target_path":target_path,
                "target_field":target_field,"expression":expression}),
            error,
        ),
        other => envelope(
            "ProgramValidationError",
            "Unexpected",
            json!({"debug":format!("{other:#?}")}),
            error,
        ),
    }
}

fn boundary_error(error: &CsvJsonBoundaryError) -> Json {
    let (variant, fields) = match error {
        CsvJsonBoundaryError::Source(error) => ("Source", json!({"error":source_error(error)})),
        CsvJsonBoundaryError::PolicyNames { field } => ("PolicyNames", json!({"field":field})),
        CsvJsonBoundaryError::ProgramField { field } => ("ProgramField", json!({"field":field})),
        CsvJsonBoundaryError::TargetField {
            owner: value,
            field,
        } => ("TargetField", json!({"owner":owner(value),"field":field})),
        CsvJsonBoundaryError::EmbeddedSchema {
            owner: value,
            error,
        } => (
            "EmbeddedSchema",
            json!({"owner":owner(value),"error":codec_error(error)}),
        ),
        CsvJsonBoundaryError::JsonFormatOptions {
            owner: value,
            error,
        } => (
            "JsonFormatOptions",
            json!({"owner":owner(value),"error":cause(error.as_ref())}),
        ),
        CsvJsonBoundaryError::Validation(error) => {
            ("Validation", json!({"error":validation_error(error)}))
        }
    };
    envelope("CsvJsonBoundaryError", variant, fields, error)
}

fn children(node: &mut SchemaNode) -> Result<&mut Vec<SchemaNode>, Box<dyn Error>> {
    match &mut node.kind {
        SchemaKind::Group { children, .. } => Ok(children),
        _ => Err("fixture mutation requires a group".into()),
    }
}

fn chain(depth: usize) -> SchemaNode {
    let mut node = SchemaNode::scalar("Leaf", ScalarType::String);
    for index in 1..depth {
        node = SchemaNode::group(format!("Level{index}"), vec![node]);
    }
    node
}

fn project_scope<'a>(
    scope: &'a mut Scope,
    path: &[String],
) -> Result<&'a mut Scope, Box<dyn Error>> {
    match path.split_first() {
        None => Ok(scope),
        Some((head, tail)) => project_scope(
            scope
                .children
                .iter_mut()
                .find(|child| child.target_field == *head)
                .ok_or("missing authored Project scope")?,
            tail,
        ),
    }
}

fn program_scope<'a>(
    scope: &'a mut TargetScope,
    path: &[String],
) -> Result<&'a mut TargetScope, Box<dyn Error>> {
    match path.split_first() {
        None => Ok(scope),
        Some((head, tail)) => program_scope(
            scope
                .children
                .iter_mut()
                .find(|child| child.target_field == *head)
                .ok_or("missing authored Program scope")?,
            tail,
        ),
    }
}

fn index(value: &Json) -> Result<usize, Box<dyn Error>> {
    Ok(usize::try_from(
        value.as_u64().ok_or("expected fixture index")?,
    )?)
}

fn mutate(
    project: &mut Project,
    program: &mut Program,
    policy: &mut CsvJsonBoundaryPolicy,
    mutations: &Json,
) -> Result<(), Box<dyn Error>> {
    for mutation in mutations.as_array().ok_or("expected typed mutations")? {
        for (name, value) in mutation.as_object().ok_or("expected mutation object")? {
            match name.as_str() {
                "source_options_set" => {
                    let mut options = serde_json::to_value(&project.source_options)?;
                    for (name, value) in value.as_object().ok_or("expected options object")? {
                        options[name] = value.clone();
                    }
                    project.source_options = serde_json::from_value(options)?;
                }
                "source_kind_scalar" => {
                    project.source.kind = SchemaKind::Scalar {
                        ty: serde_json::from_value(value.clone())?,
                    }
                }
                "source_repeating" => {
                    project.source.repeating = serde_json::from_value(value.clone())?
                }
                "source_required" => {
                    let SchemaKind::Group { required, .. } = &mut project.source.kind else {
                        return Err("expected source group".into());
                    };
                    *required = serde_json::from_value(value.clone())?;
                }
                "source_flat_columns" => {
                    *children(&mut project.source)? = (0..index(value)?)
                        .map(|index| {
                            SchemaNode::scalar(format!("Field{index}"), ScalarType::String)
                        })
                        .collect()
                }
                "source_xml_optional" => {
                    project.source.xml_optional = serde_json::from_value(value.clone())?
                }
                "column_kind_group" => {
                    let child = &mut children(&mut project.source)?[index(value)?];
                    *child = SchemaNode::group(child.name.clone(), Vec::new());
                }
                "column_repeating" => {
                    children(&mut project.source)?[index(value)?].repeating = true
                }
                "column_scalar_union" => {
                    let types: Vec<ScalarType> = serde_json::from_value(value["types"].clone())?;
                    let types = ScalarTypeSet::new(types).ok_or("invalid authored type set")?;
                    children(&mut project.source)?[index(&value["column"])?].kind =
                        SchemaKind::ScalarUnion { types };
                }
                "column_nullable" => children(&mut project.source)?[index(value)?].nullable = true,
                "column_name" => {
                    children(&mut project.source)?[index(&value["column"])?].name =
                        serde_json::from_value(value["name"].clone())?
                }
                "policy_names" => {
                    policy.extra_target_names = serde_json::from_value(value.clone())?
                }
                "named_target_name" => {
                    let index = index(&value["index"])?;
                    let name: String = serde_json::from_value(value["name"].clone())?;
                    project.extra_targets[index].name = name.clone();
                    program.extra_targets[index].name = name;
                }
                "add_named_source" => {
                    let name: String = serde_json::from_value(value.clone())?;
                    project.extra_sources.push(mapping::NamedSource {
                        name: name.clone(),
                        path: "authored.csv".into(),
                        schema: project.source.clone(),
                        options: project.source_options.clone(),
                        dynamic_path: None,
                    });
                    program.extra_sources.push(NamedSourceProgram {
                        name,
                        source: project.source.clone(),
                        dynamic: None,
                    });
                }
                "program_xml_boundary" => {
                    program.xml_boundary = Some(XmlBoundaryProgram {
                        input: XmlInputPolicy {
                            allow_inactive_root_type_members: serde_json::from_value(
                                value["input"]["allow_inactive_root_type_members"].clone(),
                            )?,
                            root_view_policy: serde_json::from_value(
                                value["input"]["root_view_policy"].clone(),
                            )?,
                        },
                        extra_inputs: Vec::new(),
                        output: XmlOutputPolicy {
                            declaration: serde_json::from_value(
                                value["output"]["declaration"].clone(),
                            )?,
                            indent: serde_json::from_value(value["output"]["indent"].clone())?,
                            default_namespace: serde_json::from_value(
                                value["output"]["default_namespace"].clone(),
                            )?,
                            schema_hints: serde_json::from_value(
                                value["output"]["schema_hints"].clone(),
                            )?,
                        },
                        extra_outputs: Vec::new(),
                    })
                }
                "primary_schema_repeating" => {
                    project.target.repeating = serde_json::from_value(value.clone())?
                }
                "primary_root_source_iteration" => {
                    let source: Vec<String> = serde_json::from_value(value.clone())?;
                    project.root.set_source(Some(source.clone()));
                    program.root.iteration = Some(IterationPlan::source(source));
                }
                "primary_program_root_repeating" => {
                    program.root.repeating = serde_json::from_value(value.clone())?
                }
                "named_schema_repeating" => {
                    project.extra_targets[index(value)?].schema.repeating = true
                }
                "named_root_source_iteration" => {
                    let index = index(&value["index"])?;
                    let source: Vec<String> = serde_json::from_value(value["source"].clone())?;
                    project.extra_targets[index]
                        .root
                        .set_source(Some(source.clone()));
                    program.extra_targets[index].root.iteration =
                        Some(IterationPlan::source(source));
                }
                "dynamic_document_scope" => {
                    let path: Vec<String> = serde_json::from_value(value["path"].clone())?;
                    let (project_root, program_root) =
                        if let Some(index) = value.get("target_index") {
                            let index = self::index(index)?;
                            (
                                &mut project.extra_targets[index].root,
                                &mut program.extra_targets[index].root,
                            )
                        } else {
                            (&mut project.root, &mut program.root)
                        };
                    let scope = project_scope(project_root, &path)?;
                    if !scope.set_output_path(Some(999)) {
                        return Err("authored dynamic scope lacks source iteration".into());
                    }
                    let scope = program_scope(program_root, &path)?;
                    let source = scope
                        .iteration
                        .as_ref()
                        .and_then(IterationPlan::source_iteration)
                        .ok_or("authored Program scope lacks source iteration")?
                        .path()
                        .to_vec();
                    scope.iteration = Some(IterationPlan::dynamic_documents(source, 999));
                }
                "source_schema_chain_depth" => project.source = chain(index(value)?),
                "primary_schema_chain_depth" => project.target = chain(index(value)?),
                "named_schema_chain_depth" => {
                    project.extra_targets[index(&value["index"])?].schema =
                        chain(index(&value["depth"])?)
                }
                "named_group_required" => {
                    let node = &mut project.extra_targets[index(&value["index"])?].schema;
                    let SchemaKind::Group { required, .. } = &mut node.kind else {
                        return Err("expected named group".into());
                    };
                    *required = serde_json::from_value(value["required"].clone())?;
                }
                "primary_binding_expression" => {
                    let expression = u32::try_from(value.as_u64().ok_or("expected expression")?)?;
                    project.root.bindings[0].node = expression;
                    program.root.bindings[0].expression = expression;
                }
                "source_path" => project.source_path = Some(serde_json::from_value(value.clone())?),
                "construct_source_error" => {}
                other => return Err(format!("unknown shared fixture mutation {other}").into()),
            }
        }
    }
    // Mutate the independently lowered valid Program directly. Invalid/deep
    // Projects never pass through ordinary lowering ahead of the admission API.
    program.source = project.source.clone();
    program.target = project.target.clone();
    for (target, authored) in program.extra_targets.iter_mut().zip(&project.extra_targets) {
        target.target = authored.schema.clone();
    }
    Ok(())
}

fn profile_result(
    result: &Result<CsvJsonBoundaryProfile, CsvJsonBoundaryError>,
    project: &Project,
    policy: &CsvJsonBoundaryPolicy,
    path: &Path,
    route: &str,
) -> Result<Json, Box<dyn Error>> {
    match result {
        Err(error) => Ok(boundary_error(error)),
        Ok(profile) => {
            let target =
                codegen_schema::decode(&profile.target_descriptor, MAX_EMBEDDED_JSON_SCHEMA_BYTES);
            write_debug(
                path.join(format!("{route}-TARGET-DECODE-ORIGINAL.txt")),
                &target,
            )?;
            let mut named_equal =
                profile.extra_target_descriptors.len() == project.extra_targets.len();
            for (index, descriptor) in profile.extra_target_descriptors.iter().enumerate() {
                let decoded =
                    codegen_schema::decode(&descriptor.descriptor, MAX_EMBEDDED_JSON_SCHEMA_BYTES);
                write_debug(
                    path.join(format!("{route}-NAMED-{index}-DECODE-ORIGINAL.txt")),
                    &decoded,
                )?;
                named_equal &= project.extra_targets.get(index).is_some_and(|authored| {
                    descriptor.name == authored.name
                        && decoded
                            .as_ref()
                            .is_ok_and(|actual| *actual == authored.schema)
                });
            }
            let fields = match &project.source.kind {
                SchemaKind::Group { children, .. } => children
                    .iter()
                    .map(|child| match child.kind {
                        SchemaKind::Scalar { ty } => Some(CsvInputField {
                            name: child.name.clone(),
                            ty,
                        }),
                        _ => None,
                    })
                    .collect::<Option<Vec<_>>>(),
                _ => None,
            };
            let equal = target
                .as_ref()
                .is_ok_and(|actual| *actual == project.target)
                && named_equal
                && fields.as_ref() == Some(&profile.fields)
                && profile.source == policy.source;
            Ok(
                json!({"success":true,"complete_profile_schema_descriptors_equal_input":equal,
                "fields":profile.fields.iter().map(|field| &field.name).collect::<Vec<_>>(),"primary_json":true,
                "named_targets":profile.extra_target_descriptors.iter().map(|target| &target.name).collect::<Vec<_>>(),
                "mapping_executed":false}),
            )
        }
    }
}

fn normalized_old(mut old: Json) -> Json {
    old["type"] = json!("CsvInputBoundaryError");
    old
}

fn run_case(
    case: &Json,
    base: &Project,
    base_program: &Program,
    base_policy: &CsvJsonBoundaryPolicy,
    path: &Path,
) -> Result<Json, Box<dyn Error>> {
    let mut project = base.clone();
    let mut program = base_program.clone();
    let mut policy = base_policy.clone();
    mutate(
        &mut project,
        &mut program,
        &mut policy,
        &case["typed_mutations"],
    )?;
    write_debug(path.join("PROJECT-INPUT-ORIGINAL.txt"), &project)?;
    write_debug(path.join("PROGRAM-INPUT-ORIGINAL.txt"), &program)?;
    write_debug(path.join("POLICY-INPUT-ORIGINAL.txt"), &policy)?;
    match case["entry"].as_str().ok_or("missing fixture entry")? {
        "from_input_format_options" => {
            let common = CsvInputPolicy::from_input_format_options(&project.source_options);
            write_debug(path.join("COMMON-ORIGINAL.txt"), &common)?;
            let old = CsvInputPolicy::from_format_options(&project.source_options);
            write_debug(path.join("CSV-X12-ORIGINAL.txt"), &old)?;
            let common_snapshot = match &common {
                Err(error) => source_error(error),
                Ok(policy) => json!({"unexpected_success":format!("{policy:#?}")}),
            };
            let old_snapshot = match &old {
                Err(error) => old_source_error(error),
                Ok(policy) => json!({"unexpected_success":format!("{policy:#?}")}),
            };
            let new = common.map_err(CsvJsonBoundaryError::from);
            write_debug(path.join("CSV-JSON-CONVERSION-ORIGINAL.txt"), &new)?;
            let new_snapshot = match &new {
                Err(error) => boundary_error(error),
                Ok(policy) => json!({"unexpected_success":format!("{policy:#?}")}),
            };
            Ok(
                json!({"common":common_snapshot,"existing_csv_x12":old_snapshot,"csv_json":new_snapshot,
                "complete_existing_source_contract_equal":normalized_old(old_snapshot.clone()) == common_snapshot}),
            )
        }
        "source_error_conversion" => {
            let value = &case["typed_mutations"][0]["construct_source_error"];
            if value["variant"] != "SourceNameBytes" {
                return Err("unknown authored source conversion".into());
            }
            let make = || -> Result<CsvInputBoundaryError, Box<dyn Error>> {
                Ok(CsvInputBoundaryError::SourceNameBytes {
                    maximum: index(&value["maximum"])?,
                    observed: index(&value["observed"])?,
                })
            };
            let common = make()?;
            write_debug(path.join("COMMON-ORIGINAL.txt"), &common)?;
            let snapshot = source_error(&common);
            let new = CsvJsonBoundaryError::from(common);
            write_debug(path.join("CSV-JSON-CONVERSION-ORIGINAL.txt"), &new)?;
            let old = CsvX12BoundaryError::from(make()?);
            write_debug(path.join("CSV-X12-CONVERSION-ORIGINAL.txt"), &old)?;
            Ok(
                json!({"csv_json":boundary_error(&new),"complete_existing_source_contract_equal":
                normalized_old(old_source_error(&old)) == snapshot}),
            )
        }
        "prepare_project_and_program" => {
            let project_result = prepare_csv_json_project_boundary(&project, &policy);
            write_debug(path.join("PROJECT-RESULT-ORIGINAL.txt"), &project_result)?;
            let program_result = prepare_csv_json_boundary(&program, &policy);
            write_debug(path.join("PROGRAM-RESULT-ORIGINAL.txt"), &program_result)?;
            Ok(
                json!({"project":profile_result(&project_result,&project,&policy,path,"PROJECT")?,
                "program":profile_result(&program_result,&project,&policy,path,"PROGRAM")?}),
            )
        }
        "prepare_program" => {
            let result = prepare_csv_json_boundary(&program, &policy);
            write_debug(path.join("PROGRAM-RESULT-ORIGINAL.txt"), &result)?;
            profile_result(&result, &project, &policy, path, "PROGRAM")
        }
        other => Err(format!("unknown shared fixture entry {other}").into()),
    }
}

#[test]
fn csv_json_shared_admission_complete_oracles() -> Result<(), Box<dyn Error>> {
    let path = evidence()?;
    println!("CSV_JSON_POLICY_EVIDENCE={}", path.display());
    for name in ["project.json", "policy.json", "admission-cases.json"] {
        std::fs::copy(fixtures().join(name), path.join(name))?;
    }
    let base: Result<Project, _> = serde_json::from_value(literal("project.json")?);
    write_debug(path.join("BASE-PROJECT-DECODE-ORIGINAL.txt"), &base)?;
    let base = base?;
    let base_program = crate::lower(&base);
    write_debug(path.join("BASE-LOWER-ORIGINAL.txt"), &base_program)?;
    let base_program = base_program?;
    let base_policy = policy(&literal("policy.json")?);
    write_debug(path.join("BASE-POLICY-DECODE-ORIGINAL.txt"), &base_policy)?;
    let base_policy = base_policy?;
    let cases = literal("admission-cases.json")?;
    let cases = cases.as_array().ok_or("expected complete fixture array")?;
    let mut actual_ids = BTreeSet::new();
    let mut comparisons = Vec::new();
    let mut shared = 0;
    let mut cli = 0;
    for case in cases {
        let id = case["id"].as_str().ok_or("missing fixture id")?;
        let case_path = path.join(id);
        std::fs::create_dir(&case_path)?;
        write_json(case_path.join("CASE-ORIGINAL.json"), case)?;
        let unique = actual_ids.insert(id.to_owned());
        if case["entry"] == "cli_policy" {
            cli += 1;
            comparisons.push(json!({"id":id,"passed":unique && CLI_IDS.contains(&id),"owner":"cli_policy_tests"}));
            continue;
        }
        shared += 1;
        let actual = run_case(case, &base, &base_program, &base_policy, &case_path);
        write_debug(case_path.join("COMPLETE-CASE-RESULT-ORIGINAL.txt"), &actual)?;
        let actual = match actual {
            Ok(actual) => actual,
            Err(error) => {
                json!({"setup_error_debug":format!("{error:?}"),"message":error.to_string()})
            }
        };
        write_json(case_path.join("ACTUAL-ORIGINAL.json"), &actual)?;
        let expected = match case["entry"].as_str() {
            Some("from_input_format_options") => {
                json!({"common":case["expected"],"existing_csv_x12":case["expected_existing_csv_x12_factory"],
                "csv_json":case["expected_csv_json_conversion"],"complete_existing_source_contract_equal":true})
            }
            Some("source_error_conversion") => {
                json!({"csv_json":case["expected"],"complete_existing_source_contract_equal":true})
            }
            Some("prepare_project_and_program") => {
                json!({"project":case["expected"],"program":case["expected"]})
            }
            Some("prepare_program") => case["expected"].clone(),
            _ => Json::Null,
        };
        write_json(case_path.join("EXPECTED-ORIGINAL.json"), &expected)?;
        comparisons.push(json!({"id":id,"passed":unique && SHARED_IDS.contains(&id) && case["execute_mapping"] == false && actual == expected,
            "actual":actual,"expected":expected}));
    }
    let expected_ids = SHARED_IDS
        .into_iter()
        .chain(CLI_IDS)
        .map(str::to_owned)
        .collect::<BTreeSet<_>>();
    let passed = shared == 41
        && cli == 5
        && actual_ids == expected_ids
        && comparisons.iter().all(|row| row["passed"] == true);
    write_json(
        path.join("FINAL-ORIGINAL.json"),
        &json!({"passed":passed,"shared_controls":shared,"cli_owned_controls":cli,
        "actual_ids":actual_ids,"expected_ids":expected_ids,"comparisons":comparisons,"mapping_executed":false}),
    )?;
    assert!(
        passed,
        "complete CSV-to-JSON policy oracle mismatch; originals retained at {}",
        path.display()
    );
    Ok(())
}
