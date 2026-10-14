use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{Instance, Value};
use mapping::{Binding, Project, ScopeIteration, ScopeSequence};

#[path = "fixtures/edi-json-occurrences/expected_project.rs"]
mod expectation;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/edi-json-occurrences").join(name)
}

struct Package(PathBuf);
impl Package {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!("ferrule_edi_json_{}_{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir(&path).unwrap();
        for name in ["Envelope.Config", "990.Config", "target.schema.json", "literals.schema.json"] {
            std::fs::copy(fixture(name), path.join(name)).unwrap();
        }
        Self(path)
    }
    fn import(&self, text: &str) -> mfd::Imported {
        let path = self.0.join("mapping.mfd");
        std::fs::write(&path, text).unwrap();
        mfd::import(&path).unwrap()
    }
}
impl Drop for Package { fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.0); } }

// Complete self-authored physical envelope. Cases insert only their explicitly
// authored N1 item segments; a fragment alone is never parsed as X12 input.
fn wire(segments: &str) -> String {
    let count = segments.matches("N1").count() + 2;
    format!("ISA*00*          *00*          *ZZ*GENERIC-SENDER *ZZ*GENERIC-RECEIVE*261001*1200*^*00501*000000001*0*P*:~GS*FA*GENERIC*GENERIC*20261001*1200*1*X*005010~ST*990*0001~{segments}SE*{count}*0001~GE*1*1~IEA*1*000000001~")
}

fn expected_source(labels: &[Option<&str>]) -> Instance {
    fn scalar(value: &str) -> Instance { Instance::Scalar(Value::String(value.into())) }
    fn group(fields: Vec<(&str, Instance)>) -> Instance {
        Instance::Group(fields.into_iter().map(|(name, value)| (name.into(), value)).collect::<Vec<_>>().into())
    }
    let count = (labels.len() + 2).to_string();
    group(vec![
        ("ISA", group(vec![("A01", scalar("00")), ("A02", scalar("          ")), ("A03", scalar("00")), ("A04", scalar("          ")), ("A05", scalar("ZZ")), ("A06", scalar("GENERIC-SENDER ")), ("A07", scalar("ZZ")), ("A08", scalar("GENERIC-RECEIVE")), ("A09", scalar("261001")), ("A10", scalar("1200")), ("A11", scalar("^")), ("A12", scalar("00501")), ("A13", scalar("000000001")), ("A14", scalar("0")), ("A15", scalar("P")), ("A16", scalar(":"))])),
        ("Group", Instance::Repeated(vec![group(vec![
            ("GS", group(vec![("G01", scalar("FA")), ("G02", scalar("GENERIC")), ("G03", scalar("GENERIC")), ("G04", scalar("20261001")), ("G05", scalar("1200")), ("G06", scalar("1")), ("G07", scalar("X")), ("G08", scalar("005010"))])),
            ("Message", Instance::Repeated(vec![group(vec![
                ("ST", group(vec![("S01", scalar("990")), ("S02", scalar("0001"))])),
                ("Item", Instance::Repeated(labels.iter().map(|label| group(vec![("N1", group(vec![("Label", label.map_or(Instance::Scalar(Value::Null), scalar))]))])).collect())),
                ("SE", group(vec![("T01", scalar(&count)), ("T02", scalar("0001"))])),
            ])])),
            ("GE", group(vec![("E01", scalar("1")), ("E02", scalar("1"))])),
        ])])),
        ("IEA", group(vec![("I01", scalar("1")), ("I02", scalar("000000001"))])),
    ])
}

