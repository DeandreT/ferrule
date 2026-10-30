use std::path::{Path, PathBuf};

use ir::{Instance, Value};
use mapping::{AggregateOp, Node};

#[path = "filtered_counts/hosts.rs"]
mod hosts;
#[path = "filtered_counts/preflight.rs"]
mod preflight;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/filtered-counts")
        .join(name)
}

fn imported() -> mfd::Imported {
    imported_design("guarded-counts.mfd")
}

fn imported_design(design: &str) -> mfd::Imported {
    preflight_path(design, &fixture(design));
    let imported = mfd::import(&fixture(design)).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(
        engine::validate(&imported.project).is_empty(),
        "{:?}",
        engine::validate(&imported.project)
    );
    assert!(mfd::assess_import(&imported).executable);
    preflight_path(design, &fixture(design));
    let strict = mfd::import_with_profile(
        &fixture(design),
        &mfd::ImportOptions::default(),
        mfd::ImportProfile::Executable,
    )
    .unwrap();
    assert_eq!(
        mapping::project_file::encode_pretty(&strict.imported.project).unwrap(),
        mapping::project_file::encode_pretty(&imported.project).unwrap()
    );
    imported
}

#[derive(Clone, Copy)]
struct Cohort {
    name: &'static str,
    design: &'static str,
    cases: &'static str,
    rejections: &'static str,
    kind: &'static str,
}

fn cohorts() -> [Cohort; 3] {
    [
        Cohort {
            name: "xml",
            design: "guarded-counts.mfd",
            cases: include_str!("fixtures/filtered-counts/guarded-cases.json"),
            rejections: include_str!("fixtures/filtered-counts/guarded-xml-invalid.json"),
            kind: "xml",
        },
        Cohort {
            name: "bool",
            design: "guarded-bool-counts.mfd",
            cases: include_str!("fixtures/filtered-counts/guarded-bool-cases.json"),
            rejections: include_str!("fixtures/filtered-counts/guarded-bool-invalid.json"),
            kind: "xml",
        },
        Cohort {
            name: "edi",
            design: "guarded-edi-counts.mfd",
            cases: include_str!("fixtures/filtered-counts/guarded-edi-cases.json"),
            rejections: include_str!("fixtures/filtered-counts/guarded-edi-invalid.json"),
            kind: "x12",
        },
    ]
}

#[derive(Debug)]
enum InputError {
    Xml(format_xml::XmlFormatError),
    Edi(format_edi::EdiFormatError),
}

impl InputError {
    fn category(&self) -> &'static str {
        match self {
            Self::Xml(format_xml::XmlFormatError::ScalarParse { .. }) => "ScalarParse",
            Self::Xml(format_xml::XmlFormatError::UnexpectedXmlNil { .. }) => "UnexpectedXmlNil",
            Self::Xml(format_xml::XmlFormatError::Parse(_)) => "Parse",
            Self::Edi(format_edi::EdiFormatError::NotX12(_)) => "NotX12",
            _ => "unexpected input error",
        }
    }
}

fn parse_input(
    cohort: Cohort,
    project: &mapping::Project,
    input: &str,
) -> Result<Instance, InputError> {
    match cohort.kind {
        "xml" => format_xml::from_str(input, &project.source).map_err(InputError::Xml),
        "x12" => format_edi::x12::from_str(input, &project.source, false).map_err(InputError::Edi),
        _ => panic!("unknown authored input cohort"),
    }
}

fn assert_complete_typed_output(actual: &Instance, expected: &serde_json::Value) {
    match expected {
        serde_json::Value::Object(fields) => {
            let Instance::Group(actual_fields) = actual else {
                panic!("expected whole group: {actual:?}")
            };
            assert_eq!(actual_fields.len(), fields.len());
            for (name, expected) in fields {
                assert_complete_typed_output(actual.field(name).unwrap(), expected);
            }
        }
        serde_json::Value::Array(items) => {
            let actual = actual.as_repeated().unwrap();
            assert_eq!(actual.len(), items.len());
            for (actual, expected) in actual.iter().zip(items) {
                assert_complete_typed_output(actual, expected);
            }
        }
        serde_json::Value::Number(value) => assert_eq!(
            actual.as_scalar(),
            Some(&Value::Int(value.as_i64().unwrap()))
        ),
        serde_json::Value::String(value) => {
            assert_eq!(actual.as_scalar(), Some(&Value::String(value.clone())))
        }
        _ => panic!("outside the authored output domain"),
    }
}

