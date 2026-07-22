use std::collections::BTreeMap;
use std::error::Error;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{Instance, ScalarType, SchemaNode, Value, XML_ELEMENTS_FIELD, XML_TEXT_FIELD};
use mapping::{Binding, Graph, Node, Project, Scope};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> std::io::Result<Self> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_mfd_xml_output_values_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path)?;
        Ok(Self(path))
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn scalar(name: &str) -> SchemaNode {
    SchemaNode::scalar(name, ScalarType::String)
}

fn field(name: &str, value: impl Into<String>) -> (String, Instance) {
    (
        name.to_string(),
        Instance::Scalar(Value::String(value.into())),
    )
}

#[test]
fn nested_collection_find_writes_every_runtime_xml_name() -> Result<(), Box<dyn Error>> {
    let metadata_field = SchemaNode::group(
        "field",
        vec![
            scalar("key"),
            SchemaNode::group(
                "attribute",
                vec![scalar("LocalName"), scalar(XML_TEXT_FIELD).text()],
            )
            .repeating(),
        ],
    )
    .repeating();
    let source_schema = SchemaNode::group(
        "Input",
        vec![
            SchemaNode::group("meta", vec![metadata_field]),
            SchemaNode::group("values", vec![scalar("code"), scalar("text")]).repeating(),
        ],
    );
    let target_schema = SchemaNode::group(
        "Output",
        vec![
            SchemaNode::group(
                XML_ELEMENTS_FIELD,
                vec![scalar("LocalName"), scalar(XML_TEXT_FIELD).text()],
            )
            .repeating(),
        ],
    );
    let graph = Graph {
        nodes: BTreeMap::from([
            (
                0,
                Node::SourceField {
                    path: vec!["code".into()],
                    frame: Some(vec!["values".into()]),
                },
            ),
            (
                1,
                Node::SourceField {
                    path: vec!["meta".into(), "field".into(), "key".into()],
                    frame: None,
                },
            ),
            (
                2,
                Node::Call {
                    function: "equal".into(),
                    args: vec![0, 1],
                },
            ),
            (
                3,
                Node::SourceField {
                    path: vec!["LocalName".into()],
                    frame: Some(vec!["meta".into(), "field".into(), "attribute".into()]),
                },
            ),
            (
                4,
                Node::Const {
                    value: Value::String("name".into()),
                },
            ),
            (
                5,
                Node::Call {
                    function: "equal".into(),
                    args: vec![3, 4],
                },
            ),
            (
                6,
                Node::Call {
                    function: "and".into(),
                    args: vec![2, 5],
                },
            ),
            (
                7,
                Node::SourceField {
                    path: vec![XML_TEXT_FIELD.into()],
                    frame: Some(vec!["meta".into(), "field".into(), "attribute".into()]),
                },
            ),
            (
                8,
                Node::CollectionFind {
                    collection: vec!["meta".into(), "field".into(), "attribute".into()],
                    predicate: 6,
                    value: 7,
                },
            ),
            (
                9,
                Node::SourceField {
                    path: vec!["text".into()],
                    frame: Some(vec!["values".into()]),
                },
            ),
        ]),
    };
    let mut item_scope = Scope {
        target_field: XML_ELEMENTS_FIELD.into(),
        bindings: vec![
            Binding {
                target_field: "LocalName".into(),
                node: 8,
            },
            Binding {
                target_field: XML_TEXT_FIELD.into(),
                node: 9,
            },
        ],
        ..Scope::default()
    };
    item_scope.set_source(Some(vec!["values".into()]));
    let project = Project {
        source: source_schema,
        target: target_schema,
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph,
        root: Scope {
            children: vec![item_scope],
            ..Scope::default()
        },
    };
    assert!(engine::validate(&project).is_empty());
    let metadata = |key: &str, name: &str| {
        Instance::Group(vec![
            field("key", key),
            (
                "attribute".into(),
                Instance::Repeated(vec![Instance::Group(vec![
                    field("LocalName", "name"),
                    field(XML_TEXT_FIELD, name),
                ])]),
            ),
        ])
    };
    let source = Instance::Group(vec![
        (
            "meta".into(),
            Instance::Group(vec![(
                "field".into(),
                Instance::Repeated(vec![metadata("one", "First"), metadata("two", "Second")]),
            )]),
        ),
        (
            "values".into(),
            Instance::Repeated(vec![
                Instance::Group(vec![field("code", "one"), field("text", "alpha")]),
                Instance::Group(vec![field("code", "two"), field("text", "beta")]),
            ]),
        ),
    ]);

    let output = engine::run(&project, &source)?;
    let xml = format_xml::to_string(&project.target, &output)?;
    assert!(xml.contains("<First>alpha</First>"), "{xml}");
    assert!(xml.contains("<Second>beta</Second>"), "{xml}");
    Ok(())
}