#[test]
fn physical_x12_string_occurrences_keep_complete_project_and_optional_absence() {
    let imported = mfd::import(&fixture("implicit.mfd")).unwrap();
    let expected = expectation::expected();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert_eq!(serde_json::to_value(&imported.project).unwrap(), serde_json::to_value(&expected).unwrap());
    assert!(mfd::assess_import(&imported).executable);
    let strict = mfd::import_with_profile(&fixture("implicit.mfd"), &mfd::ImportOptions::default(), mfd::ImportProfile::Executable).unwrap();
    assert_eq!(serde_json::to_value(&strict.imported.project).unwrap(), serde_json::to_value(&expected).unwrap());
    let cases: serde_json::Value = serde_json::from_str(include_str!("fixtures/edi-json-occurrences/cases.json")).unwrap();
    let labels: [&[Option<&str>]; 6] = [&[], &[Some("A")], &[Some("A"), Some("B")], &[Some("same"), Some("same")], &[None], &[None]];
    assert_eq!(cases.as_array().unwrap().len(), labels.len());
    for (case, labels) in cases.as_array().unwrap().iter().zip(labels) {
        let input = format_edi::x12::from_str(&wire(case["segments"].as_str().unwrap()), &expected.source, expected.source_options.lenient_segments).unwrap();
        assert_eq!(input, expected_source(labels), "{}", case["name"]);
        let output = engine::run(&imported.project, &input).unwrap();
        let json: serde_json::Value = serde_json::from_str(&format_json::to_string(&expected.target, &output).unwrap()).unwrap();
        assert_eq!(json, case["expected"], "{}", case["name"]);
        assert_eq!(json["Groups"].as_array().unwrap().len(), 1);
        assert_eq!(json["Groups"][0]["Transactions"].as_array().unwrap().len(), 1);
        assert_eq!(json["Groups"][0]["Transactions"][0]["Batches"].as_array().unwrap().len(), labels.len());
        assert!(json["Groups"][0]["Transactions"][0]["Batches"].as_array().unwrap().iter().all(|batch| batch["Records"].as_array().unwrap().len() == 2));
    }
}

#[test]
fn false_and_zero_literals_are_present_without_promoting_absent_string_data() {
    let imported = mfd::import(&fixture("literals.mfd")).unwrap();
    let expected = expectation::literals();
    assert!(imported.warnings.is_empty());
    assert_eq!(serde_json::to_value(&imported.project).unwrap(), serde_json::to_value(&expected).unwrap());
    assert!(mfd::assess_import(&imported).executable);
    let input = format_edi::x12::from_str(&wire("N1~"), &expected.source, true).unwrap();
    let output = engine::run(&imported.project, &input).unwrap();
    let json: serde_json::Value = serde_json::from_str(&format_json::to_string(&expected.target, &output).unwrap()).unwrap();
    assert_eq!(json, serde_json::json!({"Groups":[{"Transactions":[{"Batches":[{"Records":[{"Branch":"left","Enabled":false,"Count":0},{"Branch":"right","Enabled":false,"Count":0}]}]}]}]}));
}

fn authored_instance(literal: &serde_json::Value) -> Instance {
    match literal {
        serde_json::Value::Object(fields) => Instance::Group(fields.iter().map(|(name, value)| (name.clone(), authored_instance(value))).collect::<Vec<_>>().into()),
        serde_json::Value::Array(items) => Instance::Repeated(items.iter().map(authored_instance).collect()),
        serde_json::Value::String(value) => Instance::Scalar(Value::String(value.clone())),
        serde_json::Value::Null => Instance::Scalar(Value::Null),
        _ => panic!("Outside authored source String/null domain"),
    }
}

#[test]
fn complete_group_and_transaction_frames_do_not_cross_project_rows() {
    let imported = mfd::import(&fixture("implicit.mfd")).unwrap();
    assert_eq!(serde_json::to_value(&imported.project).unwrap(), serde_json::to_value(expectation::expected()).unwrap());
    let cases: serde_json::Value = serde_json::from_str(include_str!("fixtures/edi-json-occurrences/frame-cases.json")).unwrap();
    assert_eq!(cases.as_array().unwrap().len(), 1);
    for case in cases.as_array().unwrap() {
        let input = format_edi::x12::from_str(case["wire"].as_str().unwrap(), &imported.project.source, true).unwrap();
        assert_eq!(input, authored_instance(&case["source"]));
        let output = engine::run(&imported.project, &input).unwrap();
        let actual: serde_json::Value = serde_json::from_str(&format_json::to_string(&imported.project.target, &output).unwrap()).unwrap();
        assert_eq!(actual, case["expected"]);
    }
}