#[test]
fn physical_scalar_filters_keep_literal_complete_counts_and_current_owners() {
    run_valid_cases(cohorts()[0]);
}

fn run_valid_cases(cohort: Cohort) {
    let project = imported_design(cohort.design).project;
    let cases: Vec<serde_json::Value> = serde_json::from_str(cohort.cases).unwrap();
    for case in cases {
        let input = parse_input(cohort, &project, case["input"].as_str().unwrap()).unwrap();
        let before = input.clone();
        let outcome = engine::run(&project, &input);
        eprintln!(
            "{} {} mapping outcome: {outcome:?}",
            cohort.name, case["name"]
        );
        let output = outcome.unwrap();
        assert_eq!(input, before, "caller input changed: {}", case["name"]);
        let json: serde_json::Value =
            serde_json::from_str(&format_json::to_string(&project.target, &output).unwrap())
                .unwrap();
        println!(
            "{}",
            serde_json::json!({"profile":cohort.name,"case":case["name"],"stage":"mapping","typed":format!("{output:?}"),"output":json})
        );
        assert_complete_typed_output(&output, &case["expected"]);
        assert_eq!(json, case["expected"], "{}", case["name"]);
    }
}

#[test]
fn imported_count_graph_retains_selection_instead_of_a_bare_count() {
    let project = imported().project;
    for binding in &project.root.bindings {
        assert!(
            matches!(
                project.graph.nodes[&binding.node],
                Node::Aggregate {
                    function: AggregateOp::Sum,
                    expression: Some(_),
                    ..
                }
            ),
            "{binding:?}"
        );
        if let Node::Aggregate {
            collection,
            expression: Some(expression),
            ..
        } = &project.graph.nodes[&binding.node]
            && collection != &["Batch", "Record", "Values"]
        {
            let Node::If {
                condition,
                then,
                else_,
            } = &project.graph.nodes[expression]
            else {
                panic!("singular count lost its outer presence boundary");
            };
            assert!(
                matches!(&project.graph.nodes[condition], Node::Call { function, .. } if function == "exists")
            );
            assert!(matches!(&project.graph.nodes[then], Node::If { .. }));
            assert!(matches!(
                &project.graph.nodes[else_],
                Node::Const {
                    value: Value::Int(0)
                }
            ));
        }
    }
    assert!(!project.graph.nodes.values().any(|node| matches!(
        node,
        Node::Aggregate {
            function: AggregateOp::Count,
            ..
        }
    )));
    for expected in [
        "equal",
        "normalize_space",
        "trim",
        "not_equal",
        "is_empty",
        "not",
        "and",
        "exists",
    ] {
        assert!(
            project
                .graph
                .nodes
                .values()
                .any(|node| matches!(node, Node::Call { function, .. } if function == expected)),
            "lost predicate operation {expected}"
        );
    }
    assert!(project.graph.nodes.values().any(|node| matches!(node,
        Node::SourceField { frame: Some(frame), path }
        if frame == &["Batch", "Record"] && path == &["Text"]
    )));
    assert!(project.graph.nodes.values().any(|node| matches!(node,
        Node::Aggregate { function: AggregateOp::Sum, collection, expression: Some(_), .. }
        if collection == &["Batch", "Record", "Values"]
    )));
}

fn replace_one(text: &str, from: &str, to: &str) -> String {
    assert_eq!(text.matches(from).count(), 1, "fixture fragment: {from}");
    let result = text.replace(from, to);
    assert_ne!(result, text);
    result
}

struct Package(PathBuf);

impl Package {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_filtered_counts_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        for name in [
            "source.xsd",
            "target.schema.json",
            "bool-source.xsd",
            "bool-target.schema.json",
            "guarded-source.xsd",
            "guarded-bool-source.xsd",
            "guarded-bool-target.schema.json",
            "edi-target.schema.json",
            "Envelope.Config",
            "940.Config",
            "refusal-source.xsd",
            "refusal-target.schema.json",
        ] {
            std::fs::copy(fixture(name), path.join(name)).unwrap();
        }
        Self(path)
    }
}

