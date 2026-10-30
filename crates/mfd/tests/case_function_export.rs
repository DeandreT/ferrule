use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{Instance, Value};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Result<Self, std::io::Error> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_mfd_case_function_export_{}_{}",
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

/// One way a design can spell the case and date functions it uses.
struct Spelling {
    label: &'static str,
    language: &'static str,
    upper: (&'static str, &'static str),
    lower: (&'static str, &'static str),
    date: &'static str,
}

const SPELLINGS: [Spelling; 3] = [
    Spelling {
        label: "built-in lang",
        language: "builtin",
        upper: ("uppercase", "lang"),
        lower: ("lowercase", "lang"),
        date: "lang",
    },
    Spelling {
        label: "XSLT 2.0 xpath2",
        language: "xslt2",
        upper: ("upper-case", "xpath2"),
        lower: ("lower-case", "xpath2"),
        date: "lang",
    },
    // Earlier ferrule exports wrote these pairs, which the reference
    // application cannot find for the built-in language.
    Spelling {
        label: "earlier ferrule core",
        language: "builtin",
        upper: ("upper-case", "core"),
        lower: ("lower-case", "core"),
        date: "core",
    },
];

fn write_design(directory: &Path, spelling: &Spelling) -> Result<PathBuf, std::io::Error> {
    std::fs::write(
        directory.join("source.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="Input"><xs:complexType><xs:sequence>
    <xs:element name="Text" type="xs:string"/>
    <xs:element name="Stamp" type="xs:string"/>
  </xs:sequence></xs:complexType></xs:element>
</xs:schema>"#,
    )?;
    std::fs::write(
        directory.join("target.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="Output"><xs:complexType><xs:sequence>
    <xs:element name="Upper" type="xs:string"/>
    <xs:element name="Lower" type="xs:string"/>
    <xs:element name="Day" type="xs:string"/>
  </xs:sequence></xs:complexType></xs:element>
</xs:schema>"#,
    )?;
    let design = directory.join("mapping.mfd");
    let Spelling {
        language,
        upper: (upper, upper_library),
        lower: (lower, lower_library),
        date,
        ..
    } = spelling;
    std::fs::write(
        &design,
        format!(
            r#"<mapping version="26"><component name="map"><properties SelectedLanguage="{language}"/><structure><children>
  <component name="source" library="xml" kind="14"><data>
    <root><entry name="Input"><entry name="Text" outkey="10"/><entry name="Stamp" outkey="11"/></entry></root>
    <document schema="source.xsd" inputinstance="source.xml" instanceroot="{{}}Input"/>
  </data></component>
  <component name="{upper}" library="{upper_library}" kind="5"><sources><datapoint pos="0" key="20"/></sources><targets><datapoint pos="0" key="21"/></targets></component>
  <component name="{lower}" library="{lower_library}" kind="5"><sources><datapoint pos="0" key="22"/></sources><targets><datapoint pos="0" key="23"/></targets></component>
  <component name="date-from-datetime" library="{date}" kind="5"><sources><datapoint pos="0" key="24"/></sources><targets><datapoint pos="0" key="25"/></targets></component>
  <component name="target" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data>
    <root><entry name="Output"><entry name="Upper" inpkey="30"/><entry name="Lower" inpkey="31"/><entry name="Day" inpkey="32"/></entry></root>
    <document schema="target.xsd" outputinstance="target.xml" instanceroot="{{}}Output"/>
  </data></component>
</children><graph><vertices>
  <vertex vertexkey="10"><edges><edge vertexkey="20"/><edge vertexkey="22"/></edges></vertex>
  <vertex vertexkey="11"><edges><edge vertexkey="24"/></edges></vertex>
  <vertex vertexkey="21"><edges><edge vertexkey="30"/></edges></vertex>
  <vertex vertexkey="23"><edges><edge vertexkey="31"/></edges></vertex>
  <vertex vertexkey="25"><edges><edge vertexkey="32"/></edges></vertex>
</vertices></graph></structure></component></mapping>"#
        ),
    )?;
    Ok(design)
}

fn execute(project: &mapping::Project) -> Result<Instance, Box<dyn std::error::Error>> {
    let source = format_xml::from_str(
        "<Input><Text>Mixed Case</Text><Stamp>2010-12-01T15:02:39</Stamp></Input>",
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
fn case_functions_export_under_built_in_language_libraries_and_round_trip()
-> Result<(), Box<dyn std::error::Error>> {
    for spelling in &SPELLINGS {
        let label = spelling.label;
        let directory = TempDir::new()?;
        let imported = mfd::import(&write_design(&directory.0, spelling)?)?;
        assert!(
            imported.warnings.is_empty(),
            "{label}: {:?}",
            imported.warnings
        );
        assert!(engine::validate(&imported.project).is_empty(), "{label}");
        let original = execute(&imported.project)?;
        assert_eq!(
            field(&original, "Upper"),
            Some(Value::String("MIXED CASE".into())),
            "{label}"
        );
        assert_eq!(
            field(&original, "Lower"),
            Some(Value::String("mixed case".into())),
            "{label}"
        );
        assert!(field(&original, "Day").is_some(), "{label}");

        let exported_path = directory.0.join("roundtrip.mfd");
        let warnings = mfd::export(&imported.project, &exported_path)?;
        assert!(warnings.is_empty(), "{label}: {warnings:?}");
        let exported = std::fs::read_to_string(&exported_path)?;
        assert!(
            exported.contains("SelectedLanguage=\"builtin\""),
            "{label}: {exported}"
        );
        let mut functions = function_components(&exported)?;
        functions.sort();
        assert_eq!(
            functions,
            [
                ("date-from-datetime", "lang"),
                ("lowercase", "lang"),
                ("uppercase", "lang"),
            ]
            .map(|(name, library)| (name.to_string(), library.to_string())),
            "{label}"
        );

        let roundtrip = mfd::import(&exported_path)?;
        assert!(
            roundtrip.warnings.is_empty(),
            "{label}: {:?}",
            roundtrip.warnings
        );
        assert!(engine::validate(&roundtrip.project).is_empty(), "{label}");
        assert_eq!(execute(&roundtrip.project)?, original, "{label}");
    }
    Ok(())
}