fn write_node_function_fixture(dir: &Path) -> Result<PathBuf, Box<dyn Error>> {
    std::fs::write(
        dir.join("target.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="Output"><xs:complexType><xs:sequence>
    <xs:element name="Amount" type="xs:decimal" minOccurs="0"/>
  </xs:sequence></xs:complexType></xs:element>
</xs:schema>"#,
    )?;
    let mapping = dir.join("mapping.mfd");
    std::fs::write(
        &mapping,
        r#"<mapping version="31"><resources/><component name="map"><structure><children>
  <component name="Rows" library="text" kind="16"><data>
    <root><entry name="FileInstance"><entry name="document"><entry name="Rows" outkey="1">
      <outputnodefunctions><rule applyto="descendants"><function name="drop-missing"/><filter datatype="string"/></rule></outputnodefunctions>
      <entry name="Amount" outkey="2"/>
    </entry></entry></entry></root>
    <text type="csv" inputinstance="input.csv"><settings separator="," firstrownames="true"><names root="Rows" block="Rows"><field0 name="Amount" type="string"/></names></settings></text>
  </data></component>
  <component name="Output" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data>
    <root><entry name="FileInstance"><entry name="document"><entry name="Output"><entry name="Amount" inpkey="20"/></entry></entry></entry></root>
    <document schema="target.xsd" instanceroot="{}Output"/>
  </data></component>
</children></structure><connections><edge from="2" to="20"/></connections></component>
<component name="drop-missing" library="mapforce_nodefunction"><structure><children>
  <component name="raw" library="core" kind="6"><targets><datapoint pos="0" key="101"/></targets><data><input datatype="anySimpleType"/><parameter usageKind="input" name="raw"/></data></component>
  <component name="missing" library="core" kind="2"><targets><datapoint pos="0" key="102"/></targets><data><constant value="N/A" datatype="string"/></data></component>
  <component name="not-equal" library="core" kind="5"><sources><datapoint pos="0" key="103"/><datapoint pos="1" key="104"/></sources><targets><datapoint pos="0" key="105"/></targets></component>
  <component name="filter" library="core" kind="3"><sources><datapoint pos="0" key="106"/><datapoint pos="1" key="107"/></sources><targets><datapoint pos="0" key="108"/><datapoint pos="1"/></targets></component>
  <component name="result" library="core" kind="7"><sources><datapoint pos="0" key="109"/></sources><data><output datatype="anySimpleType"/><parameter usageKind="output" name="result"/></data></component>
</children></structure><connections>
  <edge from="101" to="103"/><edge from="102" to="104"/><edge from="105" to="107"/><edge from="101" to="106"/><edge from="108" to="109"/>
</connections></component></mapping>"#,
    )?;
    Ok(mapping)
}

#[test]
fn output_node_function_removes_invalid_numeric_sentinel_before_xml_write()
-> Result<(), Box<dyn Error>> {
    let dir = TempDir::new()?;
    let imported = mfd::import(&write_node_function_fixture(&dir.0)?)?;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(engine::validate(&imported.project).is_empty());

    let valid = Instance::Group(vec![field("Amount", "12.5")]);
    let valid_output = engine::run(&imported.project, &valid)?;
    let valid_xml = format_xml::to_string(&imported.project.target, &valid_output)?;
    assert!(valid_xml.contains("<Amount>12.5</Amount>"), "{valid_xml}");

    let missing = Instance::Group(vec![field("Amount", "N/A")]);
    let missing_output = engine::run(&imported.project, &missing)?;
    assert_eq!(
        missing_output.field("Amount").and_then(Instance::as_scalar),
        Some(&Value::Null)
    );
    let missing_xml = format_xml::to_string(&imported.project.target, &missing_output)?;
    assert!(!missing_xml.contains("<Amount>"), "{missing_xml}");
    Ok(())
}