impl Drop for Package {
    fn drop(&mut self) {
        eprintln!("retained filtered-count fixture: {}", self.0.display());
    }
}

fn assert_refused(label: &str, path: &Path) {
    preflight_path(label, path);
    let imported = mfd::import(path).unwrap();
    println!(
        "{}",
        serde_json::json!({"case":label,"stage":"admission","warnings":imported.warnings})
    );
    assert!(
        imported
            .warnings
            .iter()
            .any(|warning| warning.contains("aggregate `count`")
                && warning.contains("physical filtered count")),
        "{label}: {:?}",
        imported.warnings
    );
    assert!(!mfd::assess_import(&imported).executable, "{label}");
    preflight_path(label, path);
    assert!(
        mfd::import_with_profile(
            path,
            &mfd::ImportOptions::default(),
            mfd::ImportProfile::Executable
        )
        .is_err(),
        "{label}"
    );
    assert!(
        !imported.project.graph.nodes.values().any(|node| matches!(
            node,
            Node::Aggregate {
                function: AggregateOp::Count,
                ..
            }
        )),
        "{label}"
    );
}

fn selection_design(components: &str, edges: &str, predicate: u32) -> String {
    format!(
        r#"<mapping version="26"><component name="map"><structure><children>
<component name="source" library="xml" kind="14"><data><root><entry name="Source"><entry name="Record" outkey="1"><entry name="Text" outkey="2"/><entry name="Other" outkey="3"/><entry name="Gate" outkey="4"/></entry></entry></root><document schema="refusal-source.xsd" instanceroot="{{}}Source"/></data></component>
<component name="constant" library="core" kind="2"><targets><datapoint key="20"/></targets><data><constant value="keep" datatype="string"/></data></component>
<component name="constant" library="core" kind="2"><targets><datapoint key="21"/></targets><data><constant value="true" datatype="boolean"/></data></component>
<component name="constant" library="core" kind="2"><targets><datapoint key="22"/></targets><data><constant value="false" datatype="boolean"/></data></component>
{components}
<component name="filter" library="core" kind="3"><sources><datapoint pos="0" key="40"/><datapoint pos="1" key="41"/></sources><targets><datapoint pos="0" key="42"/><datapoint pos="1" key="43"/></targets></component>
<component name="count" library="core" kind="5"><sources><datapoint/><datapoint pos="1" key="50"/></sources><targets><datapoint key="51"/></targets></component>
<component name="count" library="core" kind="5"><sources><datapoint/><datapoint pos="1" key="52"/></sources><targets><datapoint key="53"/></targets></component>
<component name="target" library="json" kind="31"><properties XSLTDefaultOutput="1"/><data><root><entry name="document"><entry name="root"><entry name="object"><entry name="Included" type="json-property"><entry name="number" inpkey="100"/></entry><entry name="Excluded" type="json-property"><entry name="number" inpkey="101"/></entry></entry></entry></entry></root><json schema="refusal-target.schema.json"/></data></component>
</children><connections>{edges}<edge from="2" to="40"/><edge from="{predicate}" to="41"/><edge from="42" to="50"/><edge from="43" to="52"/><edge from="51" to="100"/><edge from="53" to="101"/></connections></structure></component></mapping>"#
    )
}

const EQUAL: &str = r#"<component name="equal" library="core" kind="5"><sources><datapoint pos="0" key="30"/><datapoint pos="1" key="31"/></sources><targets><datapoint key="32"/></targets></component>"#;

