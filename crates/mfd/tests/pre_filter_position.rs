//! Independently authored pre-filter position regressions.

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{Instance, Value};

struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_pre_filter_position_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn fixture() -> (Directory, mapping::Project) {
    fixture_variant(false)
}

fn fixture_variant(user_function: bool) -> (Directory, mapping::Project) {
    let directory = Directory::new();
    std::fs::write(directory.0.join("source.xsd"), r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Input"><xs:complexType><xs:sequence><xs:element name="Token" type="xs:string"/></xs:sequence></xs:complexType></xs:element></xs:schema>"#).unwrap();
    std::fs::write(directory.0.join("target.xsd"), r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Output"><xs:complexType><xs:sequence><xs:element name="Rows" minOccurs="0" maxOccurs="unbounded"><xs:complexType><xs:sequence><xs:element name="Value" type="xs:integer"/><xs:element name="Position" type="xs:integer"/></xs:sequence></xs:complexType></xs:element><xs:element name="Label" type="xs:string"/></xs:sequence></xs:complexType></xs:element></xs:schema>"#).unwrap();
    let path = directory.0.join("mapping.mfd");
    std::fs::write(&path, r#"<mapping version="22"><component name="Mapping"><structure><children>
<component name="Input" library="xml" kind="14"><data><root><entry name="Input"><entry name="Token" outkey="11"/></entry></root><document schema="source.xsd" inputinstance="input.xml" instanceroot="{}Input"/></data></component>
<component name="Output" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data><root><entry name="Output"><entry name="Rows" inpkey="20"><entry name="Value" inpkey="21"/><entry name="Position" inpkey="22"/></entry><entry name="Label" inpkey="23"/></entry></root><document schema="target.xsd" outputinstance="output.xml" instanceroot="{}Output"/></data></component>
<component name="constant" library="core" kind="2"><targets><datapoint pos="0" key="100"/></targets><data><constant value="3" datatype="integer"/></data></component>
<component name="constant" library="core" kind="2"><targets><datapoint pos="0" key="101"/></targets><data><constant value="5" datatype="integer"/></data></component>
<component name="constant" library="core" kind="2"><targets><datapoint pos="0" key="102"/></targets><data><constant value="10" datatype="integer"/></data></component>
<component name="generate-sequence" library="core" kind="5"><sources><datapoint pos="0" key="200"/><datapoint pos="1" key="201"/></sources><targets><datapoint pos="0" key="202"/></targets></component>
<component name="position" library="core" kind="5"><sources><datapoint pos="0" key="210"/></sources><targets><datapoint pos="0" key="211"/></targets></component>
<component name="greater" library="core" kind="5"><sources><datapoint pos="0" key="220"/><datapoint pos="1" key="221"/></sources><targets><datapoint pos="0" key="222"/></targets></component>
<component name="multiply" library="core" kind="5"><sources><datapoint pos="0" key="230"/><datapoint pos="1" key="231"/></sources><targets><datapoint pos="0" key="232"/></targets></component>
<component name="add" library="core" kind="5"><sources><datapoint pos="0" key="240"/><datapoint pos="1" key="241"/></sources><targets><datapoint pos="0" key="242"/></targets></component>
<component name="filter" library="core" kind="3"><sources><datapoint pos="0" key="300"/><datapoint pos="1" key="301"/></sources><targets><datapoint pos="0" key="302"/><datapoint pos="1" key="303"/></targets></component>
</children><graph><vertices>
<vertex vertexkey="11"><edges><edge vertexkey="23"/></edges></vertex>
<vertex vertexkey="100"><edges><edge vertexkey="200"/><edge vertexkey="221"/></edges></vertex>
<vertex vertexkey="101"><edges><edge vertexkey="201"/></edges></vertex>
<vertex vertexkey="102"><edges><edge vertexkey="231"/></edges></vertex>
<vertex vertexkey="202"><edges><edge vertexkey="210"/><edge vertexkey="220"/><edge vertexkey="300"/></edges></vertex>
<vertex vertexkey="222"><edges><edge vertexkey="301"/></edges></vertex>
<vertex vertexkey="302"><edges><edge vertexkey="20"/><edge vertexkey="230"/></edges></vertex>
<vertex vertexkey="211"><edges><edge vertexkey="241"/><edge vertexkey="22"/></edges></vertex>
<vertex vertexkey="232"><edges><edge vertexkey="240"/></edges></vertex>
<vertex vertexkey="242"><edges><edge vertexkey="21"/></edges></vertex>
</vertices></graph></structure></component></mapping>"#).unwrap();
    if user_function {
        let text = std::fs::read_to_string(&path).unwrap()
            .replace(r#"<component name="add" library="core" kind="5"><sources><datapoint pos="0" key="240"/><datapoint pos="1" key="241"/></sources><targets><datapoint pos="0" key="242"/></targets></component>"#, r#"<component name="Sum" library="position_test" kind="19"><data><root><entry name="left" inpkey="240" componentid="501"/><entry name="right" inpkey="241" componentid="502"/></root><root rootindex="1"><entry name="Result" outkey="242" componentid="503"/></root></data></component>"#)
            .replace("</mapping>", r#"<component name="Sum" library="position_test" uid="500" editable="1" blackbox="0"><structure><children>
<component name="left" library="core" uid="501" kind="6"><targets><datapoint pos="0" key="501"/></targets><data><input datatype="integer"/><parameter usageKind="input" name="left"/></data></component>
<component name="right" library="core" uid="502" kind="6"><targets><datapoint pos="0" key="502"/></targets><data><input datatype="integer"/><parameter usageKind="input" name="right"/></data></component>
<component name="Result" library="core" uid="503" kind="7"><sources><datapoint pos="0" key="503"/></sources><data><output datatype="integer"/><parameter usageKind="output" name="Result"/></data></component>
<component name="add" library="core" uid="504" kind="5"><sources><datapoint pos="0" key="504"/><datapoint pos="1" key="505"/></sources><targets><datapoint pos="0" key="506"/></targets></component>
</children><graph><vertices><vertex vertexkey="501"><edges><edge vertexkey="504"/></edges></vertex><vertex vertexkey="502"><edges><edge vertexkey="505"/></edges></vertex><vertex vertexkey="506"><edges><edge vertexkey="503"/></edges></vertex></vertices></graph></structure></component></mapping>"#);
        std::fs::write(&path, text).unwrap();
    }
    let imported =
        mfd::import_with_profile(&path, &Default::default(), mfd::ImportProfile::Executable)
            .unwrap();
    (directory, imported.imported.project)
}

#[test]
fn direct_calls_and_output_keep_the_selected_unfiltered_generator_position() {
    for user_function in [false, true] {
        let (_directory, project) = fixture_variant(user_function);
        let input =
            format_xml::from_str("<Input><Token>safe</Token></Input>", &project.source).unwrap();
        let row = |value, position| {
            Instance::Group(
                vec![
                    ("Value".into(), Instance::Scalar(Value::Int(value))),
                    ("Position".into(), Instance::Scalar(Value::Int(position))),
                ]
                .into(),
            )
        };
        let expected = Instance::Group(
            vec![
                (
                    "Label".into(),
                    Instance::Scalar(Value::String("safe".into())),
                ),
                (
                    "Rows".into(),
                    Instance::Repeated(vec![row(42, 2), row(53, 3)]),
                ),
            ]
            .into(),
        );
        assert_eq!(engine::run(&project, &input).unwrap(), expected);
    }
}

#[test]
fn exported_position_is_wired_to_the_original_producer() {
    let (directory, project) = fixture();
    let input =
        format_xml::from_str("<Input><Token>safe</Token></Input>", &project.source).unwrap();
    let expected = engine::run(&project, &input).unwrap();
    let path = directory.0.join("exported.mfd");
    mfd::export(&project, &path).unwrap();
    let imported =
        mfd::import_with_profile(&path, &Default::default(), mfd::ImportProfile::Executable)
            .unwrap();
    assert!(
        imported
            .imported
            .project
            .graph
            .nodes
            .values()
            .any(|node| matches!(node, mapping::Node::GeneratedPosition { .. }))
    );
    assert_eq!(
        engine::run(&imported.imported.project, &input).unwrap(),
        expected
    );
}

#[test]
fn legacy_compact_position_remains_a_different_expression() {
    let (_directory, mut project) = fixture();
    for node in project.graph.nodes.values_mut() {
        if matches!(node, mapping::Node::GeneratedPosition { .. }) {
            *node = mapping::Node::Position { collection: vec![] };
        }
    }
    let input =
        format_xml::from_str("<Input><Token>safe</Token></Input>", &project.source).unwrap();
    let output = engine::run(&project, &input).unwrap();
    let rows = output.field("Rows").unwrap();
    let expected = Instance::Repeated(
        [(41, 1), (52, 2)]
            .map(|(value, position)| {
                Instance::Group(
                    vec![
                        ("Value".into(), Instance::Scalar(Value::Int(value))),
                        ("Position".into(), Instance::Scalar(Value::Int(position))),
                    ]
                    .into(),
                )
            })
            .into(),
    );
    assert_eq!(rows, &expected);
}

#[test]
fn tokenizer_positions_are_not_reconstructed_from_item_values() {
    let (_directory, mut project) = fixture();
    let item = project.root.children[0].sequence().unwrap().item();
    let position = project
        .graph
        .nodes
        .iter()
        .find_map(|(&id, node)| {
            matches!(node, mapping::Node::GeneratedPosition { .. }).then_some(id)
        })
        .unwrap();
    project.graph.nodes.extend([
        (
            1000,
            mapping::Node::Const {
                value: Value::String("same|same|same".into()),
            },
        ),
        (
            1001,
            mapping::Node::Const {
                value: Value::String("|".into()),
            },
        ),
        (
            1002,
            mapping::Node::Const {
                value: Value::Int(1),
            },
        ),
        (
            1003,
            mapping::Node::Call {
                function: "greater_than".into(),
                args: vec![position, 1002],
            },
        ),
    ]);
    let rows = &mut project.root.children[0];
    rows.iteration = mapping::ScopeIteration::Sequence(mapping::SequenceExpr::Tokenize {
        input: 1000,
        delimiter: 1001,
        item,
    });
    rows.filter = Some(1003);
    rows.bindings[0].node = item;
    project.target = ir::SchemaNode::group(
        "Output",
        vec![
            ir::SchemaNode::group(
                "Rows",
                vec![
                    ir::SchemaNode::scalar("Value", ir::ScalarType::String),
                    ir::SchemaNode::scalar("Position", ir::ScalarType::Int),
                ],
            )
            .repeating(),
            ir::SchemaNode::scalar("Label", ir::ScalarType::String),
        ],
    );
    assert!(
        engine::validate(&project).is_empty(),
        "{:?}",
        engine::validate(&project)
    );
    let input =
        format_xml::from_str("<Input><Token>safe</Token></Input>", &project.source).unwrap();
    let output = engine::run(&project, &input).unwrap();
    let expected = Instance::Repeated(
        [2, 3]
            .map(|position| {
                Instance::Group(
                    vec![
                        (
                            "Value".into(),
                            Instance::Scalar(Value::String("same".into())),
                        ),
                        ("Position".into(), Instance::Scalar(Value::Int(position))),
                    ]
                    .into(),
                )
            })
            .into(),
    );
    assert_eq!(output.field("Rows"), Some(&expected));
}
