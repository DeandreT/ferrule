use std::fs;
use std::path::{Path, PathBuf};

use ir::{Instance, Value};
use mapping::Node;

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "ferrule-mfd-sequence-exists-{}-{}",
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

fn mapping() -> &'static str {
    r#"<mapping version="26"><component name="map"><structure><children>
      <component name="source" library="xml" kind="14"><data><root><entry name="Source"><entry name="Tool" outkey="10"><entry name="Code" outkey="11"/></entry><entry name="MissionKit"><entry name="Edition" outkey="15"/><entry name="ToolCodes" outkey="16"/></entry></entry></root><document schema="source.xsd" inputinstance="source.xml" instanceroot="{}Source"/></data></component>
      <component name="constant" library="core" uid="20" kind="2"><targets><datapoint key="20"/></targets><data><constant value="Enterprise" datatype="string"/></data></component>
      <component name="equal" library="core" uid="21" kind="5"><sources><datapoint key="21"/><datapoint key="22"/></sources><targets><datapoint key="23"/></targets></component>
      <component name="edition" library="core" uid="22" kind="3"><sources><datapoint key="24"/><datapoint key="25"/></sources><targets><datapoint key="26"/><datapoint/></targets></component>
      <component name="constant" library="core" uid="23" kind="2"><targets><datapoint key="27"/></targets><data><constant value="2" datatype="decimal"/></data></component>
      <component name="tokenize-by-length" library="core" uid="24" kind="5"><sources><datapoint key="28"/><datapoint key="29"/></sources><targets><datapoint key="30"/></targets></component>
      <component name="equal" library="core" uid="25" kind="5"><sources><datapoint key="31"/><datapoint key="32"/></sources><targets><datapoint key="33"/></targets></component>
      <component name="matching-token" library="core" uid="26" kind="3"><sources><datapoint key="34"/><datapoint key="35"/></sources><targets><datapoint key="36"/><datapoint/></targets></component>
      <component name="exists" library="core" uid="27" kind="5"><sources><datapoint key="37"/></sources><targets><datapoint key="38"/></targets></component>
      <component name="target" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data><root><entry name="Target"><entry name="Row" inpkey="50"><entry name="Code" inpkey="51"/><entry name="Member" inpkey="52"/><entry name="Raw" inpkey="53"/></entry><entry name="Token" inpkey="54"><entry name="Value" inpkey="55"/></entry></entry></root><document schema="target.xsd" outputinstance="target.xml" instanceroot="{}Target"/></data></component>
    </children><graph><vertices>
      <vertex vertexkey="15"><edges><edge vertexkey="21"/></edges></vertex><vertex vertexkey="20"><edges><edge vertexkey="22"/></edges></vertex><vertex vertexkey="23"><edges><edge vertexkey="25"/></edges></vertex><vertex vertexkey="16"><edges><edge vertexkey="24"/></edges></vertex>
      <vertex vertexkey="26"><edges><edge vertexkey="28"/></edges></vertex><vertex vertexkey="27"><edges><edge vertexkey="29"/></edges></vertex>
      <vertex vertexkey="11"><edges><edge vertexkey="31"/><edge vertexkey="51"/></edges></vertex><vertex vertexkey="30"><edges><edge vertexkey="32"/><edge vertexkey="34"/></edges></vertex><vertex vertexkey="33"><edges><edge vertexkey="35"/></edges></vertex><vertex vertexkey="36"><edges><edge vertexkey="37"/></edges></vertex>
      <vertex vertexkey="10"><edges><edge vertexkey="50"/></edges></vertex><vertex vertexkey="38"><edges><edge vertexkey="52"/></edges></vertex>
    </vertices></graph></structure></component></mapping>"#
}

