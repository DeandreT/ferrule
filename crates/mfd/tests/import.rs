use std::path::{Path, PathBuf};

use ir::{Instance, ScalarType, SchemaKind, SchemaNode, Value, XML_TEXT_FIELD};
use mapping::{Node, SequenceExpr};

#[path = "import/json_alternatives.rs"]
mod json_alternatives;
#[path = "import/multi_source.rs"]
mod multi_source;
#[path = "import/sequence_controls.rs"]
mod sequence_controls;
#[path = "import/udf.rs"]
mod udf;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

/// A scratch dir for export roundtrips, removed on drop.
struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("ferrule_mfd_{tag}_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn scalar(instance: &Instance, field: &str) -> Value {
    instance
        .field(field)
        .and_then(Instance::as_scalar)
        .cloned()
        .unwrap_or_else(|| panic!("no scalar field `{field}`"))
}

#[test]
fn target_node_defaults_fill_missing_connected_and_unconnected_scalars() {
    let temp = TempDir::new("target_node_defaults");
    let source_xsd = temp.0.join("source.xsd");
    let target_xsd = temp.0.join("target.xsd");
    let mapping = temp.0.join("defaults.mfd");
    std::fs::write(
        &source_xsd,
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="Source"><xs:complexType><xs:sequence>
    <xs:element name="Value" type="xs:string" minOccurs="0"/>
    <xs:element name="SourceDefault" type="xs:string" minOccurs="0"/>
    <xs:element name="Maybe" type="xs:string" minOccurs="0"/>
  </xs:sequence></xs:complexType></xs:element>
</xs:schema>"#,
    )
    .unwrap();
    std::fs::write(
        &target_xsd,
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="Target"><xs:complexType><xs:sequence>
    <xs:element name="Name" type="xs:string"/>
    <xs:element name="Value" type="xs:string"/>
    <xs:element name="SourceDefault" type="xs:string"/>
    <xs:element name="Maybe" minOccurs="0"><xs:complexType><xs:simpleContent>
      <xs:extension base="xs:string"/>
    </xs:simpleContent></xs:complexType></xs:element>
  </xs:sequence></xs:complexType></xs:element>
</xs:schema>"#,
    )
    .unwrap();
    std::fs::write(
        &mapping,
        r#"<mapping version="31"><component name="defaultmap1" uid="1">
  <structure><children>
    <component name="Source" library="xml" kind="14" uid="2"><data><root>
      <entry name="FileInstance"><entry name="document"><entry name="Source">
        <entry name="Value" outkey="1"/>
        <entry name="SourceDefault" outkey="3"><outputnodefunctions><rule applyto="self"><default value="from-source"/></rule></outputnodefunctions></entry>
        <entry name="Maybe" outkey="5"/>
      </entry></entry></entry>
    </root><document schema="source.xsd" inputinstance="source.xml" instanceroot="{}Source"/></data></component>
    <component name="Target" library="xml" kind="14" uid="3"><properties XSLTDefaultOutput="1"/><data><root>
      <entry name="FileInstance"><entry name="document"><entry name="Target">
        <entry name="Name"><inputnodefunctions><rule applyto="self"><default value="generated"/></rule></inputnodefunctions></entry>
        <entry name="Value" inpkey="2"><inputnodefunctions><rule applyto="self"><default value="fallback"/></rule></inputnodefunctions></entry>
        <entry name="SourceDefault" inpkey="4"/>
        <entry name="Maybe" inpkey="6"/>
      </entry></entry></entry>
    </root><document schema="target.xsd" outputinstance="target.xml" instanceroot="{}Target"/></data></component>
  </children></structure><connections><edge from="1" to="2"/><edge from="3" to="4"/><edge from="5" to="6"/></connections>
</component></mapping>"#,
    )
    .unwrap();

    let imported = mfd::import(&mapping).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);

    let missing = engine::run(
        &imported.project,
        &Instance::Group(vec![
            ("Value".into(), Instance::Scalar(Value::Null)),
            ("SourceDefault".into(), Instance::Scalar(Value::Null)),
            ("Maybe".into(), Instance::Scalar(Value::Null)),
        ]),
    )
    .unwrap();
    assert_eq!(
        missing.field("Name").and_then(Instance::as_scalar),
        Some(&Value::String("generated".into()))
    );
    assert_eq!(
        missing.field("Value").and_then(Instance::as_scalar),
        Some(&Value::String("fallback".into()))
    );
    assert_eq!(
        missing.field("SourceDefault").and_then(Instance::as_scalar),
        Some(&Value::String("from-source".into()))
    );
    let missing_xml = format_xml::to_string(&imported.project.target, &missing).unwrap();
    let missing_round_trip = format_xml::from_str(&missing_xml, &imported.project.target).unwrap();
    assert!(missing_round_trip.field("Maybe").is_none());

    let present = engine::run(
        &imported.project,
        &Instance::Group(vec![
            (
                "Value".into(),
                Instance::Scalar(Value::String("provided".into())),
            ),
            (
                "SourceDefault".into(),
                Instance::Scalar(Value::String("also-provided".into())),
            ),
            (
                "Maybe".into(),
                Instance::Scalar(Value::String("present".into())),
            ),
        ]),
    )
    .unwrap();
    assert_eq!(
        present.field("Value").and_then(Instance::as_scalar),
        Some(&Value::String("provided".into()))
    );
    assert_eq!(
        present.field("SourceDefault").and_then(Instance::as_scalar),
        Some(&Value::String("also-provided".into()))
    );
    let present_xml = format_xml::to_string(&imported.project.target, &present).unwrap();
    let present_round_trip = format_xml::from_str(&present_xml, &imported.project.target).unwrap();
    assert_eq!(
        present_round_trip
            .field("Maybe")
            .and_then(|group| group.field(XML_TEXT_FIELD))
            .and_then(Instance::as_scalar),
        Some(&Value::String("present".into()))
    );
}

