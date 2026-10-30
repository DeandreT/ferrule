use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{Instance, Value};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Result<Self, std::io::Error> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_mfd_translate_function_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path)?;
        Ok(Self(path))
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A self-authored design with one `translate` fed by source fields and one
/// fed by constants that strips digits through an empty replacement string.
fn write_design(directory: &Path, language: &str) -> Result<PathBuf, std::io::Error> {
    std::fs::write(
        directory.join("source.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="Input"><xs:complexType><xs:sequence>
    <xs:element name="Text" type="xs:string"/>
    <xs:element name="Search" type="xs:string"/>
    <xs:element name="Replacement" type="xs:string"/>
  </xs:sequence></xs:complexType></xs:element>
</xs:schema>"#,
    )?;
    std::fs::write(
        directory.join("target.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="Output"><xs:complexType><xs:sequence>
    <xs:element name="Swapped" type="xs:string"/>
    <xs:element name="Stripped" type="xs:string"/>
  </xs:sequence></xs:complexType></xs:element>
</xs:schema>"#,
    )?;
    let design = directory.join("mapping.mfd");
    std::fs::write(
        &design,
        format!(
            r#"<mapping version="26"><component name="map"><properties SelectedLanguage="{language}"/><structure><children>
  <component name="source" library="xml" kind="14"><data>
    <root><entry name="Input"><entry name="Text" outkey="10"/><entry name="Search" outkey="11"/><entry name="Replacement" outkey="12"/></entry></root>
    <document schema="source.xsd" inputinstance="source.xml" instanceroot="{{}}Input"/>
  </data></component>
  <component name="translate" library="core" kind="5"><sources><datapoint pos="0" key="20"/><datapoint pos="1" key="21"/><datapoint pos="2" key="22"/></sources><targets><datapoint pos="0" key="23"/></targets></component>
  <component name="constant" library="core" kind="2"><targets><datapoint key="40"/></targets><data><constant value="0123456789" datatype="string"/></data></component>
  <component name="constant" library="core" kind="2"><targets><datapoint key="41"/></targets><data><constant value="" datatype="string"/></data></component>
  <component name="translate" library="core" kind="5"><sources><datapoint pos="0" key="24"/><datapoint pos="1" key="25"/><datapoint pos="2" key="26"/></sources><targets><datapoint pos="0" key="27"/></targets></component>
  <component name="target" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data>
    <root><entry name="Output"><entry name="Swapped" inpkey="30"/><entry name="Stripped" inpkey="31"/></entry></root>
    <document schema="target.xsd" outputinstance="target.xml" instanceroot="{{}}Output"/>
  </data></component>
</children><graph><vertices>
  <vertex vertexkey="10"><edges><edge vertexkey="20"/><edge vertexkey="24"/></edges></vertex>
  <vertex vertexkey="11"><edges><edge vertexkey="21"/></edges></vertex>
  <vertex vertexkey="12"><edges><edge vertexkey="22"/></edges></vertex>
  <vertex vertexkey="40"><edges><edge vertexkey="25"/></edges></vertex>
  <vertex vertexkey="41"><edges><edge vertexkey="26"/></edges></vertex>
  <vertex vertexkey="23"><edges><edge vertexkey="30"/></edges></vertex>
  <vertex vertexkey="27"><edges><edge vertexkey="31"/></edges></vertex>
</vertices></graph></structure></component></mapping>"#
        ),
    )?;
    Ok(design)
}

fn execute(project: &mapping::Project) -> Result<Instance, Box<dyn std::error::Error>> {
    // The repeated `a` keeps its first replacement; `c` has none and is removed.
    let source = format_xml::from_str(
        "<Input><Text>[1a2,b3c]\u{1f642}</Text><Search>[,]a\u{1f642}ac</Search><Replacement>(.)\u{e9}\u{10348}x</Replacement></Input>",
        &project.source,
    )?;
    Ok(engine::run(project, &source)?)
}

fn field(output: &Instance, name: &str) -> Option<Value> {
    output.field(name).and_then(Instance::as_scalar).cloned()
}

/// Every kind-5 function component as a (name, library) pair.
fn function_components(xml: &str) -> Result<Vec<(String, String)>, roxmltree::Error> {
    let document = roxmltree::Document::parse(xml)?;
    Ok(document
        .descendants()
        .filter(|node| node.has_tag_name("component") && node.attribute("kind") == Some("5"))
        .map(|node| {
            (
                node.attribute("name").unwrap_or_default().to_string(),
                node.attribute("library").unwrap_or_default().to_string(),
            )
        })
        .collect())
}

#[test]
fn translate_imports_executes_and_round_trips_under_the_core_library()
-> Result<(), Box<dyn std::error::Error>> {
    for language in ["builtin", "xslt2"] {
        let directory = TempDir::new()?;
        let imported = mfd::import(&write_design(&directory.0, language)?)?;
        assert!(
            imported.warnings.is_empty(),
            "{language}: {:?}",
            imported.warnings
        );
        assert!(engine::validate(&imported.project).is_empty(), "{language}");
        let original = execute(&imported.project)?;
        assert_eq!(
            field(&original, "Swapped"),
            Some(Value::String("(1\u{e9}2.b3)\u{10348}".into())),
            "{language}"
        );
        assert_eq!(
            field(&original, "Stripped"),
            Some(Value::String("[a,bc]\u{1f642}".into())),
            "{language}"
        );

        let exported_path = directory.0.join("roundtrip.mfd");
        let warnings = mfd::export(&imported.project, &exported_path)?;
        assert!(warnings.is_empty(), "{language}: {warnings:?}");
        let exported = std::fs::read_to_string(&exported_path)?;
        assert!(
            exported.contains("SelectedLanguage=\"builtin\""),
            "{language}: {exported}"
        );
        assert_eq!(
            function_components(&exported)?,
            [("translate", "core"), ("translate", "core")]
                .map(|(name, library)| (name.to_string(), library.to_string())),
            "{language}"
        );

        let roundtrip = mfd::import(&exported_path)?;
        assert!(
            roundtrip.warnings.is_empty(),
            "{language}: {:?}",
            roundtrip.warnings
        );
        assert!(
            engine::validate(&roundtrip.project).is_empty(),
            "{language}"
        );
        assert_eq!(execute(&roundtrip.project)?, original, "{language}");
    }
    Ok(())
}