#[test]
fn edge_order_does_not_reorder_declarations_and_empty_literal_still_creates_an_object() {
    let text = include_str!("fixtures/edi-json-occurrences/implicit.mfd");
    let package = Package::new();
    let edges = text.replace("<edge from=\"101\" to=\"51\"/><edge from=\"11\" to=\"52\"/><edge from=\"102\" to=\"61\"/><edge from=\"11\" to=\"62\"/>",
        "<edge from=\"11\" to=\"62\"/><edge from=\"102\" to=\"61\"/><edge from=\"11\" to=\"52\"/><edge from=\"101\" to=\"51\"/>");
    assert_ne!(text, edges);
    assert_eq!(serde_json::to_value(package.import(&edges).project).unwrap(), serde_json::to_value(expectation::expected()).unwrap());
    let mut expected = expectation::expected();
    expected.graph.nodes.insert(0, mapping::Node::Const { value: Value::String("".into()) });
    let imported = package.import(&text.replace("value=\"left\"", "value=\"\""));
    assert_eq!(serde_json::to_value(&imported.project).unwrap(), serde_json::to_value(&expected).unwrap());
    let input = format_edi::x12::from_str(&wire("N1~"), &expected.source, true).unwrap();
    let output = engine::run(&imported.project, &input).unwrap();
    let json: serde_json::Value = serde_json::from_str(&format_json::to_string(&expected.target, &output).unwrap()).unwrap();
    assert_eq!(json, serde_json::json!({"Groups":[{"Transactions":[{"Batches":[{"Records":[{"Branch":""},{"Branch":"right"}]}]}]}]}));
}

fn legacy() -> Project {
    let mut expected = expectation::expected();
    let records = &mut expected.root.children[0].children[0].children[0].children[0];
    records.iteration = ScopeIteration::None;
    records.bindings = vec![Binding { target_field: "Branch".into(), node: 0 }, Binding { target_field: "Label".into(), node: 2 }, Binding { target_field: "Branch".into(), node: 1 }];
    expected
}

fn assert_legacy(imported: mfd::Imported, expected: Project) {
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert_eq!(serde_json::to_value(&imported.project).unwrap(), serde_json::to_value(&expected).unwrap());
    let expected_import = mfd::Imported { project: expected, warnings: vec![], mapping_path: imported.mapping_path.clone() };
    let report = mfd::assess_import(&expected_import);
    assert!(!report.executable);
    assert_eq!(mfd::assess_import(&imported), report);
    match mfd::import_with_profile(&imported.mapping_path, &mfd::ImportOptions::default(), mfd::ImportProfile::Executable) {
        Err(mfd::MfdError::IncompatibleImport(actual)) => assert_eq!(*actual, report),
        _ => panic!("expected exact typed legacy disposition"),
    }
}

#[test]
fn missing_enclosing_physical_driver_keeps_the_complete_legacy_project() {
    for (edge, missing_group) in [("<edge from=\"8\" to=\"38\"/>", true), ("<edge from=\"9\" to=\"39\"/>", false)] {
        let text = include_str!("fixtures/edi-json-occurrences/implicit.mfd");
        let altered = text.replace(edge, "");
        assert_ne!(text, altered);
        let mut expected = legacy();
        if missing_group {
            expected.root.children[0].iteration = ScopeIteration::None;
            expected.root.children[0].children[0].iteration =
                ScopeIteration::Source(vec!["Group".into(), "Message".into()]);
        } else {
            expected.root.children[0].children[0].iteration = ScopeIteration::None;
            expected.root.children[0].children[0].children[0].iteration =
                ScopeIteration::Source(vec!["Message".into(), "Item".into()]);
        }
        let package = Package::new();
        assert_legacy(package.import(&altered), expected);
    }
}

