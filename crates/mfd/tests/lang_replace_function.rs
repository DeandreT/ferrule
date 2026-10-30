//! `lang.replace` substitutes literal text with connected inputs; the regular-
//! expression `core.replace` and `xpath2.replace` spellings keep their meaning.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{Instance, Value};
use mapping::{Node, Project};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Result<Self, std::io::Error> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_mfd_lang_replace_{}_{}",
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

/// One `library.replace` component whose three inputs are source fields.
fn write_design(directory: &Path, library: &str) -> Result<PathBuf, std::io::Error> {
    std::fs::write(
        directory.join("source.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="Input"><xs:complexType><xs:sequence>
    <xs:element name="Value" type="xs:string"/>
    <xs:element name="Search" type="xs:string"/>
    <xs:element name="Replacement" type="xs:string"/>
  </xs:sequence></xs:complexType></xs:element>
</xs:schema>"#,
    )?;
    std::fs::write(
        directory.join("target.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="Output"><xs:complexType><xs:sequence>
    <xs:element name="Result" type="xs:string"/>
  </xs:sequence></xs:complexType></xs:element>
</xs:schema>"#,
    )?;
    let design = directory.join(format!("{library}.mfd"));
    std::fs::write(
        &design,
        format!(
            r#"<mapping version="26"><component name="map"><properties SelectedLanguage="builtin"/><structure><children>
  <component name="source" library="xml" kind="14"><data>
    <root><entry name="Input"><entry name="Value" outkey="10"/><entry name="Search" outkey="11"/><entry name="Replacement" outkey="12"/></entry></root>
    <document schema="source.xsd" inputinstance="source.xml" instanceroot="{{}}Input"/>
  </data></component>
  <component name="replace" library="{library}" kind="5"><sources><datapoint pos="0" key="20"/><datapoint pos="1" key="21"/><datapoint pos="2" key="22"/></sources><targets><datapoint pos="0" key="23"/></targets></component>
  <component name="target" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data>
    <root><entry name="Output"><entry name="Result" inpkey="30"/></entry></root>
    <document schema="target.xsd" outputinstance="target.xml" instanceroot="{{}}Output"/>
  </data></component>
</children><graph><vertices>
  <vertex vertexkey="10"><edges><edge vertexkey="20"/></edges></vertex>
  <vertex vertexkey="11"><edges><edge vertexkey="21"/></edges></vertex>
  <vertex vertexkey="12"><edges><edge vertexkey="22"/></edges></vertex>
  <vertex vertexkey="23"><edges><edge vertexkey="30"/></edges></vertex>
</vertices></graph></structure></component></mapping>"#
        ),
    )?;
    Ok(design)
}

fn xml_text(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn run(project: &Project, value: &str, search: &str, replacement: &str) -> TestResult<Value> {
    let source = format_xml::from_str(
        &format!(
            "<Input><Value>{}</Value><Search>{}</Search><Replacement>{}</Replacement></Input>",
            xml_text(value),
            xml_text(search),
            xml_text(replacement)
        ),
        &project.source,
    )?;
    let output = engine::run(project, &source)?;
    output
        .field("Result")
        .and_then(Instance::as_scalar)
        .cloned()
        .ok_or_else(|| "missing Result".into())
}

fn called_functions(project: &Project) -> Vec<&str> {
    project
        .graph
        .nodes
        .values()
        .filter_map(|node| match node {
            Node::Call { function, .. } => Some(function.as_str()),
            _ => None,
        })
        .collect()
}

#[test]
fn connected_lang_replace_is_literal_through_export_and_reimport() -> TestResult {
    let directory = TempDir::new()?;
    let imported = mfd::import(&write_design(&directory.0, "lang")?)?;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(engine::validate(&imported.project).is_empty());
    assert_eq!(called_functions(&imported.project), ["replace_literal"]);

    let exported_path = directory.0.join("roundtrip.mfd");
    assert!(mfd::export(&imported.project, &exported_path)?.is_empty());
    let exported = std::fs::read_to_string(&exported_path)?;
    assert!(exported.contains("SelectedLanguage=\"builtin\""));
    assert!(exported.contains("name=\"replace\" library=\"lang\""));
    let reimported = mfd::import(&exported_path)?;
    assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
    assert_eq!(called_functions(&reimported.project), ["replace_literal"]);

    // Results observed natively for the same connected inputs.
    for project in [&imported.project, &reimported.project] {
        for (value, search, replacement, expected) in [
            ("p.q.r", ".", "$1\\", "p$1\\q$1\\r"),
            ("1*2(3)$4\\5", "*", "x", "1x2(3)$4\\5"),
            ("1*2(3)$4\\5", "(", "[", "1*2[3)$4\\5"),
            ("1*2(3)$4\\5", "$", "D", "1*2(3)D4\\5"),
            ("1*2(3)$4\\5", "\\", "/", "1*2(3)$4/5"),
            ("aaa", "aa", "b", "ba"),
            ("abc", "x", "y", "abc"),
        ] {
            assert_eq!(
                run(project, value, search, replacement)?,
                Value::String(expected.into()),
                "{value} {search} {replacement}"
            );
        }
        // Native execution fails with an empty search text, connected or not.
        for value in ["abc", ""] {
            let error = run(project, value, "", "-").unwrap_err().to_string();
            assert!(error.contains("search text is empty"), "{error}");
        }
    }
    Ok(())
}

#[test]
fn regular_expression_replace_spellings_are_unchanged() -> TestResult {
    for library in ["core", "xpath2"] {
        let directory = TempDir::new()?;
        let imported = mfd::import(&write_design(&directory.0, library)?)?;
        assert!(
            imported.warnings.is_empty(),
            "{library}: {:?}",
            imported.warnings
        );
        assert_eq!(
            called_functions(&imported.project),
            ["replace"],
            "{library}"
        );
        assert_eq!(
            run(&imported.project, "a.c abc", ".", "-")?,
            Value::String("-------".into()),
            "{library}"
        );
        assert_eq!(
            run(&imported.project, "Ferrule", "(Ferr)(ule)", "$1-$2")?,
            Value::String("Ferr-ule".into()),
            "{library}"
        );
        let error = mfd::export(&imported.project, &directory.0.join("roundtrip.mfd"))
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("replaces literal text"),
            "{library}: {error}"
        );
    }
    Ok(())
}
