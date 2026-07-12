use std::fs;
use std::path::{Path, PathBuf};

use ir::{Instance, Value};
use mapping::{Node, ScopeIteration};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "ferrule-mfd-join-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |duration| duration.as_nanos())
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn write(path: &Path, contents: &str) {
    fs::write(path, contents).unwrap();
}

fn setup() -> TempDir {
    let dir = TempDir::new();
    write(
        &dir.0.join("left.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="LeftRoot"><xs:complexType><xs:sequence><xs:element name="Left" maxOccurs="unbounded"><xs:complexType><xs:sequence><xs:element name="Id" type="xs:string"/><xs:element name="Label" type="xs:string"/></xs:sequence></xs:complexType></xs:element></xs:sequence></xs:complexType></xs:element></xs:schema>"#,
    );
    write(
        &dir.0.join("right.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="RightRoot"><xs:complexType><xs:sequence><xs:element name="Right" maxOccurs="unbounded"><xs:complexType><xs:sequence><xs:element name="Code" type="xs:string"/><xs:element name="Description" type="xs:string"/></xs:sequence></xs:complexType></xs:element></xs:sequence></xs:complexType></xs:element></xs:schema>"#,
    );
    write(
        &dir.0.join("target.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Target"><xs:complexType><xs:sequence><xs:element name="Row" maxOccurs="unbounded"><xs:complexType><xs:sequence><xs:element name="Label" type="xs:string"/><xs:element name="Description" type="xs:string"/><xs:element name="Position" type="xs:integer"/></xs:sequence></xs:complexType></xs:element></xs:sequence></xs:complexType></xs:element></xs:schema>"#,
    );
    write(&dir.0.join("mapping.mfd"), mapping());
    dir
}

fn mapping() -> &'static str {
    r#"<mapping version="26"><component name="map"><structure><children>
      <component name="LeftSource" library="xml" kind="14"><data><root><entry name="LeftRoot"><entry name="Left" outkey="1"><entry name="Id" outkey="2"/><entry name="Label" outkey="3"/></entry></entry></root><document schema="left.xsd" inputinstance="left.xml" instanceroot="{}LeftRoot"/></data></component>
      <component name="RightSource" library="xml" kind="14"><data><root><entry name="RightRoot"><entry name="Right" outkey="4"><entry name="Code" outkey="5"/><entry name="Description" outkey="6"/></entry></entry></root><document schema="right.xsd" inputinstance="right.xml" instanceroot="{}RightRoot"/></data></component>
      <component name="join" library="core" uid="32" kind="32"><data><root><entry name="document"><entry name="tuple" outkey="90"><entry name="dynamic_tree_node0"><entry name="Left" inpkey="10"><entry name="Label" outkey="12"/></entry></entry><entry name="dynamic_tree_node1"><entry name="Right" inpkey="20"><entry name="Description" outkey="22"/></entry></entry></entry></entry></root><join><joinkeys><keypair><first-key path-id="101"/><second-key path-id="102"/></keypair></joinkeys><keypaths><entry><condition/><entry name="Id" outkey="101"><condition/></entry><entry name="Code" outkey="102"><condition/></entry></entry></keypaths></join></data></component>
      <component name="first-items" library="core" uid="33" kind="5"><sources><datapoint pos="0" key="30"/></sources><targets><datapoint pos="0" key="31"/></targets></component>
      <component name="position" library="core" uid="34" kind="5"><sources><datapoint pos="0" key="32"/></sources><targets><datapoint pos="0" key="33"/></targets></component>
      <component name="Target" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data><root><entry name="Target"><entry name="Row" inpkey="40"><entry name="Label" inpkey="41"/><entry name="Description" inpkey="42"/><entry name="Position" inpkey="43"/></entry></entry></root><document schema="target.xsd" outputinstance="target.xml" instanceroot="{}Target"/></data></component>
    </children><graph><vertices>
      <vertex vertexkey="1"><edges><edge vertexkey="10"/></edges></vertex><vertex vertexkey="4"><edges><edge vertexkey="20"/></edges></vertex>
      <vertex vertexkey="90"><edges><edge vertexkey="30"/><edge vertexkey="32"/></edges></vertex><vertex vertexkey="31"><edges><edge vertexkey="40"/></edges></vertex>
      <vertex vertexkey="12"><edges><edge vertexkey="41"/></edges></vertex><vertex vertexkey="22"><edges><edge vertexkey="42"/></edges></vertex><vertex vertexkey="33"><edges><edge vertexkey="43"/></edges></vertex>
    </vertices></graph></structure></component></mapping>"#
}

fn row(first: (&str, &str), second: (&str, &str)) -> Instance {
    Instance::Group(vec![
        (
            first.0.into(),
            Instance::Scalar(Value::String(first.1.into())),
        ),
        (
            second.0.into(),
            Instance::Scalar(Value::String(second.1.into())),
        ),
    ])
}

fn sources() -> (Instance, Instance) {
    (
        Instance::Group(vec![(
            "Left".into(),
            Instance::Repeated(vec![
                row(("Id", "A"), ("Label", "L1")),
                row(("Id", "A"), ("Label", "L2")),
            ]),
        )]),
        Instance::Group(vec![(
            "Right".into(),
            Instance::Repeated(vec![
                row(("Code", "A"), ("Description", "R1")),
                row(("Code", "A"), ("Description", "R2")),
            ]),
        )]),
    )
}

#[test]
fn imports_join_fields_position_and_first_items_control() {
    let dir = setup();
    let imported = mfd::import(&dir.0.join("mapping.mfd")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let row_scope = &imported.project.root.children[0];
    let ScopeIteration::InnerJoin { id, .. } = row_scope.iteration else {
        panic!("row should use a joined iteration");
    };
    assert_eq!(id.get(), 32);
    assert!(
        imported
            .project
            .graph
            .nodes
            .values()
            .any(|node| matches!(node, Node::JoinField { .. }))
    );
    assert!(
        imported
            .project
            .graph
            .nodes
            .values()
            .any(|node| matches!(node, Node::JoinPosition { .. }))
    );
    let validation = engine::validate(&imported.project);
    assert!(validation.is_empty(), "{validation:?}");

    let (left, right) = sources();
    let output = engine::run_with_sources(
        &imported.project,
        &left,
        vec![(imported.project.extra_sources[0].name.clone(), right)],
    )
    .unwrap();
    let rows = output.field("Row").and_then(Instance::as_repeated).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0].field("Label").and_then(Instance::as_scalar),
        Some(&Value::String("L1".into()))
    );
    assert_eq!(
        rows[0].field("Position").and_then(Instance::as_scalar),
        Some(&Value::Int(1))
    );
}

#[test]
fn inner_join_preserves_duplicate_matches() {
    let dir = setup();
    let direct = mapping()
        .replace("<component name=\"first-items\" library=\"core\" uid=\"33\" kind=\"5\"><sources><datapoint pos=\"0\" key=\"30\"/></sources><targets><datapoint pos=\"0\" key=\"31\"/></targets></component>", "")
        .replace("<vertex vertexkey=\"31\"><edges><edge vertexkey=\"40\"/></edges></vertex>", "<vertex vertexkey=\"90\"><edges><edge vertexkey=\"40\"/></edges></vertex>");
    write(&dir.0.join("mapping.mfd"), &direct);
    let imported = mfd::import(&dir.0.join("mapping.mfd")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let (left, right) = sources();
    let output = engine::run_with_sources(
        &imported.project,
        &left,
        vec![(imported.project.extra_sources[0].name.clone(), right)],
    )
    .unwrap();
    assert_eq!(
        output
            .field("Row")
            .and_then(Instance::as_repeated)
            .unwrap()
            .len(),
        4
    );
}

#[test]
fn grouping_a_join_is_rejected_once_with_an_actionable_warning() {
    let dir = setup();
    let grouped = mapping()
        .replace(
            "<component name=\"first-items\" library=\"core\" uid=\"33\" kind=\"5\"><sources><datapoint pos=\"0\" key=\"30\"/></sources><targets><datapoint pos=\"0\" key=\"31\"/></targets></component>",
            "<component name=\"group-by\" library=\"core\" uid=\"33\" kind=\"5\"><sources><datapoint pos=\"0\" key=\"30\"/><datapoint pos=\"1\" key=\"34\"/></sources><targets><datapoint pos=\"0\" key=\"31\"/></targets></component>",
        )
        .replace(
            "<vertex vertexkey=\"12\"><edges><edge vertexkey=\"41\"/></edges></vertex>",
            "<vertex vertexkey=\"12\"><edges><edge vertexkey=\"34\"/><edge vertexkey=\"41\"/></edges></vertex>",
        );
    write(&dir.0.join("mapping.mfd"), &grouped);
    let imported = mfd::import(&dir.0.join("mapping.mfd")).unwrap();
    assert_eq!(imported.warnings.len(), 1, "{:?}", imported.warnings);
    assert!(imported.warnings[0].contains("followed by grouping"));
}

#[test]
fn branch_root_output_can_feed_joined_rows_without_a_tuple_port() {
    let dir = setup();
    let branch_output = mapping()
        .replace(
            "<entry name=\"tuple\" outkey=\"90\">",
            "<entry name=\"tuple\">",
        )
        .replace(
            "<entry name=\"Left\" inpkey=\"10\">",
            "<entry name=\"Left\" inpkey=\"10\" outkey=\"90\">",
        );
    write(&dir.0.join("mapping.mfd"), &branch_output);
    let imported = mfd::import(&dir.0.join("mapping.mfd")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(matches!(
        imported.project.root.children[0].iteration,
        ScopeIteration::InnerJoin { .. }
    ));
}

#[test]
fn nested_non_repeating_join_projection_reuses_the_parent_tuple() {
    let dir = setup();
    write(
        &dir.0.join("target.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Target"><xs:complexType><xs:sequence><xs:element name="Row" maxOccurs="unbounded"><xs:complexType><xs:sequence><xs:element name="Label" type="xs:string"/><xs:element name="Article"><xs:complexType><xs:sequence><xs:element name="Description" type="xs:string"/></xs:sequence></xs:complexType></xs:element><xs:element name="Position" type="xs:integer"/></xs:sequence></xs:complexType></xs:element></xs:sequence></xs:complexType></xs:element></xs:schema>"#,
    );
    let nested = mapping()
        .replace(
            "<entry name=\"Right\" inpkey=\"20\">",
            "<entry name=\"Right\" inpkey=\"20\" outkey=\"21\">",
        )
        .replace(
            "<entry name=\"Description\" inpkey=\"42\"/>",
            "<entry name=\"Article\" inpkey=\"44\"><entry name=\"Description\" inpkey=\"42\"/></entry>",
        )
        .replace(
            "<vertex vertexkey=\"22\"><edges>",
            "<vertex vertexkey=\"21\"><edges><edge vertexkey=\"44\"/></edges></vertex><vertex vertexkey=\"22\"><edges>",
        );
    write(&dir.0.join("mapping.mfd"), &nested);

    let imported = mfd::import(&dir.0.join("mapping.mfd")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let row_scope = &imported.project.root.children[0];
    assert!(matches!(
        row_scope.iteration,
        ScopeIteration::InnerJoin { .. }
    ));
    assert!(matches!(
        row_scope.children[0].iteration,
        ScopeIteration::None
    ));
    assert!(engine::validate(&imported.project).is_empty());

    let (left, right) = sources();
    let output = engine::run_with_sources(
        &imported.project,
        &left,
        vec![(imported.project.extra_sources[0].name.clone(), right)],
    )
    .unwrap();
    let row = &output.field("Row").and_then(Instance::as_repeated).unwrap()[0];
    assert_eq!(
        row.field("Article")
            .and_then(|article| article.field("Description"))
            .and_then(Instance::as_scalar),
        Some(&Value::String("R1".into()))
    );
}

#[test]
fn rejected_join_shape_suppresses_downstream_warning_cascades() {
    let dir = setup();
    let schema = fs::read_to_string(dir.0.join("right.xsd"))
        .unwrap()
        .replace(" name=\"Right\" maxOccurs=\"unbounded\"", " name=\"Right\"");
    write(&dir.0.join("right.xsd"), &schema);

    let imported = mfd::import(&dir.0.join("mapping.mfd")).unwrap();
    assert_eq!(imported.warnings.len(), 1, "{:?}", imported.warnings);
    assert!(imported.warnings[0].contains("must be a repeating structural source"));
}

#[test]
fn parse_time_join_rejection_retains_output_ownership() {
    let dir = setup();
    let invalid = mapping().replace(
        "<keypaths><entry><condition/><entry name=\"Id\" outkey=\"101\"><condition/></entry><entry name=\"Code\" outkey=\"102\"><condition/></entry></entry></keypaths>",
        "<keypaths/>",
    );
    write(&dir.0.join("mapping.mfd"), &invalid);

    let imported = mfd::import(&dir.0.join("mapping.mfd")).unwrap();
    assert_eq!(imported.warnings.len(), 1, "{:?}", imported.warnings);
    assert!(imported.warnings[0].contains("declares no key paths"));
}

#[test]
fn malformed_join_filter_is_not_silently_dropped() {
    let dir = setup();
    let invalid = mapping().replace(
        "<component name=\"first-items\" library=\"core\" uid=\"33\" kind=\"5\"><sources><datapoint pos=\"0\" key=\"30\"/></sources><targets><datapoint pos=\"0\" key=\"31\"/></targets></component>",
        "<component name=\"filter\" library=\"core\" uid=\"33\" kind=\"3\"><sources><datapoint pos=\"0\" key=\"30\"/></sources><targets><datapoint pos=\"0\" key=\"31\"/><datapoint/></targets></component>",
    );
    write(&dir.0.join("mapping.mfd"), &invalid);

    let imported = mfd::import(&dir.0.join("mapping.mfd")).unwrap();
    assert_eq!(imported.warnings.len(), 1, "{:?}", imported.warnings);
    assert!(imported.warnings[0].contains("missing or unsupported filter predicate"));
}

#[test]
fn join_cannot_repeat_a_non_repeating_xml_document_root() {
    let dir = setup();
    write(
        &dir.0.join("target.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Target"><xs:complexType><xs:sequence><xs:element name="Label" type="xs:string"/><xs:element name="Description" type="xs:string"/><xs:element name="Position" type="xs:integer"/></xs:sequence></xs:complexType></xs:element></xs:schema>"#,
    );
    let root = mapping().replace(
        "<entry name=\"Target\"><entry name=\"Row\" inpkey=\"40\"><entry name=\"Label\" inpkey=\"41\"/><entry name=\"Description\" inpkey=\"42\"/><entry name=\"Position\" inpkey=\"43\"/></entry></entry>",
        "<entry name=\"Target\" inpkey=\"40\"><entry name=\"Label\" inpkey=\"41\"/><entry name=\"Description\" inpkey=\"42\"/><entry name=\"Position\" inpkey=\"43\"/></entry>",
    );
    write(&dir.0.join("mapping.mfd"), &root);

    let imported = mfd::import(&dir.0.join("mapping.mfd")).unwrap();
    assert_eq!(imported.warnings.len(), 1, "{:?}", imported.warnings);
    assert!(imported.warnings[0].contains("cannot iterate a non-repeating XML document root"));
}

#[test]
fn rejected_join_provenance_propagates_through_scalar_functions() {
    let dir = setup();
    let target_schema = fs::read_to_string(dir.0.join("target.xsd"))
        .unwrap()
        .replace(
            "</xs:sequence></xs:complexType></xs:element></xs:sequence>",
            "</xs:sequence></xs:complexType></xs:element><xs:element name=\"Summary\" type=\"xs:string\"/></xs:sequence>",
        );
    write(&dir.0.join("target.xsd"), &target_schema);
    let invalid = mapping()
        .replace(
            "<keypaths><entry><condition/><entry name=\"Id\" outkey=\"101\"><condition/></entry><entry name=\"Code\" outkey=\"102\"><condition/></entry></entry></keypaths>",
            "<keypaths/>",
        )
        .replace(
            "<component name=\"Target\" library=\"xml\" kind=\"14\">",
            "<component name=\"concat\" library=\"core\" uid=\"35\" kind=\"5\"><sources><datapoint pos=\"0\" key=\"50\"/></sources><targets><datapoint pos=\"0\" key=\"51\"/></targets></component><component name=\"Target\" library=\"xml\" kind=\"14\">",
        )
        .replace(
            "<entry name=\"Position\" inpkey=\"43\"/></entry></entry>",
            "<entry name=\"Position\" inpkey=\"43\"/></entry><entry name=\"Summary\" inpkey=\"45\"/></entry>",
        )
        .replace(
            "<vertex vertexkey=\"12\"><edges><edge vertexkey=\"41\"/></edges></vertex>",
            "<vertex vertexkey=\"12\"><edges><edge vertexkey=\"41\"/><edge vertexkey=\"50\"/></edges></vertex>",
        )
        .replace(
            "</vertices></graph>",
            "<vertex vertexkey=\"51\"><edges><edge vertexkey=\"45\"/></edges></vertex></vertices></graph>",
        );
    write(&dir.0.join("mapping.mfd"), &invalid);

    let imported = mfd::import(&dir.0.join("mapping.mfd")).unwrap();
    assert_eq!(imported.warnings.len(), 1, "{:?}", imported.warnings);
    assert!(imported.warnings[0].contains("declares no key paths"));
    assert!(
        imported
            .project
            .root
            .bindings
            .iter()
            .all(|binding| binding.target_field != "Summary")
    );
}
