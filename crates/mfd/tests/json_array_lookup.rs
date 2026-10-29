use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use ir::{Instance, Value};

struct TempDir(PathBuf);
impl TempDir {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_json_lookup_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for TempDir {
    fn drop(&mut self) {
        if std::thread::panicking() {
            eprintln!("retained lookup fixture: {}", self.0.display());
            return;
        }
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/json-array-lookup")
}
fn options(path: &Path) -> mfd::ImportOptions {
    // These self-authored fixtures share schemas inside one explicit package.
    // Do not weaken the importer's default confinement for parent references.
    let root = path
        .parent()
        .unwrap()
        .ancestors()
        .take(3)
        .find(|root| {
            root.join("lookup.schema.json").is_file() && root.join("target.schema.json").is_file()
        })
        .expect("known synthetic lookup package root");
    mfd::ImportOptions::default().with_package_root(root)
}
fn imported(path: &Path) -> mapping::Project {
    let imported = mfd::import_with_profile(path, &options(path), mfd::ImportProfile::Executable)
        .unwrap()
        .imported;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(
        engine::validate(&imported.project).is_empty(),
        "{:?}",
        engine::validate(&imported.project)
    );
    assert!(imported.project.extra_sources.is_empty());
    imported.project
}
fn run(
    project: &mapping::Project,
    case: &serde_json::Value,
) -> Result<Instance, engine::EngineError> {
    let mut parameters = engine::RuntimeParameters::new();
    match case["parameter_failure"].as_str() {
        Some("missing") => {}
        Some("null") => parameters.insert("LookupText", Value::Null).unwrap(),
        Some("int") => parameters.insert("LookupText", Value::Int(7)).unwrap(),
        _ => parameters
            .insert(
                "LookupText",
                Value::String(case["lookup"].as_str().unwrap().into()),
            )
            .unwrap(),
    }
    let primary = if let Some(input) = case["input"].as_str() {
        format_edi::x12::from_str(input, &project.source, false).unwrap()
    } else {
        format_json::from_str(&case["primary"].to_string(), &project.source).unwrap()
    };
    let context =
        engine::ExecutionContext::new(Path::new("mapping.mfd")).with_parameters(&parameters);
    engine::run_with_context(project, &primary, &context)
}
fn cases() -> Vec<serde_json::Value> {
    serde_json::from_str(include_str!("fixtures/json-array-lookup/cases.json")).unwrap()
}

#[test]
fn conditional_keys_preserve_both_branches_literals_missing_null_and_primary_item_ownership() {
    for profile in ["conditional", "conditional/edi"] {
        let root = fixtures().join(profile);
        let project = imported(&root.join("mapping.mfd"));
        let cases: Vec<serde_json::Value> =
            serde_json::from_str(&std::fs::read_to_string(root.join("cases.json")).unwrap())
                .unwrap();
        for case in cases {
            let result = run(&project, &case);
            if let Some(message) = case["error"].as_str() {
                assert!(
                    matches!(result, Err(engine::EngineError::Function(functions::FunctionError::InvalidArgument {
                    function: "json_parse_array_length", message: actual
                })) if actual == message),
                    "{case}: {result:?}"
                );
            } else {
                assert_eq!(
                    result.unwrap(),
                    format_json::from_str(&case["expected"].to_string(), &project.target).unwrap(),
                    "{case}"
                );
            }
        }
        assert!(
            project
                .graph
                .nodes
                .values()
                .any(|node| matches!(node, mapping::Node::If { .. }))
        );
    }
}

fn copy_all_fixtures() -> TempDir {
    fn copy(source: &Path, target: &Path) {
        std::fs::create_dir_all(target).unwrap();
        for entry in std::fs::read_dir(source).unwrap() {
            let entry = entry.unwrap();
            let path = target.join(entry.file_name());
            if entry.file_type().unwrap().is_dir() {
                copy(&entry.path(), &path);
            } else {
                std::fs::copy(entry.path(), path).unwrap();
            }
        }
    }
    let dir = TempDir::new();
    copy(&fixtures(), &dir.0);
    dir
}

#[test]
fn conditional_key_raw_inputs_cycles_foreign_branches_and_unproved_operations_refuse() {
    for profile in ["conditional", "conditional/edi"] {
        for (from, to) in [
            (
                "<vertex vertexkey=\"4\"><edges><edge vertexkey=\"102\"/></edges></vertex>",
                "",
            ),
            (
                "<vertex vertexkey=\"93\"><edges><edge vertexkey=\"101\"/></edges></vertex>",
                "",
            ),
            ("pos=\"1\" key=\"102\"", "pos=\"0\" key=\"102\""),
            (
                "<vertex vertexkey=\"2\"><edges><edge vertexkey=\"72\"/>",
                "<vertex vertexkey=\"2\"><edges><edge vertexkey=\"72\"/><edge vertexkey=\"102\"/>",
            ),
            (
                "<vertex vertexkey=\"81\"><edges><edge vertexkey=\"73\"/></edges></vertex>",
                "<vertex vertexkey=\"80\"><edges><edge vertexkey=\"73\"/></edges></vertex>",
            ),
            (
                "<vertex vertexkey=\"4\"><edges><edge vertexkey=\"102\"/></edges></vertex>",
                "<vertex vertexkey=\"52\"><edges><edge vertexkey=\"102\"/></edges></vertex>",
            ),
            (
                "<vertex vertexkey=\"4\"><edges><edge vertexkey=\"102\"/></edges></vertex>",
                "<vertex vertexkey=\"23\"><edges><edge vertexkey=\"102\"/></edges></vertex>",
            ),
            (
                "value=\"ID\" datatype=\"string\"",
                "value=\"7\" datatype=\"integer\"",
            ),
            (
                "value=\"FALLBACK\" datatype=\"string\"",
                "value=\"false\" datatype=\"boolean\"",
            ),
            ("<datapoint pos=\"2\" key=\"103\"/>", "<datapoint/>"),
        ] {
            let dir = copy_all_fixtures();
            let path = dir.0.join(profile).join("mapping.mfd");
            replace(&path, from, to);
            refused(&path);
        }
        // Even an untaken branch cannot read a sibling/ancestor item.
        let dir = copy_all_fixtures();
        let path = dir.0.join(profile).join("mapping.mfd");
        let foreign = if profile.ends_with("edi") { "7" } else { "6" };
        replace(
            &path,
            "<vertex vertexkey=\"4\"><edges><edge vertexkey=\"102\"/></edges></vertex>",
            &format!(
                "<vertex vertexkey=\"{foreign}\"><edges><edge vertexkey=\"102\"/></edges></vertex>"
            ),
        );
        refused(&path);

        for function in ["now", "auto-number", "position", "concat"] {
            let dir = copy_all_fixtures();
            let path = dir.0.join(profile).join("mapping.mfd");
            replace(
                &path,
                "</children>",
                &format!(
                    "<component name=\"{function}\" library=\"core\" kind=\"5\"><targets><datapoint key=\"95\"/></targets></component></children>"
                ),
            );
            replace(
                &path,
                "<vertex vertexkey=\"4\"><edges><edge vertexkey=\"102\"/></edges></vertex>",
                "<vertex vertexkey=\"95\"><edges><edge vertexkey=\"102\"/></edges></vertex>",
            );
            refused(&path);
        }
        let dir = copy_all_fixtures();
        let root = dir.0.join(profile);
        let path = root.join("mapping.mfd");
        std::fs::write(root.join("secondary.schema.json"), r#"{"type":"object","properties":{"Items":{"type":"array","items":{"type":"object","properties":{"ValueB":{"type":"string"}}}}}}"#).unwrap();
        replace(
            &path,
            "</children>",
            r#"<component name="Secondary" library="json" kind="31"><data><root><entry name="FileInstance"><entry name="document"><entry name="root"><entry name="object"><entry name="Items" type="json-property"><entry name="array"><entry name="item" type="json-item"><entry name="object"><entry name="ValueB" type="json-property"><entry name="string" outkey="95"/></entry></entry></entry></entry></entry></entry></entry></entry></entry></root><json schema="secondary.schema.json" inputinstance="secondary.json"/></data></component></children>"#,
        );
        replace(
            &path,
            "<vertex vertexkey=\"4\"><edges><edge vertexkey=\"102\"/></edges></vertex>",
            "<vertex vertexkey=\"95\"><edges><edge vertexkey=\"102\"/></edges></vertex>",
        );
        refused(&path);
    }
}

#[test]
fn conditional_keys_require_a_primary_leaf_and_bound_expression_work_and_depth() {
    let dir = copy_all_fixtures();
    let path = dir.0.join("conditional/mapping.mfd");
    replace(
        &path,
        "<vertex vertexkey=\"80\"><edges><edge vertexkey=\"31\"/><edge vertexkey=\"41\"/></edges></vertex>",
        "<vertex vertexkey=\"80\"><edges><edge vertexkey=\"31\"/></edges></vertex>",
    );
    replace(
        &path,
        "<vertex vertexkey=\"60\"><edges>",
        "<vertex vertexkey=\"60\"><edges><edge vertexkey=\"41\"/>",
    );
    refused(&path);

    let dir = copy_all_fixtures();
    let path = dir.0.join("conditional/mapping.mfd");
    let mut components = String::new();
    let mut edges = String::new();
    for index in 0..20 {
        let input = 2000 + index * 3;
        let output = 1200 + index;
        let otherwise = if index == 19 { 4 } else { output + 1 };
        components.push_str(&format!("<component name=\"if-else\" library=\"core\" kind=\"4\"><sources><datapoint key=\"{input}\"/><datapoint key=\"{}\"/><datapoint key=\"{}\"/></sources><targets><datapoint key=\"{output}\"/></targets></component>", input + 1, input + 2));
        edges.push_str(&format!("<vertex vertexkey=\"63\"><edges><edge vertexkey=\"{input}\"/></edges></vertex><vertex vertexkey=\"2\"><edges><edge vertexkey=\"{}\"/></edges></vertex><vertex vertexkey=\"{otherwise}\"><edges><edge vertexkey=\"{}\"/></edges></vertex>", input + 1, input + 2));
    }
    replace(&path, "</children>", &format!("{components}</children>"));
    replace(&path, "</vertices>", &format!("{edges}</vertices>"));
    replace(
        &path,
        "<vertex vertexkey=\"94\"><edges><edge vertexkey=\"103\"/></edges></vertex>",
        "<vertex vertexkey=\"1200\"><edges><edge vertexkey=\"103\"/></edges></vertex>",
    );
    refused(&path);
}

#[test]
fn nested_edi_primary_owns_every_detail_across_all_envelope_frames() {
    let project = imported(&fixtures().join("edi/mapping.mfd"));
    let cases: Vec<serde_json::Value> =
        serde_json::from_str(include_str!("fixtures/json-array-lookup/edi/cases.json")).unwrap();
    for case in cases {
        let result = run(&project, &case);
        if let Some(message) = case["error"].as_str() {
            assert!(
                matches!(result, Err(engine::EngineError::Function(functions::FunctionError::InvalidArgument {
                function: "json_parse_array_length", message: actual
            })) if actual == message),
                "{case}: {result:?}"
            );
        } else {
            let expected =
                format_json::from_str(&case["expected"].to_string(), &project.target).unwrap();
            assert_eq!(result.unwrap(), expected, "{case}");
        }
    }
    assert!(project.graph.nodes.values().any(|node| matches!(node,
        mapping::Node::SourceField { path, frame: Some(frame) }
        if path == &["LIN", "Code"] && frame == &["Interchange", "Group", "Message", "Detail"])));
}

#[test]
fn nested_edi_parent_keys_and_noncurrent_detail_keys_remain_refused() {
    for (from, to) in [
        (
            "<vertex vertexkey=\"2\"><edges><edge vertexkey=\"31\"/><edge vertexkey=\"41\"/></edges></vertex>",
            "<vertex vertexkey=\"2\"><edges><edge vertexkey=\"31\"/></edges></vertex><vertex vertexkey=\"7\"><edges><edge vertexkey=\"41\"/></edges></vertex>",
        ),
        (
            "<vertex vertexkey=\"1\"><edges><edge vertexkey=\"30\"/></edges></vertex>",
            "<vertex vertexkey=\"6\"><edges><edge vertexkey=\"30\"/></edges></vertex>",
        ),
    ] {
        let dir = copy_fixture();
        std::fs::create_dir(dir.0.join("edi")).unwrap();
        for name in ["mapping.mfd", "Envelope.Config", "888.Config"] {
            std::fs::copy(
                fixtures().join("edi").join(name),
                dir.0.join("edi").join(name),
            )
            .unwrap();
        }
        let path = dir.0.join("edi/mapping.mfd");
        replace(&path, from, to);
        refused(&path);
    }
}

#[test]
fn lookup_preserves_primary_items_first_match_absence_duplicates_and_full_validation() {
    let project = imported(&fixtures().join("mapping.mfd"));
    for case in cases() {
        let result = run(&project, &case);
        if let Some(kind) = case["parameter_failure"].as_str() {
            match kind {
                "missing" => assert!(
                    matches!(result, Err(engine::EngineError::MissingRuntimeParameter { name, .. }) if name == "LookupText")
                ),
                "null" => assert!(
                    matches!(
                        result,
                        Err(engine::EngineError::Function(
                            functions::FunctionError::TypeMismatch {
                                function: "json_parse_array_length",
                                got: "null"
                            }
                        ))
                    ),
                    "{case}"
                ),
                "int" => assert!(
                    matches!(
                        result,
                        Err(engine::EngineError::Function(
                            functions::FunctionError::InvalidArgument {
                                function: "json_parse_array_length",
                                message: "input does not match the JSON schema"
                            }
                        ))
                    ),
                    "{case}"
                ),
                _ => panic!("unknown parameter control: {case}"),
            }
        } else if let Some(message) = case["error"].as_str() {
            assert!(
                matches!(result, Err(engine::EngineError::Function(functions::FunctionError::InvalidArgument {
                function: "json_parse_array_length", message: actual
            })) if actual == message),
                "{case}: {result:?}"
            );
        } else {
            let expected =
                format_json::from_str(&case["expected"].to_string(), &project.target).unwrap();
            assert_eq!(result.unwrap(), expected, "{case}");
        }
    }
    let mapping::Node::SequenceAggregate {
        sequence,
        predicate: Some(predicate),
        ..
    } = project
        .graph
        .nodes
        .values()
        .find(|node| matches!(node, mapping::Node::SequenceAggregate { .. }))
        .unwrap()
    else {
        panic!("lookup reducer");
    };
    assert!(
        matches!(project.graph.nodes[&sequence.item()], mapping::Node::SourceField { ref path, frame: None } if path.is_empty())
    );
    let mapping::Node::Call { args, .. } = &project.graph.nodes[predicate] else {
        panic!("equality");
    };
    assert!(args.iter().any(|node| matches!(&project.graph.nodes[node], mapping::Node::SourceField { path, frame: Some(frame) } if path == &["Code"] && frame == &["Items"])));
}

fn copy_fixture() -> TempDir {
    let dir = TempDir::new();
    for name in [
        "mapping.mfd",
        "primary.schema.json",
        "lookup.schema.json",
        "target.schema.json",
    ] {
        std::fs::copy(fixtures().join(name), dir.0.join(name)).unwrap();
    }
    dir
}
fn replace(path: &Path, from: &str, to: &str) {
    let text = std::fs::read_to_string(path).unwrap();
    assert!(text.contains(from), "missing replacement: {from}");
    std::fs::write(path, text.replace(from, to)).unwrap();
}
fn refused(path: &Path) {
    let result = mfd::import_with_profile(path, &options(path), mfd::ImportProfile::Executable);
    assert!(
        matches!(&result, Err(mfd::MfdError::IncompatibleImport(report)) if !report.executable)
            || matches!(&result, Err(mfd::MfdError::UnsupportedImport(_))),
        "unexpected import outcome: {:?}",
        result.err()
    );
    let best = mfd::import_with_options(path, &options(path)).unwrap();
    assert!(
        best.warnings
            .iter()
            .any(|warning| warning.contains("JSON string parser")),
        "{:?}",
        best.warnings
    );
}

#[test]
fn unsupported_ownership_cardinality_shared_predicates_and_connected_unused_ports_refuse() {
    for (file, from, to) in [
        (
            "mapping.mfd",
            "name=\"LookupText\" usageKind=\"input\"",
            "name=\"LookupText\" usageKind=\"input\" optional=\"1\"",
        ),
        ("mapping.mfd", "datatype=\"string\"", "datatype=\"integer\""),
        (
            "mapping.mfd",
            "schema=\"lookup.schema.json\"",
            "schema=\"missing.schema.json\"",
        ),
        ("mapping.mfd", "<edge vertexkey=\"41\"/>", ""),
        (
            "mapping.mfd",
            "<vertex vertexkey=\"1\"><edges><edge vertexkey=\"30\"/></edges></vertex>",
            "<vertex vertexkey=\"4\"><edges><edge vertexkey=\"30\"/></edges></vertex>",
        ),
        (
            "mapping.mfd",
            "<vertex vertexkey=\"1\"><edges><edge vertexkey=\"30\"/></edges></vertex>",
            "",
        ),
        (
            "mapping.mfd",
            "<vertex vertexkey=\"52\"><edges>",
            "<vertex vertexkey=\"53\"><edges>",
        ),
        (
            "mapping.mfd",
            "<vertex vertexkey=\"42\"><edges><edge vertexkey=\"51\"/>",
            "<vertex vertexkey=\"42\"><edges><edge vertexkey=\"51\"/><edge vertexkey=\"32\"/>",
        ),
        (
            "mapping.mfd",
            "<vertex vertexkey=\"22\"><edges><edge vertexkey=\"40\"/>",
            "<vertex vertexkey=\"22\"><edges><edge vertexkey=\"40\"/><edge vertexkey=\"31\"/>",
        ),
        (
            "mapping.mfd",
            "<vertex vertexkey=\"23\"><edges><edge vertexkey=\"50\"/>",
            "<vertex vertexkey=\"23\"><edges><edge vertexkey=\"50\"/><edge vertexkey=\"31\"/>",
        ),
        (
            "mapping.mfd",
            "<vertex vertexkey=\"52\"><edges><edge vertexkey=\"33\"/></edges></vertex>",
            "<vertex vertexkey=\"52\"><edges><edge vertexkey=\"33\"/></edges></vertex><vertex vertexkey=\"24\"><edges><edge vertexkey=\"32\"/></edges></vertex>",
        ),
        (
            "lookup.schema.json",
            "\"type\":\"array\"",
            "\"type\":[\"array\",\"null\"]",
        ),
        (
            "lookup.schema.json",
            "\"type\":[\"string\",\"null\"]",
            "\"type\":\"integer\"",
        ),
    ] {
        let dir = copy_fixture();
        replace(&dir.0.join(file), from, to);
        refused(&dir.0.join("mapping.mfd"));
    }
}

#[test]
fn reversed_equality_retains_exact_identifiers_and_duplicate_order() {
    let dir = copy_fixture();
    let path = dir.0.join("mapping.mfd");
    replace(&path, "key=\"40\"", "key=\"99\"");
    replace(&path, "key=\"41\"", "key=\"40\"");
    replace(&path, "key=\"99\"", "key=\"41\"");
    let project = imported(&path);
    let case = &cases()[0];
    assert_eq!(
        run(&project, case).unwrap(),
        format_json::from_str(&case["expected"].to_string(), &project.target).unwrap()
    );
}

#[test]
fn unused_keyless_false_output_preserves_its_exact_raw_position() {
    let dir = copy_fixture();
    let path = dir.0.join("mapping.mfd");
    replace(&path, "<datapoint pos=\"1\" key=\"53\"/>", "<datapoint/>");
    let project = imported(&path);
    for case in cases().iter().filter(|case| case.get("expected").is_some()) {
        assert_eq!(
            run(&project, case).unwrap(),
            format_json::from_str(&case["expected"].to_string(), &project.target).unwrap()
        );
    }
    for malformed in [
        "<datapoint pos=\"0\"/>",
        "<datapoint pos=\"1\"/>",
        "<datapoint pos=\"2\"/>",
        "<datapoint key=\"\"/>",
        "<datapoint key=\"52\"/>",
        "<datapoint><datapoint/></datapoint>",
    ] {
        let dir = copy_fixture();
        let path = dir.0.join("mapping.mfd");
        replace(&path, "<datapoint pos=\"1\" key=\"53\"/>", malformed);
        refused(&path);
    }
    let dir = copy_fixture();
    let path = dir.0.join("mapping.mfd");
    replace(&path, "<datapoint pos=\"1\" key=\"53\"/>", "<datapoint/>");
    replace(
        &path,
        "<vertex vertexkey=\"52\"><edges>",
        "<vertex vertexkey=\"53\"><edges>",
    );
    assert!(
        mfd::import_with_profile(
            &path,
            &mfd::ImportOptions::default(),
            mfd::ImportProfile::Executable
        )
        .is_err(),
        "keyless false slot cannot acquire a stale wire identity"
    );
}

#[test]
fn globally_framed_primary_does_not_authorize_sibling_target_lookup() {
    let dir = copy_fixture();
    let path = dir.0.join("mapping.mfd");
    replace(
        &path,
        "<entry name=\"Results\" type=\"json-property\">",
        "<entry name=\"Sibling\" type=\"json-property\"><entry name=\"array\" inpkey=\"34\"><entry name=\"item\" type=\"json-item\"><entry name=\"object\"><entry name=\"Found\" type=\"json-property\"><entry name=\"string\" inpkey=\"35\"/></entry></entry></entry></entry></entry><entry name=\"Results\" type=\"json-property\">",
    );
    replace(
        &path,
        "<edge vertexkey=\"33\"/>",
        "<edge vertexkey=\"35\"/>",
    );
    replace(
        &path,
        "</vertices>",
        "<vertex vertexkey=\"4\"><edges><edge vertexkey=\"34\"/></edges></vertex></vertices>",
    );
    let schema_path = dir.0.join("target.schema.json");
    let mut schema: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&schema_path).unwrap()).unwrap();
    schema["properties"]["Sibling"] = serde_json::json!({"type":"array","items":{"type":"object","properties":{"Found":{"type":"string"}}}});
    std::fs::write(schema_path, schema.to_string()).unwrap();
    refused(&path);
}

#[test]
fn ancestor_key_is_not_the_actual_nested_primary_frame() {
    let dir = copy_fixture();
    let path = dir.0.join("mapping.mfd");
    replace(
        &path,
        "<entry name=\"Ordinal\" type=\"json-property\"><entry name=\"number\" outkey=\"3\"/></entry>",
        "<entry name=\"Ordinal\" type=\"json-property\"><entry name=\"number\" outkey=\"3\"/></entry><entry name=\"Children\" type=\"json-property\"><entry name=\"array\" outkey=\"6\"><entry name=\"item\" type=\"json-item\"><entry name=\"object\"/></entry></entry></entry>",
    );
    replace(
        &path,
        "<entry name=\"Found\" type=\"json-property\"><entry name=\"string\" inpkey=\"33\"/></entry>",
        "<entry name=\"Nested\" type=\"json-property\"><entry name=\"array\" inpkey=\"34\"><entry name=\"item\" type=\"json-item\"><entry name=\"object\"><entry name=\"Found\" type=\"json-property\"><entry name=\"string\" inpkey=\"33\"/></entry></entry></entry></entry></entry>",
    );
    replace(
        &path,
        "</vertices>",
        "<vertex vertexkey=\"6\"><edges><edge vertexkey=\"34\"/></edges></vertex></vertices>",
    );
    let source_path = dir.0.join("primary.schema.json");
    let mut source: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&source_path).unwrap()).unwrap();
    source["properties"]["Items"]["items"]["properties"]["Children"] =
        serde_json::json!({"type":"array","items":{"type":"object","properties":{}}});
    std::fs::write(source_path, source.to_string()).unwrap();
    let target_path = dir.0.join("target.schema.json");
    let mut target: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&target_path).unwrap()).unwrap();
    target["properties"]["Results"]["items"]["properties"]["Nested"] = serde_json::json!({"type":"array","items":{"type":"object","properties":{"Found":{"type":"string"}}}});
    std::fs::write(target_path, target.to_string()).unwrap();
    refused(&path);
}