fn all_rendered_designs() -> Vec<(String, String)> {
    let mut designs = [
        (
            "counts.mfd",
            include_str!("fixtures/filtered-counts/counts.mfd"),
        ),
        (
            "bool-counts.mfd",
            include_str!("fixtures/filtered-counts/bool-counts.mfd"),
        ),
        (
            "edi-counts.mfd",
            include_str!("fixtures/filtered-counts/edi-counts.mfd"),
        ),
        (
            "guarded-counts.mfd",
            include_str!("fixtures/filtered-counts/guarded-counts.mfd"),
        ),
        (
            "guarded-bool-counts.mfd",
            include_str!("fixtures/filtered-counts/guarded-bool-counts.mfd"),
        ),
        (
            "guarded-edi-counts.mfd",
            include_str!("fixtures/filtered-counts/guarded-edi-counts.mfd"),
        ),
    ]
    .into_iter()
    .map(|(label, text)| (label.into(), text.into()))
    .collect::<Vec<_>>();
    designs.extend([21, 22].map(|predicate| {
        (
            format!("literal predicate {predicate}"),
            selection_design("", "", predicate),
        )
    }));
    designs.extend(independent_designs());
    designs.extend(eager_designs());
    designs.extend(nil_designs());
    designs.push((
        "same-field nillable direct Bool".into(),
        include_str!("fixtures/filtered-counts/guarded-bool-counts.mfd").into(),
    ));
    designs.extend(source_and_x12_designs());
    designs.extend(malformed_designs());
    designs.push(("raw or shorthand".into(), raw_or_design()));
    designs
}

fn preflight_all() {
    static PREFLIGHT: std::sync::OnceLock<()> = std::sync::OnceLock::new();
    PREFLIGHT.get_or_init(|| {
        // This gate renders and parses the entire catalog before any import,
        // not merely the first variant reached by a semantic test thread.
        preflight::check_all(&all_rendered_designs()).unwrap();
    });
}

fn preflight_path(label: &str, path: &Path) {
    preflight_all();
    preflight::check_package(label, path).unwrap();
}

#[test]
fn every_rendered_fixture_is_preflighted_before_semantic_controls() {
    preflight_all();
}

#[test]
fn fixture_preflight_rejects_syntax_and_component_pin_or_edge_drift() {
    let original = selection_design("", "", 21);
    preflight::check("literal predicate 21", &original).unwrap();
    for changed in [
        replace_one(
            &original,
            "inpkey=\"101\"/></entry></entry></entry></entry></root>",
            "inpkey=\"101\"/></entry></entry></entry></root>",
        ),
        replace_one(&original, "name=\"filter\"", "name=\"different-filter\""),
        replace_one(&original, "key=\"41\"", "key=\"410\""),
        replace_one(&original, "from=\"2\" to=\"40\"", "from=\"3\" to=\"40\""),
    ] {
        assert!(preflight::check("literal predicate 21", &changed).is_err());
    }
}

#[test]
fn literal_false_and_true_keep_nil_presence_outside_exclusion_inversion() {
    for (predicate, expected) in [
        (21, serde_json::json!({"Included":2,"Excluded":0})),
        (22, serde_json::json!({"Included":0,"Excluded":2})),
    ] {
        let package = Package::new();
        let schema = include_str!("fixtures/filtered-counts/refusal-source.xsd");
        std::fs::write(
            package.0.join("refusal-source.xsd"),
            replace_one(
                schema,
                "name=\"Text\" type=\"xs:string\" minOccurs=\"0\"",
                "name=\"Text\" type=\"xs:string\" minOccurs=\"0\" nillable=\"true\"",
            ),
        )
        .unwrap();
        let path = package.0.join("selection.mfd");
        std::fs::write(&path, selection_design("", "", predicate)).unwrap();
        preflight_path(&format!("literal predicate {predicate}"), &path);
        let imported = mfd::import_with_profile(
            &path,
            &mfd::ImportOptions::default(),
            mfd::ImportProfile::Executable,
        )
        .unwrap()
        .imported;
        assert!(imported.warnings.is_empty());
        let input = format_xml::from_str("<Source xmlns:xsi='http://www.w3.org/2001/XMLSchema-instance'><Record><Text xsi:nil='true'/><Other>x</Other></Record><Record><Text/><Other>x</Other></Record><Record><Other>x</Other></Record></Source>", &imported.project.source).unwrap();
        let output = engine::run(&imported.project, &input).unwrap();
        let serialized: serde_json::Value = serde_json::from_str(
            &format_json::to_string(&imported.project.target, &output).unwrap(),
        )
        .unwrap();
        println!(
            "{}",
            serde_json::json!({"case":"literal_nil_presence","predicate":predicate,"stage":"mapping","typed":format!("{output:?}"),"output":serialized})
        );
        assert_complete_typed_output(&output, &expected);
        assert_eq!(serialized, expected);
    }
}