// Literal complete-Project deductions for each actual direct owner subset.
// The bit order is Group8->38, Message9->39, Item10->40. This oracle does not
// invoke the importer or reproduce ScopeBuilder's path-shortening algorithm.
fn legacy_driver_subset(present: u8, literals: bool) -> Project {
    let mut expected = if literals { expectation::literals() } else { legacy() };
    let records = &mut expected.root.children[0].children[0].children[0].children[0];
    records.iteration = ScopeIteration::None;
    if literals {
        records.bindings = vec![
            Binding { target_field: "Branch".into(), node: 0 },
            Binding { target_field: "Label".into(), node: 4 },
            Binding { target_field: "Enabled".into(), node: 2 },
            Binding { target_field: "Count".into(), node: 3 },
            Binding { target_field: "Branch".into(), node: 1 },
        ];
    }
    let source = |parts: &[&str]| ScopeIteration::Source(parts.iter().map(|part| (*part).into()).collect());
    let frame = |parts: &[&str]| Some(parts.iter().map(|part| (*part).into()).collect::<Vec<String>>());
    let field = |parts: &[&str]| parts.iter().map(|part| (*part).into()).collect::<Vec<String>>();
    let (iterations, frame, path) = match present {
        7 => ([source(&["Group"]), source(&["Message"]), source(&["Item"])], frame(&["Group", "Message", "Item"]), field(&["N1", "Label"])),
        6 => ([ScopeIteration::None, source(&["Group", "Message"]), source(&["Item"])], frame(&["Group", "Message", "Item"]), field(&["N1", "Label"])),
        5 => ([source(&["Group"]), ScopeIteration::None, source(&["Message", "Item"])], frame(&["Group", "Message", "Item"]), field(&["N1", "Label"])),
        4 => ([ScopeIteration::None, ScopeIteration::None, source(&["Group", "Message", "Item"])], frame(&["Group", "Message", "Item"]), field(&["N1", "Label"])),
        3 => ([source(&["Group"]), source(&["Message"]), ScopeIteration::None], frame(&["Group", "Message"]), field(&["Item", "N1", "Label"])),
        2 => ([ScopeIteration::None, source(&["Group", "Message"]), ScopeIteration::None], frame(&["Group", "Message"]), field(&["Item", "N1", "Label"])),
        1 => ([source(&["Group"]), ScopeIteration::None, ScopeIteration::None], frame(&["Group"]), field(&["Message", "Item", "N1", "Label"])),
        0 => ([ScopeIteration::None, ScopeIteration::None, ScopeIteration::None], None, field(&["Group", "Message", "Item", "N1", "Label"])),
        _ => panic!("outside the eight authored driver subsets"),
    };
    let [groups, transactions, batches] = iterations;
    expected.root.children[0].iteration = groups;
    expected.root.children[0].children[0].iteration = transactions;
    expected.root.children[0].children[0].children[0].iteration = batches;
    expected.graph.nodes.insert(if literals { 4 } else { 2 }, mapping::Node::SourceField { frame, path });
    expected
}

#[test]
fn all_physical_driver_subsets_keep_literal_complete_projects_and_legacy_dispositions() {
    for (design, literals) in [("implicit.mfd", false), ("literals.mfd", true)] {
        for present in 0..8 {
            let mut text = std::fs::read_to_string(fixture(design)).unwrap();
            for (bit, edge) in [(1, "<edge from=\"8\" to=\"38\"/>"), (2, "<edge from=\"9\" to=\"39\"/>"), (4, "<edge from=\"10\" to=\"40\"/>")] {
                assert_eq!(text.matches(edge).count(), 1);
                if present & bit == 0 { text = text.replace(edge, ""); }
            }
            let package = Package::new();
            let imported = package.import(&text);
            if present == 7 {
                let expected = if literals { expectation::literals() } else { expectation::expected() };
                assert!(imported.warnings.is_empty());
                assert_eq!(serde_json::to_value(&imported.project).unwrap(), serde_json::to_value(&expected).unwrap());
                assert!(mfd::assess_import(&imported).executable);
                let strict = mfd::import_with_profile(&imported.mapping_path, &mfd::ImportOptions::default(), mfd::ImportProfile::Executable).unwrap();
                assert_eq!(serde_json::to_value(&strict.imported.project).unwrap(), serde_json::to_value(&expected).unwrap());
            } else {
                assert_legacy(imported, legacy_driver_subset(present, literals));
            }
        }
    }
}