#[test]
fn raw_pin_aliases_duplicates_and_controls_cannot_normalize_into_lookup_ownership() {
    for (from, to) in [
        ("pos=\"1\" key=\"41\"", "pos=\"0\" key=\"41\""),
        ("pos=\"1\" key=\"51\"", "pos=\"0\" key=\"51\""),
        (
            "<datapoint pos=\"0\" key=\"42\"/>",
            "<datapoint pos=\"0\" key=\"42\"/><datapoint pos=\"0\" key=\"42\"/>",
        ),
        (
            "<datapoint pos=\"0\" key=\"52\"/>",
            "<datapoint pos=\"0\" key=\"52\"/><datapoint pos=\"0\" key=\"52\"/>",
        ),
        (
            "<component name=\"equal\" library=\"core\" kind=\"5\">",
            "<component name=\"equal\" library=\"core\" kind=\"5\"><data><entry name=\"parent-context\"/></data>",
        ),
    ] {
        let dir = copy_fixture();
        let path = dir.0.join("mapping.mfd");
        replace(&path, from, to);
        refused(&path);
    }
}

#[test]
fn keys_from_a_second_parser_and_multiply_repeated_primary_keys_refuse() {
    let dir = copy_fixture();
    let path = dir.0.join("mapping.mfd");
    let text = std::fs::read_to_string(&path).unwrap();
    let start = text.find("<component name=\"LookupParser\"").unwrap();
    let end = start + text[start..].find("</component>").unwrap() + "</component>".len();
    let second = text[start..end]
        .replace("LookupParser", "SecondParser")
        .replace("inpkey=\"20\"", "inpkey=\"26\"")
        .replace("outkey=\"21\"", "outkey=\"27\"")
        .replace("outkey=\"22\"", "outkey=\"25\"")
        .replace("outkey=\"23\"", "outkey=\"28\"")
        .replace("outkey=\"24\"", "outkey=\"29\"");
    std::fs::write(
        &path,
        text.replacen("</children>", &format!("{second}</children>"), 1),
    )
    .unwrap();
    replace(
        &path,
        "<vertex vertexkey=\"22\"><edges><edge vertexkey=\"40\"/>",
        "<vertex vertexkey=\"25\"><edges><edge vertexkey=\"40\"/>",
    );
    replace(
        &path,
        "<edge vertexkey=\"20\"/>",
        "<edge vertexkey=\"20\"/><edge vertexkey=\"26\"/>",
    );
    refused(&path);

    let dir = copy_fixture();
    let path = dir.0.join("mapping.mfd");
    replace(
        &path,
        "<entry name=\"Code\" type=\"json-property\"><entry name=\"string\" outkey=\"2\"/></entry>",
        "<entry name=\"Children\" type=\"json-property\"><entry name=\"array\"><entry name=\"item\" type=\"json-item\"><entry name=\"object\"><entry name=\"Code\" type=\"json-property\"><entry name=\"string\" outkey=\"2\"/></entry></entry></entry></entry></entry>",
    );
    let source_path = dir.0.join("primary.schema.json");
    let mut source: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&source_path).unwrap()).unwrap();
    source["properties"]["Items"]["items"]["properties"]["Children"] = serde_json::json!({"type":"array","items":{"type":"object","properties":{"Code":{"type":"string"}}}});
    std::fs::write(source_path, source.to_string()).unwrap();
    refused(&path);
}