#[test]
fn independent_required_optional_and_nillable_operands_are_admission_refusals() {
    for design in ["counts.mfd", "bool-counts.mfd", "edi-counts.mfd"] {
        // The original literal oracles remain untouched, but these mappings
        // are not executable under the guarded-field contract.
        assert_refused(design, &fixture(design));
    }
    for (label, text) in independent_designs() {
        let package = Package::new();
        let path = package.0.join("selection.mfd");
        std::fs::write(&path, text).unwrap();
        assert_refused(&label, &path);
    }
}

fn independent_designs() -> Vec<(String, String)> {
    [
        (
            "required sibling is not presence proof",
            EQUAL.to_owned(),
            r#"<edge from="3" to="30"/><edge from="20" to="31"/>"#.to_owned(),
            32,
        ),
        (
            "optional direct Bool sibling",
            String::new(),
            String::new(),
            4,
        ),
        (
            "inversion cannot turn missing predicate into exclusion",
            format!(
                r#"{EQUAL}<component name="logical-not" library="core" kind="5"><sources><datapoint key="33"/></sources><targets><datapoint key="34"/></targets></component>"#
            ),
            r#"<edge from="3" to="30"/><edge from="20" to="31"/><edge from="32" to="33"/>"#
                .to_owned(),
            34,
        ),
    ].into_iter().map(|(label, components, edges, predicate)| (label.into(), selection_design(&components, &edges, predicate))).collect()
}

#[test]
fn every_eager_logical_operand_requires_the_same_field_proof() {
    for (label, text) in eager_designs() {
        let package = Package::new();
        let path = package.0.join("selection.mfd");
        std::fs::write(&path, text).unwrap();
        assert_refused(&label, &path);
    }
}

fn eager_designs() -> Vec<(String, String)> {
    let mut designs = Vec::new();
    for (name, decisive) in [("logical-and", 22), ("logical-or", 21)] {
        for independent_position in [0, 1] {
            let components = format!(
                r#"{EQUAL}<component name="{name}" library="core" kind="5"><sources><datapoint pos="0" key="35"/><datapoint pos="1" key="36"/></sources><targets><datapoint key="37"/></targets></component>"#
            );
            let (left, right) = if independent_position == 0 {
                (32, decisive)
            } else {
                (decisive, 32)
            };
            let edges = format!(
                r#"<edge from="3" to="30"/><edge from="20" to="31"/><edge from="{left}" to="35"/><edge from="{right}" to="36"/>"#
            );
            designs.push((
                format!("{name} independent operand {independent_position}"),
                selection_design(&components, &edges, 37),
            ));
        }
    }
    designs
}

#[test]
fn every_nil_reading_predicate_is_refused_even_when_the_builtin_accepts_nil() {
    for (label, text) in nil_designs() {
        let package = Package::new();
        let schema = include_str!("fixtures/filtered-counts/refusal-source.xsd");
        std::fs::write(
            package.0.join("refusal-source.xsd"),
            replace_one(
                schema,
                "name=\"Text\" type=\"xs:string\" minOccurs=\"0\"",
                "name=\"Text\" type=\"xs:string\" minOccurs=\"0\" nillable=\"true\"",
            ),
        )
        .unwrap();
        let path = package.0.join("selection.mfd");
        std::fs::write(&path, text).unwrap();
        assert_refused(&label, &path);
    }
    let package = Package::new();
    std::fs::write(
        package.0.join("guarded-bool-source.xsd"),
        replace_one(
            include_str!("fixtures/filtered-counts/guarded-bool-source.xsd"),
            "name=\"Enabled\" type=\"xs:boolean\" minOccurs=\"0\"",
            "name=\"Enabled\" type=\"xs:boolean\" minOccurs=\"0\" nillable=\"true\"",
        ),
    )
    .unwrap();
    let path = package.0.join("selection.mfd");
    std::fs::write(
        &path,
        include_str!("fixtures/filtered-counts/guarded-bool-counts.mfd"),
    )
    .unwrap();
    assert_refused("same-field nillable direct Bool", &path);
}