#[test]
fn structural_filter_false_output_inverts_the_iteration_predicate() {
    let temp = TempDir::new("false_filter_output");
    std::fs::write(
        temp.0.join("source.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="Source"><xs:complexType><xs:sequence>
    <xs:element name="Item" maxOccurs="unbounded"><xs:complexType><xs:sequence>
      <xs:element name="Keep" type="xs:boolean"/><xs:element name="Value" type="xs:string"/>
    </xs:sequence></xs:complexType></xs:element>
  </xs:sequence></xs:complexType></xs:element>
</xs:schema>"#,
    )
    .unwrap();
    std::fs::write(
        temp.0.join("target.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="Target"><xs:complexType><xs:sequence>
    <xs:element name="Item" maxOccurs="unbounded"><xs:complexType><xs:sequence>
      <xs:element name="Value" type="xs:string"/>
    </xs:sequence></xs:complexType></xs:element>
  </xs:sequence></xs:complexType></xs:element>
</xs:schema>"#,
    )
    .unwrap();
    let mapping = temp.0.join("filter.mfd");
    std::fs::write(
        &mapping,
        r#"<mapping version="31"><component name="defaultmap1" uid="1"><structure><children>
  <component name="Source" library="xml" kind="14" uid="2"><data><root><entry name="FileInstance"><entry name="document"><entry name="Source">
    <entry name="Item" outkey="10"><entry name="Keep" outkey="11"/><entry name="Value" outkey="12"/></entry>
  </entry></entry></entry></root><document schema="source.xsd" inputinstance="source.xml" instanceroot="{}Source"/></data></component>
  <component name="Target" library="xml" kind="14" uid="3"><properties XSLTDefaultOutput="1"/><data><root><entry name="FileInstance"><entry name="document"><entry name="Target">
    <entry name="Item" inpkey="30"><entry name="Value" inpkey="31"/></entry>
  </entry></entry></entry></root><document schema="target.xsd" outputinstance="target.xml" instanceroot="{}Target"/></data></component>
  <component name="filter" library="core" kind="3" uid="4"><sources><datapoint pos="0" key="20"/><datapoint pos="1" key="21"/></sources><targets><datapoint pos="0" key="22"/><datapoint pos="1" key="23"/></targets></component>
</children></structure><connections><edge from="10" to="20"/><edge from="11" to="21"/><edge from="23" to="30"/><edge from="12" to="31"/></connections></component></mapping>"#,
    )
    .unwrap();

    let imported = mfd::import(&mapping).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let item = |keep, value: &str| {
        Instance::Group(vec![
            ("Keep".into(), Instance::Scalar(Value::Bool(keep))),
            (
                "Value".into(),
                Instance::Scalar(Value::String(value.into())),
            ),
        ])
    };
    let source = Instance::Group(vec![(
        "Item".into(),
        Instance::Repeated(vec![
            item(true, "kept-by-true"),
            item(false, "false-branch"),
        ]),
    )]);
    let output = engine::run(&imported.project, &source).unwrap();
    let items = output
        .field("Item")
        .and_then(Instance::as_repeated)
        .unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(
        items[0].field("Value").and_then(Instance::as_scalar),
        Some(&Value::String("false-branch".into()))
    );
}

#[test]
fn filtered_structural_ancestor_constrains_nested_target_iteration() {
    let temp = TempDir::new("ancestor_filter");
    std::fs::write(
        temp.0.join("source.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="Source"><xs:complexType><xs:sequence>
    <xs:element name="Office" maxOccurs="unbounded"><xs:complexType><xs:sequence>
      <xs:element name="Keep" type="xs:boolean"/>
      <xs:element name="Contact" maxOccurs="unbounded"><xs:complexType><xs:sequence>
        <xs:element name="Value" type="xs:string"/>
      </xs:sequence></xs:complexType></xs:element>
    </xs:sequence></xs:complexType></xs:element>
  </xs:sequence></xs:complexType></xs:element>
</xs:schema>"#,
    )
    .unwrap();
    std::fs::write(
        temp.0.join("target.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="Target"><xs:complexType><xs:sequence>
    <xs:element name="Person" maxOccurs="unbounded"><xs:complexType><xs:sequence>
      <xs:element name="Value" type="xs:string"/>
    </xs:sequence></xs:complexType></xs:element>
  </xs:sequence></xs:complexType></xs:element>
</xs:schema>"#,
    )
    .unwrap();
    let mapping = temp.0.join("filter.mfd");
    std::fs::write(
        &mapping,
        r#"<mapping version="31"><component name="defaultmap1" uid="1"><structure><children>
  <component name="Source" library="xml" kind="14" uid="2"><data><root><entry name="FileInstance"><entry name="document"><entry name="Source">
    <entry name="Office" outkey="10"><entry name="Keep" outkey="11"/><entry name="Contact" outkey="12"><entry name="Value" outkey="13"/></entry></entry>
  </entry></entry></entry></root><document schema="source.xsd" inputinstance="source.xml" instanceroot="{}Source"/></data></component>
  <component name="filter" library="core" kind="3" uid="3"><sources><datapoint pos="0" key="20"/><datapoint pos="1" key="21"/></sources><targets><datapoint pos="0" key="22"/><datapoint/></targets></component>
  <component name="Target" library="xml" kind="14" uid="4"><properties XSLTDefaultOutput="1"/><data><root><entry name="FileInstance"><entry name="document"><entry name="Target" inpkey="30">
    <entry name="Person" inpkey="31"><entry name="Value" inpkey="32"/></entry>
  </entry></entry></entry></root><document schema="target.xsd" outputinstance="target.xml" instanceroot="{}Target"/></data></component>
</children></structure><connections><edge from="10" to="20"/><edge from="11" to="21"/><edge from="22" to="30"/><edge from="12" to="31"/><edge from="13" to="32"/></connections></component></mapping>"#,
    )
    .unwrap();

    let imported = mfd::import(&mapping).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(engine::validate(&imported.project).is_empty());
    let contact = |value: &str| {
        Instance::Group(vec![(
            "Value".into(),
            Instance::Scalar(Value::String(value.into())),
        )])
    };
    let office = |keep, values: &[&str]| {
        Instance::Group(vec![
            ("Keep".into(), Instance::Scalar(Value::Bool(keep))),
            (
                "Contact".into(),
                Instance::Repeated(values.iter().map(|value| contact(value)).collect()),
            ),
        ])
    };
    let source = Instance::Group(vec![(
        "Office".into(),
        Instance::Repeated(vec![
            office(false, &["A"]),
            office(true, &["B", "C"]),
            office(false, &["D"]),
        ]),
    )]);

    let output = engine::run(&imported.project, &source).unwrap();
    let values = output
        .field("Person")
        .and_then(Instance::as_repeated)
        .unwrap()
        .iter()
        .map(|person| scalar(person, "Value"))
        .collect::<Vec<_>>();
    assert_eq!(
        values,
        [Value::String("B".into()), Value::String("C".into())]
    );
}

#[test]
fn imports_schemas_scopes_and_functions() {
    let imported = mfd::import(&fixture("people.mfd")).unwrap();
    let project = &imported.project;

    // Schemas come from the referenced XSDs (typed, repeating).
    assert_eq!(project.source.name, "Company");
    assert!(project.source.child("Staff").unwrap().repeating);
    assert_eq!(project.target.name, "People");
    assert!(project.target.child("Person").unwrap().repeating);

    // The Staff -> Person repeating connection becomes a scope.
    assert_eq!(project.root.children.len(), 1);
    let person = &project.root.children[0];
    assert_eq!(person.target_field, "Person");
    assert_eq!(
        person.source().map(|path| path.to_vec()),
        Some(vec!["Staff".to_string()])
    );

    // Name <- concat(First, " ", Last); Age <- Age.
    assert_eq!(person.bindings.len(), 2);
    let name_binding = person
        .bindings
        .iter()
        .find(|b| b.target_field == "Name")
        .unwrap();
    let Node::Call { function, args } = &project.graph.nodes[&name_binding.node] else {
        panic!("Name should be bound to a call");
    };
    assert_eq!(function, "concat");
    assert_eq!(args.len(), 3);
    assert!(matches!(
        &project.graph.nodes[&args[0]],
        Node::SourceField { path, .. } if path == &["First"]
    ));
    assert!(matches!(
        &project.graph.nodes[&args[1]],
        Node::Const { value: Value::String(s) } if s == " "
    ));

    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
}

#[test]
fn imported_project_runs() {
    let imported = mfd::import(&fixture("people.mfd")).unwrap();
    let source = format_xml::read(&fixture("people.xml"), &imported.project.source).unwrap();
    let target = engine::run(&imported.project, &source).unwrap();

    let people = target
        .field("Person")
        .and_then(Instance::as_repeated)
        .unwrap();
    assert_eq!(people.len(), 2);
    assert_eq!(
        people[0].field("Name").and_then(Instance::as_scalar),
        Some(&Value::String("Alice Carter".into()))
    );
    assert_eq!(
        people[1].field("Age").and_then(Instance::as_scalar),
        Some(&Value::Int(41))
    );
}

#[test]
fn generic_xml_elements_preserve_runtime_names_and_document_order() {
    let imported = mfd::import(&fixture("generic-elements.mfd")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);

    let source_elements = imported
        .project
        .source
        .child("Items")
        .and_then(|items| items.child(ir::XML_ELEMENTS_FIELD))
        .unwrap();
    assert!(source_elements.repeating);
    assert!(source_elements.child(ir::XML_LOCAL_NAME_FIELD).is_some());
    assert!(source_elements.child("Label").is_some());

    let target_elements = imported
        .project
        .target
        .child("Values")
        .and_then(|values| values.child(ir::XML_ELEMENTS_FIELD))
        .unwrap();
    assert!(target_elements.repeating);
    assert!(target_elements.child(ir::XML_TEXT_FIELD).unwrap().text);

    let source =
        format_xml::read(&fixture("generic-elements.xml"), &imported.project.source).unwrap();
    let target = engine::run(&imported.project, &source).unwrap();
    let xml = format_xml::to_string(&imported.project.target, &target).unwrap();
    assert!(xml.contains("<Alpha>first</Alpha>"), "{xml}");
    assert!(xml.contains("<Beta>second</Beta>"), "{xml}");
    assert!(xml.find("<Alpha>") < xml.find("<Beta>"), "{xml}");
}

#[test]
fn scalar_calls_iterate_and_transform_nested_generic_element_text() {
    let imported = mfd::import(&fixture("generic-text-transform.mfd")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);

    let outer = imported
        .project
        .target
        .child("Employees")
        .and_then(|employees| employees.child(ir::XML_ELEMENTS_FIELD))
        .unwrap();
    let inner = outer.child(ir::XML_ELEMENTS_FIELD).unwrap();
    assert!(inner.repeating);
    assert!(inner.child(XML_TEXT_FIELD).is_some_and(|text| text.text));

    let employees_scope = imported
        .project
        .root
        .children
        .iter()
        .find(|scope| scope.target_field == "Employees")
        .unwrap();
    let outer_scope = employees_scope
        .children
        .iter()
        .find(|scope| scope.target_field == ir::XML_ELEMENTS_FIELD)
        .unwrap();
    assert_eq!(
        outer_scope.source().map(|path| path.to_vec()),
        Some(vec![
            "Employees".to_string(),
            ir::XML_ELEMENTS_FIELD.to_string(),
        ])
    );
    assert!(
        outer_scope
            .bindings
            .iter()
            .all(|binding| binding.target_field != XML_TEXT_FIELD)
    );
    let inner_scope = outer_scope
        .children
        .iter()
        .find(|scope| scope.target_field == ir::XML_ELEMENTS_FIELD)
        .unwrap();
    assert_eq!(
        inner_scope.source().map(|path| path.to_vec()),
        Some(vec![ir::XML_ELEMENTS_FIELD.to_string()])
    );
    let text_binding = inner_scope
        .bindings
        .iter()
        .find(|binding| binding.target_field == XML_TEXT_FIELD)
        .unwrap();
    let Node::Call { function, args } = &imported.project.graph.nodes[&text_binding.node] else {
        panic!("generic text should be bound to a scalar call");
    };
    assert_eq!(function, "upper");
    assert!(matches!(
        &imported.project.graph.nodes[&args[0]],
        Node::SourceField { path, frame: Some(frame) }
            if path == &[XML_TEXT_FIELD]
                && frame == &["Employees", ir::XML_ELEMENTS_FIELD, ir::XML_ELEMENTS_FIELD]
    ));

    let source = format_xml::read(
        &fixture("generic-text-transform.xml"),
        &imported.project.source,
    )
    .unwrap();
    let target = engine::run(&imported.project, &source).unwrap();
    let employees = target.field("Employees").unwrap();
    let outer_elements = employees
        .field(ir::XML_ELEMENTS_FIELD)
        .and_then(Instance::as_repeated)
        .unwrap();
    assert_eq!(outer_elements.len(), 2);
    assert_eq!(
        scalar(&outer_elements[0], ir::XML_NODE_NAME_FIELD),
        Value::String("Engineer".into())
    );
    assert_eq!(
        scalar(&outer_elements[1], ir::XML_NODE_NAME_FIELD),
        Value::String("Designer".into())
    );
    let engineer_fields = outer_elements[0]
        .field(ir::XML_ELEMENTS_FIELD)
        .and_then(Instance::as_repeated)
        .unwrap();
    assert_eq!(engineer_fields.len(), 2);
    assert_eq!(
        scalar(&engineer_fields[0], ir::XML_NODE_NAME_FIELD),
        Value::String("first".into())
    );
    assert_eq!(
        scalar(&engineer_fields[0], XML_TEXT_FIELD),
        Value::String("ANA".into())
    );
    assert_eq!(
        scalar(&engineer_fields[1], ir::XML_NODE_NAME_FIELD),
        Value::String("city".into())
    );
    assert_eq!(
        scalar(&engineer_fields[1], XML_TEXT_FIELD),
        Value::String("PARIS".into())
    );

    let xml = format_xml::to_string(&imported.project.target, &target).unwrap();
    assert!(xml.contains("<first>ANA</first>"), "{xml}");
    assert!(xml.contains("<city>OSLO</city>"), "{xml}");
    assert!(xml.find("<Engineer>") < xml.find("<Designer>"), "{xml}");
}

#[test]
fn xsd_includes_supply_component_schemas_and_the_project_runs() {
    let imported = mfd::import(&fixture("includes.mfd")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let project = &imported.project;

    let item = project.source.child("Item").unwrap();
    assert!(item.repeating);
    assert!(matches!(
        item.child("Qty").unwrap().kind,
        SchemaKind::Scalar {
            ty: ScalarType::Int
        }
    ));
    let line = project.target.child("Line").unwrap();
    assert!(line.repeating);
    assert!(matches!(
        line.child("Amount").unwrap().kind,
        SchemaKind::Scalar {
            ty: ScalarType::Int
        }
    ));

    let source = format_xml::read(&fixture("includes.xml"), &project.source).unwrap();
    let target = engine::run(project, &source).unwrap();
    let lines = target
        .field("Line")
        .and_then(Instance::as_repeated)
        .unwrap();
    assert_eq!(lines.len(), 2);
    assert_eq!(scalar(&lines[0], "Code"), Value::String("A-10".into()));
    assert_eq!(scalar(&lines[1], "Amount"), Value::Int(7));
}

#[test]
fn export_then_import_roundtrips_semantically() {
    let imported = mfd::import(&fixture("people.mfd")).unwrap();
    let dir = std::env::temp_dir().join(format!("ferrule_mfd_roundtrip_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let out = dir.join("people.mfd");

    let warnings = mfd::export(&imported.project, &out).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    let reimported = mfd::import(&out).unwrap();
    std::fs::remove_dir_all(&dir).unwrap();

    let a = &imported.project;
    let b = &reimported.project;
    assert_eq!(a.source, b.source);
    assert_eq!(a.target, b.target);
    // Scope shape survives.
    assert_eq!(b.root.children.len(), 1);
    assert_eq!(b.root.children[0].source(), a.root.children[0].source());
    assert_eq!(
        b.root.children[0].bindings.len(),
        a.root.children[0].bindings.len()
    );
    // The reimported project must still run and produce the same output.
    let source = format_xml::read(&fixture("people.xml"), &b.source).unwrap();
    let out_a = engine::run(a, &source).unwrap();
    let out_b = engine::run(b, &source).unwrap();
    assert_eq!(out_a, out_b);
}

#[test]
fn xml_attributes_import_run_and_roundtrip() {
    let imported = mfd::import(&fixture("books.mfd")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let project = &imported.project;

    let book = project.source.child("Book").unwrap();
    assert!(book.repeating);
    assert!(book.child("isbn").unwrap().attribute);
    assert!(book.child("pages").unwrap().attribute);
    assert!(matches!(
        book.child("pages").unwrap().kind,
        SchemaKind::Scalar {
            ty: ScalarType::Int
        }
    ));
    assert!(!book.child("Title").unwrap().attribute);
    assert!(
        project
            .target
            .child("Entry")
            .unwrap()
            .child("id")
            .unwrap()
            .attribute
    );

    let source = format_xml::read(&fixture("books.xml"), &project.source).unwrap();
    let target = engine::run(project, &source).unwrap();
    let entries = target
        .field("Entry")
        .and_then(Instance::as_repeated)
        .unwrap();
    assert_eq!(entries.len(), 2);
    assert_eq!(scalar(&entries[0], "id"), Value::String("978-1".into()));
    assert_eq!(scalar(&entries[0], "Name"), Value::String("Systems".into()));
    assert_eq!(scalar(&entries[1], "Pages"), Value::Int(180));

    let dir = TempDir::new("books");
    let out = dir.0.join("books.mfd");
    let warnings = mfd::export(project, &out).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    let reimported = mfd::import(&out).unwrap();
    assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
    assert_eq!(project.source, reimported.project.source);
    assert_eq!(project.target, reimported.project.target);
    // Binding order may differ (the exporter keys ports in schema order),
    // so compare the written documents, whose field order the schema fixes.
    let out_b = engine::run(&reimported.project, &source).unwrap();
    let write = |name: &str, instance: &Instance| {
        let path = dir.0.join(name);
        format_xml::write(&path, &project.target, instance).unwrap();
        std::fs::read_to_string(path).unwrap()
    };
    assert_eq!(write("a.xml", &target), write("b.xml", &out_b));
}

#[test]
fn xml_simple_content_imports_runs_and_roundtrips() {
    let imported = mfd::import(&fixture("simple-content.mfd")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let project = &imported.project;

    let source_price = project
        .source
        .child("Item")
        .unwrap()
        .child("Price")
        .unwrap();
    let source_text = source_price.child(ir::XML_TEXT_FIELD).unwrap();
    assert!(source_text.text);
    assert!(matches!(
        source_text.kind,
        SchemaKind::Scalar {
            ty: ScalarType::Float
        }
    ));
    assert!(source_price.child("currency").unwrap().attribute);

    let source = format_xml::read(&fixture("simple-content.xml"), &project.source).unwrap();
    let target = engine::run(project, &source).unwrap();
    let entries = target
        .field("Entry")
        .and_then(Instance::as_repeated)
        .unwrap();
    let amount = entries[1].field("Amount").unwrap();
    assert_eq!(scalar(amount, ir::XML_TEXT_FIELD), Value::Float(8.75));
    assert_eq!(scalar(amount, "currency"), Value::String("EUR".into()));

    let dir = TempDir::new("simple_content");
    let xml_out = dir.0.join("prices.xml");
    format_xml::write(&xml_out, &project.target, &target).unwrap();
    let xml = std::fs::read_to_string(&xml_out).unwrap();
    assert!(xml.contains("<Amount currency=\"USD\">12.5</Amount>"));

    let mfd_out = dir.0.join("prices.mfd");
    let warnings = mfd::export(project, &mfd_out).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    let exported = std::fs::read_to_string(&mfd_out).unwrap();
    assert!(!exported.contains("name=\"#text\""));
    let reimported = mfd::import(&mfd_out).unwrap();
    assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
    assert_eq!(reimported.project.source, project.source);
    assert_eq!(reimported.project.target, project.target);
    let rerun = engine::run(&reimported.project, &source).unwrap();
    assert_eq!(rerun, target);
}

#[test]
fn xml_to_json_with_ref_schema_imports_runs_and_roundtrips() {
    let imported = mfd::import(&fixture("stock.mfd")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let project = &imported.project;

    // The JSON Schema resolves through its root-level and nested $refs.
    assert_eq!(project.target.name, "Stock");
    assert!(project.target.repeating);
    let batches = project.target.child("batches").unwrap();
    assert!(batches.repeating);
    assert!(batches.child("code").is_some());
    assert_eq!(project.target_path.as_deref(), Some("stock-out.json"));

    // Row iteration lands on the root scope; batches nest inside it.
    assert_eq!(
        project.root.source().map(|path| path.to_vec()),
        Some(vec!["Item".to_string()])
    );
    let batches_scope = &project.root.children[0];
    assert_eq!(batches_scope.target_field, "batches");
    assert_eq!(
        batches_scope.source().map(|path| path.to_vec()),
        Some(vec!["Batch".to_string()])
    );

    let source = format_xml::read(&fixture("stock.xml"), &project.source).unwrap();
    let target = engine::run(project, &source).unwrap();
    let rows = target.as_repeated().unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(scalar(&rows[0], "sku"), Value::String("A1".into()));
    assert_eq!(scalar(&rows[0], "qty"), Value::Int(4));
    let batches = rows[0]
        .field("batches")
        .and_then(Instance::as_repeated)
        .unwrap();
    assert_eq!(batches.len(), 2);
    assert_eq!(scalar(&batches[1], "code"), Value::String("B2".into()));

    let dir = TempDir::new("stock");
    let out = dir.0.join("stock.mfd");
    let warnings = mfd::export(project, &out).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    assert!(dir.0.join("stock-target.schema.json").exists());
    let reimported = mfd::import(&out).unwrap();
    assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
    assert_eq!(project.source, reimported.project.source);
    assert_eq!(project.target, reimported.project.target);
    let out_b = engine::run(&reimported.project, &source).unwrap();
    assert_eq!(target, out_b);
}

#[test]
fn json_source_designs_import_and_run() {
    let imported = mfd::import(&fixture("inventory.mfd")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let project = &imported.project;

    assert_eq!(project.source.name, "Inventory");
    assert_eq!(project.source_path.as_deref(), Some("inventory.json"));
    assert!(project.source.child("items").unwrap().repeating);

    let line = &project.root.children[0];
    assert_eq!(line.target_field, "Line");
    assert_eq!(
        line.source().map(|path| path.to_vec()),
        Some(vec!["items".to_string()])
    );

    let source = format_json::read(&fixture("inventory.json"), &project.source).unwrap();
    let target = engine::run(project, &source).unwrap();
    assert_eq!(scalar(&target, "Store"), Value::String("Downtown".into()));
    let lines = target
        .field("Line")
        .and_then(Instance::as_repeated)
        .unwrap();
    assert_eq!(lines.len(), 2);
    assert_eq!(scalar(&lines[1], "Product"), Value::String("Gadget".into()));
    assert_eq!(scalar(&lines[1], "Count"), Value::Int(3));
}

#[test]
fn json_components_without_schema_fall_back_to_the_entry_tree() {
    let imported = mfd::import(&fixture("noschema-json.mfd")).unwrap();
    assert!(
        imported
            .warnings
            .iter()
            .any(|w| w.contains("no schema reference")),
        "{:?}",
        imported.warnings
    );
    let source = &imported.project.source;
    assert_eq!(source.name, "orders");
    assert!(matches!(
        source.child("customer").unwrap().kind,
        SchemaKind::Scalar {
            ty: ScalarType::String
        }
    ));
    assert!(matches!(
        source.child("total").unwrap().kind,
        SchemaKind::Scalar {
            ty: ScalarType::Float
        }
    ));
}

#[test]
fn csv_source_designs_import_and_run() {
    let imported = mfd::import(&fixture("people-csv.mfd")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let project = &imported.project;

    assert_eq!(project.source.name, "Staff");
    assert!(!project.source.repeating);
    assert!(matches!(
        project.source.child("Age").unwrap().kind,
        SchemaKind::Scalar {
            ty: ScalarType::Int
        }
    ));
    assert_eq!(project.source_path.as_deref(), Some("people.csv"));
    assert_eq!(project.source_options.delimiter, Some(','));
    assert_eq!(project.source_options.has_header_row, Some(true));
    assert_eq!(
        project.source_options.tabular_kind,
        Some(mapping::TabularBoundaryKind::Csv)
    );

    // The row block feeds the Person iteration; rows arrive as the
    // enclosing Repeated, so the scope path is empty.
    let person = &project.root.children[0];
    assert_eq!(person.target_field, "Person");
    assert_eq!(person.source(), Some([].as_slice()));

    let rows = format_csv::read(&fixture("people.csv"), &project.source, Some(','), true).unwrap();
    let target = engine::run(project, &Instance::Repeated(rows)).unwrap();
    let people = target
        .field("Person")
        .and_then(Instance::as_repeated)
        .unwrap();
    assert_eq!(people.len(), 2);
    assert_eq!(
        scalar(&people[0], "Name"),
        Value::String("Alice Carter".into())
    );
    assert_eq!(scalar(&people[1], "Age"), Value::Int(41));
}

#[test]
fn csv_target_designs_import_run_and_roundtrip() {
    let imported = mfd::import(&fixture("people-to-csv.mfd")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let project = &imported.project;

    assert_eq!(project.target.name, "PeopleRows");
    assert_eq!(project.target_path.as_deref(), Some("people-out.csv"));
    assert_eq!(project.target_options.delimiter, Some(';'));
    assert_eq!(project.target_options.has_header_row, Some(false));
    assert_eq!(
        project.target_options.tabular_kind,
        Some(mapping::TabularBoundaryKind::Csv)
    );

    // Rows iterate on the root scope itself.
    assert_eq!(
        project.root.source().map(|path| path.to_vec()),
        Some(vec!["Staff".to_string()])
    );

    let source = format_xml::read(&fixture("people.xml"), &project.source).unwrap();
    let target = engine::run(project, &source).unwrap();
    let rows = target.as_repeated().unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(
        scalar(&rows[0], "Name"),
        Value::String("Alice Carter".into())
    );
    assert_eq!(scalar(&rows[1], "Age"), Value::Int(41));

    let dir = TempDir::new("people_to_csv");
    let out = dir.0.join("people-to-csv.mfd");
    let warnings = mfd::export(project, &out).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    let reimported = mfd::import(&out).unwrap();
    assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
    assert_eq!(project.target, reimported.project.target);
    assert_eq!(reimported.project.target_options.delimiter, Some(';'));
    assert_eq!(
        reimported.project.target_options.tabular_kind,
        Some(mapping::TabularBoundaryKind::Csv)
    );
    assert_eq!(
        reimported.project.target_options.has_header_row,
        Some(false)
    );
    let out_b = engine::run(&reimported.project, &source).unwrap();
    assert_eq!(target, out_b);
}

#[test]
fn db_target_designs_import_run_and_roundtrip() {
    // Stage the design in a scratch dir with a typed (empty) SQLite table
    // next to it, so the importer's introspection path is exercised
    // without a binary fixture in the repo.
    let dir = TempDir::new("people_to_db");
    for f in ["people-to-db.mfd", "people-source.xsd", "people.xml"] {
        std::fs::copy(fixture(f), dir.0.join(f)).unwrap();
    }
    let table = ir::SchemaNode::group(
        "People",
        vec![
            ir::SchemaNode::scalar("Name", ScalarType::String),
            ir::SchemaNode::scalar("Age", ScalarType::Int),
        ],
    )
    .repeating();
    let db_path = dir.0.join("people-out.sqlite");
    format_db::write(&db_path, &table, &[]).unwrap();

    let imported = mfd::import(&dir.0.join("people-to-db.mfd")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let project = &imported.project;

    // Schema came from introspecting the SQLite file (typed).
    assert_eq!(project.target, table);
    assert_eq!(project.target_path.as_deref(), Some("people-out.sqlite"));
    // Rows iterate on the root scope, like the other flat-rows formats.
    assert_eq!(
        project.root.source().map(|path| path.to_vec()),
        Some(vec!["Staff".to_string()])
    );

    let source = format_xml::read(&fixture("people.xml"), &project.source).unwrap();
    let target = engine::run(project, &source).unwrap();
    let rows = target.as_repeated().unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(
        scalar(&rows[0], "Name"),
        Value::String("Alice Carter".into())
    );

    // The rows actually land in (and read back from) the database.
    format_db::write(&db_path, &project.target, rows).unwrap();
    let read_back = format_db::read(&db_path, &project.target).unwrap();
    assert_eq!(read_back.len(), 2);
    assert_eq!(scalar(&read_back[1], "Age"), Value::Int(41));

    // Export emits a db component + datasource; reimport is faithful.
    let out = dir.0.join("people-to-db-2.mfd");
    let warnings = mfd::export(project, &out).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    let text = std::fs::read_to_string(&out).unwrap();
    assert!(text.contains("library=\"db\""), "{text}");
    assert!(text.contains("database_connection"), "{text}");
    let reimported = mfd::import(&out).unwrap();
    assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
    assert_eq!(project.target, reimported.project.target);
    assert_eq!(
        reimported.project.target_path.as_deref(),
        Some("people-out.sqlite")
    );
    let out_b = engine::run(&reimported.project, &source).unwrap();
    assert_eq!(target, out_b);
}

#[test]
fn aggregate_position_filter_and_scalar_designs_import_run_and_roundtrip() {
    let imported = mfd::import(&fixture("orders.mfd")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let project = &imported.project;

    // count(Item) and sum(Item/Price) evaluate inside the Order scope;
    // string-join(Order, Id, ", ") evaluates at the root, so its
    // collection keeps the Order segment.
    let order_scope = &project.root.children[0];
    assert_eq!(order_scope.target_field, "Order");
    assert!(matches!(
        order_scope
            .filter
            .and_then(|id| project.graph.nodes.get(&id)),
        Some(Node::Call { function, .. }) if function == "starts_with"
    ));
    let id_binding = order_scope
        .bindings
        .iter()
        .find(|binding| binding.target_field == "Id")
        .unwrap();
    assert!(matches!(
        &project.graph.nodes[&id_binding.node],
        Node::SourceField { path, .. } if path == &["Id"]
    ));
    let count_binding = order_scope
        .bindings
        .iter()
        .find(|b| b.target_field == "ItemCount")
        .unwrap();
    assert!(matches!(
        &project.graph.nodes[&count_binding.node],
        Node::Aggregate { function: mapping::AggregateOp::Count, collection, value, .. }
            if collection == &["Item"] && value.is_empty()
    ));
    let total_binding = order_scope
        .bindings
        .iter()
        .find(|b| b.target_field == "Total")
        .unwrap();
    assert!(matches!(
        &project.graph.nodes[&total_binding.node],
        Node::Aggregate { function: mapping::AggregateOp::Sum, collection, value, .. }
            if collection == &["Item"] && value == &["Price"]
    ));
    let doubled_binding = order_scope
        .bindings
        .iter()
        .find(|b| b.target_field == "DoubledTotal")
        .unwrap();
    let doubled_expression = match &project.graph.nodes[&doubled_binding.node] {
        Node::Aggregate {
            function: mapping::AggregateOp::Sum,
            collection,
            value,
            expression: Some(expression),
            ..
        } if collection == &["Item"] && value.is_empty() => *expression,
        other => panic!("expected computed sum aggregate, got {other:?}"),
    };
    assert!(matches!(
        &project.graph.nodes[&doubled_expression],
        Node::Call { function, .. } if function == "multiply"
    ));
    let position_binding = order_scope
        .bindings
        .iter()
        .find(|b| b.target_field == "Position")
        .unwrap();
    assert!(matches!(
        &project.graph.nodes[&position_binding.node],
        Node::Position { collection } if collection == &["Order"]
    ));
    let padded_binding = order_scope
        .bindings
        .iter()
        .find(|binding| binding.target_field == "PaddedPosition")
        .unwrap();
    assert!(matches!(
        &project.graph.nodes[&padded_binding.node],
        Node::Call { function, .. } if function == "pad_string_left"
    ));
    let limit_binding = order_scope
        .bindings
        .iter()
        .find(|binding| binding.target_field == "WithinLimit")
        .unwrap();
    assert!(matches!(
        &project.graph.nodes[&limit_binding.node],
        Node::Call { function, .. } if function == "less_or_equal"
    ));
    let ids_binding = project
        .root
        .bindings
        .iter()
        .find(|b| b.target_field == "AllIds")
        .unwrap();
    assert!(matches!(
        &project.graph.nodes[&ids_binding.node],
        Node::Aggregate {
            function: mapping::AggregateOp::Join,
            collection,
            value,
            arg: Some(_),
            ..
        }
            if collection == &["Order"] && value == &["Id"]
    ));

    let source = format_xml::read(&fixture("orders.xml"), &project.source).unwrap();
    let target = engine::run(project, &source).unwrap();
    assert_eq!(scalar(&target, "AllIds"), Value::String("A-1, B-2".into()));
    let orders = target
        .field("Order")
        .and_then(Instance::as_repeated)
        .unwrap();
    assert_eq!(orders.len(), 1);
    assert_eq!(scalar(&orders[0], "ItemCount"), Value::Int(2));
    assert_eq!(scalar(&orders[0], "Total"), Value::Float(4.0));
    assert_eq!(scalar(&orders[0], "DoubledTotal"), Value::Float(8.0));
    assert_eq!(scalar(&orders[0], "Position"), Value::Int(1));
    assert_eq!(
        scalar(&orders[0], "PaddedPosition"),
        Value::String("01".into())
    );
    assert_eq!(scalar(&orders[0], "WithinLimit"), Value::Bool(true));

    let dir = TempDir::new("orders");
    let out = dir.0.join("orders.mfd");
    let warnings = mfd::export(project, &out).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    let exported = std::fs::read_to_string(&out).unwrap();
    assert!(exported.contains("name=\"pad-string-left\" library=\"lang\""));
    let reimported = mfd::import(&out).unwrap();
    assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
    let out_b = engine::run(&reimported.project, &source).unwrap();
    assert_eq!(target, out_b);
}

#[test]
fn group_by_designs_import_run_and_roundtrip() {
    let imported = mfd::import(&fixture("temps.mfd")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let project = &imported.project;

    // The group-by component becomes the scope's grouping key; its key
    // output feeds the Year binding as the key expression itself.
    let stats = &project.root.children[0];
    assert_eq!(stats.target_field, "YearlyStats");
    assert_eq!(
        stats.source().map(|path| path.to_vec()),
        Some(vec!["Row".to_string()])
    );
    let group_key = stats.group_by.expect("scope should group");
    assert!(matches!(
        &project.graph.nodes[&group_key],
        Node::Call { function, .. } if function == "substring_before"
    ));
    let year = stats
        .bindings
        .iter()
        .find(|b| b.target_field == "Year")
        .unwrap();
    assert_eq!(year.node, group_key);

    let source = format_xml::read(&fixture("temps.xml"), &project.source).unwrap();
    let target = engine::run(project, &source).unwrap();
    let years = target
        .field("YearlyStats")
        .and_then(Instance::as_repeated)
        .unwrap();
    assert_eq!(years.len(), 2);
    assert_eq!(scalar(&years[0], "Year"), Value::String("2024".into()));
    assert_eq!(scalar(&years[0], "MinTemp"), Value::Float(2.0));
    assert_eq!(scalar(&years[0], "MaxTemp"), Value::Float(22.0));
    assert_eq!(scalar(&years[0], "AvgTemp"), Value::Float(12.0));
    assert_eq!(scalar(&years[1], "Year"), Value::String("2025".into()));
    assert_eq!(scalar(&years[1], "AvgTemp"), Value::Float(4.0));

    let dir = TempDir::new("temps");
    let out = dir.0.join("temps.mfd");
    let warnings = mfd::export(project, &out).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    let reimported = mfd::import(&out).unwrap();
    assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
    assert!(reimported.project.root.children[0].group_by.is_some());
    let out_b = engine::run(&reimported.project, &source).unwrap();
    assert_eq!(target, out_b);
}

#[test]
fn sorted_first_items_import_run_and_roundtrip() {
    let imported = mfd::import(&fixture("ranked.mfd")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let project = &imported.project;

    let top = project
        .root
        .children
        .iter()
        .find(|scope| scope.target_field == "Top")
        .unwrap();
    assert_eq!(
        top.source().map(|path| path.to_vec()),
        Some(vec!["Score".into()])
    );
    assert!(top.sort_descending);
    assert!(matches!(
        &project.graph.nodes[&top.sort_by.unwrap()],
        Node::SourceField { path, .. } if path == &["Points"]
    ));
    assert!(matches!(
        &project.graph.nodes[&top.take.unwrap()],
        Node::Const {
            value: Value::Int(2)
        }
    ));
    let best = project
        .root
        .children
        .iter()
        .find(|scope| scope.target_field == "Best")
        .unwrap();
    assert!(matches!(
        &project.graph.nodes[&best.take.unwrap()],
        Node::Const {
            value: Value::Int(1)
        }
    ));
    let distinct = project
        .root
        .children
        .iter()
        .find(|scope| scope.target_field == "Distinct")
        .unwrap();
    assert!(distinct.group_by.is_some());
    assert!(distinct.take.is_none());
    assert_eq!(distinct.bindings.len(), 2);

    let source = format_xml::read(&fixture("ranked.xml"), &project.source).unwrap();
    let target = engine::run(project, &source).unwrap();
    let top_rows = target.field("Top").and_then(Instance::as_repeated).unwrap();
    assert_eq!(top_rows.len(), 2);
    assert_eq!(scalar(&top_rows[0], "Name"), Value::String("First".into()));
    assert_eq!(scalar(&top_rows[0], "Position"), Value::Int(1));
    assert_eq!(scalar(&top_rows[1], "Name"), Value::String("Second".into()));
    assert_eq!(scalar(&top_rows[1], "Position"), Value::Int(2));
    let best_rows = target
        .field("Best")
        .and_then(Instance::as_repeated)
        .unwrap();
    assert_eq!(best_rows.len(), 1);
    assert_eq!(scalar(&best_rows[0], "Name"), Value::String("First".into()));
    let distinct_rows = target
        .field("Distinct")
        .and_then(Instance::as_repeated)
        .unwrap();
    let distinct_names: Vec<_> = distinct_rows
        .iter()
        .map(|row| scalar(row, "Name"))
        .collect();
    assert_eq!(
        distinct_names,
        vec![
            Value::String("Low".into()),
            Value::String("First".into()),
            Value::String("Second".into()),
            Value::String("Third".into()),
        ]
    );

    let dir = TempDir::new("ranked");
    let out = dir.0.join("ranked.mfd");
    let warnings = mfd::export(project, &out).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    let exported = std::fs::read_to_string(&out).unwrap();
    assert!(exported.contains("<key direction=\"descending\"/>"));
    assert_eq!(exported.matches("name=\"first-items\"").count(), 2);
    let reimported = mfd::import(&out).unwrap();
    assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
    let rerun = engine::run(&reimported.project, &source).unwrap();
    assert_eq!(target, rerun);
}

#[test]
fn constructed_variables_preserve_nested_source_frames() {
    let imported = mfd::import(&fixture("framed.mfd")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let project = &imported.project;
    let people_scope = &project.root.children[0];
    assert_eq!(people_scope.target_field, "Person");
    assert_eq!(
        people_scope.source().map(|path| path.to_vec()),
        Some(vec!["Office".into(), "Department".into(), "Person".into()])
    );
    assert!(people_scope.sort_by.is_some());

    let mut name_frames: Vec<_> = project
        .graph
        .nodes
        .values()
        .filter_map(|node| match node {
            Node::SourceField { path, frame } if path == &["Name"] => frame.clone(),
            _ => None,
        })
        .collect();
    name_frames.sort();
    assert_eq!(
        name_frames,
        vec![
            vec![String::from("Office")],
            vec![String::from("Office"), String::from("Department")],
        ]
    );

    let source = format_xml::read(&fixture("framed.xml"), &project.source).unwrap();
    let target = engine::run(project, &source).unwrap();
    let rows = target
        .field("Person")
        .and_then(Instance::as_repeated)
        .unwrap();
    let values: Vec<_> = rows
        .iter()
        .map(|row| (scalar(row, "First"), scalar(row, "Details")))
        .collect();
    assert_eq!(
        values,
        vec![
            (
                Value::String("Amy".into()),
                Value::String("Alpha (West)".into())
            ),
            (
                Value::String("Bob".into()),
                Value::String("Beta (East)".into())
            ),
            (
                Value::String("Zed".into()),
                Value::String("Alpha (West)".into())
            ),
        ]
    );

    let dir = TempDir::new("framed");
    let out = dir.0.join("framed.mfd");
    let warnings = mfd::export(project, &out).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    let reimported = mfd::import(&out).unwrap();
    assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
    let rerun = engine::run(&reimported.project, &source).unwrap();
    assert_eq!(target, rerun);
}

#[test]
fn indexed_xml_entry_names_import_run_and_roundtrip() {
    let imported = mfd::import(&fixture("indexed.mfd")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let project = &imported.project;

    assert!(
        project
            .graph
            .nodes
            .values()
            .filter_map(|node| match node {
                Node::SourceField { path, .. } => Some(path),
                _ => None,
            })
            .flatten()
            .all(|segment| !segment.contains(':') && !segment.starts_with('@'))
    );
    let expense_scope = project
        .root
        .children
        .iter()
        .find(|scope| scope.target_field == "expense-item")
        .unwrap();
    assert_eq!(
        expense_scope.source().map(|path| path.to_vec()),
        Some(vec!["expense-item".into()])
    );
    let filter = expense_scope
        .filter
        .expect("expense scope should be filtered");
    assert!(matches!(
        &project.graph.nodes[&filter],
        Node::Call { function, .. } if function == "less_than"
    ));

    let source = format_xml::read(&fixture("indexed.xml"), &project.source).unwrap();
    let target = engine::run(project, &source).unwrap();
    let person = target.field("Person").unwrap();
    assert_eq!(scalar(person, "Name"), Value::String("Ada".into()));
    let expenses = target
        .field("expense-item")
        .and_then(Instance::as_repeated)
        .unwrap();
    assert_eq!(expenses.len(), 2);
    assert_eq!(scalar(&expenses[0], "amount"), Value::Float(100.0));
    assert_eq!(
        scalar(&expenses[0], "status"),
        Value::String("approved".into())
    );
    assert_eq!(scalar(&expenses[1], "amount"), Value::Float(50.0));
    assert_eq!(scalar(&expenses[1], "status"), Value::String("cash".into()));

    let dir = TempDir::new("indexed");
    let out = dir.0.join("indexed.mfd");
    let warnings = mfd::export(project, &out).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    let reimported = mfd::import(&out).unwrap();
    assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
    let rerun = engine::run(&reimported.project, &source).unwrap();
    assert_eq!(target, rerun);
}

#[test]
fn generic_functions_drop_trailing_optional_pins_but_preserve_interior_defaults() {
    let imported = mfd::import(&fixture("format-number-optional.mfd")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let person = &imported.project.root.children[0];

    let name = person
        .bindings
        .iter()
        .find(|binding| binding.target_field == "Name")
        .unwrap();
    let Node::Call { function, args } = &imported.project.graph.nodes[&name.node] else {
        panic!("Name should be bound to format-number");
    };
    assert_eq!(function, "format_number");
    assert_eq!(args.len(), 2);

    let age = person
        .bindings
        .iter()
        .find(|binding| binding.target_field == "Age")
        .unwrap();
    let Node::Call { function, args } = &imported.project.graph.nodes[&age.node] else {
        panic!("Age should be bound to format-number");
    };
    assert_eq!(function, "format_number");
    assert_eq!(args.len(), 4);
    assert!(matches!(
        &imported.project.graph.nodes[&args[2]],
        Node::Const { value: Value::String(value) } if value == "."
    ));
    assert!(matches!(
        &imported.project.graph.nodes[&args[3]],
        Node::Const { value: Value::String(value) } if value == "_"
    ));
}

#[test]
fn mapped_xpath2_scalar_function_imports_and_runs() {
    let imported = mfd::import(&fixture("xpath2-upper.mfd")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);

    let source = format_xml::read(&fixture("people.xml"), &imported.project.source).unwrap();
    let target = engine::run(&imported.project, &source).unwrap();
    let people = target
        .field("Person")
        .and_then(Instance::as_repeated)
        .unwrap();

    assert_eq!(people.len(), 2);
    assert_eq!(scalar(&people[0], "Name"), Value::String("ALICE".into()));
    assert_eq!(scalar(&people[1], "Name"), Value::String("BO".into()));
}

#[test]
fn json_lines_format_option_survives_mfd_export_and_reimport() {
    let mut project = mfd::import(&fixture("people.mfd")).unwrap().project;
    project.target_path = Some("people.jsonl".into());
    let dir = TempDir::new("json_lines");
    let design = dir.0.join("mapping.mfd");

    let warnings = mfd::export(&project, &design).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    let text = std::fs::read_to_string(&design).unwrap();
    assert!(text.contains("jsonlines=\"1\""), "{text}");

    let reimported = mfd::import(&design).unwrap();
    assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
    assert!(reimported.project.target_options.json_lines);
    assert_eq!(
        reimported.project.target_path.as_deref(),
        Some("people.jsonl")
    );
}

#[test]
fn scalar_document_root_binding_warns_instead_of_panicking() {
    let imported = mfd::import(&fixture("scalar-root-target.mfd")).unwrap();
    assert_eq!(imported.warnings.len(), 1, "{:?}", imported.warnings);
    assert!(
        imported.warnings[0].contains("connection into a scalar document root is not supported"),
        "{:?}",
        imported.warnings
    );
    assert!(imported.project.root.bindings.is_empty());
    assert!(imported.project.graph.nodes.is_empty());
}

#[test]
fn failed_export_preserves_existing_design_and_schema_artifacts() {
    let mut project = mfd::import(&fixture("people.mfd")).unwrap().project;
    let dir = TempDir::new("atomic_export");
    let design = dir.0.join("atomic.mfd");
    mfd::export(&project, &design).unwrap();

    let source_schema = dir.0.join("atomic-source.xsd");
    let target_schema = dir.0.join("atomic-target.xsd");
    let before = [
        std::fs::read(&design).unwrap(),
        std::fs::read(&source_schema).unwrap(),
        std::fs::read(&target_schema).unwrap(),
    ];

    project.source.name = "ChangedSource".into();
    project.target_path = Some("invalid.csv".into());
    assert!(matches!(
        mfd::export(&project, &design),
        Err(mfd::MfdError::Unsupported(message))
            if message.contains("target side maps to a csv file")
    ));

    assert_eq!(std::fs::read(&design).unwrap(), before[0]);
    assert_eq!(std::fs::read(&source_schema).unwrap(), before[1]);
    assert_eq!(std::fs::read(&target_schema).unwrap(), before[2]);
}

#[test]
fn invalid_xml_schema_roles_do_not_publish_export_artifacts() {
    let mut project = mfd::import(&fixture("people.mfd")).unwrap().project;
    project.source = SchemaNode::group(
        "Source",
        vec![
            SchemaNode::scalar(XML_TEXT_FIELD, ScalarType::Int).text(),
            SchemaNode::scalar("Child", ScalarType::String),
        ],
    );
    let dir = TempDir::new("invalid_schema_export");
    let design = dir.0.join("invalid.mfd");

    assert!(matches!(
        mfd::export(&project, &design),
        Err(mfd::MfdError::SchemaExport(
            format_xml::XmlFormatError::MixedContent { .. }
        ))
    ));
    assert!(!design.exists());
    assert!(!dir.0.join("invalid-source.xsd").exists());
    assert!(!dir.0.join("invalid-target.xsd").exists());
}

#[test]
fn failed_design_write_does_not_replace_schema_siblings() {
    let project = mfd::import(&fixture("people.mfd")).unwrap().project;
    let dir = TempDir::new("blocked_design_export");
    let design = dir.0.join("blocked.mfd");
    std::fs::create_dir(&design).unwrap();
    let source_schema = dir.0.join("blocked-source.xsd");
    let target_schema = dir.0.join("blocked-target.xsd");
    std::fs::write(&source_schema, "old source").unwrap();
    std::fs::write(&target_schema, "old target").unwrap();

    assert!(matches!(
        mfd::export(&project, &design),
        Err(mfd::MfdError::Io(_))
    ));
    assert_eq!(
        std::fs::read_to_string(source_schema).unwrap(),
        "old source"
    );
    assert_eq!(
        std::fs::read_to_string(target_schema).unwrap(),
        "old target"
    );
}

#[test]
fn blocked_schema_destination_does_not_publish_other_artifacts() {
    let mut project = mfd::import(&fixture("people.mfd")).unwrap().project;
    let dir = TempDir::new("blocked_schema_export");
    let design = dir.0.join("blocked-schema.mfd");
    mfd::export(&project, &design).unwrap();

    let source_schema = dir.0.join("blocked-schema-source.xsd");
    let target_schema = dir.0.join("blocked-schema-target.xsd");
    let old_design = std::fs::read(&design).unwrap();
    let old_source = std::fs::read(&source_schema).unwrap();
    std::fs::remove_file(&target_schema).unwrap();
    std::fs::create_dir(&target_schema).unwrap();
    project.source.name = "ChangedSource".into();
    project.target.name = "ChangedTarget".into();

    assert!(matches!(
        mfd::export(&project, &design),
        Err(mfd::MfdError::Io(error)) if error.kind() == std::io::ErrorKind::IsADirectory
    ));
    assert_eq!(std::fs::read(&design).unwrap(), old_design);
    assert_eq!(std::fs::read(&source_schema).unwrap(), old_source);
    assert!(target_schema.is_dir());
}