fn emit(root: &Path, profile: &str) {
    let edi = profile.ends_with("edi");
    let project = imported(&fixtures().join(profile).join("mapping.mfd"));
    let program = codegen::lower(&project).unwrap();
    let options = codegen_rust::Options {
        package_name: "json-lookup-mapping".into(),
        runtime_dependency: codegen_rust::RuntimeDependency::Path(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../codegen-runtime")
                .canonicalize()
                .unwrap()
                .display()
                .to_string(),
        ),
    };
    let (rust, csharp) = if edi {
        let mut source =
            codegen::X12BoundaryOptions::from_format_options(&project.source_options).unwrap();
        source.envelope_profile = codegen::X12EnvelopeProfile::MapForceInterchanges;
        source.lenient_segments = false;
        let policy = codegen::X12BoundaryPolicy {
            source: Some(source),
            target: None,
        };
        (
            codegen_rust::emit_with_x12(&program, &options, &policy).unwrap(),
            codegen_csharp::emit_with_x12(&program, &policy).unwrap(),
        )
    } else {
        (
            codegen_rust::emit(&program, &options).unwrap(),
            codegen_csharp::emit(&program).unwrap(),
        )
    };
    for (language, artifacts) in [("rust", rust), ("csharp", csharp)] {
        for file in artifacts.files() {
            let path = root.join(language).join(file.path.as_str());
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, &file.contents).unwrap();
        }
    }
    for (name, schema) in [("source", &project.source), ("target", &project.target)] {
        std::fs::write(
            root.join(format!("{name}-schema.json")),
            serde_json::to_string(schema).unwrap(),
        )
        .unwrap();
    }
}

