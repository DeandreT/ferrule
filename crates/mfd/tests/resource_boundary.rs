use std::error::Error;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use ir::{Instance, ScalarType, SchemaKind, Value};

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Result<Self, std::io::Error> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_mfd_boundary_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(path.join("package/sub"))?;
        std::fs::create_dir_all(path.join("outside"))?;
        eprintln!("MFD_RESOURCE_BOUNDARY_ORIGINALS={}", path.display());
        Ok(Self(path))
    }
    fn package(&self) -> PathBuf {
        self.0.join("package")
    }
    fn import(&self) -> Result<mfd::Imported, mfd::MfdError> {
        let result = mfd::import_with_options(
            &self.package().join("mapping.mfd"),
            &mfd::ImportOptions::default().with_package_root(self.package()),
        );
        let outcome = match &result {
            Ok(imported) => {
                serde_json::json!({ "project": imported.project, "warnings": imported.warnings })
            }
            Err(error) => serde_json::json!({ "error": error.to_string() }),
        };
        std::fs::write(
            self.0.join("import-outcome.json"),
            serde_json::to_vec_pretty(&outcome).unwrap(),
        )?;
        result
    }

    fn observe(&self, label: &str, outcome: impl std::fmt::Debug) -> Result<(), std::io::Error> {
        std::fs::write(self.0.join(format!("{label}.txt")), format!("{outcome:#?}"))
    }

    fn observe_profile(
        &self,
        label: &str,
        outcome: &Result<mfd::ImportOutcome, mfd::MfdError>,
    ) -> Result<(), std::io::Error> {
        let snapshot = match outcome {
            Ok(outcome) => format!(
                "Ok({:#?})",
                (
                    &outcome.imported.project,
                    &outcome.imported.warnings,
                    &outcome.imported.mapping_path,
                    &outcome.report,
                )
            ),
            Err(error) => format!("Err({error:#?})"),
        };
        std::fs::write(self.0.join(format!("{label}.txt")), snapshot)
    }
}

fn write(path: &Path, text: &str) -> Result<(), std::io::Error> {
    std::fs::create_dir_all(path.parent().unwrap())?;
    std::fs::write(path, text)
}

fn plain_mapping(source: &str) -> String {
    format!(
        r#"<mapping><component name="map"><structure><children>
      {source}
      <component name="target" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data>
        <root><entry name="Report"><entry name="Result" inpkey="20"/></entry></root>
        <document schema="target.xsd" instanceroot="{{}}Report"/>
      </data></component>
    </children><graph><vertices><vertex vertexkey="10"><edges><edge vertexkey="20"/></edges></vertex></vertices></graph></structure></component></mapping>"#
    )
}

fn xml_source() -> &'static str {
    r#"<component name="source" library="xml" kind="14"><data>
      <root><entry name="Source"><entry name="Value" outkey="10"/></entry></root>
      <document schema="source.xsd" instanceroot="{}Source"/>
    </data></component>"#
}

fn http_source() -> &'static str {
    r#"<component name="source" library="webservice" kind="20"><data>
      <wsdl kind="call" sourceMode="manual" httpmethod="GET" url="http://127.0.0.1:9/fixture"/>
      <root><entry name="Response"><entry name="body" type="doc-xml">
        <document schemafile="source.xsd" root="Source" encoding="UTF-8"/>
        <entry name="Source"><entry name="Value" outkey="10"/></entry>
      </entry></entry></root>
    </data></component>"#
}

const TYPES: &str = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:simpleType name="SelectedType"><xs:restriction base="xs:int"/></xs:simpleType></xs:schema>"#;

