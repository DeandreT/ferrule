//! Neutral explicit-entry tests; no original customer mapping or payload.
use ir::{Instance, Value};
use std::path::PathBuf;

#[path = "x12_document_body_scalars/native_r3.rs"]
mod native_r3;
#[path = "x12_document_body_scalars/native_r4.rs"]
mod native_r4;
#[path = "x12_document_body_scalars/native_r5.rs"]
mod native_r5;

fn generated_child(parent: &std::path::Path, test_name: &str) -> PathBuf {
    let directory = parent.join(test_name);
    std::fs::create_dir_all(&directory).unwrap();
    directory
}

fn generated_test_directory(test_name: &str) -> PathBuf {
    let parent = std::env::var_os("MFD051_GENERATED_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::temp_dir().join(format!("mfd051-generated-{}", std::process::id()))
        });
    generated_child(&parent, test_name)
}

#[test]
fn generation_directories_are_order_independent() {
    let root = generated_test_directory("directory-order-regression");
    for (index, order) in [["native-r4", "basic-owned"], ["basic-owned", "native-r4"]]
        .into_iter()
        .enumerate()
    {
        let shared = root.join(index.to_string());
        for name in order {
            let child = generated_child(&shared, name);
            std::fs::write(child.join("owned-marker.txt"), name).unwrap();
        }
        for name in ["native-r4", "basic-owned"] {
            assert_eq!(
                std::fs::read_to_string(shared.join(name).join("owned-marker.txt")).unwrap(),
                name
            );
        }
    }
}

