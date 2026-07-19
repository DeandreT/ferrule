use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{Binding, Graph, IterationOutput, Node, Project, Scope, ScopeIteration};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_mfd_mapped_group_sequence_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn write(path: &Path, text: &str) {
    std::fs::write(path, text).unwrap();
}

fn write_fixture(dir: &Path) {
    write(
        &dir.join("source.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Source"><xs:complexType><xs:sequence><xs:element name="Person" maxOccurs="unbounded"><xs:complexType><xs:sequence><xs:element name="Name" type="xs:string"/><xs:element name="Include" type="xs:boolean"/></xs:sequence></xs:complexType></xs:element></xs:sequence></xs:complexType></xs:element></xs:schema>"#,
    );
    write(
        &dir.join("target.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Target"><xs:complexType><xs:sequence><xs:element name="Selected"><xs:complexType><xs:sequence><xs:element name="Name" type="xs:string"/><xs:element name="Include" type="xs:boolean"/></xs:sequence></xs:complexType></xs:element></xs:sequence></xs:complexType></xs:element></xs:schema>"#,
    );
    write(
        &dir.join("mapping.mfd"),
        r#"<mapping version="26"><component name="map"><structure><children>
          <component name="source" library="xml" kind="14"><data><root><entry name="FileInstance"><entry name="document"><entry name="Source"><entry name="Person" outkey="10"><entry name="Name" outkey="11"/><entry name="Include" outkey="12"/></entry></entry></entry></entry></root><document schema="source.xsd" inputinstance="source.xml" instanceroot="{}Source"/></data></component>
          <component name="filter" library="core" kind="3"><sources><datapoint pos="0" key="20"/><datapoint pos="1" key="21"/></sources><targets><datapoint pos="0" key="30"/><datapoint/></targets></component>
          <component name="target" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data><root><entry name="FileInstance"><entry name="document"><entry name="Target"><entry name="Selected" inpkey="40"/></entry></entry></entry></root><document schema="target.xsd" outputinstance="target.xml" instanceroot="{}Target"/></data></component>
        </children><graph><edges><edge edgekey="90"><data><dataconnection type="2"/></data></edge></edges><vertices><vertex vertexkey="10"><edges><edge vertexkey="20"/></edges></vertex><vertex vertexkey="12"><edges><edge vertexkey="21"/></edges></vertex><vertex vertexkey="30"><edges><edge vertexkey="40" edgekey="90"/></edges></vertex></vertices></graph></structure></component></mapping>"#,
    );
}

fn write_concatenated_fixture(dir: &Path) {
    write(
        &dir.join("source.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Source"><xs:complexType><xs:sequence><xs:element name="Domestic" maxOccurs="unbounded"><xs:complexType><xs:sequence><xs:element name="Name" type="xs:string"/><xs:element name="Code" type="xs:string"/></xs:sequence></xs:complexType></xs:element><xs:element name="International" maxOccurs="unbounded"><xs:complexType><xs:sequence><xs:element name="Name" type="xs:string"/><xs:element name="Code" type="xs:string"/></xs:sequence></xs:complexType></xs:element></xs:sequence></xs:complexType></xs:element></xs:schema>"#,
    );
    write(
        &dir.join("target.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Target"><xs:complexType><xs:sequence><xs:element name="Address" minOccurs="0"><xs:complexType><xs:sequence><xs:element name="Name" type="xs:string"/><xs:element name="Code" type="xs:string"/></xs:sequence></xs:complexType></xs:element></xs:sequence></xs:complexType></xs:element></xs:schema>"#,
    );
    write(
        &dir.join("mapping.mfd"),
        r#"<mapping version="26"><component name="map"><structure><children>
          <component name="source" library="xml" kind="14"><data><root><entry name="FileInstance"><entry name="document"><entry name="Source"><entry name="Domestic" outkey="10"><entry name="Name" outkey="11"/><entry name="Code" outkey="12"/></entry><entry name="International" outkey="20"><entry name="Name" outkey="21"/><entry name="Code" outkey="22"/></entry></entry></entry></entry></root><document schema="source.xsd" inputinstance="source.xml" instanceroot="{}Source"/></data></component>
          <component name="target" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data><root><entry name="FileInstance"><entry name="document"><entry name="Target"><entry name="Address" inpkey="30"><entry name="Name"/><entry name="Code"/></entry><entry name="Address" inpkey="40"><entry name="Name"/><entry name="Code"/></entry></entry></entry></entry></root><document schema="target.xsd" outputinstance="target.xml" instanceroot="{}Target"/></data></component>
        </children><graph><edges><edge edgekey="90"><data><dataconnection type="2"/></data></edge><edge edgekey="91"><data><dataconnection type="2"/></data></edge></edges><vertices><vertex vertexkey="10"><edges><edge vertexkey="30" edgekey="90"/></edges></vertex><vertex vertexkey="20"><edges><edge vertexkey="40" edgekey="91"/></edges></vertex></vertices></graph></structure></component></mapping>"#,
    );
}

fn write_conditioned_concatenated_fixture(dir: &Path) {
    write(
        &dir.join("typed-address.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema" xmlns:t="urn:ferrule:typed-address" targetNamespace="urn:ferrule:typed-address" elementFormDefault="qualified">
          <xs:complexType name="Address"><xs:sequence><xs:element name="name" type="xs:string"/></xs:sequence></xs:complexType>
          <xs:complexType name="EUAddress"><xs:complexContent><xs:extension base="t:Address"><xs:sequence><xs:element name="postcode" type="xs:string"/></xs:sequence></xs:extension></xs:complexContent></xs:complexType>
          <xs:complexType name="USAddress"><xs:complexContent><xs:extension base="t:Address"><xs:sequence><xs:element name="state" type="xs:string"/></xs:sequence></xs:extension></xs:complexContent></xs:complexType>
          <xs:element name="Source"><xs:complexType><xs:sequence><xs:element name="Message" maxOccurs="unbounded"><xs:complexType><xs:sequence><xs:element name="Address" type="t:Address"/></xs:sequence></xs:complexType></xs:element></xs:sequence></xs:complexType></xs:element>
          <xs:element name="Target"><xs:complexType><xs:sequence><xs:element name="Address" type="t:Address" minOccurs="0"/></xs:sequence></xs:complexType></xs:element>
        </xs:schema>"#,
    );
    let condition = |type_name: &str| {
        format!(
            r#"<condition><expression><function name="equal" library="core"><expression><attribute ns="http://www.w3.org/2001/XMLSchema-instance" name="type"/></expression><expression><constant value="{{urn:ferrule:typed-address}}{type_name}" datatype="QName"/></expression></function></expression></condition>"#
        )
    };
    write(
        &dir.join("mapping.mfd"),
        &format!(
            r#"<mapping version="26"><component name="map"><structure><children>
              <component name="source" library="xml" kind="14"><data><root><entry name="FileInstance"><entry name="document"><entry name="Source"><entry name="Message"><entry name="Address" outkey="10">{eu}</entry><entry name="Address" outkey="20">{us}</entry></entry></entry></entry></entry></root><document schema="typed-address.xsd" inputinstance="source.xml" instanceroot="{{urn:ferrule:typed-address}}Source"/></data></component>
              <component name="target" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data><root><entry name="FileInstance"><entry name="document"><entry name="Target"><entry name="Address" inpkey="30">{eu}</entry><entry name="Address" inpkey="40">{us}</entry></entry></entry></entry></root><document schema="typed-address.xsd" outputinstance="target.xml" instanceroot="{{urn:ferrule:typed-address}}Target"/></data></component>
            </children><graph><edges><edge edgekey="90"><data><dataconnection type="2"/></data></edge><edge edgekey="91"><data><dataconnection type="2"/></data></edge></edges><vertices><vertex vertexkey="10"><edges><edge vertexkey="30" edgekey="90"/></edges></vertex><vertex vertexkey="20"><edges><edge vertexkey="40" edgekey="91"/></edges></vertex></vertices></graph></structure></component></mapping>"#,
            eu = condition("EUAddress"),
            us = condition("USAddress"),
        ),
    );
}

fn write_conditioned_descendant_driver_fixture(dir: &Path) {
    write(
        &dir.join("typed-message.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema" xmlns:t="urn:ferrule:typed-message" targetNamespace="urn:ferrule:typed-message" elementFormDefault="qualified">
          <xs:complexType name="Address"><xs:sequence><xs:element name="name" type="xs:string"/></xs:sequence></xs:complexType>
          <xs:complexType name="USAddress"><xs:complexContent><xs:extension base="t:Address"><xs:sequence><xs:element name="state" type="xs:string"/></xs:sequence></xs:extension></xs:complexContent></xs:complexType>
          <xs:complexType name="EUAddress"><xs:complexContent><xs:extension base="t:Address"><xs:sequence><xs:element name="postcode" type="xs:string"/></xs:sequence></xs:extension></xs:complexContent></xs:complexType>
          <xs:element name="Source"><xs:complexType><xs:sequence><xs:element name="Message" maxOccurs="unbounded"><xs:complexType><xs:sequence><xs:element name="Address" type="t:Address"/></xs:sequence></xs:complexType></xs:element></xs:sequence></xs:complexType></xs:element>
          <xs:element name="Target"><xs:complexType><xs:sequence><xs:element name="Message" maxOccurs="unbounded"><xs:complexType><xs:sequence><xs:element name="Address"><xs:complexType><xs:sequence><xs:element name="name" type="xs:string"/></xs:sequence></xs:complexType></xs:element></xs:sequence></xs:complexType></xs:element></xs:sequence></xs:complexType></xs:element>
        </xs:schema>"#,
    );
    let condition = r#"<condition><expression><function name="equal" library="core"><expression><attribute ns="http://www.w3.org/2001/XMLSchema-instance" name="type"/></expression><expression><constant value="{urn:ferrule:typed-message}USAddress" datatype="QName"/></expression></function></expression></condition>"#;
    write(
        &dir.join("mapping.mfd"),
        &format!(
            r#"<mapping version="26"><component name="map"><structure><children>
              <component name="source" library="xml" kind="14"><data><root><entry name="FileInstance"><entry name="document"><entry name="Source"><entry name="Message"><entry name="Address" outkey="10">{condition}<entry name="name" outkey="11"/></entry></entry></entry></entry></entry></root><document schema="typed-message.xsd" inputinstance="source.xml" instanceroot="{{urn:ferrule:typed-message}}Source"/></data></component>
              <component name="target" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data><root><entry name="FileInstance"><entry name="document"><entry name="Target"><entry name="Message" inpkey="30"><entry name="Address"><entry name="name" inpkey="31"/></entry></entry></entry></entry></entry></root><document schema="typed-message.xsd" outputinstance="target.xml" instanceroot="{{urn:ferrule:typed-message}}Target"/></data></component>
            </children><graph><vertices><vertex vertexkey="10"><edges><edge vertexkey="30"/></edges></vertex><vertex vertexkey="11"><edges><edge vertexkey="31"/></edges></vertex></vertices></graph></structure></component></mapping>"#,
        ),
    );
}

fn rewrite_mapping(dir: &Path, rewrite: impl FnOnce(String) -> String) {
    let path = dir.join("mapping.mfd");
    let mapping = std::fs::read_to_string(&path).unwrap();
    write(&path, &rewrite(mapping));
}

fn output_xml(project: &mapping::Project, source_xml: &str) -> String {
    let source = format_xml::from_str(source_xml, &project.source).unwrap();
    let target = engine::run(project, &source).unwrap();
    format_xml::to_string(&project.target, &target).unwrap()
}

fn mapped_names(project: &mapping::Project, source_xml: &str) -> Vec<String> {
    let source = format_xml::from_str(source_xml, &project.source).unwrap();
    let target = engine::run(project, &source).unwrap();
    target
        .field("Selected")
        .and_then(Instance::as_mapped_sequence)
        .unwrap()
        .iter()
        .filter_map(|item| item.field("Name").and_then(Instance::as_scalar))
        .filter_map(|value| match value {
            Value::String(value) => Some(value.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn compatible_structural_sequence_feeds_are_concatenated_in_target_port_order() {
    let dir = TempDir::new();
    write_concatenated_fixture(&dir.0);
    let imported = mfd::import(&dir.0.join("mapping.mfd")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(
        engine::validate(&imported.project).is_empty(),
        "{:?}",
        engine::validate(&imported.project)
    );

    let address = imported
        .project
        .root
        .children
        .iter()
        .find(|scope| scope.target_field == "Address")
        .unwrap();
    assert_eq!(address.iteration_output, IterationOutput::MappedSequence);
    let segments = address.concatenated().unwrap().iter().collect::<Vec<_>>();
    assert_eq!(segments.len(), 2);
    assert_eq!(
        segments[0].source(),
        Some(["Domestic".to_string()].as_slice())
    );
    assert_eq!(
        segments[1].source(),
        Some(["International".to_string()].as_slice())
    );

    let source = format_xml::from_str(
        "<Source><Domestic><Name>North</Name><Code>N1</Code></Domestic><Domestic><Name>South</Name><Code>S2</Code></Domestic><International><Name>East</Name><Code>E3</Code></International></Source>",
        &imported.project.source,
    )
    .unwrap();
    let target = engine::run(&imported.project, &source).unwrap();
    let addresses = target
        .field("Address")
        .and_then(Instance::as_mapped_sequence)
        .unwrap();
    let names = addresses
        .iter()
        .filter_map(|address| address.field("Name").and_then(Instance::as_scalar))
        .collect::<Vec<_>>();
    assert_eq!(
        names,
        vec![
            &Value::String("North".into()),
            &Value::String("South".into()),
            &Value::String("East".into()),
        ]
    );
    let xml = format_xml::to_string(&imported.project.target, &target).unwrap();
    assert_eq!(xml.matches("<Address>").count(), 3);
    assert!(xml.find("North").unwrap() < xml.find("South").unwrap());
    assert!(xml.find("South").unwrap() < xml.find("East").unwrap());
}

#[test]
fn xsi_type_conditioned_structural_feeds_filter_and_preserve_type_identity() {
    let dir = TempDir::new();
    write_conditioned_concatenated_fixture(&dir.0);
    let imported = mfd::import(&dir.0.join("mapping.mfd")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(
        engine::validate(&imported.project).is_empty(),
        "{:?}",
        engine::validate(&imported.project)
    );

    let source = format_xml::from_str(
        r#"<Source xmlns="urn:ferrule:typed-address" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xmlns:t="urn:ferrule:typed-address"><Message><Address xsi:type="t:USAddress"><name>West</name><state>CA</state></Address></Message><Message><Address xsi:type="t:EUAddress"><name>North</name><postcode>N1</postcode></Address></Message></Source>"#,
        &imported.project.source,
    )
    .unwrap();
    let target = engine::run(&imported.project, &source).unwrap();
    let addresses = target
        .field("Address")
        .and_then(Instance::as_mapped_sequence)
        .unwrap();
    assert_eq!(addresses.len(), 2);
    assert_eq!(
        addresses[0].field("name").and_then(Instance::as_scalar),
        Some(&Value::String("North".into()))
    );
    assert_eq!(
        addresses[1].field("name").and_then(Instance::as_scalar),
        Some(&Value::String("West".into()))
    );
    assert_eq!(
        addresses[0]
            .field(ir::XML_TYPE_FIELD)
            .and_then(Instance::as_scalar),
        Some(&Value::String(
            "{urn:ferrule:typed-address}EUAddress".into()
        ))
    );
    assert_eq!(
        addresses[1]
            .field(ir::XML_TYPE_FIELD)
            .and_then(Instance::as_scalar),
        Some(&Value::String(
            "{urn:ferrule:typed-address}USAddress".into()
        ))
    );
    let xml = format_xml::to_string(&imported.project.target, &target).unwrap();
    let eu = xml.find("xsi:type=\"ft:EUAddress\"").unwrap();
    let us = xml.find("xsi:type=\"ft:USAddress\"").unwrap();
    assert!(eu < us, "{xml}");
}

#[test]
fn conditioned_descendant_structural_feed_filters_its_parent_occurrence() {
    let dir = TempDir::new();
    write_conditioned_descendant_driver_fixture(&dir.0);
    let imported = mfd::import(&dir.0.join("mapping.mfd")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(
        engine::validate(&imported.project).is_empty(),
        "{:?}",
        engine::validate(&imported.project)
    );

    let source = format_xml::from_str(
        r#"<Source xmlns="urn:ferrule:typed-message" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xmlns:t="urn:ferrule:typed-message"><Message><Address xsi:type="t:EUAddress"><name>North</name><postcode>N1</postcode></Address></Message><Message><Address xsi:type="t:USAddress"><name>West</name><state>CA</state></Address></Message></Source>"#,
        &imported.project.source,
    )
    .unwrap();
    let output = engine::run(&imported.project, &source).unwrap();
    let messages = output
        .field("Message")
        .and_then(Instance::as_repeated)
        .unwrap();

    assert_eq!(messages.len(), 1);
    assert_eq!(
        messages[0]
            .field("Address")
            .and_then(|address| address.field("name"))
            .and_then(Instance::as_scalar),
        Some(&Value::String("West".into()))
    );
}

#[test]
#[ignore = "needs the local MapForce sample set; informational only"]
fn local_read_messages_filters_parent_orders_and_preserves_conditioned_addresses() {
    let sample_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../samples/ReferenceSamples");
    let mapping = sample_dir.join("ReadMessages.mfd");
    if !mapping.is_file() {
        return;
    }
    let imported = mfd::import(&mapping).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(
        engine::validate(&imported.project).is_empty(),
        "{:?}",
        engine::validate(&imported.project)
    );
    let source =
        format_xml::read(&sample_dir.join("messages.xml"), &imported.project.source).unwrap();
    let target = engine::run(&imported.project, &source).unwrap();
    let orders = target
        .field("purchaseOrder")
        .and_then(Instance::as_repeated)
        .unwrap();
    assert_eq!(orders.len(), 2);
    for order in orders {
        let ship_to = order
            .field("shipTo")
            .and_then(Instance::as_mapped_sequence)
            .unwrap();
        assert_eq!(ship_to.len(), 1);
        assert_eq!(
            ship_to[0]
                .field(ir::XML_TYPE_FIELD)
                .and_then(Instance::as_scalar),
            Some(&Value::String(
                "{http://www.altova.com/IPO}US-Address".into()
            ))
        );
        let bill_to = order
            .field("billTo")
            .and_then(Instance::as_mapped_sequence)
            .unwrap();
        assert_eq!(bill_to.len(), 1);
        assert_eq!(
            bill_to[0]
                .field(ir::XML_TYPE_FIELD)
                .and_then(Instance::as_scalar),
            Some(&Value::String(
                "{http://www.altova.com/IPO}US-Address".into()
            ))
        );
    }
}

fn nested_source_group_project(copy_extra: bool) -> Project {
    let source = SchemaNode::group(
        "Source",
        vec![
            SchemaNode::group(
                "Order",
                vec![SchemaNode::group(
                    "Customer",
                    vec![
                        SchemaNode::scalar("Name", ScalarType::String),
                        SchemaNode::scalar("Extra", ScalarType::String),
                    ],
                )],
            )
            .repeating(),
        ],
    );
    let target = SchemaNode::group(
        "Target",
        vec![SchemaNode::group(
            "Header",
            vec![
                SchemaNode::scalar("Name", ScalarType::String),
                SchemaNode::scalar("Extra", ScalarType::String),
            ],
        )],
    );
    let mut nodes = BTreeMap::from([(
        0,
        Node::SourceField {
            frame: Some(vec!["Order".into()]),
            path: vec!["Customer".into(), "Name".into()],
        },
    )]);
    let mut bindings = vec![Binding {
        target_field: "Name".into(),
        node: 0,
    }];
    if copy_extra {
        nodes.insert(
            1,
            Node::SourceField {
                frame: Some(vec!["Order".into()]),
                path: vec!["Customer".into(), "Extra".into()],
            },
        );
        bindings.push(Binding {
            target_field: "Extra".into(),
            node: 1,
        });
    }
    Project {
        source,
        target,
        source_path: Some("source.xml".into()),
        target_path: Some("target.xml".into()),
        source_options: mapping::FormatOptions::default(),
        target_options: mapping::FormatOptions::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        graph: Graph { nodes },
        root: Scope {
            children: vec![Scope {
                target_field: "Header".into(),
                iteration: ScopeIteration::Source(vec!["Order".into()]),
                iteration_output: IterationOutput::MappedSequence,
                bindings,
                ..Scope::default()
            }],
            ..Scope::default()
        },
    }
}

fn nested_source_xml() -> &'static str {
    "<Source><Order><Customer><Name>Ada</Name><Extra>A</Extra></Customer></Order><Order><Customer><Name>Grace</Name><Extra>G</Extra></Customer></Order></Source>"
}

#[test]
fn filtered_group_port_emits_zero_one_or_many_non_repeating_xml_elements() {
    let dir = TempDir::new();
    write_fixture(&dir.0);
    let imported = mfd::import(&dir.0.join("mapping.mfd")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(
        engine::validate(&imported.project).is_empty(),
        "{:?}",
        engine::validate(&imported.project)
    );
    let selected = imported
        .project
        .root
        .children
        .iter()
        .find(|scope| scope.target_field == "Selected")
        .unwrap();
    assert_eq!(selected.iteration_output, IterationOutput::MappedSequence);
    assert!(!imported.project.target.child("Selected").unwrap().repeating);

    let cases = [
        (
            "<Source><Person><Name>none</Name><Include>false</Include></Person></Source>",
            Vec::<String>::new(),
        ),
        (
            "<Source><Person><Name>one</Name><Include>true</Include></Person></Source>",
            vec!["one".to_string()],
        ),
        (
            "<Source><Person><Name>first</Name><Include>true</Include></Person><Person><Name>discarded</Name><Include>false</Include></Person><Person><Name>second</Name><Include>true</Include></Person></Source>",
            vec!["first".to_string(), "second".to_string()],
        ),
    ];
    for (source, expected) in &cases {
        assert_eq!(mapped_names(&imported.project, source), *expected);
        assert_eq!(
            output_xml(&imported.project, source)
                .matches("<Selected>")
                .count(),
            expected.len()
        );
    }

    let exported = dir.0.join("roundtrip.mfd");
    let warnings = mfd::export(&imported.project, &exported).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    let design = std::fs::read_to_string(&exported).unwrap();
    assert!(!design.contains("dataconnection type=\"2\""), "{design}");
    assert!(design.contains("name=\"Name\" inpkey="), "{design}");
    assert!(design.contains("name=\"Include\" inpkey="), "{design}");
    let reimported = mfd::import(&exported).unwrap();
    assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
    assert!(
        engine::validate(&reimported.project).is_empty(),
        "{:?}",
        engine::validate(&reimported.project)
    );
    for (source, _) in &cases {
        assert_eq!(
            mapped_names(&imported.project, source),
            mapped_names(&reimported.project, source)
        );
        assert_eq!(
            output_xml(&imported.project, source),
            output_xml(&reimported.project, source)
        );
    }

    let mut non_xml_target = imported.project.clone();
    non_xml_target.target_path = Some("target.json".to_string());
    let non_xml_path = dir.0.join("non-xml.mfd");
    assert!(matches!(
        mfd::export(&non_xml_target, &non_xml_path),
        Err(mfd::MfdError::Unsupported(message)) if message.contains("only for XML targets")
    ));
    assert!(!non_xml_path.exists());

    let mut computed = imported.project.clone();
    let selected = computed
        .root
        .children
        .iter()
        .find(|scope| scope.target_field == "Selected")
        .unwrap();
    let name = selected
        .bindings
        .iter()
        .find(|binding| binding.target_field == "Name")
        .unwrap()
        .node;
    computed.graph.nodes.insert(
        name,
        Node::Const {
            value: Value::String("computed".into()),
        },
    );
    let computed_path = dir.0.join("computed.mfd");
    assert!(mfd::export(&computed, &computed_path).unwrap().is_empty());
    assert!(
        !std::fs::read_to_string(&computed_path)
            .unwrap()
            .contains("dataconnection type=\"2\"")
    );
    let computed_roundtrip = mfd::import(&computed_path).unwrap();
    assert!(
        computed_roundtrip.warnings.is_empty(),
        "{:?}",
        computed_roundtrip.warnings
    );
    for (source, _) in &cases {
        assert_eq!(
            output_xml(&computed, source),
            output_xml(&computed_roundtrip.project, source)
        );
    }

    let mut first = imported.project.clone();
    first
        .root
        .children
        .iter_mut()
        .find(|scope| scope.target_field == "Selected")
        .unwrap()
        .iteration_output = IterationOutput::First;
    let first_path = dir.0.join("first.mfd");
    write(&first_path, "existing design");
    assert!(matches!(
        mfd::export(&first, &first_path),
        Err(mfd::MfdError::Unsupported(message)) if message.contains("first-item")
    ));
    assert_eq!(
        std::fs::read_to_string(first_path).unwrap(),
        "existing design"
    );
}

#[test]
fn computed_text_mapping_uses_a_distinct_occurrence_port() {
    let dir = TempDir::new();
    let source = SchemaNode::group(
        "Source",
        vec![
            SchemaNode::group("Row", vec![SchemaNode::scalar("Value", ScalarType::String)])
                .repeating(),
        ],
    );
    let target = SchemaNode::group(
        "Target",
        vec![SchemaNode::group(
            "Item",
            vec![SchemaNode::scalar(ir::XML_TEXT_FIELD, ScalarType::String).text()],
        )],
    );
    let project = Project {
        source,
        target,
        source_path: Some("source.xml".into()),
        target_path: Some("target.xml".into()),
        source_options: mapping::FormatOptions::default(),
        target_options: mapping::FormatOptions::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        graph: Graph {
            nodes: BTreeMap::from([(
                0,
                Node::Const {
                    value: Value::String("computed".into()),
                },
            )]),
        },
        root: Scope {
            children: vec![Scope {
                target_field: "Item".into(),
                iteration: ScopeIteration::Source(vec!["Row".into()]),
                iteration_output: IterationOutput::MappedSequence,
                bindings: vec![Binding {
                    target_field: ir::XML_TEXT_FIELD.into(),
                    node: 0,
                }],
                ..Scope::default()
            }],
            ..Scope::default()
        },
    };

    let path = dir.0.join("computed-text.mfd");
    assert!(mfd::export(&project, &path).unwrap().is_empty());
    let design = std::fs::read_to_string(&path).unwrap();
    assert!(design.contains("name=\"#text\""), "{design}");
    let imported = mfd::import(&path).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(
        engine::validate(&imported.project).is_empty(),
        "{:?}",
        engine::validate(&imported.project)
    );
    let source_xml = "<Source><Row><Value>A</Value></Row><Row><Value>B</Value></Row></Source>";
    let expected = output_xml(&project, source_xml);
    assert_eq!(expected.matches("<Item>computed</Item>").count(), 2);
    assert_eq!(expected, output_xml(&imported.project, source_xml));
}

#[test]
fn nested_mapped_sequence_resolves_an_outward_source_collection() {
    let dir = TempDir::new();
    let source = SchemaNode::group(
        "Source",
        vec![
            SchemaNode::group("Order", vec![SchemaNode::scalar("Id", ScalarType::String)])
                .repeating(),
            SchemaNode::group(
                "Catalog",
                vec![
                    SchemaNode::group(
                        "Entry",
                        vec![SchemaNode::scalar("Value", ScalarType::String)],
                    )
                    .repeating(),
                ],
            ),
        ],
    );
    let target = SchemaNode::group(
        "Target",
        vec![
            SchemaNode::group(
                "Container",
                vec![
                    SchemaNode::scalar("Id", ScalarType::String),
                    SchemaNode::group(
                        "Item",
                        vec![SchemaNode::scalar("Value", ScalarType::String)],
                    ),
                ],
            )
            .repeating(),
        ],
    );
    let project = Project {
        source,
        target,
        source_path: Some("source.xml".into()),
        target_path: Some("target.xml".into()),
        source_options: mapping::FormatOptions::default(),
        target_options: mapping::FormatOptions::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        graph: Graph {
            nodes: BTreeMap::from([
                (
                    0,
                    Node::SourceField {
                        frame: Some(vec!["Order".into()]),
                        path: vec!["Id".into()],
                    },
                ),
                (
                    1,
                    Node::Const {
                        value: Value::String("computed".into()),
                    },
                ),
            ]),
        },
        root: Scope {
            children: vec![Scope {
                target_field: "Container".into(),
                iteration: ScopeIteration::Source(vec!["Order".into()]),
                bindings: vec![Binding {
                    target_field: "Id".into(),
                    node: 0,
                }],
                children: vec![Scope {
                    target_field: "Item".into(),
                    iteration: ScopeIteration::Source(vec!["Catalog".into(), "Entry".into()]),
                    iteration_output: IterationOutput::MappedSequence,
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
    let source_xml = "<Source><Order><Id>A</Id></Order><Order><Id>B</Id></Order><Catalog><Entry><Value>one</Value></Entry><Entry><Value>two</Value></Entry></Catalog></Source>";
    let expected = output_xml(&project, source_xml);
    assert_eq!(expected.matches("<Container>").count(), 2);
    assert_eq!(expected.matches("<Item>").count(), 4);
    assert_eq!(expected.matches("<Value>computed</Value>").count(), 4);

    let path = dir.0.join("outward-source.mfd");
    assert!(mfd::export(&project, &path).unwrap().is_empty());
    let imported = mfd::import(&path).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert_eq!(expected, output_xml(&imported.project, source_xml));
}

#[test]
fn differently_named_complete_group_exports_as_ordinary_wiring() {
    let dir = TempDir::new();
    let project = nested_source_group_project(true);
    let path = dir.0.join("nested-copy.mfd");
    assert!(mfd::export(&project, &path).unwrap().is_empty());
    let design = std::fs::read_to_string(&path).unwrap();
    assert!(!design.contains("dataconnection type=\"2\""), "{design}");
    assert!(design.contains("name=\"Name\" inpkey="), "{design}");
    assert!(design.contains("name=\"Extra\" inpkey="), "{design}");

    let imported = mfd::import(&path).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert_eq!(
        output_xml(&project, nested_source_xml()),
        output_xml(&imported.project, nested_source_xml())
    );
}

#[test]
fn explicit_subset_exports_as_an_ordinary_occurrence_wire() {
    let dir = TempDir::new();
    let project = nested_source_group_project(false);
    let path = dir.0.join("nested-explicit.mfd");
    assert!(mfd::export(&project, &path).unwrap().is_empty());
    let design = std::fs::read_to_string(&path).unwrap();
    assert!(!design.contains("dataconnection type=\"2\""));

    let imported = mfd::import(&path).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let expected = output_xml(&project, nested_source_xml());
    assert!(!expected.contains("<Extra>"));
    assert_eq!(expected, output_xml(&imported.project, nested_source_xml()));
}

#[test]
fn duplicate_target_port_aliases_for_one_feed_create_one_mapped_scope() {
    let dir = TempDir::new();
    write_fixture(&dir.0);
    rewrite_mapping(&dir.0, |mapping| {
        mapping
            .replace(
                r#"<entry name="Selected" inpkey="40"/>"#,
                r#"<entry name="Selected" inpkey="40"/><entry name="Selected" inpkey="41"/>"#,
            )
            .replace(
                r#"<edge vertexkey="40" edgekey="90"/>"#,
                r#"<edge vertexkey="40" edgekey="90"/><edge vertexkey="41" edgekey="90"/>"#,
            )
    });

    let imported = mfd::import(&dir.0.join("mapping.mfd")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let selected = imported
        .project
        .root
        .children
        .iter()
        .filter(|scope| scope.target_field == "Selected")
        .collect::<Vec<_>>();
    assert_eq!(selected.len(), 1);
    assert_eq!(
        selected[0].iteration_output,
        IterationOutput::MappedSequence
    );
}

#[test]
fn compatible_filtered_and_direct_group_feeds_are_concatenated() {
    let dir = TempDir::new();
    write_fixture(&dir.0);
    rewrite_mapping(&dir.0, |mapping| {
        mapping
            .replace(
                r#"<entry name="Selected" inpkey="40"/>"#,
                r#"<entry name="Selected" inpkey="40"/><entry name="Selected" inpkey="41"/>"#,
            )
            .replace(
                r#"<edge edgekey="90"><data><dataconnection type="2"/></data></edge>"#,
                r#"<edge edgekey="90"><data><dataconnection type="2"/></data></edge><edge edgekey="91"><data><dataconnection type="2"/></data></edge>"#,
            )
            .replace(
                r#"<vertex vertexkey="10"><edges><edge vertexkey="20"/></edges></vertex>"#,
                r#"<vertex vertexkey="10"><edges><edge vertexkey="20"/><edge vertexkey="41" edgekey="91"/></edges></vertex>"#,
            )
    });

    let imported = mfd::import(&dir.0.join("mapping.mfd")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let selected = imported
        .project
        .root
        .children
        .iter()
        .find(|scope| scope.target_field == "Selected")
        .unwrap();
    let segments = selected.concatenated().unwrap().iter().collect::<Vec<_>>();
    assert_eq!(segments.len(), 2);
    assert_eq!(
        segments[0].source(),
        Some(["Person".to_string()].as_slice())
    );
    assert!(segments[0].filter.is_some());
    assert_eq!(
        segments[1].source(),
        Some(["Person".to_string()].as_slice())
    );
    assert!(segments[1].filter.is_none());
    assert_eq!(
        mapped_names(
            &imported.project,
            "<Source><Person><Name>A</Name><Include>true</Include></Person><Person><Name>B</Name><Include>false</Include></Person></Source>",
        ),
        vec!["A", "A", "B"]
    );
}
