use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{Instance, ScalarType, SchemaKind, SchemaNode, Value};
use mapping::Node;

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_mfd_target_datetime_cast_{}_{}",
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

fn write_design(directory: &Path, cast_mode: bool) -> PathBuf {
    std::fs::write(
        directory.join("source.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
          <xs:element name="Source"><xs:complexType><xs:sequence>
            <xs:element name="Day" type="xs:string"/>
            <xs:element name="Timestamp" type="xs:string"/>
          </xs:sequence></xs:complexType></xs:element>
        </xs:schema>"#,
    )
    .unwrap();
    std::fs::write(
        directory.join("target.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
          <xs:simpleType name="RecordedAt"><xs:restriction base="xs:dateTime"/></xs:simpleType>
          <xs:element name="Target"><xs:complexType><xs:sequence>
            <xs:element name="Received" type="xs:dateTime"/>
            <xs:element name="Existing" type="RecordedAt"/>
          </xs:sequence></xs:complexType></xs:element>
        </xs:schema>"#,
    )
    .unwrap();

    let cast_attribute = if cast_mode {
        r#" casttotargettypemode="cast-in-subtree""#
    } else {
        ""
    };
    let design = directory.join("mapping.mfd");
    std::fs::write(
        &design,
        format!(
            r#"<mapping version="31"><component name="map"><structure><children>
              <component name="source" library="xml" kind="14"><data>
                <root><entry name="FileInstance"><entry name="document"><entry name="Source"><entry name="Day" outkey="1"/><entry name="Timestamp" outkey="2"/></entry></entry></entry></root>
                <document schema="source.xsd" inputinstance="source.xml" instanceroot="{{}}Source"/>
              </data></component>
              <component name="target" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data>
                <root><entry name="FileInstance"><entry name="document"{cast_attribute}><entry name="Target"><entry name="Received" inpkey="11"/><entry name="Existing" inpkey="12"/></entry></entry></entry></root>
                <document schema="target.xsd" outputinstance="target.xml" instanceroot="{{}}Target"/>
              </data></component>
            </children><graph><vertices>
              <vertex vertexkey="1"><edges><edge vertexkey="11"/></edges></vertex>
              <vertex vertexkey="2"><edges><edge vertexkey="12"/></edges></vertex>
            </vertices></graph></structure></component></mapping>"#
        ),
    )
    .unwrap();
    design
}

fn source() -> Instance {
    Instance::Group(vec![
        (
            "Day".to_string(),
            Instance::Scalar(Value::String("2031-08-17+05:45".to_string())),
        ),
        (
            "Timestamp".to_string(),
            Instance::Scalar(Value::String("2031-08-17T06:07:08.9Z".to_string())),
        ),
    ])
}

fn write_nested_design(directory: &Path) -> PathBuf {
    std::fs::write(
        directory.join("source.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
          <xs:element name="Source"><xs:complexType><xs:sequence>
            <xs:element name="Day" type="xs:string"/>
            <xs:element name="Existing" type="xs:string"/>
            <xs:element name="Row" minOccurs="0" maxOccurs="unbounded"><xs:complexType><xs:sequence>
              <xs:element name="When" type="xs:string"/>
            </xs:sequence></xs:complexType></xs:element>
          </xs:sequence></xs:complexType></xs:element>
        </xs:schema>"#,
    )
    .unwrap();
    std::fs::write(
        directory.join("target.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
          <xs:element name="Target"><xs:complexType><xs:sequence>
            <xs:element name="Header"><xs:complexType><xs:sequence>
              <xs:element name="Received" type="xs:dateTime"/>
            </xs:sequence></xs:complexType></xs:element>
            <xs:element name="Existing" type="xs:string"/>
            <xs:element name="Row" minOccurs="0" maxOccurs="unbounded"><xs:complexType><xs:sequence>
              <xs:element name="Received" type="xs:dateTime"/>
            </xs:sequence></xs:complexType></xs:element>
          </xs:sequence></xs:complexType></xs:element>
        </xs:schema>"#,
    )
    .unwrap();
    let design = directory.join("nested.mfd");
    std::fs::write(
        &design,
        r#"<mapping version="31"><component name="map"><structure><children>
          <component name="source" library="xml" kind="14"><data>
            <root><entry name="FileInstance"><entry name="document"><entry name="Source">
              <entry name="Day" outkey="1"/><entry name="Existing" outkey="2"/>
              <entry name="Row" outkey="3"><entry name="When" outkey="4"/></entry>
            </entry></entry></entry></root>
            <document schema="source.xsd" inputinstance="source.xml" instanceroot="{}Source"/>
          </data></component>
          <component name="target" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data>
            <root><entry name="FileInstance"><entry name="document" casttotargettypemode="cast-in-subtree"><entry name="Target">
              <entry name="Header"><entry name="Received" inpkey="11"/></entry>
              <entry name="Existing" inpkey="12"/>
              <entry name="Row" inpkey="13"><entry name="Received" inpkey="14"/></entry>
            </entry></entry></entry></root>
            <document schema="target.xsd" outputinstance="target.xml" instanceroot="{}Target"/>
          </data></component>
        </children><graph><vertices>
          <vertex vertexkey="1"><edges><edge vertexkey="11"/></edges></vertex>
          <vertex vertexkey="2"><edges><edge vertexkey="12"/></edges></vertex>
          <vertex vertexkey="3"><edges><edge vertexkey="13"/></edges></vertex>
          <vertex vertexkey="4"><edges><edge vertexkey="14"/></edges></vertex>
        </vertices></graph></structure></component></mapping>"#,
    )
    .unwrap();
    design
}

fn nested_source(values: &[&str]) -> Instance {
    Instance::Group(vec![
        (
            "Day".into(),
            Instance::Scalar(Value::String("2031-08-17+05:45".into())),
        ),
        (
            "Existing".into(),
            Instance::Scalar(Value::String("uncast".into())),
        ),
        (
            "Row".into(),
            Instance::Repeated(
                values
                    .iter()
                    .map(|value| {
                        Instance::Group(vec![(
                            "When".into(),
                            Instance::Scalar(Value::String((*value).into())),
                        )])
                    })
                    .collect(),
            ),
        ),
    ])
}

fn nested_cast<'a>(project: &'a mapping::Project, field: &str) -> &'a mapping::Scope {
    project
        .root
        .children
        .iter()
        .find(|scope| scope.target_field == field)
        .unwrap()
}

#[test]
fn nested_and_repeated_datetime_casts_export_with_native_xsd_types() {
    let directory = TempDir::new();
    let design = write_nested_design(&directory.0);
    let imported = mfd::import(&design).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(engine::validate(&imported.project).is_empty());
    for field in ["Header", "Row"] {
        let scope = nested_cast(&imported.project, field);
        assert!(scope.bindings.iter().any(|binding| {
            matches!(
                imported.project.graph.nodes.get(&binding.node),
                Some(Node::Call { function, .. }) if function == "coerce_datetime"
            )
        }));
    }

    let input = nested_source(&["2031-08-17T06:07:08Z", "2031-08-18+05:45"]);
    let expected = engine::run(&imported.project, &input).unwrap();
    let exported = directory.0.join("nested-round-trip.mfd");
    let report = mfd::preflight_export(&imported.project, &exported).unwrap();
    assert!(report.is_native_compatible(), "{report:?}");
    assert!(
        mfd::export_with_profile(&imported.project, &exported, mfd::ExportProfile::NativeMfd)
            .unwrap()
            .is_native_compatible()
    );
    let rendered = std::fs::read_to_string(&exported).unwrap();
    let document = roxmltree::Document::parse(&rendered).unwrap();
    let target = document
        .descendants()
        .find(|node| node.has_tag_name("component") && node.attribute("name") == Some("Target"))
        .unwrap();
    assert_eq!(
        target
            .descendants()
            .find(|node| node.has_tag_name("entry") && node.attribute("name") == Some("document"))
            .and_then(|node| node.attribute("casttotargettypemode")),
        Some("cast-in-subtree")
    );
    let schema_file = target
        .descendants()
        .find(|node| node.has_tag_name("document"))
        .and_then(|node| node.attribute("schema"))
        .unwrap();
    let xsd = std::fs::read_to_string(directory.0.join(schema_file)).unwrap();
    let xsd_doc = roxmltree::Document::parse(&xsd).unwrap();
    let received = xsd_doc
        .descendants()
        .filter(|node| node.has_tag_name("element") && node.attribute("name") == Some("Received"))
        .collect::<Vec<_>>();
    assert_eq!(received.len(), 2);
    assert!(
        received
            .iter()
            .all(|node| node.attribute("type") == Some("xs:dateTime"))
    );
    assert_eq!(
        xsd_doc
            .descendants()
            .find(|node| node.has_tag_name("element") && node.attribute("name") == Some("Existing"))
            .and_then(|node| node.attribute("type")),
        Some("xs:string")
    );

    let reimported = mfd::import(&exported).unwrap();
    assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
    assert_eq!(engine::run(&reimported.project, &input).unwrap(), expected);
    let bad = nested_source(&["invalid datetime"]);
    for project in [&imported.project, &reimported.project] {
        let error = engine::run(project, &bad).unwrap_err().to_string();
        assert!(error.contains("coerce_datetime"), "{error}");
    }
}

#[test]
fn nested_datetime_cast_shared_with_filter_stays_non_native() {
    let directory = TempDir::new();
    let mut project = mfd::import(&write_nested_design(&directory.0))
        .unwrap()
        .project;
    let call = nested_cast(&project, "Row").bindings[0].node;
    let filter = project.graph.nodes.keys().next_back().unwrap() + 1;
    project.graph.nodes.insert(
        filter,
        Node::Call {
            function: "exists".into(),
            args: vec![call],
        },
    );
    project
        .root
        .children
        .iter_mut()
        .find(|scope| scope.target_field == "Row")
        .unwrap()
        .filter = Some(filter);
    let rejected = directory.0.join("not-created/rejected.mfd");
    let report = mfd::preflight_export(&project, &rejected).unwrap();
    assert!(!report.is_native_compatible(), "{report:?}");
    assert!(
        report
            .issues
            .iter()
            .any(|issue| issue.component == "coerce_datetime")
    );
    assert!(!rejected.parent().unwrap().exists());
}

#[test]
fn nested_datetime_cast_with_multiple_binding_owners_stays_non_native() {
    let directory = TempDir::new();
    let mut project = mfd::import(&write_nested_design(&directory.0))
        .unwrap()
        .project;
    let header = project
        .root
        .children
        .iter_mut()
        .find(|scope| scope.target_field == "Header")
        .unwrap();
    header.bindings.push(header.bindings[0].clone());
    let rejected = directory.0.join("not-created/rejected.mfd");
    let report = mfd::preflight_export(&project, &rejected).unwrap();
    assert!(!report.is_native_compatible(), "{report:?}");
    assert!(
        report
            .issues
            .iter()
            .any(|issue| issue.component == "coerce_datetime")
    );
    assert!(!rejected.parent().unwrap().exists());
}

#[test]
#[ignore = "needs the local ignored ReferenceSamples corpus; informational only"]
fn local_idoc_order_keeps_edi_native_export_blockers() {
    let sample =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../samples/ReferenceSamples/IDoc_Order.mfd");
    if !sample.is_file() {
        return;
    }
    let imported = mfd::import(&sample).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(imported.project.graph.nodes.values().any(|node| {
        matches!(node, Node::Call { function, .. } if function == "coerce_datetime")
    }));
    let directory = TempDir::new();
    let report = mfd::preflight_export(&imported.project, &directory.0.join("idoc.mfd")).unwrap();
    assert!(!report.is_native_compatible());
    assert!(
        report
            .issues
            .iter()
            .any(|issue| issue.feature == mfd::ExportCompatibilityFeature::EdiSchema)
    );
    assert!(
        report
            .issues
            .iter()
            .any(|issue| issue.feature == mfd::ExportCompatibilityFeature::EdiLayout)
    );
    assert!(
        !report
            .issues
            .iter()
            .any(|issue| issue.component == "coerce_datetime"),
        "{report:?}"
    );
}

#[test]
fn cast_in_subtree_coerces_connected_datetime_leaves_and_serializes_them() {
    let directory = TempDir::new();
    let design = write_design(&directory.0, true);
    let imported = mfd::import(&design).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(engine::validate(&imported.project).is_empty());

    for field in ["Received", "Existing"] {
        let binding = imported
            .project
            .root
            .bindings
            .iter()
            .find(|binding| binding.target_field == field)
            .unwrap();
        assert!(matches!(
            imported.project.graph.nodes.get(&binding.node),
            Some(Node::Call { function, .. }) if function == "coerce_datetime"
        ));
    }

    let output = engine::run(&imported.project, &source()).unwrap();
    assert_eq!(
        output.field("Received").and_then(Instance::as_scalar),
        Some(&Value::String("2031-08-17T00:00:00+05:45".to_string()))
    );
    assert_eq!(
        output.field("Existing").and_then(Instance::as_scalar),
        Some(&Value::String("2031-08-17T06:07:08.9Z".to_string()))
    );
    let xml = format_xml::to_string(&imported.project.target, &output).unwrap();
    assert!(xml.contains("<Received>2031-08-17T00:00:00+05:45</Received>"));
    assert!(xml.contains("<Existing>2031-08-17T06:07:08.9Z</Existing>"));

    let exported = directory.0.join("round-trip.mfd");
    let report = mfd::preflight_export(&imported.project, &exported).unwrap();
    assert!(report.is_native_compatible(), "{report:?}");
    assert!(!exported.exists());
    let published =
        mfd::export_with_profile(&imported.project, &exported, mfd::ExportProfile::NativeMfd)
            .unwrap();
    assert!(published.is_native_compatible(), "{published:?}");
    let design_xml = std::fs::read_to_string(&exported).unwrap();
    let document = roxmltree::Document::parse(&design_xml).unwrap();
    assert!(!document.descendants().any(|node| {
        node.has_tag_name("component") && node.attribute("library") == Some("ferrule")
    }));
    let target = document
        .descendants()
        .find(|node| node.has_tag_name("component") && node.attribute("name") == Some("Target"))
        .unwrap();
    let target_document = target
        .descendants()
        .find(|node| node.has_tag_name("entry") && node.attribute("name") == Some("document"))
        .unwrap();
    assert_eq!(
        target_document.attribute("casttotargettypemode"),
        Some("cast-in-subtree")
    );
    let schema_file = target
        .descendants()
        .find(|node| node.has_tag_name("document"))
        .and_then(|node| node.attribute("schema"))
        .unwrap();
    let schema_xml = std::fs::read_to_string(directory.0.join(schema_file)).unwrap();
    let schema = roxmltree::Document::parse(&schema_xml).unwrap();
    for field in ["Received", "Existing"] {
        assert_eq!(
            schema
                .descendants()
                .find(|node| node.has_tag_name("element") && node.attribute("name") == Some(field))
                .and_then(|node| node.attribute("type")),
            Some("xs:dateTime"),
            "{field}"
        );
    }
    let reimported = mfd::import(&exported).unwrap();
    assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
    let round_trip = engine::run(&reimported.project, &source()).unwrap();
    assert_eq!(round_trip, output);

    let null_day = Instance::Group(vec![
        ("Day".into(), Instance::Scalar(Value::Null)),
        (
            "Timestamp".into(),
            Instance::Scalar(Value::String("2031-08-17T06:07:08.9Z".into())),
        ),
    ]);
    assert_eq!(
        engine::run(&imported.project, &null_day).unwrap(),
        engine::run(&reimported.project, &null_day).unwrap()
    );
    let bad_day = Instance::Group(vec![
        (
            "Day".into(),
            Instance::Scalar(Value::String("2031-02-29".into())),
        ),
        (
            "Timestamp".into(),
            Instance::Scalar(Value::String("2031-08-17T06:07:08.9Z".into())),
        ),
    ]);
    for project in [&imported.project, &reimported.project] {
        let error = engine::run(project, &bad_day).unwrap_err().to_string();
        assert!(error.contains("coerce_datetime"), "{error}");
    }
}

#[test]
fn shared_datetime_cast_stays_an_extension_and_native_export_writes_nothing() {
    let directory = TempDir::new();
    let design = write_design(&directory.0, true);
    let mut project = mfd::import(&design).unwrap().project;
    let received = project
        .root
        .bindings
        .iter()
        .find(|binding| binding.target_field == "Received")
        .unwrap()
        .node;
    project
        .root
        .bindings
        .iter_mut()
        .find(|binding| binding.target_field == "Existing")
        .unwrap()
        .node = received;
    let rejected = directory.0.join("not-created/rejected.mfd");
    let report = mfd::preflight_export(&project, &rejected).unwrap();
    assert!(!report.is_native_compatible(), "{report:?}");
    assert!(
        report
            .issues
            .iter()
            .any(|issue| issue.component == "coerce_datetime")
    );
    let error = mfd::export_with_profile(&project, &rejected, mfd::ExportProfile::NativeMfd)
        .unwrap_err()
        .to_string();
    assert!(error.contains("coerce_datetime"), "{error}");
    assert!(!rejected.parent().unwrap().exists());
}

#[test]
fn datetime_cast_with_defaulted_sibling_rejects_native_export_without_artifacts() {
    let directory = TempDir::new();
    let design = write_design(&directory.0, true);
    let mut project = mfd::import(&design).unwrap().project;
    let SchemaKind::Group { children, .. } = &mut project.target.kind else {
        panic!("target must be a group");
    };
    let mut sibling = SchemaNode::scalar("Other", ScalarType::String);
    sibling.default = Some("untouched".into());
    children.push(sibling);

    let rejected = directory.0.join("not-created/rejected.mfd");
    let report = mfd::preflight_export(&project, &rejected).unwrap();
    assert!(!report.is_native_compatible(), "{report:?}");
    assert!(
        report
            .issues
            .iter()
            .any(|issue| issue.component == "coerce_datetime")
    );
    let error = mfd::export_with_profile(&project, &rejected, mfd::ExportProfile::NativeMfd)
        .unwrap_err()
        .to_string();
    assert!(error.contains("coerce_datetime"), "{error}");
    assert!(!rejected.parent().unwrap().exists());
}

#[test]
fn target_without_cast_mode_preserves_the_connected_lexical_value() {
    let directory = TempDir::new();
    let imported = mfd::import(&write_design(&directory.0, false)).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let output = engine::run(&imported.project, &source()).unwrap();
    assert_eq!(
        output.field("Received").and_then(Instance::as_scalar),
        Some(&Value::String("2031-08-17+05:45".to_string()))
    );
}

#[test]
fn cast_in_subtree_reads_utf16_target_schema_metadata() {
    let directory = TempDir::new();
    let design = write_design(&directory.0, true);
    let target_schema = directory.0.join("target.xsd");
    let text = std::fs::read_to_string(&target_schema).unwrap();
    let mut bytes = vec![0xff, 0xfe];
    for unit in text.encode_utf16() {
        bytes.extend(unit.to_le_bytes());
    }
    std::fs::write(target_schema, bytes).unwrap();

    let imported = mfd::import(&design).unwrap();

    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let received = imported
        .project
        .root
        .bindings
        .iter()
        .find(|binding| binding.target_field == "Received")
        .unwrap();
    assert!(matches!(
        imported.project.graph.nodes.get(&received.node),
        Some(Node::Call { function, .. }) if function == "coerce_datetime"
    ));
}

#[test]
fn cast_in_subtree_accepts_dtd_without_xsd_type_metadata() {
    let directory = TempDir::new();
    let design = write_design(&directory.0, true);
    std::fs::write(
        directory.0.join("target.dtd"),
        r#"<!ELEMENT Target (Received, Existing)>
           <!ELEMENT Received (#PCDATA)>
           <!ELEMENT Existing (#PCDATA)>"#,
    )
    .unwrap();
    let mapping = std::fs::read_to_string(&design)
        .unwrap()
        .replace("schema=\"target.xsd\"", "schema=\"target.dtd\"");
    std::fs::write(&design, mapping).unwrap();

    let imported = mfd::import(&design).unwrap();

    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let received = imported
        .project
        .root
        .bindings
        .iter()
        .find(|binding| binding.target_field == "Received")
        .unwrap();
    assert!(!matches!(
        imported.project.graph.nodes.get(&received.node),
        Some(Node::Call { function, .. }) if function == "coerce_datetime"
    ));
}

#[test]
fn relocated_package_resolves_windows_parent_target_cast_schema()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = TempDir::new();
    let original = directory.0.join("original");
    let maps = original.join("maps/orders");
    let schemas = original.join("schemas");
    std::fs::create_dir_all(&maps)?;
    std::fs::create_dir_all(&schemas)?;
    let design = write_design(&maps, true);
    std::fs::rename(maps.join("target.xsd"), schemas.join("target.xsd"))?;
    let mapping = std::fs::read_to_string(&design)?.replace(
        "schema=\"target.xsd\"",
        "schema=\"..\\..\\schemas\\target.xsd\"",
    );
    std::fs::write(&design, mapping)?;

    let relocated = directory.0.join("relocated");
    std::fs::rename(&original, &relocated)?;
    let relocated_design = relocated.join(design.strip_prefix(&original)?);
    let options = mfd::ImportOptions::default().with_package_root(&relocated);
    let imported = mfd::import_with_options(&relocated_design, &options)?;

    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let received = imported
        .project
        .root
        .bindings
        .iter()
        .find(|binding| binding.target_field == "Received")
        .ok_or("Received target binding is missing")?;
    assert!(matches!(
        imported.project.graph.nodes.get(&received.node),
        Some(Node::Call { function, .. }) if function == "coerce_datetime"
    ));
    Ok(())
}

#[cfg(unix)]
#[test]
fn target_cast_schema_symlink_escape_falls_back_without_reopening_it()
-> Result<(), Box<dyn std::error::Error>> {
    use std::os::unix::fs::symlink;

    let directory = TempDir::new();
    let package = directory.0.join("package");
    let maps = package.join("maps/orders");
    let schemas = package.join("schemas");
    std::fs::create_dir_all(&maps)?;
    std::fs::create_dir_all(&schemas)?;
    let design = write_design(&maps, true);
    let outside_schema = directory.0.join("outside-target.xsd");
    std::fs::rename(maps.join("target.xsd"), &outside_schema)?;
    symlink(&outside_schema, schemas.join("target.xsd"))?;
    let mapping = std::fs::read_to_string(&design)?.replace(
        "schema=\"target.xsd\"",
        "schema=\"..\\..\\schemas\\target.xsd\"",
    );
    std::fs::write(&design, mapping)?;

    let options = mfd::ImportOptions::default().with_package_root(&package);
    let imported = mfd::import_with_options(&design, &options)?;

    assert!(imported.warnings.iter().any(|warning| {
        warning.contains("target cast-in-subtree metadata")
            && warning.contains("resolves outside package root")
    }));
    let received = imported
        .project
        .root
        .bindings
        .iter()
        .find(|binding| binding.target_field == "Received")
        .ok_or("Received target binding is missing")?;
    assert!(!matches!(
        imported.project.graph.nodes.get(&received.node),
        Some(Node::Call { function, .. }) if function == "coerce_datetime"
    ));
    let output = engine::run(&imported.project, &source())?;
    assert_eq!(
        output.field("Received").and_then(Instance::as_scalar),
        Some(&Value::String("2031-08-17+05:45".to_string()))
    );
    Ok(())
}