fn text(value: &str) -> Instance {
    Instance::Scalar(Value::String(value.into()))
}
fn group(fields: Vec<(&str, Instance)>) -> Instance {
    Instance::Group(
        fields
            .into_iter()
            .map(|(k, v)| (k.into(), v))
            .collect::<Vec<_>>()
            .into(),
    )
}
struct Fixture(PathBuf);
impl Fixture {
    fn new(functions: &str, edges: &str, body: Option<(&str, &str)>) -> Self {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "ferrule-x12-scalars-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir(&root).unwrap();
        let source =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/x12_document_bodies");
        for name in ["Envelope.Config", "850.Config", "Defs.Segment"] {
            std::fs::copy(source.join(name), root.join(name)).unwrap();
        }
        let original = std::fs::read_to_string(source.join("mapping.mfd")).unwrap();
        let source_component = original
            .lines()
            .find(|line| line.contains("name=\"Source\""))
            .unwrap();
        let (schema, entries) = body.unwrap_or((
            r#"{"type":"object","properties":{"result":{"type":"string"},"present":{"type":"boolean"}}}"#,
            r#"<entry name="result" type="json-property"><entry name="string" inpkey="71"/></entry><entry name="present" type="json-property"><entry name="boolean" inpkey="72"/></entry>"#,
        ));
        let mut entries = entries.to_string();
        for port in [71, 72, 73] {
            if !edges.contains(&format!("<edge vertexkey=\"{port}\"/>")) {
                entries = entries.replace(&format!(" inpkey=\"{port}\""), "");
            }
        }
        std::fs::write(root.join("target.json"), schema).unwrap();
        std::fs::write(root.join("mapping.mfd"), format!(r#"<mapping version="36"><component name="map" uid="1"><structure><children>
<component name="Input" library="core" kind="6" uid="2"><sources><datapoint/></sources><targets><datapoint pos="0" key="1"/></targets><data><input datatype="string"/><parameter usageKind="input" name="Input"/></data></component>
<component name="concat" library="core" kind="5" uid="5"><sources><datapoint pos="0" key="51"/><datapoint pos="1" key="52"/></sources><targets><datapoint pos="0" key="4"/></targets></component>
<component name="constant" library="core" kind="2" uid="4"><targets><datapoint pos="0" key="3"/></targets><data><constant value=".json" datatype="string"/></data></component>
{source_component}
<component name="Target" library="json" kind="31" uid="11"><properties DynamicFilenames="1" XSLTDefaultOutput="1"/><data><root><entry name="FileInstance" inpkey="40"><entry name="document" casttotargettypemode="cast-in-subtree"><entry name="root"><entry name="object">{entries}</entry></entry></entry></entry></root><json schema="target.json"/></data></component>
{functions}
</children><graph><vertices><vertex vertexkey="1"><edges><edge vertexkey="10"/></edges></vertex><vertex vertexkey="21"><edges><edge vertexkey="51"/></edges></vertex><vertex vertexkey="3"><edges><edge vertexkey="52"/></edges></vertex><vertex vertexkey="4"><edges><edge vertexkey="40"/></edges></vertex>{edges}</vertices></graph></structure></component></mapping>"#)).unwrap();
        Self(root)
    }
    fn change(&self, name: &str, from: &str, to: &str) {
        let path = self.0.join(name);
        let before = std::fs::read_to_string(&path).unwrap();
        assert!(before.contains(from), "missing mutation: {from}");
        std::fs::write(path, before.replacen(from, to, 1)).unwrap();
    }
    fn import(&self) -> Result<mfd::ImportedX12DocumentBodies, mfd::MfdError> {
        mfd::import_x12_document_bodies_with_options(
            &self.0.join("mapping.mfd"),
            &mfd::ImportOptions::default().with_package_root(&self.0),
        )
    }
}

fn unary(library: &str, name: &str, input: u32, output: u32, uid: u32) -> String {
    format!(
        r#"<component name="{name}" library="{library}" kind="5" uid="{uid}"><sources><datapoint pos="0" key="{input}"/></sources><targets><datapoint pos="0" key="{output}"/></targets></component>"#
    )
}
fn edge(from: u32, to: u32) -> String {
    format!(r#"<vertex vertexkey="{from}"><edges><edge vertexkey="{to}"/></edges></vertex>"#)
}
fn map() -> String {
    r#"<component name="value-map" library="core" kind="23" uid="20"><sources><datapoint pos="0" key="101"/></sources><targets><datapoint pos="0" key="102"/></targets><data><valuemap defaultValueMode="none"><valuemapTable><entry from="A" to="alpha"/><entry from="B" to="beta"/><entry from="" to="empty-key"/><entry from="E" to=""/><entry from=" A " to="padded"/><entry from="a" to="lower"/><entry from="&amp;&lt;" to="α&amp;&lt;"/></valuemapTable><input name="input" type="string"/><result name="result" type="string"/></valuemap></data></component>"#.into()
}
fn mapped() -> Fixture {
    Fixture::new(
        &(map() + &unary("db", "is-not-null", 103, 104, 21)),
        &(edge(21, 101) + &edge(102, 71) + &edge(102, 103) + &edge(104, 72)),
        None,
    )
}
fn input(project: &mapping::Project, value: Option<Value>) -> Instance {
    let mut fields = vec![];
    if let Some(value) = value {
        fields.push(("F1", Instance::Scalar(value)));
    }
    let beg = group(fields);
    group(vec![(
        "BEG",
        if project.source.child("BEG").unwrap().repeating {
            Instance::Repeated(vec![beg])
        } else {
            beg
        },
    )])
}
fn run(project: &mapping::Project, value: Option<Value>) -> Result<Instance, engine::EngineError> {
    engine::run(project, &input(project, value))
}
fn string(value: &str) -> Option<Value> {
    Some(Value::String(value.into()))
}

fn sequence_scalar_fixture(last_guard: bool) -> Fixture {
    let mut functions = r#"
<component name="constant" library="core" kind="2" uid="20"><targets><datapoint pos="0" key="200"/></targets><data><constant value="MATCH" datatype="string"/></data></component>
<component name="equal" library="core" kind="5" uid="21"><sources><datapoint pos="0" key="201"/><datapoint pos="1" key="202"/></sources><targets><datapoint pos="0" key="203"/></targets></component>
<component name="if-else" library="core" kind="4" uid="22"><sources><datapoint pos="0" key="204"/><datapoint pos="1" key="205"/><datapoint/></sources><targets><datapoint pos="0" key="206"/></targets></component>
"#.to_owned();
    let mut edges = edge(23, 201) + &edge(200, 202) + &edge(203, 204) + &edge(24, 205);
    if last_guard {
        functions += &unary("db", "is-not-null", 207, 208, 23);
        functions += r#"<component name="if-else" library="core" kind="4" uid="24"><sources><datapoint pos="0" key="209"/><datapoint pos="1" key="210"/><datapoint/></sources><targets><datapoint pos="0" key="211"/></targets></component>"#;
        edges += &(edge(206, 207) + &edge(206, 210) + &edge(208, 209) + &edge(211, 71));
    } else {
        functions += r#"<component name="format-dateTime" library="core" kind="5" uid="23"><sources><datapoint pos="0" key="207"/><datapoint pos="1" key="208"/><datapoint/></sources><targets><datapoint pos="0" key="209"/></targets></component><component name="constant" library="core" kind="2" uid="24"><targets><datapoint pos="0" key="210"/></targets><data><constant value="[Y0001]-[M01]-[D01]" datatype="string"/></data></component>"#;
        edges += &(edge(206, 207) + &edge(210, 208) + &edge(209, 71));
    }
    let fixture = Fixture::new(&functions, &edges, None);
    fixture.change(
        "Defs.Segment",
        r#"name="F2" type="decimal""#,
        r#"name="F2" type="string""#,
    );
    fixture.change(
        "Defs.Segment",
        r#"<Data ref="F2"/>"#,
        r#"<Data ref="F2" minOccurs="0"/>"#,
    );
    fixture
}

fn sequence_input(rows: &[(&str, Option<&str>)]) -> Instance {
    group(vec![(
        "Rows",
        Instance::Repeated(
            rows.iter()
                .map(|(tag, value)| {
                    let mut fields = vec![("F1", text(tag))];
                    if let Some(value) = value {
                        fields.push(("F2", text(value)));
                    }
                    group(vec![("LIN", group(fields))])
                })
                .collect(),
        ),
    )])
}

#[test]
fn presence_guard_selects_last_value_of_its_shared_sequence() {
    let fixture = sequence_scalar_fixture(true);
    let imported = fixture.import().unwrap();
    let body = &imported.targets[0].body;
    for (rows, expected) in [
        (
            vec![("MATCH", Some("first")), ("MATCH", Some("last"))],
            text("last"),
        ),
        (
            vec![("MATCH", Some("first")), ("OTHER", Some("ignored"))],
            text("first"),
        ),
        (
            vec![("MATCH", Some("first")), ("MATCH", None)],
            text("first"),
        ),
        (
            vec![("MATCH", Some("first")), ("MATCH", Some(""))],
            text(""),
        ),
        (
            vec![("OTHER", Some("ignored"))],
            Instance::Scalar(Value::Null),
        ),
    ] {
        let actual = engine::run(body, &sequence_input(&rows)).unwrap();
        assert_eq!(actual.field("result"), Some(&expected));
    }
    assert!(body.graph.nodes.values().any(|node| matches!(
        node,
        mapping::Node::Aggregate {
            function: mapping::AggregateOp::LastPresentText,
            ..
        }
    )));
    assert!(codegen::lower(body).is_ok());
}

#[test]
fn conditional_date_selects_before_conversion_without_demanding_later_values() {
    let fixture = sequence_scalar_fixture(false);
    let imported = fixture.import().unwrap();
    let body = &imported.targets[0].body;
    for rows in [
        vec![("MATCH", Some("2026-10-24")), ("MATCH", Some("BAD"))],
        vec![("OTHER", Some("BAD")), ("MATCH", Some("2026-10-24"))],
    ] {
        let actual = engine::run(body, &sequence_input(&rows)).unwrap();
        assert_eq!(actual.field("result"), Some(&text("2026-10-24")));
    }
    for first in ["BAD", ""] {
        assert!(
            engine::run(
                body,
                &sequence_input(&[("MATCH", Some(first)), ("MATCH", Some("2026-10-24"))])
            )
            .is_err()
        );
    }
    assert!(
        body.graph
            .nodes
            .values()
            .any(|node| matches!(node, mapping::Node::CollectionFind { .. }))
    );
    assert!(codegen::lower(body).is_ok());
}

fn xml_sequence_fixture() -> Fixture {
    let fixture = sequence_scalar_fixture(false);
    let mapping = std::fs::read_to_string(fixture.0.join("mapping.mfd")).unwrap();
    let start = mapping.find(r#"<component name="Target""#).unwrap();
    let end = start + mapping[start..].find("</component>").unwrap() + "</component>".len();
    fixture.change(
        "mapping.mfd",
        &mapping[start..end],
        r#"<component name="Target" library="xml" kind="14" uid="11"><properties DynamicFilenames="1"/><data><root><entry name="FileInstance" inpkey="40"><entry name="document"><entry name="Target"><entry name="result" inpkey="71"/></entry></entry></entry></root><document schema="target.xsd" instanceroot="{}Target"/></data></component>"#,
    );
    std::fs::write(
        fixture.0.join("target.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Target"><xs:complexType><xs:sequence><xs:element name="result" type="xs:string" minOccurs="0"/></xs:sequence></xs:complexType></xs:element></xs:schema>"#,
    )
    .unwrap();
    fixture
}

#[test]
fn xml_conditional_date_still_converts_later_matching_values() {
    let fixture = xml_sequence_fixture();
    let imported = fixture.import().unwrap();
    let body = &imported.targets[0].body;
    assert_eq!(
        engine::run(
            body,
            &sequence_input(&[("MATCH", Some("2026-10-24")), ("MATCH", Some("2026-10-25"))])
        )
        .unwrap()
        .field("result"),
        Some(&Instance::MappedSequence(vec![
            text("2026-10-24"),
            text("2026-10-25"),
        ]))
    );
    assert!(
        engine::run(
            body,
            &sequence_input(&[("MATCH", Some("2026-10-24")), ("MATCH", Some("BAD"))])
        )
        .is_err()
    );
    assert!(
        !body
            .graph
            .nodes
            .values()
            .any(|node| matches!(node, mapping::Node::CollectionFind { .. }))
    );
}

#[test]
fn xml_direct_date_text_conversion_is_reached_per_occurrence() {
    let fixture = xml_sequence_fixture();
    fixture.change("mapping.mfd", &edge(209, 71), &edge(24, 71));
    fixture.change(
        "Defs.Segment",
        r#"name="F2" type="string""#,
        r#"name="F2" type="date" minLength="8" maxLength="8""#,
    );
    let imported = fixture.import().unwrap();
    let body = &imported.targets[0].body;
    let value = engine::run(
        body,
        &sequence_input(&[("A", Some("2024-02-29")), ("B", None)]),
    )
    .unwrap();
    assert_eq!(
        value.field("result"),
        Some(&Instance::MappedSequence(vec![text("2024-02-29")]))
    );
    for invalid in ["BAD", "2026-02-29", "2026-13-01", "2026-10-24T12:00:00"] {
        assert!(
            engine::run(
                body,
                &sequence_input(&[("A", Some("2024-02-29")), ("B", Some(invalid))])
            )
            .is_err(),
            "accepted {invalid}"
        );
    }
    assert!(codegen::lower(body).is_ok());
}

#[test]
fn json_typed_date_selection_validates_only_the_selected_date() {
    let fixture = sequence_scalar_fixture(false);
    fixture.change(
        "Defs.Segment",
        r#"name="F2" type="string""#,
        r#"name="F2" type="date" minLength="8" maxLength="8""#,
    );
    let imported = fixture.import().unwrap();
    let body = &imported.targets[0].body;
    for invalid in ["2026-02-29", "2026-10-24T12:00:00", "BAD"] {
        assert!(engine::run(body, &sequence_input(&[("MATCH", Some(invalid))])).is_err());
        assert_eq!(
            engine::run(
                body,
                &sequence_input(&[("MATCH", Some("2024-02-29")), ("MATCH", Some(invalid))])
            )
            .unwrap()
            .field("result"),
            Some(&text("2024-02-29")),
        );
    }
}

#[test]
fn value_map_all_entries_misses_empty_null_case_and_whitespace() {
    let fixture = mapped();
    let imported = fixture.import().unwrap();
    let project = &imported.targets[0].body;
    for (key, expected) in [
        ("A", "alpha"),
        ("B", "beta"),
        ("", "empty-key"),
        ("E", ""),
        (" A ", "padded"),
        ("a", "lower"),
        ("&<", "α&<"),
    ] {
        let output = run(project, string(key)).unwrap();
        assert_eq!(output.field("result"), Some(&text(expected)), "{key:?}");
        assert_eq!(
            output.field("present"),
            Some(&Instance::Scalar(Value::Bool(true)))
        );
    }
    for key in [
        string("missing"),
        string("A "),
        string(" A"),
        string("b"),
        string(" "),
        Some(Value::Null),
        None,
    ] {
        let output = run(project, key.clone()).unwrap();
        assert_eq!(
            output.field("result"),
            Some(&Instance::Scalar(Value::Null)),
            "{key:?}"
        );
        assert!(
            format_json::to_value(&project.target, &output)
                .unwrap()
                .get("result")
                .is_none()
        );
        assert_eq!(
            output.field("present"),
            Some(&Instance::Scalar(Value::Bool(false)))
        );
    }
    assert!(codegen::lower(project).is_ok());
    assert!(
        project
            .graph
            .nodes
            .values()
            .any(|n| matches!(n, mapping::Node::ValueMap { default: None, .. }))
    );
}

#[test]
fn accepts_only_disabled_default_forms_and_explicit_string_types() {
    for mode in [
        "",
        r#"defaultValueMode="none""#,
        r#"enableDefaultValue="0""#,
        r#"defaultValueMode="none" enableDefaultValue="0""#,
    ] {
        let fixture = mapped();
        fixture.change("mapping.mfd", r#"defaultValueMode="none""#, mode);
        fixture.change(
            "mapping.mfd",
            r#"name="input" type="string""#,
            r#"name="input" type="String""#,
        );
        fixture.import().unwrap();
    }
}

#[test]
fn main_selection_preserves_uncalled_library_definitions_in_either_order() {
    // Reuse main graph UIDs and ports in an unsupported definition: namespaces
    // must not leak, and no definition body is deleted, evaluated or inlined.
    let definition = r#"<component name="unused_helper" library="ExampleFunctions" uid="900" inline="1"><structure><children><component name="unsupported" library="ExampleFunctions" kind="99" uid="20"><targets><datapoint pos="0" key="102"/></targets></component></children></structure></component>"#;
    for before_main in [false, true] {
        let fixture = mapped();
        if before_main {
            fixture.change(
                "mapping.mfd",
                "<mapping version=\"36\">",
                &format!("<mapping version=\"36\">{definition}"),
            );
        } else {
            fixture.change(
                "mapping.mfd",
                "</mapping>",
                &format!("{definition}</mapping>"),
            );
        }
        fixture.change(
            "mapping.mfd",
            "name=\"map\" uid=\"1\"",
            "name=\"ArbitraryMain\" uid=\"801\"",
        );
        let original = std::fs::read(fixture.0.join("mapping.mfd")).unwrap();
        let imported = fixture.import().unwrap();
        assert_eq!(
            run(&imported.targets[0].body, string("A"))
                .unwrap()
                .field("result"),
            Some(&text("alpha"))
        );
        assert_eq!(
            std::fs::read(fixture.0.join("mapping.mfd")).unwrap(),
            original
        );
    }
}

#[test]
fn main_selection_refuses_ambiguous_main_and_reached_definition_calls() {
    let fixture = mapped();
    fixture.change("mapping.mfd", "</mapping>", r#"<component name="SecondMain" uid="900"><structure><children/></structure></component></mapping>"#);
    assert!(
        fixture
            .import()
            .err()
            .unwrap()
            .to_string()
            .contains("requires one unqualified main mapping wrapper")
    );

    let fixture = mapped();
    fixture.change("mapping.mfd", "</mapping>", r#"<component name="helper" library="ExampleFunctions" uid="900" inline="1"><structure><children/></structure></component></mapping>"#);
    fixture.change(
        "mapping.mfd",
        &map(),
        &unary("ExampleFunctions", "helper", 101, 102, 20),
    );
    // Existing connected inputs/outputs now reach the unsupported call. A
    // sibling declaration must not turn it into an identity or inline body.
    assert!(
        fixture
            .import()
            .err()
            .unwrap()
            .to_string()
            .contains("unsupported function ExampleFunctions:helper")
    );
}

#[test]
fn value_map_display_labels_do_not_define_semantic_columns() {
    for (input_label, result_label) in [
        ("SourceCode", "MappedCategory"),
        ("SharedLabel", "SharedLabel"),
    ] {
        let fixture = mapped();
        fixture.change(
            "mapping.mfd",
            r#"name="input" type="string""#,
            &format!(r#"name="{input_label}" type="String""#),
        );
        fixture.change(
            "mapping.mfd",
            r#"name="result" type="string""#,
            &format!(r#"name="{result_label}" type="string""#),
        );
        let original = std::fs::read(fixture.0.join("mapping.mfd")).unwrap();
        let imported = fixture.import().unwrap();
        let project = &imported.targets[0].body;
        assert_eq!(
            run(project, string("A")).unwrap().field("result"),
            Some(&text("alpha"))
        );
        assert_eq!(
            run(project, string("E")).unwrap().field("result"),
            Some(&text(""))
        );
        assert_eq!(
            run(project, string("missing")).unwrap().field("result"),
            Some(&Instance::Scalar(Value::Null))
        );
        assert_eq!(
            std::fs::read(fixture.0.join("mapping.mfd")).unwrap(),
            original
        );
    }
}

#[test]
fn display_labels_do_not_relax_value_map_structure_types_or_defaults() {
    for (from, to) in [
        (r#"name="input" type="string""#, r#"name="" type="string""#),
        (
            r#"name="result" type="string""#,
            r#"name=" " type="string""#,
        ),
        (r#"name="input" type="string""#, r#"type="string""#),
        (
            r#"name="result" type="string""#,
            r#"name="MappedCategory" type="integer""#,
        ),
        (
            r#"name="input" type="string""#,
            r#"name="SourceCode" type="MysteryString""#,
        ),
        (
            r#"name="result" type="string""#,
            r#"name="MappedCategory" type="string" defaultValue="fallback""#,
        ),
        (r#"pos="0" key="101""#, r#"pos="1" key="101""#),
        (
            r#"<input name="input" type="string"/>"#,
            r#"<result name="SourceCode" type="string"/>"#,
        ),
        (
            r#"<entry from="B" to="beta"/>"#,
            r#"<entry from="A" to="beta"/>"#,
        ),
    ] {
        let fixture = mapped();
        fixture.change("mapping.mfd", from, to);
        assert!(
            fixture.import().is_err(),
            "accepted display-label mutation: {from} -> {to}"
        );
    }
}

#[test]
fn value_map_rejects_defaults_foreign_unknown_malformed_and_duplicate_metadata() {
    let mutations = [
        (r#"defaultValueMode="none""#, r#"defaultValueMode="custom""#),
        (r#"defaultValueMode="none""#, r#"defaultValueMode="input""#),
        (
            r#"defaultValueMode="none""#,
            r#"defaultValueMode="unknown""#,
        ),
        (r#"defaultValueMode="none""#, r#"enableDefaultValue="1""#),
        (
            r#"name="result" type="string""#,
            r#"name="result" type="string" defaultValue="""#,
        ),
        (
            r#"name="input" type="string""#,
            r#"name="input" type="integer""#,
        ),
        (
            r#"name="result" type="string""#,
            r#"name="result" type="MysteryString""#,
        ),
        (r#"from="B" to="beta""#, r#"from="A" to="beta""#),
        (r#"from="B" to="beta""#, r#"from="B""#),
        (r#"from="B" to="beta""#, r#"from="B" to="beta" extra="1""#),
        ("<valuemapTable>", "<valuemapTable>unknown"),
        ("<valuemapTable>", "<valuemapTable xmlns=\"urn:foreign\">"),
        ("<valuemapTable>", "<valuemapTable><unknown/>"),
        ("<valuemapTable>", "<valuemapTable foreign=\"1\">"),
        ("</valuemap>", "<unknown/></valuemap>"),
        (
            "</valuemap>",
            "<input name=\"input\" type=\"string\"/></valuemap>",
        ),
        ("</valuemap></data>", "</valuemap></data><data/>"),
        (r#"key="101""#, r#"key="101" xmlns:f="urn:x" f:default="x""#),
        (r#"pos="0" key="101""#, r#"pos="1" key="101""#),
        (r#"pos="0" key="101""#, ""),
        (r#"key="101"/>"#, r#"key="101"><unknown/></datapoint>"#),
        (
            r#"key="102"/>"#,
            r#"key="102"/><datapoint pos="1" key="105"/>"#,
        ),
        (
            r#"key="101"/>"#,
            r#"key="101"/></sources><sources><datapoint pos="0" key="105"/>"#,
        ),
        (r#"<edge vertexkey="101"/>"#, ""),
    ];
    for (from, to) in mutations {
        let fixture = mapped();
        fixture.change("mapping.mfd", from, to);
        assert!(fixture.import().is_err(), "accepted {from} -> {to}");
    }
    for kind in ["023", "+23"] {
        let fixture = mapped();
        fixture.change("mapping.mfd", "kind=\"23\"", &format!("kind=\"{kind}\""));
        assert!(fixture.import().is_err());
    }
    for entries in [257, 1] {
        let fixture = mapped();
        let rows = if entries == 1 {
            format!(r#"<entry from="A" to="{}"/>"#, "x".repeat(65_536))
        } else {
            (0..entries)
                .map(|i| format!(r#"<entry from="{i}" to="x"/>"#))
                .collect()
        };
        fixture.change(
            "mapping.mfd",
            "<valuemapTable>",
            &format!("<valuemapTable>{rows}"),
        );
        assert!(fixture.import().is_err());
    }
}

#[test]
fn guid_format_preserves_hex_case_presence_and_rejects_malformed_values() {
    let fixture = Fixture::new(
        &unary("lang", "format-guid-string", 101, 102, 20),
        &(edge(21, 101) + &edge(102, 71)),
        None,
    );
    let imported = fixture.import().unwrap();
    let project = &imported.targets[0].body;
    for (value, expected) in [
        (
            "0123456789abcdefABCDEF0123456789",
            "01234567-89ab-cdef-ABCD-EF0123456789",
        ),
        (
            "ABCDEF0123456789ABCDEF0123456789",
            "ABCDEF01-2345-6789-ABCD-EF0123456789",
        ),
    ] {
        assert_eq!(
            run(project, string(value)).unwrap().field("result"),
            Some(&text(expected))
        );
    }
    for value in [None, Some(Value::Null)] {
        let output = run(project, value).unwrap();
        assert_eq!(output.field("result"), Some(&Instance::Scalar(Value::Null)));
        assert!(
            format_json::to_value(&project.target, &output)
                .unwrap()
                .get("result")
                .is_none()
        );
    }
    for value in [
        "",
        "0123456789abcdefABCDEF012345678",
        "0123456789abcdefABCDEF01234567890",
        "01234567-89ab-cdef-ABCD-EF0123456789",
        "g123456789abcdefABCDEF0123456789",
        "é123456789abcdefABCDEF012345678",
    ] {
        assert!(run(project, string(value)).is_err(), "accepted {value:?}");
    }
    assert!(codegen::lower(project).is_ok());
}

#[test]
fn null_predicate_is_always_present_and_reached_required_fields_still_fail() {
    let fixture = Fixture::new(
        &unary("db", "is-not-null", 101, 102, 20),
        &(edge(21, 101) + &edge(102, 72)),
        None,
    );
    let imported = fixture.import().unwrap();
    let project = &imported.targets[0].body;
    for (value, expected) in [
        (None, false),
        (Some(Value::Null), false),
        (string(""), true),
        (string("A"), true),
    ] {
        assert_eq!(
            run(project, value).unwrap().field("present"),
            Some(&Instance::Scalar(Value::Bool(expected)))
        );
    }
    fixture.change(
        "Defs.Segment",
        r#"ref="F1" minOccurs="0""#,
        r#"ref="F1" minOccurs="1""#,
    );
    let required = fixture.import().unwrap();
    assert!(run(&required.targets[0].body, None).is_err());
    assert!(run(&required.targets[0].body, Some(Value::Null)).is_err());
}

fn conditional(branch: &str, branch_edge: u32, selected: &str) -> Fixture {
    let functions = format!(
        r#"{branch}<component name="constant" library="core" kind="2" uid="30"><targets><datapoint pos="0" key="110"/></targets><data><constant value="{selected}" datatype="decimal"/></data></component><component name="boolean" library="core" kind="5" uid="31"><sources><datapoint pos="0" key="111"/></sources><targets><datapoint pos="0" key="112"/></targets></component><component name="constant" library="core" kind="2" uid="32"><targets><datapoint pos="0" key="113"/></targets><data><constant value="safe" datatype="string"/></data></component><component name="if-else" library="core" kind="4" uid="33"><sources><datapoint pos="0" key="114"/><datapoint pos="1" key="115"/><datapoint pos="2" key="116"/></sources><targets><datapoint pos="0" key="117"/></targets></component>"#
    );
    Fixture::new(
        &functions,
        &(edge(21, 101)
            + &edge(110, 111)
            + &edge(112, 114)
            + &edge(113, 115)
            + &edge(branch_edge, 116)
            + &edge(117, 71)),
        None,
    )
}

#[test]
fn guid_and_null_predicate_keep_unchosen_branches_lazy() {
    for branch in [
        unary("lang", "format-guid-string", 101, 102, 20),
        unary("db", "is-not-null", 101, 102, 20),
    ] {
        let fixture = conditional(&branch, 102, "1");
        fixture.change(
            "Defs.Segment",
            r#"ref="F1" minOccurs="0""#,
            r#"ref="F1" minOccurs="1""#,
        );
        let imported = fixture.import().unwrap();
        assert_eq!(
            run(&imported.targets[0].body, None)
                .unwrap()
                .field("result"),
            Some(&text("safe"))
        );
    }
    let fixture = conditional(&unary("lang", "format-guid-string", 101, 102, 20), 102, "0");
    let imported = fixture.import().unwrap();
    assert!(run(&imported.targets[0].body, string("bad-guid")).is_err());
    let fixture = conditional(&map(), 102, "1");
    fixture.change(
        "Defs.Segment",
        r#"ref="F1" minOccurs="0""#,
        r#"ref="F1" minOccurs="1""#,
    );
    let imported = fixture.import().unwrap();
    assert_eq!(
        run(&imported.targets[0].body, None)
            .unwrap()
            .field("result"),
        Some(&text("safe"))
    );
}

#[test]
fn new_unary_functions_reject_unqualified_signatures_and_metadata() {
    for (library, name) in [("lang", "format-guid-string"), ("db", "is-not-null")] {
        for (from, to) in [
            (r#"kind="5" uid="20""#, r#"kind="05" uid="20""#),
            (r#"key="101"/>"#, r#"key="101"><default/></datapoint>"#),
            (r#"key="101"/>"#, r#"key="101" foreign="true"/>"#),
            (r#"pos="0" key="101""#, r#"pos="2" key="101""#),
            (r#"pos="0" key="101""#, ""),
            (r#"<edge vertexkey="101"/>"#, ""),
            (
                r#"key="102"/></targets>"#,
                r#"key="102"/></targets><data><unknown/></data>"#,
            ),
        ] {
            let fixture = Fixture::new(
                &unary(library, name, 101, 102, 20),
                &(edge(21, 101) + &edge(102, 71)),
                None,
            );
            fixture.change("mapping.mfd", from, to);
            assert!(fixture.import().is_err(), "accepted {name}: {from} -> {to}");
        }
    }
}

#[test]
fn map_metadata_bounds_are_inclusive_and_empty_table_is_a_miss() {
    let original = map();
    let begin = original.find("<valuemapTable>").unwrap() + "<valuemapTable>".len();
    let end = original.find("</valuemapTable>").unwrap();
    for rows in [
        String::new(),
        (0..256)
            .map(|i| format!(r#"<entry from="{i}" to="x"/>"#))
            .collect(),
        format!(r#"<entry from="A" to="{}"/>"#, "x".repeat(65_535)),
    ] {
        let mut component = original.clone();
        component.replace_range(begin..end, &rows);
        let fixture = Fixture::new(&component, &(edge(21, 101) + &edge(102, 71)), None);
        let imported = fixture.import().unwrap();
        assert_eq!(
            run(&imported.targets[0].body, string("not-listed"))
                .unwrap()
                .field("result"),
            Some(&Instance::Scalar(Value::Null))
        );
    }
}

#[test]
fn ordinary_import_retains_custom_default_and_first_duplicate_key() {
    let fixture = mapped();
    for (name, content) in [
        (
            "source.xsd",
            include_str!("fixtures/format_guid_string/source.xsd"),
        ),
        (
            "target.xsd",
            include_str!("fixtures/format_guid_string/target.xsd"),
        ),
    ] {
        std::fs::write(fixture.0.join(name), content).unwrap();
    }
    std::fs::write(fixture.0.join("mapping.mfd"), r#"<mapping version="26"><component name="main"><structure><children>
<component name="Input" library="xml" kind="14" uid="1"><data><root><entry name="Input"><entry name="Hex" outkey="11"/></entry></root><document schema="source.xsd" instanceroot="{}Input"/></data></component>
<component name="value-map" library="core" kind="23" uid="2"><sources><datapoint pos="0" key="20"/></sources><targets><datapoint pos="0" key="21"/></targets><data><valuemap defaultValueMode="custom"><valuemapTable><entry from="A" to="first"/><entry from="A" to="second"/></valuemapTable><input name="input" type="string"/><result name="result" type="string" defaultValue="fallback"/></valuemap></data></component>
<component name="Output" library="xml" kind="14" uid="3"><data><root><entry name="Output"><entry name="Formatted" inpkey="31"/></entry></root><document schema="target.xsd" instanceroot="{}Output"/></data></component>
</children><connections><edge from="11" to="20"/><edge from="21" to="31"/></connections></structure></component></mapping>"#).unwrap();
    let imported = mfd::import(&fixture.0.join("mapping.mfd")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    for (value, expected) in [("A", "first"), ("missing", "fallback")] {
        let output = engine::run(&imported.project, &group(vec![("Hex", text(value))])).unwrap();
        assert_eq!(output.field("Formatted"), Some(&text(expected)));
    }
}

#[test]
fn formatted_create_guid_remains_volatile_and_cannot_feed_a_value_map() {
    let create = r#"<component name="create-guid" library="lang" kind="5" uid="22"><targets><datapoint pos="0" key="106"/></targets></component>"#;
    let format = unary("lang", "format-guid-string", 105, 107, 23);
    let fixture = Fixture::new(
        &(create.to_owned() + &format),
        &(edge(106, 105) + &edge(107, 71)),
        None,
    );
    let imported = fixture.import().unwrap();
    let first = run(&imported.targets[0].body, string("A")).unwrap();
    let second = run(&imported.targets[0].body, string("A")).unwrap();
    assert_ne!(first.field("result"), second.field("result"));
    let mapped = Fixture::new(
        &(create.to_owned() + &format + &map()),
        &(edge(106, 105) + &edge(107, 101) + &edge(102, 71)),
        None,
    );
    assert!(
        mapped
            .import()
            .err()
            .unwrap()
            .to_string()
            .contains("volatile value-map input")
    );
}

#[test]
fn map_and_predicate_do_not_leak_between_rows_or_messages() {
    let schema = r#"{"type":"object","properties":{"rows":{"type":"array","items":{"type":"object","properties":{"result":{"type":"string"},"present":{"type":"boolean"}}}}}}"#;
    let entries = r#"<entry name="rows" type="json-property"><entry name="array"><entry name="item"><entry name="object" inpkey="73"><entry name="result" type="json-property"><entry name="string" inpkey="71"/></entry><entry name="present" type="json-property"><entry name="boolean" inpkey="72"/></entry></entry></entry></entry></entry>"#;
    let fixture = Fixture::new(
        &(map() + &unary("db", "is-not-null", 103, 104, 21)),
        &(edge(23, 101) + &edge(22, 73) + &edge(102, 71) + &edge(102, 103) + &edge(104, 72)),
        Some((schema, entries)),
    );
    let imported = fixture.import().unwrap();
    let project = &imported.targets[0].body;
    for keys in [["A", "missing", "E", "B"], ["B", "E", "missing", "A"]] {
        let line = project.source.child("Rows").unwrap().child("LIN").unwrap();
        let rows = keys
            .iter()
            .map(|key| {
                let value = group(vec![("F1", text(key)), ("F2", text("1"))]);
                group(vec![(
                    "LIN",
                    if line.repeating {
                        Instance::Repeated(vec![value])
                    } else {
                        value
                    },
                )])
            })
            .collect();
        let source = group(vec![("Rows", Instance::Repeated(rows))]);
        let output = engine::run(project, &source).unwrap();
        let rows = output.field("rows").unwrap().as_repeated().unwrap();
        assert_eq!(rows.len(), 4);
        for (row, key) in rows.iter().zip(keys) {
            let expected = match key {
                "A" => Some(text("alpha")),
                "B" => Some(text("beta")),
                "E" => Some(text("")),
                _ => Some(Instance::Scalar(Value::Null)),
            };
            assert_eq!(row.field("result"), expected.as_ref());
            assert_eq!(
                row.field("present"),
                Some(&Instance::Scalar(Value::Bool(key != "missing")))
            );
        }
    }
}

fn inherited_fixture(required: bool) -> Fixture {
    let schema = r#"{"type":"object","properties":{"rows":{"type":"array","items":{"type":"object","properties":{"root":{"type":"string"},"present":{"type":"boolean"},"row":{"type":"string"}}}}}}"#;
    let entries = r#"<entry name="rows" type="json-property"><entry name="array"><entry name="item"><entry name="object" inpkey="73"><entry name="root" type="json-property"><entry name="string" inpkey="71"/></entry><entry name="present" type="json-property"><entry name="boolean" inpkey="72"/></entry><entry name="row" type="json-property"><entry name="string" inpkey="74"/></entry></entry></entry></entry></entry>"#;
    let fixture = Fixture::new(
        &(unary("db", "is-not-null", 101, 102, 20) + &root_identity_map()),
        &(edge(25, 73)
            + &edge(21, 201)
            + &edge(202, 71)
            + &edge(21, 101)
            + &edge(102, 72)
            + &edge(23, 74)),
        Some((schema, entries)),
    );
    fixture.change(
        "mapping.mfd",
        r#"<entry name="Rows">"#,
        r#"<entry name="Rows" outkey="25">"#,
    );
    fixture.change(
        "850.Config",
        r#"<Segment ref="LIN"/>"#,
        r#"<Segment ref="LIN"/><Segment ref="BEG" minOccurs="0"/>"#,
    );
    if required {
        fixture.change(
            "Defs.Segment",
            r#"ref="F1" minOccurs="0""#,
            r#"ref="F1" minOccurs="1""#,
        );
    }
    fixture
}

fn root_identity_map() -> String {
    r#"<component name="value-map" library="core" kind="23" uid="29"><sources><datapoint pos="0" key="201"/></sources><targets><datapoint pos="0" key="202"/></targets><data><valuemap defaultValueMode="none"><valuemapTable><entry from="ROOT-A" to="ROOT-A"/><entry from="ROOT-B" to="ROOT-B"/><entry from="ROOT-C" to="ROOT-C"/><entry from="ROOT-X" to="ROOT-X"/><entry from="" to=""/></valuemapTable><input name="input" type="string"/><result name="result" type="string"/></valuemap></data></component>"#.into()
}

fn inherited_input(project: &mapping::Project, root: Option<Value>) -> Instance {
    let mut source = input(project, root);
    let line = project.source.child("Rows").unwrap().child("LIN").unwrap();
    let rows = [None, string("ROW-SHADOW"), Some(Value::Null)]
        .into_iter()
        .enumerate()
        .map(|(index, shadow)| {
            let lin = group(vec![
                ("F1", text(&format!("row-{index}"))),
                ("F2", text("1")),
            ]);
            let shadow = group(
                shadow
                    .into_iter()
                    .map(|value| ("F1", Instance::Scalar(value)))
                    .collect(),
            );
            group(vec![
                (
                    "LIN",
                    if line.repeating {
                        Instance::Repeated(vec![lin])
                    } else {
                        lin
                    },
                ),
                ("BEG", shadow),
            ])
        })
        .collect();
    let Instance::Group(fields) = &mut source else {
        unreachable!()
    };
    fields.push(("Rows".into(), Instance::Repeated(rows)));
    source
}

#[test]
fn root_scalar_owner_survives_row_name_collisions_and_required_reads() {
    for required in [false, true] {
        let fixture = inherited_fixture(required);
        let imported = fixture.import().unwrap();
        let project = &imported.targets[0].body;
        for root in [
            string("ROOT-A"),
            string("ROOT-B"),
            None,
            Some(Value::Null),
            string(""),
        ] {
            let actual = engine::run(project, &inherited_input(project, root.clone()));
            if required && matches!(root, None | Some(Value::Null)) {
                assert!(
                    matches!(actual, Err(engine::EngineError::MappingException { message, .. }) if message == Some("required source field is absent".into()))
                );
                continue;
            }
            let actual = actual.unwrap();
            for row in actual.field("rows").unwrap().as_repeated().unwrap() {
                assert_eq!(
                    row.field("root"),
                    Some(&Instance::Scalar(root.clone().unwrap_or(Value::Null)))
                );
                assert_eq!(
                    row.field("present"),
                    Some(&Instance::Scalar(Value::Bool(!matches!(
                        root,
                        None | Some(Value::Null)
                    ))))
                );
            }
        }
        assert!(codegen::lower(project).is_ok());
    }
}

fn from_json(value: &serde_json::Value) -> Instance {
    use serde_json::Value as Json;
    match value {
        Json::Object(fields) => Instance::Group(
            fields
                .iter()
                .map(|(k, v)| (k.clone(), from_json(v)))
                .collect::<Vec<_>>()
                .into(),
        ),
        Json::Array(items) => Instance::Repeated(items.iter().map(from_json).collect()),
        Json::String(value) => Instance::Scalar(Value::String(value.clone())),
        Json::Bool(value) => Instance::Scalar(Value::Bool(*value)),
        Json::Null => Instance::Scalar(Value::Null),
        _ => panic!("unexpected authored value"),
    }
}
fn to_json(value: &Instance) -> serde_json::Value {
    use serde_json::{Value as Json, json};
    match value {
        Instance::Group(fields) => Json::Object(
            fields
                .iter()
                .map(|(k, v)| (k.clone(), to_json(v)))
                .collect(),
        ),
        Instance::Repeated(items) | Instance::MappedSequence(items) => {
            Json::Array(items.iter().map(to_json).collect())
        }
        Instance::Scalar(Value::String(value)) => json!(value),
        Instance::Scalar(Value::Bool(value)) => json!(value),
        Instance::Scalar(Value::Null) => Json::Null,
        _ => panic!("unexpected authored output"),
    }
}

fn combined_fixture(required: bool) -> Fixture {
    let schema = r#"{"type":"object","properties":{"rows":{"type":"array","items":{"type":"object","properties":{"root":{"type":"string"},"rootPresent":{"type":"boolean"},"mapped":{"type":"string"},"mapPresent":{"type":"boolean"},"inputPresent":{"type":"boolean"},"guid":{"type":"string"}}}}}}"#;
    let entries = r#"<entry name="rows" type="json-property"><entry name="array"><entry name="item"><entry name="object" inpkey="73"><entry name="root" type="json-property"><entry name="string" inpkey="71"/></entry><entry name="rootPresent" type="json-property"><entry name="boolean" inpkey="72"/></entry><entry name="mapped" type="json-property"><entry name="string" inpkey="74"/></entry><entry name="mapPresent" type="json-property"><entry name="boolean" inpkey="75"/></entry><entry name="inputPresent" type="json-property"><entry name="boolean" inpkey="76"/></entry><entry name="guid" type="json-property"><entry name="string" inpkey="77"/></entry></entry></entry></entry></entry>"#;
    let functions = map()
        + &root_identity_map()
        + &unary("db", "is-not-null", 103, 104, 21)
        + &unary("db", "is-not-null", 105, 106, 22)
        + &unary("db", "is-not-null", 107, 108, 23)
        + &unary("lang", "format-guid-string", 109, 110, 24)
        + r#"<component name="constant" library="core" kind="2" uid="25"><targets><datapoint pos="0" key="111"/></targets><data><constant value="format" datatype="string"/></data></component><component name="constant" library="core" kind="2" uid="26"><targets><datapoint pos="0" key="112"/></targets><data><constant value="safe" datatype="string"/></data></component><component name="equal" library="core" kind="5" uid="27"><sources><datapoint pos="0" key="113"/><datapoint pos="1" key="114"/></sources><targets><datapoint pos="0" key="115"/></targets></component><component name="if-else" library="core" kind="4" uid="28"><sources><datapoint pos="0" key="116"/><datapoint pos="1" key="117"/><datapoint pos="2" key="118"/></sources><targets><datapoint pos="0" key="119"/></targets></component>"#;
    let edges = edge(25, 73)
        + &edge(21, 201)
        + &edge(202, 71)
        + &edge(21, 105)
        + &edge(106, 72)
        + &edge(23, 101)
        + &edge(102, 74)
        + &edge(102, 103)
        + &edge(104, 75)
        + &edge(23, 107)
        + &edge(108, 76)
        + &edge(26, 109)
        + &edge(24, 113)
        + &edge(111, 114)
        + &edge(115, 116)
        + &edge(110, 117)
        + &edge(112, 118)
        + &edge(119, 77);
    let fixture = Fixture::new(&functions, &edges, Some((schema, entries)));
    fixture.change(
        "mapping.mfd",
        r#"<entry name="Rows">"#,
        r#"<entry name="Rows" outkey="25">"#,
    );
    fixture.change(
        "mapping.mfd",
        r#"<entry name="F2" outkey="24"/>"#,
        r#"<entry name="F2" outkey="24"/><entry name="F3" outkey="26"/>"#,
    );
    fixture.change(
        "Defs.Segment",
        r#"<Data name="F143""#,
        r#"<Data name="F3" type="string"/><Data name="F143""#,
    );
    fixture.change(
        "Defs.Segment",
        r#"<Data ref="F2"/>"#,
        r#"<Data ref="F2"/><Data ref="F3" minOccurs="0"/>"#,
    );
    fixture.change(
        "850.Config",
        r#"<Segment ref="LIN"/>"#,
        r#"<Segment ref="LIN"/><Segment ref="BEG" minOccurs="0"/>"#,
    );
    if required {
        fixture.change(
            "Defs.Segment",
            r#"ref="F1" minOccurs="0""#,
            r#"ref="F1" minOccurs="1""#,
        );
    }
    fixture
}

fn compiled_cases(required: bool) -> serde_json::Value {
    use serde_json::{Value as Json, json};
    let row = |code: Option<&str>, flag: &str, guid: Option<&str>, shadow: Json| {
        let mut lin = serde_json::Map::new();
        if let Some(code) = code {
            lin.insert("F1".into(), json!(code));
        }
        lin.insert("F2".into(), json!(flag));
        lin.insert("F3".into(), json!(guid));
        json!({"LIN":lin,"BEG":shadow})
    };
    let expected = |root: Json, mapped: Json, input_present: bool, guid: Json| json!({"rootPresent":!root.is_null(), "root":root, "mapPresent":!mapped.is_null(), "mapped":mapped, "inputPresent":input_present, "guid":guid});
    let guid = "0123456789abcdefABCDEF0123456789";
    let formatted = "01234567-89ab-cdef-ABCD-EF0123456789";
    let mut cases = vec![
        json!({"label":"multi-row-map-empty-miss-guid-lazy","input":{"BEG":{"F1":"ROOT-A"},"Rows":[row(Some("A"),"skip",Some("bad"),json!({})),row(Some("E"),"skip",Some("bad"),json!({"F1":"SHADOW"})),row(Some("missing"),"format",Some(guid),json!({"F1":null})),row(Some(""),"skip",Some("bad"),json!({})),row(None,"skip",Some("bad"),json!({"F1":"SHADOW"})),row(None,"format",None,json!({}))]},"expected":{"rows":[expected(json!("ROOT-A"),json!("alpha"),true,json!("safe")),expected(json!("ROOT-A"),json!(""),true,json!("safe")),expected(json!("ROOT-A"),Json::Null,true,json!(formatted)),expected(json!("ROOT-A"),json!("empty-key"),true,json!("safe")),expected(json!("ROOT-A"),Json::Null,false,json!("safe")),expected(json!("ROOT-A"),Json::Null,false,Json::Null)]}}),
        json!({"label":"next-message-isolation","input":{"BEG":{"F1":"ROOT-B"},"Rows":[row(Some("B"),"skip",Some("bad"),json!({"F1":"OTHER"}))]},"expected":{"rows":[expected(json!("ROOT-B"),json!("beta"),true,json!("safe"))]}}),
        json!({"label":"missing-root-collision","input":{"BEG":{},"Rows":[row(Some("A"),"skip",Some("bad"),json!({"F1":"SHADOW"}))]},"expected":{"rows":[expected(Json::Null,json!("alpha"),true,json!("safe"))]}}),
        json!({"label":"null-root-collision","input":{"BEG":{"F1":null},"Rows":[row(Some("E"),"skip",Some("bad"),json!({"F1":"SHADOW"}))]},"expected":{"rows":[expected(Json::Null,json!(""),true,json!("safe"))]}}),
        json!({"label":"empty-root-is-present","input":{"BEG":{"F1":""},"Rows":[row(Some("E"),"skip",Some("bad"),json!({}))]},"expected":{"rows":[expected(json!(""),json!(""),true,json!("safe"))]}}),
        json!({"label":"selected-invalid-guid","input":{"BEG":{"F1":"ROOT-X"},"Rows":[row(Some("A"),"format",Some("bad"),json!({}))]},"error":"guid"}),
        json!({"label":"after-error-message-isolation","input":{"BEG":{"F1":"ROOT-C"},"Rows":[row(Some("B"),"skip",Some("bad"),json!({}))]},"expected":{"rows":[expected(json!("ROOT-C"),json!("beta"),true,json!("safe"))]}}),
    ];
    cases[0]["input"]["Rows"][5]["LIN"]["F1"] = Json::Null;
    if required {
        for case in &mut cases[2..4] {
            case.as_object_mut().unwrap().remove("expected");
            case["error"] = json!("required");
        }
    }
    Json::Array(cases)
}

#[test]
fn emit_and_verify_neutral_generated_scalar_cases() {
    use serde_json::json;
    let directory = generated_test_directory("basic-owned");
    for required in [false, true] {
        let fixture = combined_fixture(required);
        let imported = fixture.import().unwrap();
        let project = &imported.targets[0].body;
        let cases = compiled_cases(required);
        for case in cases.as_array().unwrap() {
            let actual = engine::run(project, &from_json(&case["input"]));
            match case["error"].as_str() {
                Some("required") => assert!(
                    matches!(actual, Err(engine::EngineError::MappingException { message, .. }) if message == Some("required source field is absent".into())),
                    "{case}"
                ),
                Some("guid") => assert!(
                    actual
                        .err()
                        .unwrap()
                        .to_string()
                        .contains("format_guid_string"),
                    "{case}"
                ),
                None => assert_eq!(
                    to_json(&actual.unwrap()),
                    case["expected"],
                    "{}",
                    case["label"]
                ),
                _ => unreachable!(),
            }
        }
        let program = codegen::lower(project).unwrap();
        let root = directory.join(if required { "required" } else { "optional" });
        std::fs::create_dir(&root).unwrap();
        std::fs::write(
            root.join("cases.json"),
            serde_json::to_vec_pretty(&cases).unwrap(),
        )
        .unwrap();
        std::fs::write(
            root.join("project.json"),
            serde_json::to_vec_pretty(project).unwrap(),
        )
        .unwrap();
        let options = codegen_rust::Options {
            package_name: format!(
                "mfd051_scalar_candidate_{}",
                if required { "required" } else { "optional" }
            ),
            runtime_dependency: codegen_rust::RuntimeDependency::Path(
                PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("../codegen-runtime")
                    .to_string_lossy()
                    .into_owned(),
            ),
        };
        let ordinary_rust = codegen_rust::emit(&program, &options)
            .err()
            .unwrap()
            .to_string();
        let ordinary_csharp = codegen_csharp::emit(&program).err().unwrap().to_string();
        std::fs::write(root.join("emitter-gates.json"), serde_json::to_vec_pretty(&json!({"ordinary_rust":ordinary_rust,"ordinary_csharp":ordinary_csharp,"qualified_entry":"explicit basic-owned sources"})).unwrap()).unwrap();
        for (name, artifacts) in [
            (
                "rust",
                codegen_rust::emit_with_basic_owned_sources(&program, &options).unwrap(),
            ),
            (
                "csharp",
                codegen_csharp::emit_with_basic_owned_sources(&program).unwrap(),
            ),
        ] {
            let target = root.join(name);
            std::fs::create_dir(&target).unwrap();
            for file in artifacts.files() {
                let path = target.join(file.path.as_str());
                std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                std::fs::write(path, &file.contents).unwrap();
            }
            if name == "rust" {
                let manifest = target.join("Cargo.toml");
                let text = std::fs::read_to_string(&manifest)
                    .unwrap()
                    .replace("[dependencies]\n", "[dependencies]\nserde_json = \"1\"\n");
                std::fs::write(manifest, text).unwrap();
                std::fs::write(
                    target.join("src/main.rs"),
                    include_str!("fixtures/x12_document_body_scalars/Host.rs.txt").replace(
                        "mfd051_scalar_candidate::",
                        &format!("{}::", options.package_name),
                    ),
                )
                .unwrap();
            } else {
                let harness = target.join("Harness");
                std::fs::create_dir(&harness).unwrap();
                std::fs::write(harness.join("Host.csproj"), r#"<Project Sdk="Microsoft.NET.Sdk"><PropertyGroup><OutputType>Exe</OutputType><TargetFramework>net10.0</TargetFramework><ImplicitUsings>enable</ImplicitUsings><Nullable>enable</Nullable><TreatWarningsAsErrors>true</TreatWarningsAsErrors></PropertyGroup><ItemGroup><ProjectReference Include="../Ferrule.Generated.csproj"/></ItemGroup></Project>"#).unwrap();
                std::fs::write(
                    harness.join("Program.cs"),
                    include_str!("fixtures/x12_document_body_scalars/Host.cs.txt"),
                )
                .unwrap();
                std::fs::write(
                    target.join("NuGet.Config"),
                    "<configuration><packageSources><clear /></packageSources></configuration>",
                )
                .unwrap();
            }
        }
    }
    let create = r#"<component name="create-guid" library="lang" kind="5" uid="22"><targets><datapoint pos="0" key="106"/></targets></component>"#;
    let format = unary("lang", "format-guid-string", 105, 107, 23);
    let refused = Fixture::new(
        &(create.to_owned() + &format + &map()),
        &(edge(106, 105) + &edge(107, 101) + &edge(102, 71)),
        None,
    )
    .import()
    .err()
    .unwrap()
    .to_string();
    assert!(refused.contains("volatile value-map input"));
    std::fs::write(
        directory.join("volatile-map-refusal.json"),
        serde_json::to_vec_pretty(&json!({"error":refused, "before_emission":["rust","csharp"]}))
            .unwrap(),
    )
    .unwrap();
}