#[test]
fn typed_database_fallback_distinguishes_missing_metadata_from_boundary_denial()
-> Result<(), Box<dyn Error>> {
    for missing in [true, false] {
        let fixture = Fixture::new()?;
        let outside = fixture.0.join("outside/metadata.sqlite");
        let outside_bytes = b"Wholly authored resource; never opened as database metadata.";
        write(&outside, std::str::from_utf8(outside_bytes)?)?;
        let declared = if missing {
            "missing.sqlite".to_owned()
        } else {
            outside.display().to_string()
        };
        let design = format!(
            r#"<mapping><resources><datasources><datasource name="fixture"><database_connection name="fixture" ConnectionString="{declared}" database_kind="SQLite" import_kind="SQLite"/></datasource></datasources></resources>
              <component name="map"><structure><children>{}
              <component name="target" library="db" kind="15"><properties XSLTDefaultOutput="1"/><data><root><entry name="Records" type="table"><entry name="Value" datatype="integer" inpkey="20"/></entry></root><database ref="fixture"/></data></component>
              </children><graph><vertices><vertex vertexkey="10"><edges><edge vertexkey="20"/></edges></vertex></vertices></graph></structure></component></mapping>"#,
            xml_source()
        );
        write(&fixture.package().join("mapping.mfd"), &design)?;
        write(
            &fixture.package().join("source.xsd"),
            r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Source"><xs:complexType><xs:sequence><xs:element name="Value" type="xs:int"/></xs:sequence></xs:complexType></xs:element></xs:schema>"#,
        )?;
        let imported = fixture.import()?;
        let input =
            Instance::Group(vec![("Value".to_owned(), Instance::Scalar(Value::Int(17)))].into());
        let expected =
            Instance::Group(vec![("Value".to_owned(), Instance::Scalar(Value::Int(17)))].into());
        let output = engine::run(&imported.project, &input);
        fixture.observe("execution", (&input, &output, &expected))?;
        let outside_after = std::fs::read(&outside)?;
        fixture.observe("outside-bytes", (&outside_bytes, &outside_after))?;
        let missing_path_exists = fixture.package().join("missing.sqlite").exists();
        fixture.observe("missing-path-exists", missing_path_exists)?;
        let refusal = if missing {
            None
        } else {
            let refusal = mfd::import_with_profile(
                &fixture.package().join("mapping.mfd"),
                &mfd::ImportOptions::default().with_package_root(fixture.package()),
                mfd::ImportProfile::Executable,
            );
            fixture.observe_profile("executable-refusal", &refusal)?;
            Some(refusal)
        };
        assert_eq!(output?, expected);
        assert_eq!(outside_after.as_slice(), outside_bytes.as_slice());
        assert!(!missing_path_exists);
        assert_eq!(
            imported
                .project
                .target
                .child("Value")
                .map(|node| &node.kind),
            Some(&SchemaKind::Scalar {
                ty: ScalarType::Int
            })
        );
        if missing {
            assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
        } else {
            assert!(
                imported.warnings.iter().any(|warning| {
                    warning.contains("resolves outside package root")
                        || warning.contains("uses a Windows drive or UNC path")
                }),
                "{:?}",
                imported.warnings
            );
            assert!(matches!(
                refusal,
                Some(Err(mfd::MfdError::IncompatibleImport(_)))
            ));
        }
    }
    Ok(())
}