fn setup(mfd: &str) -> TempDir {
    let dir = TempDir::new();
    write(
        &dir.0.join("source.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Source"><xs:complexType><xs:sequence><xs:element name="Tool" maxOccurs="unbounded"><xs:complexType><xs:attribute name="Code" type="xs:string"/></xs:complexType></xs:element><xs:element name="MissionKit" maxOccurs="unbounded"><xs:complexType><xs:attribute name="Edition" type="xs:string"/><xs:attribute name="ToolCodes" type="xs:string"/></xs:complexType></xs:element></xs:sequence></xs:complexType></xs:element></xs:schema>"#,
    );
    write(
        &dir.0.join("target.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Target"><xs:complexType><xs:sequence><xs:element name="Row" maxOccurs="unbounded"><xs:complexType><xs:sequence><xs:element name="Code" type="xs:string"/><xs:element name="Member" type="xs:boolean"/><xs:element name="Raw" type="xs:string"/></xs:sequence></xs:complexType></xs:element><xs:element name="Token" maxOccurs="unbounded"><xs:complexType><xs:sequence><xs:element name="Value" type="xs:string"/></xs:sequence></xs:complexType></xs:element></xs:sequence></xs:complexType></xs:element></xs:schema>"#,
    );
    write(&dir.0.join("mapping.mfd"), mfd);
    dir
}

fn record(fields: &[(&str, Value)]) -> Instance {
    Instance::Group(
        fields
            .iter()
            .map(|(name, value)| ((*name).into(), Instance::Scalar(value.clone())))
            .collect(),
    )
}

#[test]
fn imports_filtered_token_existence_and_sibling_scalar_lookup() {
    let dir = setup(mapping());
    let imported = mfd::import(&dir.0.join("mapping.mfd")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(imported.project.graph.nodes.values().any(|node| matches!(node, Node::Lookup { collection, key, value, .. } if collection == &["MissionKit"] && key == &["Edition"] && value == &["ToolCodes"])));
    assert!(
        imported
            .project
            .graph
            .nodes
            .values()
            .any(|node| matches!(node, Node::SequenceExists { .. }))
    );
    assert!(engine::validate(&imported.project).is_empty());

    let source = Instance::Group(vec![
        (
            "Tool".into(),
            Instance::Repeated(vec![
                record(&[("Code", Value::String("AA".into()))]),
                record(&[("Code", Value::String("BB".into()))]),
                record(&[("Code", Value::String("ZZ".into()))]),
            ]),
        ),
        (
            "MissionKit".into(),
            Instance::Repeated(vec![
                record(&[
                    ("Edition", Value::String("Basic".into())),
                    ("ToolCodes", Value::String("ZZ".into())),
                ]),
                record(&[
                    ("Edition", Value::String("Enterprise".into())),
                    ("ToolCodes", Value::String("AABB".into())),
                ]),
            ]),
        ),
    ]);
    let output = engine::run(&imported.project, &source).unwrap();
    let rows = output.field("Row").and_then(Instance::as_repeated).unwrap();
    assert_eq!(rows.len(), 3);
    assert_eq!(
        rows[0].field("Member").and_then(Instance::as_scalar),
        Some(&Value::Bool(true))
    );
    assert_eq!(
        rows[1].field("Member").and_then(Instance::as_scalar),
        Some(&Value::Bool(true))
    );
    assert_eq!(
        rows[2].field("Member").and_then(Instance::as_scalar),
        Some(&Value::Bool(false))
    );
}

#[test]
fn non_equality_scalar_filter_is_not_claimed_as_a_lookup() {
    let near_miss = mapping().replace(
        "<component name=\"equal\" library=\"core\" uid=\"21\"",
        "<component name=\"greater-than\" library=\"core\" uid=\"21\"",
    );
    let dir = setup(&near_miss);
    let imported = mfd::import(&dir.0.join("mapping.mfd")).unwrap();
    assert!(
        imported
            .warnings
            .iter()
            .any(|warning| warning
                .contains("is consumed as one scalar but is not an equality lookup")),
        "{:?}",
        imported.warnings
    );
    assert!(
        !imported
            .project
            .graph
            .nodes
            .values()
            .any(|node| matches!(node, Node::Lookup { .. } | Node::SequenceExists { .. }))
    );
}

#[test]
fn extra_scalar_sequence_consumer_keeps_the_unsupported_use_warning() {
    let with_scalar_consumer = mapping().replace(
        "<edge vertexkey=\"32\"/><edge vertexkey=\"34\"/>",
        "<edge vertexkey=\"32\"/><edge vertexkey=\"34\"/><edge vertexkey=\"53\"/>",
    );
    let dir = setup(&with_scalar_consumer);
    let imported = mfd::import(&dir.0.join("mapping.mfd")).unwrap();

    let scalar_warnings: Vec<_> = imported
        .warnings
        .iter()
        .filter(|warning| {
            warning.contains(
                "sequence function `tokenize-by-length` is not connected to a repeating target",
            )
        })
        .collect();
    assert_eq!(scalar_warnings.len(), 1, "{:?}", imported.warnings);
    assert!(
        imported
            .project
            .graph
            .nodes
            .values()
            .any(|node| matches!(node, Node::SequenceExists { .. }))
    );
}

#[test]
fn reducer_and_generated_iteration_own_distinct_sequence_items() {
    let with_generated_iteration = mapping().replace(
        "<edge vertexkey=\"32\"/><edge vertexkey=\"34\"/>",
        "<edge vertexkey=\"32\"/><edge vertexkey=\"34\"/><edge vertexkey=\"54\"/><edge vertexkey=\"55\"/>",
    );
    let dir = setup(&with_generated_iteration);
    let imported = mfd::import(&dir.0.join("mapping.mfd")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);

    let reducer_item = imported
        .project
        .graph
        .nodes
        .values()
        .find_map(|node| match node {
            Node::SequenceExists { sequence, .. } => Some(sequence.item()),
            _ => None,
        })
        .unwrap();
    let iteration_item = imported
        .project
        .root
        .children
        .iter()
        .find(|scope| scope.target_field == "Token")
        .and_then(mapping::Scope::sequence)
        .map(|sequence| sequence.item())
        .unwrap();

    assert_ne!(reducer_item, iteration_item);
    assert!(engine::validate(&imported.project).is_empty());
}

#[test]
fn non_core_exists_is_not_lowered_as_a_sequence_reducer() {
    let non_core = mapping().replace(
        "<component name=\"exists\" library=\"core\"",
        "<component name=\"exists\" library=\"lang\"",
    );
    let dir = setup(&non_core);
    let imported = mfd::import(&dir.0.join("mapping.mfd")).unwrap();

    assert!(
        !imported
            .project
            .graph
            .nodes
            .values()
            .any(|node| matches!(node, Node::SequenceExists { .. }))
    );
    assert!(
        imported
            .project
            .graph
            .nodes
            .values()
            .any(|node| matches!(node, Node::Call { function, .. } if function == "exists"))
    );
    assert!(imported.warnings.iter().any(|warning| warning.contains(
        "sequence function `tokenize-by-length` is not connected to a repeating target"
    )));
}
