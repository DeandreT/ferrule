//! `.mfd` designs over XSD 1.1 wildcard exclusions: the frozen E1 and A1
//! component schemas import executably, refuse excluded names at run time,
//! and export and re-import without semantic drift.

use std::error::Error;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use format_xml::XmlFormatError;
use ir::{Instance, Value};
use mapping::Project;

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> std::io::Result<Self> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule-mfd-wildcard-exclusion-import-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path)?;
        Ok(Self(path))
    }

    fn write(&self, name: &str, contents: &str) -> std::io::Result<PathBuf> {
        let path = self.0.join(name);
        std::fs::write(&path, contents)?;
        Ok(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

const T: &str = "urn:ferrule:wx:t";

const A_XSD: &str = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
    targetNamespace="urn:ferrule:wx:a" elementFormDefault="qualified">
  <xs:element name="Known" type="xs:string"/>
  <xs:element name="Note" type="xs:string"/>
  <xs:element name="Secret" type="xs:string"/>
  <xs:attribute name="flag" type="xs:string"/>
</xs:schema>"#;

/// E1: negated namespaces with one explicit name on a `lax` wildcard.
const E1_XSD: &str = r###"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
    xmlns:t="urn:ferrule:wx:t" xmlns:a="urn:ferrule:wx:a" xmlns:b="urn:ferrule:wx:b"
    targetNamespace="urn:ferrule:wx:t" elementFormDefault="qualified">
  <xs:import namespace="urn:ferrule:wx:a" schemaLocation="a.xsd"/>
  <xs:element name="Root"><xs:complexType><xs:sequence>
    <xs:element name="Head" type="xs:string"/>
    <xs:any xmlns:a="urn:ferrule:wx:a" notNamespace="##targetNamespace ##local"
            notQName="a:Secret" processContents="lax"
            minOccurs="0" maxOccurs="unbounded"/>
  </xs:sequence></xs:complexType></xs:element>
</xs:schema>"###;

/// A1: attribute exclusions, including `##defined`.
const A1_XSD: &str = r###"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
    xmlns:t="urn:ferrule:wx:t" xmlns:a="urn:ferrule:wx:a" xmlns:b="urn:ferrule:wx:b"
    targetNamespace="urn:ferrule:wx:t" elementFormDefault="qualified">
  <xs:import namespace="urn:ferrule:wx:a" schemaLocation="a.xsd"/>
  <xs:element name="Root"><xs:complexType>
    <xs:attribute name="id" type="xs:string"/>
    <xs:anyAttribute notNamespace="##targetNamespace"
                     notQName="##defined id internal xml:space"
                     processContents="skip"/>
  </xs:complexType></xs:element>
</xs:schema>"###;

fn component(name: &str, uid: u32, entries: &str, document: &str) -> String {
    let properties = if document.contains("outputinstance") {
        r#"<properties XSLTDefaultOutput="1"/>"#
    } else {
        ""
    };
    format!(
        r#"<component name="{name}" library="xml" uid="{uid}" kind="14">{properties}<data><root><header><namespaces><namespace/><namespace uid="{T}"/></namespaces></header><entry name="FileInstance"><entry name="document"><entry name="Root" ns="1"{entries}</entry></entry></entry></root><document schema="{document}" instanceroot="{{{T}}}Root"/></data></component>"#
    )
}

/// A source and a target over the same component schema, joined by `edges`
/// (`from`, `to`, structural copy-all).
fn design(
    schema: &str,
    source_entries: &str,
    target_entries: &str,
    edges: &[(u32, u32, bool)],
) -> String {
    let source = component(
        "Source",
        2,
        source_entries,
        &format!(r#"{schema}" inputinstance="input.xml"#),
    );
    let target = component(
        "Target",
        3,
        target_entries,
        &format!(r#"{schema}" outputinstance="output.xml"#),
    );
    let edge_data = edges
        .iter()
        .enumerate()
        .filter(|(_, (_, _, copy))| *copy)
        .map(|(index, _)| {
            format!(r#"<edge edgekey="{index}"><data><dataconnection type="2"/></data></edge>"#)
        })
        .collect::<String>();
    let vertices = edges
        .iter()
        .enumerate()
        .map(|(index, (from, to, copy))| {
            let key = if *copy {
                format!(r#" edgekey="{index}""#)
            } else {
                String::new()
            };
            format!(r#"<vertex vertexkey="{from}"><edges><edge vertexkey="{to}"{key}/></edges></vertex>"#)
        })
        .collect::<String>();
    format!(
        r#"<mapping version="26"><component name="defaultmap" uid="1"><properties SelectedLanguage="builtin"/><structure><children>{source}{target}</children><graph directed="1"><edges>{edge_data}</edges><vertices>{vertices}</vertices></graph></structure></component></mapping>"#
    )
}

/// The whole document copied through its root.
fn root_copy(schema: &str) -> String {
    design(
        schema,
        r#" outkey="10">"#,
        r#" inpkey="20">"#,
        &[(10, 20, true)],
    )
}

fn input(content: &str, attributes: &str) -> String {
    format!(
        r#"<t:Root xmlns:t="{T}" xmlns:a="urn:ferrule:wx:a" xmlns:b="urn:ferrule:wx:b"{attributes}>{content}</t:Root>"#
    )
}

/// An input document and the read error it must report.
type Refusal = (String, fn(&XmlFormatError) -> bool);

fn run(project: &Project, input: &str) -> Result<(Instance, String), Box<dyn Error>> {
    let source = format_xml::from_str(input, &project.source)?;
    let output = engine::run(project, &source)?;
    let written = format_xml::to_string(&project.target, &output)?;
    Ok((output, written))
}

/// Excluded names are refused while the source document is read.
fn read_error(project: &Project, input: &str) -> XmlFormatError {
    match format_xml::from_str(input, &project.source) {
        Ok(instance) => panic!("{input} was read as {instance:?}"),
        Err(error) => error,
    }
}

fn import_executable(path: &Path) -> Result<Project, Box<dyn Error>> {
    let imported = mfd::import(path)?;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(
        engine::validate(&imported.project).is_empty(),
        "{:?}",
        engine::validate(&imported.project)
    );
    assert!(imported.project.source.uses_xml_wildcard_exclusions());
    assert!(imported.project.target.uses_xml_wildcard_exclusions());
    Ok(imported.project)
}

/// Imports `design`, checks its runs, exports it, re-imports the export, and
/// checks the same runs again with equal schemas and outputs.
fn round_trip(
    directory: &TempDir,
    label: &str,
    design: &str,
    accepted: &[String],
    refused: &[Refusal],
) -> Result<(), Box<dyn Error>> {
    let path = directory.write(&format!("{label}.mfd"), design)?;
    let imported = import_executable(&path)?;
    let outputs = accepted
        .iter()
        .map(|input| run(&imported, input))
        .collect::<Result<Vec<_>, _>>()?;
    // Every design copies each accepted input whole.
    for (input, (_, written)) in accepted.iter().zip(&outputs) {
        assert_eq!(
            format_xml::from_str(written, &imported.target)?,
            format_xml::from_str(input, &imported.source)?,
            "{label}: {written}"
        );
    }
    for (input, matches) in refused {
        let error = read_error(&imported, input);
        assert!(matches(&error), "{label}: {input}: {error}");
    }

    let exported_dir = directory.0.join(format!("{label}-export"));
    std::fs::create_dir(&exported_dir)?;
    let exported = exported_dir.join("mapping.mfd");
    let warnings = mfd::export(&imported, &exported)?;
    assert!(warnings.is_empty(), "{label}: {warnings:?}");
    let source_xsd = std::fs::read_to_string(exported_dir.join("mapping-source.xsd"))?;
    assert!(source_xsd.contains("notQName"), "{label}: {source_xsd}");

    let reimported = import_executable(&exported)?;
    assert_eq!(reimported.source, imported.source, "{label} source");
    assert_eq!(reimported.target, imported.target, "{label} target");
    for (input, output) in accepted.iter().zip(&outputs) {
        assert_eq!(&run(&reimported, input)?, output, "{label}: {input}");
    }
    for (input, matches) in refused {
        let error = read_error(&reimported, input);
        assert!(matches(&error), "{label} re-import: {input}: {error}");
    }
    Ok(())
}

#[test]
fn e1_designs_import_run_export_and_reimport_without_drift() -> Result<(), Box<dyn Error>> {
    let directory = TempDir::new()?;
    directory.write("a.xsd", A_XSD)?;
    directory.write("e1.xsd", E1_XSD)?;
    let accepted = [
        input(
            "<t:Head>h</t:Head><a:Known>k</a:Known><b:Extra>x</b:Extra>",
            "",
        ),
        input("<t:Head>h</t:Head><a:Other/><b:Secret>s</b:Secret>", ""),
    ];
    let refused: [Refusal; 3] = [
        (
            input("<t:Head>h</t:Head><a:Secret>s</a:Secret>", ""),
            |error| {
                matches!(error, XmlFormatError::XmlWildcardExcludedName { name, namespace }
                    if name == "Secret" && namespace == "urn:ferrule:wx:a")
            },
        ),
        (
            input("<t:Head>h</t:Head><t:Tail/>", ""),
            |error| matches!(error, XmlFormatError::XmlWildcardNamespaceMismatch { name, .. } if name == "Tail"),
        ),
        (
            input(r#"<t:Head>h</t:Head><Tail xmlns=""/>"#, ""),
            |error| matches!(error, XmlFormatError::XmlWildcardNamespaceMismatch { name, .. } if name == "Tail"),
        ),
    ];

    round_trip(
        &directory,
        "e1-root",
        &root_copy("e1.xsd"),
        &accepted,
        &refused,
    )?;

    // Explicit wiring: the declared and projected fields by value, and the
    // generic wildcard group as a structural copy.
    let explicit = design(
        "e1.xsd",
        r#"><entry name="Head" ns="1" outkey="11"/><entry name="Known" outkey="12"/><entry name="element()" outkey="13"/>"#,
        r#"><entry name="Head" ns="1" inpkey="21"/><entry name="Known" inpkey="22"/><entry name="element()" inpkey="23"/>"#,
        &[(11, 21, false), (12, 22, false), (13, 23, true)],
    );
    round_trip(&directory, "e1-fields", &explicit, &accepted, &refused)?;

    // The written document keeps the generic item's expanded name.
    let project = import_executable(&directory.0.join("e1-root.mfd"))?;
    let (output, written) = run(&project, &accepted[0])?;
    assert!(written.contains("Extra"), "{written}");
    let generic = output
        .field("element()")
        .and_then(Instance::as_repeated)
        .ok_or("missing element() items")?;
    assert_eq!(generic.len(), 1);
    assert_eq!(
        generic[0]
            .field("NamespaceURI")
            .and_then(Instance::as_scalar),
        Some(&Value::String("urn:ferrule:wx:b".into()))
    );
    Ok(())
}

#[test]
fn a1_designs_import_run_export_and_reimport_without_drift() -> Result<(), Box<dyn Error>> {
    let directory = TempDir::new()?;
    directory.write("a.xsd", A_XSD)?;
    directory.write("a1.xsd", A1_XSD)?;
    let accepted = [input("", r#" id="1" mode="m" b:z="z" xml:lang="en""#)];
    let refused: [Refusal; 4] = [
        (
            input("", r#" internal="x""#),
            |error| matches!(error, XmlFormatError::XmlAttributeWildcardExcludedName { name, .. } if name == "internal"),
        ),
        (input("", r#" a:flag="x""#), |error| {
            matches!(error, XmlFormatError::XmlAttributeWildcardExcludedName { name, namespace }
                if name == "flag" && namespace == "urn:ferrule:wx:a")
        }),
        (
            input("", r#" xml:space="preserve""#),
            |error| matches!(error, XmlFormatError::XmlAttributeWildcardExcludedName { name, .. } if name == "space"),
        ),
        (
            input("", r#" t:code="x""#),
            |error| matches!(error, XmlFormatError::XmlAttributeWildcardNamespaceMismatch { name, .. } if name == "code"),
        ),
    ];

    round_trip(
        &directory,
        "a1-root",
        &root_copy("a1.xsd"),
        &accepted,
        &refused,
    )?;

    let explicit = design(
        "a1.xsd",
        r#"><entry name="id" type="attribute" outkey="11"/><entry name="attribute()" outkey="12"/>"#,
        r#"><entry name="id" type="attribute" inpkey="21"/><entry name="attribute()" inpkey="22"/>"#,
        &[(11, 21, false), (12, 22, true)],
    );
    round_trip(&directory, "a1-fields", &explicit, &accepted, &refused)?;

    let project = import_executable(&directory.0.join("a1-root.mfd"))?;
    let (_, written) = run(&project, &accepted[0])?;
    for expected in [r#"id="1""#, r#"mode="m""#, "lang=\"en\"", "z=\"z\""] {
        assert!(written.contains(expected), "{expected}: {written}");
    }
    Ok(())
}

#[test]
fn selections_cannot_name_what_the_wildcard_excludes() -> Result<(), Box<dyn Error>> {
    let directory = TempDir::new()?;
    directory.write("a.xsd", A_XSD)?;
    directory.write("e1.xsd", E1_XSD)?;

    for (label, qname, local, reason) in [
        (
            "name",
            "{urn:ferrule:wx:a}Secret",
            "Secret",
            "notQName exclusions name it",
        ),
        (
            "namespace",
            "{}Loose",
            "Loose",
            "rejects namespace `##local`",
        ),
    ] {
        let selecting = design(
            "e1.xsd",
            &format!(
                r#" outkey="10"><entry name="*"><selections><qname QNameAsString="{qname}"/></selections></entry><entry name="{local}" outkey="11"/>"#
            ),
            r#"><entry name="Head" ns="1" inpkey="21"/>"#,
            &[(11, 21, false)],
        );
        let imported = mfd::import(&directory.write(&format!("{label}.mfd"), &selecting)?)?;
        assert!(
            imported.warnings.iter().any(|warning| {
                warning.contains(&format!(
                    "selected XML element `{local}` ({qname}) was refused"
                )) && warning.contains(reason)
            }),
            "{label}: {:?}",
            imported.warnings
        );
        assert!(imported.project.source.child(local).is_none(), "{label}");
        let elements = imported
            .project
            .source
            .child(ir::XML_ELEMENTS_FIELD)
            .ok_or("missing element()")?;
        assert!(elements.has_xml_wildcard_exclusions(), "{label}");
    }

    // A name the wildcard admits is still selectable.
    let admitted = design(
        "e1.xsd",
        r#" outkey="10"><entry name="*"><selections><qname QNameAsString="{urn:ferrule:wx:a}Note"/></selections></entry><entry name="Note" outkey="11"/>"#,
        r#"><entry name="Head" ns="1" inpkey="21"/>"#,
        &[(11, 21, false)],
    );
    let imported = mfd::import(&directory.write("admitted.mfd", &admitted)?)?;
    assert!(
        !imported
            .warnings
            .iter()
            .any(|warning| warning.contains("was refused")),
        "{:?}",
        imported.warnings
    );
    assert!(imported.project.source.child("Note").is_some());
    Ok(())
}
