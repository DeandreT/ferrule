//! Document-set compatibility retains path, order and every document value.

use super::*;
use serde_json::{Value as Json, json};

fn documents(project: &Project, value: &Instance) -> TestResult<Json> {
    let Instance::DocumentSet(members) = value else {
        return Err("document-output route requires a DocumentSet".into());
    };
    assert!(members.len() <= 4096);
    members
        .iter()
        .map(|member| {
            assert!(!member.path().is_empty() && member.path().len() <= 4096);
            assert_eq!(member.source_path(), member.path());
            let text = admission::target_document(&project.target, member.value(), 0)?;
            Ok(json!({"path": member.path(), "value": serde_json::from_str::<Json>(&text)?}))
        })
        .collect::<TestResult<Vec<_>>>()
        .map(Json::Array)
}

pub(crate) fn run_case(
    project: &Project,
    source: &Instance,
    extras: &[(String, Instance)],
    mapping_path: &Path,
    case: &Path,
    rust_target: &Path,
) -> TestResult<()> {
    // This route never masquerades as the single/named JSON output envelope.
    if !project.runtime_dependencies().is_empty()
        || !extras.is_empty()
        || !project.extra_sources.is_empty()
        || !project.extra_targets.is_empty()
        || !project.source_options.xml_document
        || !project.target_options.xml_document
        || project.source_options.local_xml_file_set
    {
        return Err("document-output corpus route requires one closed XML input and output".into());
    }
    let program = codegen::lower(project)?;
    admission::check_document_program(&program)?;
    assert_eq!(
        program.xml_output_mode()?,
        Some(codegen::XmlOutputMode::DynamicPrimaryDocuments)
    );
    let context = engine::ExecutionContext::new(mapping_path);
    let original = engine::run_with_context(project, source, &context)?;
    assert_eq!(original, engine::run(project, source)?);
    let xml = format_xml::to_string(&project.source, source)?;
    let restored = format_xml::from_str(&xml, &project.source)?;
    assert_eq!(&restored, source, "complete XML source transport");
    assert_eq!(
        engine::run_with_context(project, &restored, &context)?,
        original
    );
    let expected = documents(project, &original)?;
    let project_path = case.join("project.json");
    std::fs::write(
        &project_path,
        mapping::project_file::encode_pretty(project)?,
    )?;
    std::fs::write(case.join("source.xml"), xml)?;
    std::fs::write(
        case.join("source-schema.json"),
        codegen::serialize_embedded_schema(
            &project.source,
            codegen::MAX_EMBEDDED_JSON_SCHEMA_BYTES,
        )?,
    )?;
    std::fs::write(
        case.join("target-schema.json"),
        codegen::serialize_embedded_schema(
            &project.target,
            codegen::MAX_EMBEDDED_JSON_SCHEMA_BYTES,
        )?,
    )?;
    std::fs::write(
        case.join("documents.expected.json"),
        serde_json::to_vec_pretty(&expected)?,
    )?;
    let inputs = [(String::new(), case.to_path_buf())];
    let rust = super::run_rust_host(
        &project_path,
        &inputs,
        case,
        rust_target,
        include_str!("document_outputs_rust.rs.txt"),
        true,
    )?;
    let csharp = super::run_csharp_host(
        &project_path,
        &inputs,
        case,
        include_str!("document_outputs_csharp.cs.txt"),
    )?;
    for (name, output) in [("rust", rust), ("csharp", csharp)] {
        std::fs::write(
            case.join(format!("{name}-documents.actual.json")),
            &output.stdout,
        )?;
        let actual: Json = serde_json::from_slice(&output.stdout)?;
        assert!(
            envelope::ordered_json_equal(&actual.to_string(), &expected.to_string()),
            "{name} complete path/document list differs"
        );
    }
    Ok(())
}

#[test]
fn document_output_transport_preserves_authored_group_paths() -> TestResult<()> {
    let mut project: Project = serde_json::from_str(include_str!(
        "../../../../codegen/src/tests/fixtures/nested_document_collection.json"
    ))?;
    project
        .graph
        .nodes
        .insert(14, project.graph.nodes[&2].clone());
    project.graph.nodes.insert(
        2,
        Node::RuntimeParameterDefault {
            name: "size".into(),
            ty: ir::ScalarType::String,
            default: 14,
            preview: None,
        },
    );
    let cases: Json = serde_json::from_str(include_str!("../document_collections/cases.json"))?;
    let source = format_xml::from_str(cases[0]["input"].as_str().unwrap(), &project.source)?;
    let directory = TempDir::new("document_output_transport")?;
    let target = std::env::var_os("FERRULE_CODEGEN_HOST_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| directory.0.join("target"));
    run_case(
        &project,
        &source,
        &[],
        &directory.0.join("mapping.json"),
        &directory.0,
        &target,
    )
}

#[test]
fn document_transport_only_admits_closed_literal_parameter_defaults() -> TestResult<()> {
    let mut project: Project = serde_json::from_str(include_str!(
        "../../../../codegen/src/tests/fixtures/nested_document_collection.json"
    ))?;
    project
        .graph
        .nodes
        .insert(14, project.graph.nodes[&2].clone());
    project.graph.nodes.insert(
        2,
        Node::RuntimeParameterDefault {
            name: "size".into(),
            ty: ir::ScalarType::String,
            default: 14,
            preview: None,
        },
    );
    let program = codegen::lower(&project)?;
    admission::check_document_program(&program)?;
    assert!(
        admission::check_program(&program).is_err(),
        "ordinary JSON admission is unchanged"
    );
    project.graph.nodes.insert(
        14,
        Node::Call {
            function: "concat".into(),
            args: vec![15, 15],
        },
    );
    project.graph.nodes.insert(
        15,
        Node::Const {
            value: Value::String("2".into()),
        },
    );
    assert!(
        admission::check_document_program(&codegen::lower(&project)?).is_err(),
        "nonliteral default needs a separate contract"
    );
    project.graph.nodes.insert(
        2,
        Node::RuntimeParameter {
            name: "size".into(),
            ty: ir::ScalarType::String,
            preview: None,
        },
    );
    assert!(
        admission::check_document_program(&codegen::lower(&project)?).is_err(),
        "no host parameter value may be invented"
    );
    Ok(())
}