#[test]
fn declaration_order_is_distinct_from_connection_order() {
    let text = include_str!("fixtures/edi-json-occurrences/implicit.mfd");
    let first = "<entry name=\"object\"><entry name=\"Branch\" type=\"json-property\"><entry name=\"string\" inpkey=\"51\"/></entry><entry name=\"Label\" type=\"json-property\"><entry name=\"string\" inpkey=\"52\"/></entry></entry>";
    let second = "<entry name=\"object\" clone=\"1\"><entry name=\"Branch\" type=\"json-property\"><entry name=\"string\" inpkey=\"61\"/></entry><entry name=\"Label\" type=\"json-property\"><entry name=\"string\" inpkey=\"62\"/></entry></entry>";
    assert!(text.contains(first) && text.contains(second));
    let altered = text.replace(first, "DECLARATION-SWAP").replace(second, &first.replacen("<entry name=\"object\">", "<entry name=\"object\" clone=\"1\">", 1))
        .replace("DECLARATION-SWAP", &second.replacen(" clone=\"1\"", "", 1));
    let package = Package::new();
    let imported = package.import(&altered);
    let mut expected = expectation::expected();
    let segment = |node| mapping::Scope { bindings: vec![Binding { target_field: "Branch".into(), node }, Binding { target_field: "Label".into(), node: 2 }], ..Default::default() };
    expected.root.children[0].children[0].children[0].children[0].iteration = ScopeIteration::Concatenate(ScopeSequence::new(segment(1), vec![segment(0)]));
    assert_eq!(serde_json::to_value(&imported.project).unwrap(), serde_json::to_value(&expected).unwrap());
    let input = format_edi::x12::from_str(&wire("N1*A~"), &expected.source, true).unwrap();
    let output = engine::run(&imported.project, &input).unwrap();
    let json: serde_json::Value = serde_json::from_str(&format_json::to_string(&expected.target, &output).unwrap()).unwrap();
    assert_eq!(json, serde_json::json!({"Groups":[{"Transactions":[{"Batches":[{"Records":[{"Branch":"right","Label":"A"},{"Branch":"left","Label":"A"}]}]}]}]}));
}

#[test]
fn possibly_absent_data_cannot_satisfy_a_required_json_property_or_change_legacy_diagnostics() {
    let package = Package::new();
    let schema_path = package.0.join("target.schema.json");
    let text = std::fs::read_to_string(&schema_path).unwrap().replace("\"required\": [\"Branch\"]", "\"required\": [\"Branch\", \"Label\"]");
    assert!(text.contains("\"required\": [\"Branch\", \"Label\"]"));
    std::fs::write(&schema_path, text).unwrap();
    let imported = package.import(include_str!("fixtures/edi-json-occurrences/implicit.mfd"));
    let mut expected = legacy();
    // The independent schema change is reconstructed explicitly, never copied
    // from the returned imported Project.
    let record = ir::SchemaNode::group("Records", vec![ir::SchemaNode::scalar("Branch", ir::ScalarType::String), ir::SchemaNode::scalar("Label", ir::ScalarType::String)])
        .with_required_fields(vec!["Branch".into(), "Label".into()]).unwrap().repeating();
    expected.target = ir::SchemaNode::group("Target", vec![ir::SchemaNode::group("Groups", vec![ir::SchemaNode::group("Transactions", vec![ir::SchemaNode::group("Batches", vec![record]).with_required_fields(vec!["Records".into()]).unwrap().repeating()]).with_required_fields(vec!["Batches".into()]).unwrap().repeating()]).with_required_fields(vec!["Transactions".into()]).unwrap().repeating()]).with_required_fields(vec!["Groups".into()]).unwrap();
    assert_legacy(imported, expected);
}