#[test]
fn ordinary_and_http_schema_dependencies_cannot_escape_with_successful_types()
-> Result<(), Box<dyn Error>> {
    for source in [xml_source(), http_source()] {
        for (location, contained) in [("sub/../types.xsd", true), ("../outside/types.xsd", false)] {
            let fixture = Fixture::new()?;
            write(
                &fixture.package().join("mapping.mfd"),
                &plain_mapping(source),
            )?;
            write(
                &fixture.package().join("target.xsd"),
                r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Report"><xs:complexType><xs:sequence><xs:element name="Result" type="xs:int"/></xs:sequence></xs:complexType></xs:element></xs:schema>"#,
            )?;
            write(
                &fixture.package().join("source.xsd"),
                &format!(
                    r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:include schemaLocation="{location}"/><xs:element name="Source"><xs:complexType><xs:sequence><xs:element name="Value" type="SelectedType"/></xs:sequence></xs:complexType></xs:element></xs:schema>"#
                ),
            )?;
            write(&fixture.package().join("types.xsd"), TYPES)?;
            write(&fixture.0.join("outside/types.xsd"), TYPES)?;
            let imported = fixture.import()?;
            let ty = if contained {
                ScalarType::Int
            } else {
                ScalarType::String
            };
            assert_eq!(
                imported
                    .project
                    .source
                    .child("Value")
                    .map(|node| &node.kind),
                Some(&SchemaKind::Scalar { ty })
            );
            if contained {
                assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
                let input = Instance::Group(
                    vec![("Value".to_owned(), Instance::Scalar(Value::Int(17)))].into(),
                );
                let output = engine::run(&imported.project, &input);
                let expected = Instance::Group(
                    vec![("Result".to_owned(), Instance::Scalar(Value::Int(17)))].into(),
                );
                fixture.observe("execution", (&input, &output, &expected))?;
                assert_eq!(output?, expected);
            } else {
                assert!(
                    imported
                        .warnings
                        .iter()
                        .any(|warning| warning.contains("outside authorizing root")),
                    "{:?}",
                    imported.warnings
                );
                let refusal = mfd::import_with_profile(
                    &fixture.package().join("mapping.mfd"),
                    &mfd::ImportOptions::default().with_package_root(fixture.package()),
                    mfd::ImportProfile::Executable,
                );
                fixture.observe_profile("executable-refusal", &refusal)?;
                assert!(matches!(refusal, Err(mfd::MfdError::IncompatibleImport(_))));
            }
        }
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum ModuleKind {
    CSharp,
    Java,
    XQuery,
    Xslt,
}

impl ModuleKind {
    fn metadata(
        self,
    ) -> (
        &'static str,
        &'static str,
        &'static str,
        &'static str,
        &'static str,
    ) {
        match self {
            Self::CSharp => (
                "cs",
                "code",
                "Neutral.Render",
                "code/Neutral.cs",
                r#"using System.Globalization;
              public class Neutral { public static string Render(decimal amount) { return amount.ToString("0.00", CultureInfo.InvariantCulture); } }"#,
            ),
            Self::Java => (
                "java",
                "code.Neutral",
                "code.Neutral.Render",
                "code/Neutral.java",
                r#"package code;
              public class Neutral { public static String Render(java.math.BigDecimal amount) {
                java.text.NumberFormat formatter = new java.text.DecimalFormat("0.00"); return formatter.format(amount.doubleValue()); } }"#,
            ),
            Self::XQuery => (
                "xquery",
                "neutral",
                "n:double",
                "neutral.xq",
                r#"xquery version "1.0"; module namespace n="urn:neutral";
              declare function n:double($amount as xs:decimal) as xs:decimal { $amount * 2 };"#,
            ),
            Self::Xslt => (
                "xslt",
                "neutral",
                "SumValues",
                "neutral.xslt",
                r#"<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="1.0">
              <xsl:template name="SumValues"><xsl:param name="input"/><xsl:value-of select="sum($input/Item/Value)"/></xsl:template></xsl:stylesheet>"#,
            ),
        }
    }

    fn arrange(self, fixture: &Fixture) -> Result<(), std::io::Error> {
        let (language, library, function, _, _) = self.metadata();
        let (source_entry, source_type, occurrence, target_type) = match self {
            Self::Xslt => (
                r#"<entry name="Source" outkey="10"><entry name="Item"><entry name="Value"/></entry></entry>"#,
                "int",
                "",
                "int",
            ),
            Self::XQuery => (
                r#"<entry name="Source"><entry name="Value" outkey="10"/></entry>"#,
                "decimal",
                "",
                "decimal",
            ),
            _ => (
                r#"<entry name="Source"><entry name="Value" outkey="10"/></entry>"#,
                "decimal",
                "",
                "string",
            ),
        };
        let value = format!(r#"<xs:element name="Value" type="xs:{source_type}" {occurrence}/>"#);
        let fields = if matches!(self, Self::Xslt) {
            format!(
                r#"<xs:element name="Item" maxOccurs="unbounded"><xs:complexType><xs:sequence>{value}</xs:sequence></xs:complexType></xs:element>"#
            )
        } else {
            value
        };
        write(
            &fixture.package().join("source.xsd"),
            &format!(
                r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Source"><xs:complexType><xs:sequence>{fields}</xs:sequence></xs:complexType></xs:element></xs:schema>"#
            ),
        )?;
        write(
            &fixture.package().join("target.xsd"),
            &format!(
                r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Report"><xs:complexType><xs:sequence><xs:element name="Result" type="xs:{target_type}"/></xs:sequence></xs:complexType></xs:element></xs:schema>"#
            ),
        )?;
        write(
            &fixture.package().join("mapping.mfd"),
            &format!(
                r#"<mapping><component name="map"><properties SelectedLanguage="{language}"/><structure><children>
          <component name="source" library="xml" kind="14"><data><root>{source_entry}</root><document schema="source.xsd" instanceroot="{{}}Source"/></data></component>
          <component name="{function}" library="{library}" kind="5"><sources><datapoint key="11" pos="0"/></sources><targets><datapoint key="12" pos="0"/></targets></component>
          <component name="target" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data><root><entry name="Report"><entry name="Result" inpkey="20"/></entry></root><document schema="target.xsd" instanceroot="{{}}Report"/></data></component>
        </children><graph><vertices><vertex vertexkey="10"><edges><edge vertexkey="11"/></edges></vertex><vertex vertexkey="12"><edges><edge vertexkey="20"/></edges></vertex></vertices></graph></structure></component></mapping>"#
            ),
        )
    }

    fn assert_execution(
        self,
        fixture: &Fixture,
        imported: &mfd::Imported,
    ) -> Result<(), Box<dyn Error>> {
        let validation = engine::validate(&imported.project);
        let (value, expected) = match self {
            Self::Xslt => (
                Instance::Repeated(vec![
                    Instance::Group(
                        vec![("Value".to_owned(), Instance::Scalar(Value::Int(5)))].into(),
                    ),
                    Instance::Group(
                        vec![("Value".to_owned(), Instance::Scalar(Value::Int(7)))].into(),
                    ),
                ]),
                Value::Int(12),
            ),
            Self::XQuery => (Instance::Scalar(Value::Float(2.5)), Value::Float(5.0)),
            _ => (
                Instance::Scalar(Value::Float(2.5)),
                Value::String("2.50".to_owned()),
            ),
        };
        let field = if matches!(self, Self::Xslt) {
            "Item"
        } else {
            "Value"
        };
        let input = Instance::Group(vec![(field.to_owned(), value)].into());
        let output = engine::run(&imported.project, &input);
        let expected =
            Instance::Group(vec![("Result".to_owned(), Instance::Scalar(expected))].into());
        fixture.observe("execution", (&input, &validation, &output, &expected))?;
        assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
        assert!(validation.is_empty());
        assert_eq!(output?, expected);
        Ok(())
    }
}

#[test]
fn all_adjacent_module_languages_preserve_contained_executable_recipes()
-> Result<(), Box<dyn Error>> {
    for kind in [
        ModuleKind::CSharp,
        ModuleKind::Java,
        ModuleKind::XQuery,
        ModuleKind::Xslt,
    ] {
        let fixture = Fixture::new()?;
        kind.arrange(&fixture)?;
        let (_, _, _, path, text) = kind.metadata();
        write(&fixture.package().join(path), text)?;
        kind.assert_execution(&fixture, &fixture.import()?)?;
    }
    Ok(())
}

#[cfg(unix)]
#[test]
fn all_adjacent_module_languages_check_canonical_file_targets_before_reading()
-> Result<(), Box<dyn Error>> {
    use std::os::unix::fs::symlink;
    for kind in [
        ModuleKind::CSharp,
        ModuleKind::Java,
        ModuleKind::XQuery,
        ModuleKind::Xslt,
    ] {
        for contained in [true, false] {
            let fixture = Fixture::new()?;
            kind.arrange(&fixture)?;
            let (_, _, _, path, text) = kind.metadata();
            let leaf = if contained {
                fixture.package().join("sub/module.source")
            } else {
                fixture.0.join("outside/module.source")
            };
            write(&leaf, text)?;
            let candidate = fixture.package().join(path);
            std::fs::create_dir_all(candidate.parent().unwrap())?;
            symlink(&leaf, candidate)?;
            let imported = fixture.import()?;
            if contained {
                kind.assert_execution(&fixture, &imported)?;
            } else {
                assert!(
                    imported
                        .warnings
                        .iter()
                        .any(|warning| warning.contains("module resource resolves outside")),
                    "{:?}",
                    imported.warnings
                );
                let refusal = mfd::import_with_profile(
                    &fixture.package().join("mapping.mfd"),
                    &mfd::ImportOptions::default().with_package_root(fixture.package()),
                    mfd::ImportProfile::Executable,
                );
                fixture.observe_profile("executable-refusal", &refusal)?;
                assert!(matches!(refusal, Err(mfd::MfdError::IncompatibleImport(_))));
            }
        }
    }
    Ok(())
}

#[cfg(unix)]
#[test]
fn module_directory_symlinks_and_dangling_candidates_are_explicit_refusals()
-> Result<(), Box<dyn Error>> {
    use std::os::unix::fs::symlink;
    for kind in [ModuleKind::CSharp, ModuleKind::Java] {
        for contained in [true, false] {
            let fixture = Fixture::new()?;
            kind.arrange(&fixture)?;
            let (_, _, _, path, text) = kind.metadata();
            let directory = if contained {
                fixture.package().join("sub/code")
            } else {
                fixture.0.join("outside")
            };
            write(&directory.join(Path::new(path).file_name().unwrap()), text)?;
            symlink(&directory, fixture.package().join("code"))?;
            let imported = fixture.import()?;
            if contained {
                kind.assert_execution(&fixture, &imported)?;
            } else {
                assert!(
                    imported
                        .warnings
                        .iter()
                        .any(|warning| warning.contains("module resource resolves outside")),
                    "{:?}",
                    imported.warnings
                );
            }
        }
    }
    let fixture = Fixture::new()?;
    ModuleKind::XQuery.arrange(&fixture)?;
    symlink(
        fixture.package().join("absent.xq"),
        fixture.package().join("neutral.xq"),
    )?;
    let imported = fixture.import()?;
    assert!(
        imported
            .warnings
            .iter()
            .any(|warning| warning.contains("could not resolve module resource")),
        "{:?}",
        imported.warnings
    );
    Ok(())
}
