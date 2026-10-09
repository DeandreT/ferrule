use std::error::Error;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use ir::{Instance, Value};

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Result<Self, std::io::Error> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_udf_resource_boundary_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(path.join("package/sub"))?;
        std::fs::create_dir_all(path.join("outside"))?;
        eprintln!("UDF_RESOURCE_BOUNDARY_ORIGINALS={}", path.display());
        Ok(Self(path))
    }

    fn package(&self) -> PathBuf {
        self.0.join("package")
    }

    fn arrange(
        &self,
        recursive: bool,
        declared_schema: &str,
        dependency: &str,
    ) -> Result<(), std::io::Error> {
        let self_call = if recursive {
            r#"<component name="Probe" library="neutral" kind="19"/>"#
        } else {
            ""
        };
        let definition = format!(
            r#"<component name="Probe" library="neutral" kind="19"><structure><children>
              <component name="Input" library="xml" kind="14" uid="1"><properties UsageKind="input"/><data><root><entry name="Parameter"><entry name="Value" outkey="101"/></entry></root><document schema="{declared_schema}" instanceroot="{{}}Parameter"/></data></component>
              <component name="Output" library="xml" kind="14" uid="2"><properties UsageKind="output"/><data><root><entry name="Parameter"><entry name="Value" inpkey="102"/></entry></root><document schema="{declared_schema}" instanceroot="{{}}Parameter"/></data></component>
              {self_call}
            </children><graph/></structure></component>"#
        );
        let design = format!(
            r#"<mapping>{definition}<component name="map"><structure><children>
              <component name="Source" library="xml" kind="14"><data><root><entry name="Document"><entry name="Value" outkey="10"/></entry></root><document schema="main.xsd" instanceroot="{{}}Document"/></data></component>
              <component name="Target" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data><root><entry name="Document"><entry name="Value" inpkey="20"/></entry></root><document schema="main.xsd" instanceroot="{{}}Document"/></data></component>
            </children><graph><vertices><vertex vertexkey="10"><edges><edge vertexkey="20"/></edges></vertex></vertices></graph></structure></component></mapping>"#
        );
        std::fs::write(self.package().join("mapping.mfd"), design)?;
        std::fs::write(
            self.package().join("main.xsd"),
            r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Document"><xs:complexType><xs:sequence><xs:element name="Value" type="xs:int"/></xs:sequence></xs:complexType></xs:element></xs:schema>"#,
        )?;
        let parameter = format!(
            r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:include schemaLocation="{dependency}"/><xs:element name="Parameter"><xs:complexType><xs:sequence><xs:element name="Value" type="SelectedType"/></xs:sequence></xs:complexType></xs:element></xs:schema>"#
        );
        for directory in [self.package(), self.0.join("outside")] {
            std::fs::write(directory.join("parameter.xsd"), &parameter)?;
            std::fs::write(
                directory.join("types.xsd"),
                r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:simpleType name="SelectedType"><xs:restriction base="xs:int"/></xs:simpleType></xs:schema>"#,
            )?;
        }
        std::fs::write(
            self.0.join("expected-before-run.txt"),
            "Contained references: no resource warning; ordinary complete output Value=17. Escaping references: visible canonical resource refusal even when the unused UDF recognizer rejects its shape; executable import refuses publication. No outside-derived schema is accepted.",
        )?;
        Ok(())
    }

    fn observe(&self, label: &str, value: impl std::fmt::Debug) -> Result<(), std::io::Error> {
        std::fs::write(self.0.join(format!("{label}.txt")), format!("{value:#?}"))
    }

    fn observe_import(
        &self,
        label: &str,
        observed: &Result<mfd::Imported, mfd::MfdError>,
    ) -> Result<(), Box<dyn Error>> {
        let snapshot = match observed {
            Ok(imported) => {
                serde_json::json!({ "project": imported.project, "warnings": imported.warnings })
            }
            Err(error) => serde_json::json!({ "error": error.to_string() }),
        };
        std::fs::write(
            self.0.join(format!("{label}.json")),
            serde_json::to_vec_pretty(&snapshot)?,
        )?;
        Ok(())
    }
}

fn import(fixture: &Fixture, profile: mfd::ImportProfile) -> Result<mfd::Imported, mfd::MfdError> {
    let outcome = mfd::import_with_profile(
        &fixture.package().join("mapping.mfd"),
        &mfd::ImportOptions::default().with_package_root(fixture.package()),
        profile,
    );
    let snapshot = match &outcome {
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
    std::fs::write(
        fixture.0.join(format!("complete-{profile:?}-outcome.txt")),
        snapshot,
    )?;
    outcome.map(|outcome| outcome.imported)
}

fn is_boundary(warning: &str) -> bool {
    warning.contains("outside authorizing root")
        || warning.contains("outside the authorizing package root")
        || warning.contains("outside package root")
        || warning.contains("traverses above package root")
}

#[test]
fn rejected_unused_udf_recognizers_keep_contained_schema_behavior() -> Result<(), Box<dyn Error>> {
    for recursive in [false, true] {
        let fixture = Fixture::new()?;
        fixture.arrange(recursive, "parameter.xsd", "sub/../types.xsd")?;
        let observed = import(&fixture, mfd::ImportProfile::BestEffort);
        fixture.observe_import("import", &observed)?;
        let imported = observed?;
        assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
        let input =
            Instance::Group(vec![("Value".to_owned(), Instance::Scalar(Value::Int(17)))].into());
        let output = engine::run(&imported.project, &input);
        fixture.observe("output", &output)?;
        assert_eq!(output?, input);
    }
    Ok(())
}

#[test]
fn rejected_unused_udf_recognizers_cannot_hide_schema_refusals() -> Result<(), Box<dyn Error>> {
    for recursive in [false, true] {
        for (declared, dependency) in [
            ("parameter.xsd", "../outside/types.xsd"),
            ("../outside/parameter.xsd", "types.xsd"),
        ] {
            let fixture = Fixture::new()?;
            fixture.arrange(recursive, declared, dependency)?;
            let observed = import(&fixture, mfd::ImportProfile::BestEffort);
            fixture.observe_import("best-effort-import", &observed)?;
            let strict = import(&fixture, mfd::ImportProfile::Executable);
            fixture.observe_import("executable-import", &strict)?;
            let imported = observed?;
            assert!(
                imported.warnings.iter().any(|warning| is_boundary(warning)),
                "{:?}",
                imported.warnings
            );
            assert!(matches!(strict, Err(mfd::MfdError::IncompatibleImport(_))));
            // The unrelated primary mapping keeps its independently specified schema.
            assert_eq!(
                imported
                    .project
                    .source
                    .child("Value")
                    .map(|node| &node.kind),
                Some(&ir::SchemaKind::Scalar {
                    ty: ir::ScalarType::Int
                })
            );
        }
    }
    Ok(())
}
