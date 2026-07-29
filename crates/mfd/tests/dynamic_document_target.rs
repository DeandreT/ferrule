use std::error::Error;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use std::collections::BTreeMap;

use ir::{DocumentMember, Instance, ScalarType, SchemaNode, Value};
use mapping::{Binding, Graph, Node, Project, Scope, ScopeIteration};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> std::io::Result<Self> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule-mfd-dynamic-document-{}-{}",
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

#[test]
fn target_file_instance_roundtrips_and_executes_per_source_document() -> Result<(), Box<dyn Error>>
{
    let directory = TempDir::new()?;
    std::fs::write(
        directory.0.join("source.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
          <xs:element name="Source"><xs:complexType><xs:sequence>
            <xs:element name="Value" type="xs:string"/>
          </xs:sequence></xs:complexType></xs:element>
        </xs:schema>"#,
    )?;
    std::fs::write(
        directory.0.join("target.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
          <xs:element name="Target"><xs:complexType><xs:sequence>
            <xs:element name="Value" type="xs:string"/>
          </xs:sequence></xs:complexType></xs:element>
        </xs:schema>"#,
    )?;
    std::fs::write(
        directory.0.join("mapping.mfd"),
        r#"<mapping version="26"><component name="map"><structure><children>
          <component name="source" library="xml" kind="14"><data><root>
            <entry name="FileInstance" outkey="9"><entry name="document"><entry name="Source">
              <entry name="Value" outkey="10"/>
            </entry></entry></entry></root>
            <document schema="source.xsd" inputinstance="records-*.xml" instanceroot="{}Source"/>
          </data></component>
          <component name="constant" library="core" kind="2"><targets><datapoint key="30"/></targets><data><constant value="out-" datatype="string"/></data></component>
          <component name="concat" library="core" kind="5" growable="1"><sources><datapoint pos="0" key="31"/><datapoint pos="1" key="32"/></sources><targets><datapoint key="33"/></targets></component>
          <component name="target" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data><root>
            <entry name="FileInstance" inpkey="20"><file role="outputinstance" name="fallback.xml"/><entry name="document"><entry name="Target">
              <entry name="Value" inpkey="21"/>
            </entry></entry></entry></root>
            <document schema="target.xsd" instanceroot="{}Target"/>
          </data></component>
        </children><graph><vertices>
          <vertex vertexkey="30"><edges><edge vertexkey="31"/></edges></vertex>
          <vertex vertexkey="9"><edges><edge vertexkey="32"/></edges></vertex>
          <vertex vertexkey="33"><edges><edge vertexkey="20"/></edges></vertex>
          <vertex vertexkey="10"><edges><edge vertexkey="21"/></edges></vertex>
        </vertices></graph></structure></component></mapping>"#,
    )?;

    let imported = mfd::import(&directory.0.join("mapping.mfd"))?;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(engine::validate(&imported.project).is_empty());
    assert_eq!(imported.project.root.source(), Some([].as_slice()));
    assert!(imported.project.root.output_path().is_some());
    assert_eq!(imported.project.target_path, None);

    let document = |path: &str, value: &str| {
        DocumentMember::new(
            path,
            Instance::Group(vec![(
                "Value".into(),
                Instance::Scalar(Value::String(value.into())),
            )]),
        )
        .ok_or("invalid document member")
    };
    let source = Instance::DocumentSet(vec![document("a.xml", "A")?, document("b.xml", "B")?]);
    let output = engine::run(&imported.project, &source)?;
    let Instance::DocumentSet(documents) = output else {
        return Err("expected document-set output".into());
    };
    assert_eq!(documents.len(), 2);
    assert_eq!(documents[0].path(), "out-a.xml");
    assert_eq!(documents[1].path(), "out-b.xml");
    assert_eq!(
        documents[1]
            .value()
            .field("Value")
            .and_then(Instance::as_scalar),
        Some(&Value::String("B".into()))
    );

    let exported = directory.0.join("roundtrip.mfd");
    assert!(mfd::export(&imported.project, &exported)?.is_empty());
    let roundtrip = mfd::import(&exported)?;
    assert!(roundtrip.warnings.is_empty(), "{:?}", roundtrip.warnings);
    assert!(roundtrip.project.root.output_path().is_some());
    assert!(engine::validate(&roundtrip.project).is_empty());
    Ok(())
}

#[test]
fn nested_current_and_restarted_collection_sources_roundtrip_distinctly()
-> Result<(), Box<dyn Error>> {
    let item = SchemaNode::group(
        "Item",
        vec![
            SchemaNode::scalar("Name", ScalarType::String),
            SchemaNode::scalar("Value", ScalarType::String),
        ],
    )
    .repeating();
    let source_schema = SchemaNode::group("Source", vec![item]);
    let target_schema = SchemaNode::group(
        "Report",
        vec![
            SchemaNode::group(
                "Employee",
                vec![
                    SchemaNode::group(
                        "Item",
                        vec![SchemaNode::scalar("Value", ScalarType::String)],
                    )
                    .repeating(),
                ],
            )
            .repeating(),
        ],
    );
    let project = Project {
        source: source_schema,
        target: target_schema,
        source_path: Some("source.xml".into()),
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph: Graph {
            nodes: BTreeMap::from([
                (
                    0,
                    Node::SourceField {
                        path: vec!["Name".into()],
                        frame: Some(vec!["Item".into()]),
                    },
                ),
                (
                    1,
                    Node::SourceField {
                        path: vec!["Value".into()],
                        frame: Some(vec!["Item".into()]),
                    },
                ),
            ]),
        },
        root: Scope {
            iteration: ScopeIteration::DynamicDocuments {
                source: vec!["Item".into()],
                output_path: 0,
            },
            children: vec![Scope {
                target_field: "Employee".into(),
                iteration: ScopeIteration::Source(Vec::new()),
                children: vec![Scope {
                    target_field: "Item".into(),
                    iteration: ScopeIteration::Source(vec!["Item".into()]),
                    bindings: vec![Binding {
                        target_field: "Value".into(),
                        node: 1,
                    }],
                    ..Scope::default()
                }],
                ..Scope::default()
            }],
            ..Scope::default()
        },
    };
    assert!(engine::validate(&project).is_empty());
    let row = |name: &str, value: &str| {
        Instance::Group(vec![
            ("Name".into(), Instance::Scalar(Value::String(name.into()))),
            (
                "Value".into(),
                Instance::Scalar(Value::String(value.into())),
            ),
        ])
    };
    let source = Instance::Group(vec![(
        "Item".into(),
        Instance::Repeated(vec![row("first.xml", "A"), row("second.xml", "B")]),
    )]);
    let expected = engine::run(&project, &source)?;

    let directory = TempDir::new()?;
    let design = directory.0.join("nested-sources.mfd");
    assert!(mfd::export(&project, &design)?.is_empty());
    let exported = std::fs::read_to_string(&design)?;
    assert!(exported.contains("<ferrule-scope-sources version=\"1\">"));

    let roundtrip = mfd::import(&design)?;
    assert!(roundtrip.warnings.is_empty(), "{:?}", roundtrip.warnings);
    let employee = roundtrip
        .project
        .root
        .children
        .iter()
        .find(|scope| scope.target_field == "Employee")
        .ok_or("round-trip project has no Employee scope")?;
    assert_eq!(employee.source(), Some([].as_slice()));
    let item = employee
        .children
        .iter()
        .find(|scope| scope.target_field == "Item")
        .ok_or("round-trip project has no Item scope")?;
    assert_eq!(item.source(), Some(["Item".to_string()].as_slice()));
    assert_eq!(engine::run(&roundtrip.project, &source)?, expected);
    Ok(())
}