fn write_artifacts(root: &Path, artifacts: &codegen::ArtifactSet) {
    for file in artifacts.files() {
        let path = root.join(file.path.as_str());
        if let Some(parent) = path.parent() { std::fs::create_dir_all(parent).unwrap(); }
        std::fs::write(path, &file.contents).unwrap();
    }
}

#[test]
fn authored_occurrences_keep_existing_typed_and_x12_generated_boundary_admission() {
    for (design, expected, cases, frames, role) in [
        ("implicit.mfd", expectation::expected(), "cases.json", "frame-cases.json", "string"),
        ("literals.mfd", expectation::literals(), "literals-cases.json", "literals-frame-cases.json", "literals"),
    ] {
    let imported = mfd::import(&fixture(design)).unwrap();
    assert!(imported.warnings.is_empty());
    assert_eq!(serde_json::to_value(&imported.project).unwrap(), serde_json::to_value(&expected).unwrap());
    let program = codegen::lower(&imported.project).unwrap();
    let mut options = codegen::X12BoundaryOptions::from_format_options(&imported.project.source_options).unwrap();
    let single = codegen::X12BoundaryPolicy { source: Some(options.clone()), target: None };
    assert!(matches!(codegen::prepare_x12_boundary(&program, &single), Err(codegen::X12BoundaryPolicyError::Schema {
        side: codegen::X12BoundarySide::Source, path, reason: "envelope segments cannot repeat or occur in repeating loops"
    }) if path == vec!["Group".to_string(), "GS".to_string()]));
    options.envelope_profile = codegen::X12EnvelopeProfile::GroupedTransactions;
    let policy = codegen::X12BoundaryPolicy { source: Some(options), target: None };
    codegen::prepare_x12_boundary(&program, &policy).unwrap();
    let rust = codegen_rust::emit(&program, &codegen_rust::Options {
        package_name: "ferrule-edi-json-occurrences".into(),
        runtime_dependency: codegen_rust::RuntimeDependency::Path(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../codegen-runtime").canonicalize().unwrap().display().to_string()),
    }).unwrap();
    let csharp = codegen_csharp::emit_with_x12(&program, &policy).unwrap();
    if let Some(root) = std::env::var_os("FERRULE_EDI_JSON_OCCURRENCE_ARTIFACT_ROOT") {
        let root = PathBuf::from(root).join(role);
        write_artifacts(&root.join("rust"), &rust);
        write_artifacts(&root.join("csharp"), &csharp);
        std::fs::write(root.join("source.ir-schema.json"), serde_json::to_vec(&expected.source).unwrap()).unwrap();
        std::fs::write(root.join("target.ir-schema.json"), serde_json::to_vec(&expected.target).unwrap()).unwrap();
        for (source, destination) in [(cases, "cases.json"), (frames, "frame-cases.json"), ("Host.cs.txt", "Host.cs.txt"), ("Host.rs.txt", "Host.rs.txt")] {
            std::fs::copy(fixture(source), root.join(destination)).unwrap();
        }
    }
    let mut refused = imported.project;
    // Wrong retained profile is a source admission refusal, not an invented
    // native wire reader equality check for saved interchange metadata.
    refused.source_options.x12_interchange_version = Some("00401".into());
    let refused_program = codegen::lower(&refused).unwrap();
    let mut options = codegen::X12BoundaryOptions::from_format_options(&refused.source_options).unwrap();
    options.envelope_profile = codegen::X12EnvelopeProfile::GroupedTransactions;
    let refused_policy = codegen::X12BoundaryPolicy { source: Some(options), target: None };
    assert!(matches!(codegen::prepare_x12_boundary(&refused_program, &refused_policy), Err(codegen::X12BoundaryPolicyError::Schema {
        side: codegen::X12BoundarySide::Source, path, reason: "retained interchange version must agree with fixed ISA12"
    }) if path.is_empty()));
    }
}