#[test]
fn lookup_graph_lowers_and_emits_both_backends() {
    for profile in ["", "edi", "conditional", "conditional/edi"] {
        let dir = TempDir::new();
        emit(&dir.0, profile);
    }
}

#[test]
#[ignore = "compiles generated Rust and C# Release hosts; requires Cargo and .NET 10"]
fn generated_lookup_release_hosts_match_complete_outputs_and_typed_failures() {
    for profile in ["", "conditional"] {
        generated_hosts(profile);
    }
}

#[test]
#[ignore = "compiles generated Rust and C# Release EDI hosts; requires Cargo and .NET 10"]
fn generated_nested_edi_lookup_release_hosts_match_complete_outputs_and_typed_failures() {
    for profile in ["edi", "conditional/edi"] {
        generated_hosts(profile);
    }
}

fn generated_hosts(profile: &str) {
    let dir = TempDir::new();
    emit(&dir.0, profile);
    let shared = fixtures();
    let hosts = shared.join(if profile.ends_with("edi") { "edi" } else { "" });
    let fixtures = shared.join(profile);
    let rust = dir.0.join("rust");
    let manifest = rust.join("Cargo.toml");
    let text = std::fs::read_to_string(&manifest)
        .unwrap()
        .replace("[dependencies]", "[dependencies]\nserde_json = \"1\"");
    std::fs::write(
        &manifest,
        format!("{text}\n[[bin]]\nname = \"lookup-host\"\npath = \"host.rs\"\n"),
    )
    .unwrap();
    std::fs::copy(hosts.join("rust_host.rs"), rust.join("host.rs")).unwrap();
    checked(
        std::process::Command::new("cargo")
            .args(["run", "--release", "--offline", "--manifest-path"])
            .env_remove("CARGO_ENCODED_RUSTFLAGS")
            .env("RUSTFLAGS", "-D warnings")
            .arg(manifest)
            .args(["--bin", "lookup-host", "--"])
            .arg(fixtures.join("cases.json"))
            .arg(dir.0.join("source-schema.json"))
            .arg(dir.0.join("target-schema.json")),
    );
    let host = dir.0.join("csharp/host");
    std::fs::create_dir(&host).unwrap();
    std::fs::write(
        dir.0.join("NuGet.Config"),
        r#"<configuration><packageSources><clear/></packageSources></configuration>"#,
    )
    .unwrap();
    std::fs::write(host.join("Host.csproj"), r#"<Project Sdk="Microsoft.NET.Sdk"><PropertyGroup><OutputType>Exe</OutputType><TargetFramework>net10.0</TargetFramework><ImplicitUsings>enable</ImplicitUsings><Nullable>enable</Nullable><TreatWarningsAsErrors>true</TreatWarningsAsErrors><NuGetAudit>false</NuGetAudit></PropertyGroup><ItemGroup><ProjectReference Include="../Ferrule.Generated.csproj"/></ItemGroup></Project>"#).unwrap();
    std::fs::copy(hosts.join("csharp_host.cs"), host.join("Program.cs")).unwrap();
    std::fs::copy(shared.join("typed_assert.cs"), host.join("LookupOracle.cs")).unwrap();
    let home = dir.0.join("dotnet-home");
    checked(dotnet(&host, &home).arg("--version"));
    checked(
        dotnet(&host, &home)
            .args([
                "run",
                "--configuration",
                "Release",
                "--disable-build-servers",
                "-p:UseSharedCompilation=false",
                "-p:BuildInParallel=false",
                "--project",
            ])
            .arg(host.join("Host.csproj"))
            .arg("--")
            .arg(fixtures.join("cases.json"))
            .arg(dir.0.join("source-schema.json"))
            .arg(dir.0.join("target-schema.json")),
    );
}
fn dotnet(directory: &Path, home: &Path) -> std::process::Command {
    let mut command = std::process::Command::new(
        std::env::var_os("FERRULE_DOTNET_EXE").unwrap_or_else(|| "dotnet".into()),
    );
    command
        .current_dir(directory)
        .env("DOTNET_CLI_HOME", home)
        .env("DOTNET_ADD_GLOBAL_TOOLS_TO_PATH", "false")
        .env("DOTNET_NOLOGO", "1")
        .env("DOTNET_SKIP_FIRST_TIME_EXPERIENCE", "1")
        .env("DOTNET_CLI_USE_MSBUILD_SERVER", "0")
        .env("MSBUILDDISABLENODEREUSE", "1");
    command
}
fn checked(command: &mut std::process::Command) {
    let output = command.output().unwrap();
    println!(
        "{command:?}\nstatus: {}\n{}\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.status.success(),
        "{command:?}\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
