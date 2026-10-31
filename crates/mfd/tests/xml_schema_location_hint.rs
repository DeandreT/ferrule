//! Import, roles and native round trip of the default output schema-location hint.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{Instance, Value, XmlSchemaHints};

const XSI: &str = "http://www.w3.org/2001/XMLSchema-instance";

struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule-xml-schema-location-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn write(&self, name: &str, contents: &str) -> PathBuf {
        let path = self.0.join(name);
        std::fs::write(&path, contents).unwrap();
        path
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

const SCHEMA: &str = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Envelope"><xs:complexType><xs:sequence><xs:element name="Amount" type="xs:long"/><xs:element name="Label" type="xs:string"/></xs:sequence></xs:complexType></xs:element></xs:schema>"#;

/// One unqualified source copied into one target. `extra_entry` adds a target
/// root entry fed by the Label source port.
fn design(target_document: &str, properties: &str, extra_entry: &str) -> String {
    let extra_edge = if extra_entry.is_empty() {
        ""
    } else {
        r#"<edge vertexkey="5"/>"#
    };
    format!(
        r#"<mapping version="22"><component name="defaultmap" uid="1"><properties SelectedLanguage="builtin"/><structure><children>
<component name="Source" library="xml" uid="2" kind="14"><data><root><header><namespaces><namespace/><namespace uid="http://www.altova.com/mapforce"/></namespaces></header><entry name="FileInstance" ns="1"><entry name="document" ns="1"><entry name="Envelope"><entry name="Amount" outkey="1"/><entry name="Label" outkey="2"/></entry></entry></entry></root><document schema="source.xsd" instanceroot="{{}}Envelope" inputinstance="input.xml"/></data></component>
<component name="Target" library="xml" uid="3" kind="14">{properties}<data><root><header><namespaces><namespace/><namespace uid="http://www.altova.com/mapforce"/><namespace uid="{XSI}"/></namespaces></header><entry name="FileInstance" ns="1"><entry name="document" ns="1"><entry name="Envelope"><entry name="Amount" inpkey="3"/><entry name="Label" inpkey="4"/>{extra_entry}</entry></entry></entry></root>{target_document}</data></component>
</children><graph directed="1"><edges/><vertices><vertex vertexkey="1"><edges><edge vertexkey="3"/></edges></vertex><vertex vertexkey="2"><edges><edge vertexkey="4"/>{extra_edge}</edges></vertex></vertices></graph></structure></component></mapping>"#
    )
}

fn hint(location: &str) -> Option<XmlSchemaHints> {
    Some(XmlSchemaHints {
        no_namespace_location: Some(location.into()),
        derivation: Some(ir::XmlSchemaHintDerivation {
            schema_reference: location.into(),
        }),
        ..Default::default()
    })
}

fn executable(path: &Path) -> mapping::Project {
    let outcome = mfd::import_with_profile(
        path,
        &mfd::ImportOptions::default(),
        mfd::ImportProfile::Executable,
    )
    .unwrap();
    assert!(
        outcome.imported.warnings.is_empty(),
        "{:?}",
        outcome.imported.warnings
    );
    assert!(engine::validate(&outcome.imported.project).is_empty());
    outcome.imported.project
}

fn written(project: &mapping::Project, input: &str) -> String {
    let source = format_xml::from_str(input, &project.source).unwrap();
    let output = engine::run(project, &source).unwrap();
    format_xml::to_string_with_options(
        &project.target,
        &output,
        &format_xml::XmlWriteOptions {
            scalar_output: project.target_options.xml_scalar_output.clone(),
            schema_hints: project.target_options.xml_schema_hints.clone(),
            ..Default::default()
        },
    )
    .unwrap()
}

const INPUT: &str = "<Envelope><Amount>7</Amount><Label>kept</Label></Envelope>";

#[test]
fn dtd_targets_write_no_hint() {
    let directory = Directory::new();
    directory.write("source.xsd", SCHEMA);
    directory.write(
        "target.dtd",
        "<!ELEMENT Envelope (Amount, Label)>\n<!ELEMENT Amount (#PCDATA)>\n<!ELEMENT Label (#PCDATA)>\n",
    );
    let document =
        r#"<document schema="target.dtd" instanceroot="{}Envelope" outputinstance="output.xml"/>"#;
    let absent = mfd::import(&directory.write(
        "absent.mfd",
        &design(document, r#"<properties XSLTDefaultOutput="1"/>"#, ""),
    ))
    .unwrap();
    let disabled = mfd::import(&directory.write(
        "disabled.mfd",
        &design(
            document,
            r#"<properties XSLTDefaultOutput="1" AddSchemaLocation="0"/>"#,
            "",
        ),
    ))
    .unwrap();
    assert_eq!(absent.project.target_options.xml_schema_hints, None);
    assert_eq!(
        serde_json::to_value(&absent.project).unwrap(),
        serde_json::to_value(&disabled.project).unwrap()
    );
    assert_eq!(absent.warnings, disabled.warnings);
}

#[test]
fn declared_root_xsi_attribute_keeps_its_mapped_value_without_an_automatic_hint() {
    let directory = Directory::new();
    directory.write("source.xsd", SCHEMA);
    directory.write(
        "xsi.xsd",
        &format!(
            r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema" targetNamespace="{XSI}"><xs:attribute name="noNamespaceSchemaLocation" type="xs:anyURI"/></xs:schema>"#
        ),
    );
    directory.write(
        "target.xsd",
        &format!(
            r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema" xmlns:xsi="{XSI}"><xs:import namespace="{XSI}" schemaLocation="xsi.xsd"/><xs:element name="Envelope"><xs:complexType><xs:sequence><xs:element name="Amount" type="xs:long"/><xs:element name="Label" type="xs:string"/></xs:sequence><xs:attribute ref="xsi:noNamespaceSchemaLocation"/></xs:complexType></xs:element></xs:schema>"#
        ),
    );
    let path = directory.write(
        "mapping.mfd",
        &design(
            r#"<document schema="target.xsd" instanceroot="{}Envelope" outputinstance="output.xml"/>"#,
            r#"<properties XSLTDefaultOutput="1"/>"#,
            r#"<entry name="noNamespaceSchemaLocation" ns="2" type="attribute" inpkey="5"/>"#,
        ),
    );
    let project = executable(&path);
    assert_eq!(project.target_options.xml_schema_hints, None);
    let xml = written(&project, INPUT);
    let document = roxmltree::Document::parse(&xml).unwrap();
    assert_eq!(
        document
            .root_element()
            .attribute((XSI, "noNamespaceSchemaLocation")),
        Some("kept"),
        "{xml}"
    );
}

#[test]
fn imported_default_reexports_the_default_and_reimports_the_exported_schema_hint() {
    let directory = Directory::new();
    directory.write("source.xsd", SCHEMA);
    directory.write("target.xsd", SCHEMA);
    let document =
        r#"<document schema="target.xsd" instanceroot="{}Envelope" outputinstance="output.xml"/>"#;
    for (setting, imported_hint, exported_setting) in [
        ("", hint("target.xsd"), None),
        (r#" AddSchemaLocation="1""#, hint("target.xsd"), None),
        (r#" AddSchemaLocation="0""#, None, Some("0")),
    ] {
        let path = directory.write(
            "mapping.mfd",
            &design(
                document,
                &format!(r#"<properties XSLTDefaultOutput="1"{setting}/>"#),
                "",
            ),
        );
        let imported = executable(&path);
        assert_eq!(imported.target_options.xml_schema_hints, imported_hint);
        let exported = directory.0.join("export/roundtrip.mfd");
        let report =
            mfd::export_with_profile(&imported, &exported, mfd::ExportProfile::NativeMfd).unwrap();
        assert!(report.is_native_compatible(), "{report}");
        let text = std::fs::read_to_string(&exported).unwrap();
        let design = roxmltree::Document::parse(&text).unwrap();
        let properties = design
            .descendants()
            .find(|node| {
                node.has_tag_name("component")
                    && node.attribute("library") == Some("xml")
                    && node
                        .descendants()
                        .any(|entry| entry.attribute("inpkey").is_some())
            })
            .and_then(|target| {
                target
                    .children()
                    .find(|node| node.has_tag_name("properties"))
            });
        assert_eq!(
            properties.and_then(|node| node.attribute("AddSchemaLocation")),
            exported_setting,
            "{setting}"
        );
        let reimported = executable(&exported);
        assert_eq!(
            reimported.target_options.xml_schema_hints,
            imported_hint.as_ref().and(hint("roundtrip-target.xsd")),
            "{setting}"
        );
        assert_eq!(
            written(&reimported, INPUT).replace("roundtrip-target.xsd", "target.xsd"),
            written(&imported, INPUT)
        );
        std::fs::remove_dir_all(directory.0.join("export")).unwrap();
    }
}

#[test]
fn pass_through_components_carry_the_hint_only_in_their_target_role() {
    let directory = Directory::new();
    let schema = |root: &str| SCHEMA.replace("\"Envelope\"", &format!("\"{root}\""));
    directory.write("source.xsd", &schema("Source"));
    directory.write("buffer.xsd", &schema("Buffer"));
    directory.write("target.xsd", &schema("Target"));
    let path = directory.write(
        "mapping.mfd",
        r#"<mapping version="26"><component name="map"><structure><children>
          <component name="source" library="xml" kind="14"><data><root><entry name="Source"><entry name="Amount" outkey="10"/><entry name="Label" outkey="11"/></entry></root><document schema="source.xsd" inputinstance="source.xml" instanceroot="{}Source"/></data></component>
          <component name="buffer" library="xml" kind="14"><properties PassThrough="1"/><data><root><entry name="Buffer"><entry name="Amount" inpkey="20" outkey="30"/><entry name="Label" inpkey="21" outkey="31"/></entry></root><document schema="buffer.xsd" instanceroot="{}Buffer"/></data></component>
          <component name="target" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data><root><entry name="Target"><entry name="Amount" inpkey="40"/><entry name="Label" inpkey="41"/></entry></root><document schema="target.xsd" outputinstance="target.xml" instanceroot="{}Target"/></data></component>
        </children><graph><vertices><vertex vertexkey="10"><edges><edge vertexkey="20"/></edges></vertex><vertex vertexkey="11"><edges><edge vertexkey="21"/></edges></vertex><vertex vertexkey="30"><edges><edge vertexkey="40"/></edges></vertex><vertex vertexkey="31"><edges><edge vertexkey="41"/></edges></vertex></vertices></graph></structure></component></mapping>"#,
    );
    let imported = mfd::import(&path).unwrap();
    assert_eq!(
        imported.project.target_options.xml_schema_hints,
        hint("target.xsd")
    );
    let [buffer] = imported.project.extra_targets.as_slice() else {
        panic!("expected the pass-through component as an additional target");
    };
    assert_eq!(buffer.options.xml_schema_hints, hint("buffer.xsd"));
    assert!(engine::validate(&imported.project).is_empty());

    let pipeline = mfd::import_pipeline(&path).unwrap();
    assert!(pipeline.warnings.is_empty(), "{:?}", pipeline.warnings);
    let [first, second] = pipeline.pipeline.stages.as_slice() else {
        panic!("expected two stages");
    };
    assert_eq!(
        first.project.target_options.xml_schema_hints,
        hint("buffer.xsd")
    );
    assert_eq!(second.project.source_options.xml_schema_hints, None);
    assert_eq!(
        second.project.target_options.xml_schema_hints,
        hint("target.xsd")
    );
    assert!(engine::validate_pipeline(&pipeline.pipeline).is_empty());
    let source = Instance::Group(
        vec![
            ("Amount".to_string(), Instance::Scalar(Value::Int(7))),
            (
                "Label".to_string(),
                Instance::Scalar(Value::String("kept".into())),
            ),
        ]
        .into(),
    );
    let piped = engine::run_pipeline(
        &pipeline.pipeline,
        &BTreeMap::from([("source".to_string(), source)]),
    )
    .unwrap();
    assert_eq!(
        piped
            .stage("mfd-stage-2")
            .unwrap()
            .primary
            .field("Label")
            .and_then(Instance::as_scalar),
        Some(&Value::String("kept".into()))
    );
}

#[test]
fn wildcard_preview_input_on_a_target_keeps_its_hint() {
    let directory = Directory::new();
    directory.write("source.xsd", SCHEMA);
    directory.write("target.xsd", SCHEMA);
    let path = directory.write(
        "mapping.mfd",
        &design(
            r#"<document schema="target.xsd" instanceroot="{}Envelope" inputinstance="prior*.xml" outputinstance="output.xml"/>"#,
            r#"<properties XSLTDefaultOutput="1"/>"#,
            "",
        ),
    );
    let project = executable(&path);
    assert!(!project.target_options.local_xml_file_set);
    assert_eq!(project.target_options.xml_schema_hints, hint("target.xsd"));
}

/// A root `anyAttribute` fed by the mapping can carry an `xsi` attribute at run
/// time, so the mapped value wins over the automatic hint.
#[test]
fn mapped_root_attribute_wildcard_keeps_its_value_without_an_automatic_hint() {
    let directory = Directory::new();
    directory.write(
        "source.xsd",
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Envelope"><xs:complexType><xs:sequence><xs:element name="Label" type="xs:string"/><xs:element name="Name" type="xs:string"/><xs:element name="Ns" type="xs:string"/><xs:element name="Value" type="xs:string"/></xs:sequence></xs:complexType></xs:element></xs:schema>"#,
    );
    directory.write(
        "target.xsd",
        r###"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Envelope"><xs:complexType><xs:sequence><xs:element name="Label" type="xs:string"/></xs:sequence><xs:anyAttribute namespace="##any" processContents="skip"/></xs:complexType></xs:element></xs:schema>"###,
    );
    let mapping = |wildcard_edges: &str| {
        format!(
            r##"<mapping version="22"><component name="defaultmap" uid="1"><structure><children>
<component name="Source" library="xml" uid="2" kind="14"><data><root><entry name="FileInstance"><entry name="document"><entry name="Envelope"><entry name="Label" outkey="1"/><entry name="Name" outkey="2"/><entry name="Ns" outkey="3"/><entry name="Value" outkey="4"/></entry></entry></entry></root><document schema="source.xsd" instanceroot="{{}}Envelope" inputinstance="input.xml"/></data></component>
<component name="Target" library="xml" uid="3" kind="14"><properties XSLTDefaultOutput="1"/><data><root><entry name="FileInstance"><entry name="document"><entry name="Envelope"><entry name="Label" inpkey="11"/><entry name="attribute()" inpkey="15"><entry name="LocalName" inpkey="12"/><entry name="NamespaceURI" inpkey="13"/><entry name="#text" inpkey="14"/></entry></entry></entry></entry></root><document schema="target.xsd" instanceroot="{{}}Envelope" outputinstance="output.xml"/></data></component>
</children><graph directed="1"><vertices><vertex vertexkey="1"><edges><edge vertexkey="11"/></edges></vertex>{wildcard_edges}</vertices></graph></structure></component></mapping>"##
        )
    };
    let input = format!(
        "<Envelope><Label>kept</Label><Name>noNamespaceSchemaLocation</Name><Ns>{XSI}</Ns><Value>mapped.xsd</Value></Envelope>"
    );
    let mapped = executable(&directory.write(
        "mapped.mfd",
        &mapping(
            r#"<vertex vertexkey="2"><edges><edge vertexkey="12"/></edges></vertex><vertex vertexkey="3"><edges><edge vertexkey="13"/></edges></vertex><vertex vertexkey="4"><edges><edge vertexkey="14"/></edges></vertex>"#,
        ),
    ));
    assert_eq!(mapped.target_options.xml_schema_hints, None);
    let xml = written(&mapped, &input);
    let document = roxmltree::Document::parse(&xml).unwrap();
    assert_eq!(
        document
            .root_element()
            .attribute((XSI, "noNamespaceSchemaLocation")),
        Some("mapped.xsd"),
        "{xml}"
    );
    // An unconnected wildcard writes no attributes; the default hint stays.
    let unmapped = executable(&directory.write("unmapped.mfd", &mapping("")));
    assert_eq!(unmapped.target_options.xml_schema_hints, hint("target.xsd"));
}

/// A root declared in an imported schema is not described by the stored
/// schema, so no namespace/location pair points at the wrong file. The
/// namespace comparison itself is covered by the `schema_location` unit tests.
#[test]
fn root_declared_in_an_imported_schema_gets_no_hint() {
    let directory = Directory::new();
    directory.write("source.xsd", SCHEMA);
    directory.write(
        "root.xsd",
        &SCHEMA.replace(
            "<xs:schema xmlns:xs=\"http://www.w3.org/2001/XMLSchema\">",
            "<xs:schema xmlns:xs=\"http://www.w3.org/2001/XMLSchema\" targetNamespace=\"urn:root\" elementFormDefault=\"qualified\">",
        ),
    );
    directory.write(
        "wrapper.xsd",
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema" targetNamespace="urn:wrapper"><xs:import namespace="urn:root" schemaLocation="root.xsd"/></xs:schema>"#,
    );
    let target = |schema: &str| {
        format!(
            r#"<document schema="{schema}" instanceroot="{{urn:root}}Envelope" outputinstance="output.xml"/>"#
        )
    };
    let properties = r#"<properties XSLTDefaultOutput="1"/>"#;
    let wrapped = mfd::import(&directory.write(
        "wrapped.mfd",
        &design(&target("wrapper.xsd"), properties, ""),
    ))
    .unwrap();
    // The wrapper does not declare the root itself, so its namespace cannot
    // describe the root: the reader falls back and no hint names the wrapper.
    assert!(
        wrapped
            .warnings
            .iter()
            .any(|warning| warning.contains("could not read schema `wrapper.xsd`")),
        "{:?}",
        wrapped.warnings
    );
    assert_eq!(wrapped.project.target_options.xml_schema_hints, None);
    // The schema that declares the root gets the observed pair.
    let direct =
        mfd::import(&directory.write("direct.mfd", &design(&target("root.xsd"), properties, "")))
            .unwrap();
    let pair = direct.project.target_options.xml_schema_hints.unwrap();
    assert_eq!(pair.no_namespace_location, None);
    assert_eq!(
        pair.locations,
        [ir::XmlSchemaLocation {
            namespace: "urn:root".into(),
            location: "root.xsd".into(),
        }]
    );
}