fn nil_designs() -> Vec<(String, String)> {
    [
        ("equal", EQUAL.to_owned(), r#"<edge from="2" to="30"/><edge from="20" to="31"/>"#.to_owned()),
        ("empty", r#"<component name="empty" library="lang" kind="5"><sources><datapoint key="30"/></sources><targets><datapoint key="32"/></targets></component>"#.to_owned(), r#"<edge from="2" to="30"/>"#.to_owned()),
        ("trim", format!(r#"<component name="trim" library="lang" kind="5"><sources><datapoint key="33"/></sources><targets><datapoint key="34"/></targets></component>{EQUAL}"#), r#"<edge from="2" to="33"/><edge from="34" to="30"/><edge from="20" to="31"/>"#.to_owned()),
        ("normalize-space", format!(r#"<component name="normalize-space" library="lang" kind="5"><sources><datapoint key="33"/></sources><targets><datapoint key="34"/></targets></component>{EQUAL}"#), r#"<edge from="2" to="33"/><edge from="34" to="30"/><edge from="20" to="31"/>"#.to_owned()),
    ].into_iter().map(|(label, components, edges)| (label.into(), selection_design(&components, &edges, 32))).collect()
}

#[test]
fn same_path_from_another_source_and_unproved_x12_shapes_are_refused() {
    for (label, design) in source_and_x12_designs() {
        let package = Package::new();
        if label == "repeating X12 scalar including literal predicate" {
            std::fs::write(
                package.0.join("940.Config"),
                replace_one(
                    include_str!("fixtures/filtered-counts/940.Config"),
                    "name=\"Tag\" type=\"string\" minOccurs=\"0\"",
                    "name=\"Tag\" type=\"string\" minOccurs=\"0\" maxOccurs=\"unbounded\"",
                ),
            )
            .unwrap();
        }
        let path = package.0.join("selection.mfd");
        std::fs::write(&path, design).unwrap();
        assert_refused(&label, &path);
    }
}

fn source_and_x12_designs() -> Vec<(String, String)> {
    let original = include_str!("fixtures/filtered-counts/guarded-counts.mfd");
    let secondary = r#"<component name="other-source" library="xml" kind="14"><data><root><entry name="Source"><entry name="Batch"><entry name="Record"><entry name="Text" outkey="200"/></entry></entry></entry></root><document schema="guarded-source.xsd" instanceroot="{}Source"/></data></component>"#;
    let secondary_design = replace_one(
        &replace_one(original, "</children>", &format!("{secondary}</children>")),
        "from=\"13\" to=\"30\"",
        "from=\"200\" to=\"30\"",
    );
    let edi = include_str!("fixtures/filtered-counts/guarded-edi-counts.mfd");
    vec![
        (
            "same absolute path from another source".into(),
            secondary_design,
        ),
        (
            "X12 without compiled configuration witness".into(),
            replace_one(edi, " config=\"940.Config\"", ""),
        ),
        (
            "repeating X12 scalar including literal predicate".into(),
            edi.to_owned(),
        ),
    ]
}

#[test]
fn malformed_unresolved_or_foreign_filters_cannot_become_unfiltered_counts() {
    for (label, text) in malformed_designs() {
        let package = Package::new();
        let path = package.0.join("counts.mfd");
        std::fs::write(&path, text).unwrap();
        assert_refused(&label, &path);
    }
}

fn malformed_designs() -> Vec<(String, String)> {
    let original = include_str!("fixtures/filtered-counts/guarded-counts.mfd");
    [
        (
            "missing predicate",
            replace_one(original, "<edge from=\"32\" to=\"41\"/>", ""),
        ),
        (
            "conflicting predicate",
            replace_one(
                original,
                "</connections>",
                "<edge from=\"21\" to=\"41\"/></connections>",
            ),
        ),
        (
            "group instead of scalar",
            replace_one(original, "from=\"13\" to=\"40\"", "from=\"12\" to=\"40\""),
        ),
        (
            "foreign repeated owner",
            replace_one(original, "from=\"13\" to=\"30\"", "from=\"16\" to=\"30\""),
        ),
        (
            "predicate cycle",
            replace_one(original, "from=\"68\" to=\"69\"", "from=\"70\" to=\"69\""),
        ),
        (
            "unknown predicate",
            replace_one(
                original,
                "name=\"empty\" library=\"lang\"",
                "name=\"unknown-predicate\" library=\"lang\"",
            ),
        ),
        (
            "raw not shorthand",
            replace_one(original, "name=\"logical-not\"", "name=\"not\""),
        ),
        (
            "raw and shorthand",
            replace_one(original, "name=\"logical-and\"", "name=\"and\""),
        ),
        (
            "normalized malformed position",
            replace_one(original, "pos=\"1\" key=\"41\"", "pos=\"9\" key=\"41\""),
        ),
        (
            "duplicate selected source",
            replace_one(
                original,
                "name=\"Values\" outkey=\"15\"",
                "name=\"Values\" outkey=\"13\"",
            ),
        ),
        (
            "extra filter input",
            replace_one(
                original,
                "key=\"41\"/></sources>",
                "key=\"41\"/><datapoint pos=\"2\" key=\"47\"/></sources>",
            ),
        ),
        (
            "discarded downstream window",
            replace_one(
                &replace_one(
                    original,
                    "</children>",
                    "<component name=\"first-items\" library=\"core\" kind=\"5\"><sources><datapoint key=\"45\"/></sources><targets><datapoint key=\"46\"/></targets></component></children>",
                ),
                "<edge from=\"42\" to=\"50\"/>",
                "<edge from=\"42\" to=\"45\"/><edge from=\"46\" to=\"50\"/>",
            ),
        ),
    ].into_iter().map(|(label, text)| (label.into(), text)).collect()
}

#[test]
fn raw_or_shorthand_keeps_explicit_executable_count_refusal() {
    let package = Package::new();
    let path = package.0.join("counts.mfd");
    std::fs::write(&path, raw_or_design()).unwrap();
    assert_refused("raw or shorthand", &path);
}

fn raw_or_design() -> String {
    replace_one(
        include_str!("fixtures/filtered-counts/guarded-bool-counts.mfd"),
        "name=\"logical-or\" library=\"core\" kind=\"5\"><sources><datapoint pos=\"0\" key=\"50\"",
        "name=\"or\" library=\"core\" kind=\"5\"><sources><datapoint pos=\"0\" key=\"50\"",
    )
}

#[test]
fn invalid_input_has_an_exact_input_stage_error_not_a_count() {
    for cohort in cohorts() {
        let project = imported_design(cohort.design).project;
        let cases: Vec<serde_json::Value> = serde_json::from_str(cohort.rejections).unwrap();
        for case in cases {
            assert_eq!(case["stage"], "input");
            let error = parse_input(cohort, &project, case["input"].as_str().unwrap()).unwrap_err();
            println!(
                "{}",
                serde_json::json!({"profile":cohort.name,"case":case["name"],"stage":"input","category":error.category(),"error":format!("{error:?}")})
            );
            assert_eq!(
                error.category(),
                case["nativeCategory"].as_str().unwrap(),
                "{}: {}",
                cohort.name,
                case["name"]
            );
        }
    }
}

#[test]
fn bool_self_predicates_constant_expressions_and_all_or_pairs_keep_typed_counts() {
    let cohort = cohorts()[1];
    let project = imported_design(cohort.design).project;
    assert!(matches!(
        project
            .source
            .child("Record")
            .unwrap()
            .child("Enabled")
            .unwrap()
            .kind,
        ir::SchemaKind::Scalar {
            ty: ir::ScalarType::Bool
        }
    ));
    for expected in ["or", "and", "not"] {
        assert!(
            project
                .graph
                .nodes
                .values()
                .any(|node| matches!(node, Node::Call { function, .. } if function == expected)),
            "missing admitted predicate {expected}"
        );
    }
    run_valid_cases(cohort);
}

#[test]
fn actual_x12_configuration_and_wire_input_preserve_filtered_occurrences() {
    let cohort = cohorts()[2];
    let project = imported_design(cohort.design).project;
    assert_eq!(
        project.source_options.edi_kind,
        Some(mapping::EdiBoundaryKind::X12)
    );
    let group = project.source.child("Group").unwrap();
    let message = group.child("Message").unwrap();
    let record = message.child("Record").unwrap();
    assert!(group.repeating && message.repeating && record.repeating);
    assert!(
        record
            .child("N1")
            .unwrap()
            .child("Tag")
            .unwrap()
            .is_scalar()
    );
    run_valid_cases(cohort);
}
