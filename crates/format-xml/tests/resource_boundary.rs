use std::error::Error;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use format_xml::xsd;
use ir::{ScalarType, SchemaKind};

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Result<Self, std::io::Error> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_xsd_boundary_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(path.join("package/sub"))?;
        std::fs::create_dir_all(path.join("outside"))?;
        eprintln!("RESOURCE_BOUNDARY_ORIGINALS={}", path.display());
        Ok(Self(path))
    }

    fn package(&self) -> PathBuf {
        self.0.join("package")
    }

    fn observe<T: std::fmt::Debug>(
        &self,
        result: Result<T, format_xml::XmlFormatError>,
    ) -> Result<T, format_xml::XmlFormatError> {
        static NEXT_OUTCOME: AtomicU64 = AtomicU64::new(0);
        let snapshot = self.0.join(format!(
            "outcome-{}",
            NEXT_OUTCOME.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&snapshot).unwrap();
        let root = self.package().join("root.xsd");
        if root.is_file() {
            std::fs::copy(root, snapshot.join("root.xsd")).unwrap();
        }
        std::fs::write(snapshot.join("result.txt"), format!("{result:#?}")).unwrap();
        result
    }

    fn root(&self, location: &str) -> Result<PathBuf, std::io::Error> {
        let path = self.package().join("root.xsd");
        std::fs::write(
            &path,
            format!(
                r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
          <xs:include schemaLocation="{location}"/>
          <xs:element name="Document"><xs:complexType><xs:sequence>
            <xs:element name="Value" type="SelectedType"/>
          </xs:sequence></xs:complexType></xs:element>
        </xs:schema>"#
            ),
        )?;
        Ok(path)
    }
}

const TYPES: &str = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:simpleType name="SelectedType"><xs:restriction base="xs:int"/></xs:simpleType>
  <xs:complexType name="Base"><xs:sequence><xs:element name="Count" type="xs:int"/></xs:sequence></xs:complexType>
  <xs:complexType name="Derived"><xs:complexContent><xs:extension base="Base">
    <xs:sequence><xs:element name="Label" type="xs:string"/></xs:sequence>
  </xs:extension></xs:complexContent></xs:complexType>
</xs:schema>"#;

fn assert_integer(schema: &ir::SchemaNode) {
    assert_eq!(
        schema.child("Value").map(|node| &node.kind),
        Some(&SchemaKind::Scalar {
            ty: ScalarType::Int
        })
    );
}

fn assert_boundary(error: impl std::fmt::Display) {
    assert!(
        error.to_string().contains("outside authorizing root"),
        "{error}"
    );
}

#[test]
fn contained_parent_alias_and_absolute_dependencies_preserve_types() -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new()?;
    let leaf = fixture.package().join("types.xsd");
    std::fs::write(&leaf, TYPES)?;
    for location in [
        "types.xsd".to_owned(),
        "sub/../types.xsd".to_owned(),
        leaf.display().to_string(),
    ] {
        let root = fixture.root(&location)?;
        let imported = fixture.observe(xsd::import_root_with_resource_root(
            &root,
            Some("Document"),
            &fixture.package(),
        ))?;
        assert_integer(&imported);
        let derived = fixture.observe(xsd::import_type_with_resource_root(
            &root,
            "Derived",
            &fixture.package(),
        ))?;
        assert_eq!(
            derived.child("Count").map(|node| &node.kind),
            Some(&SchemaKind::Scalar {
                ty: ScalarType::Int
            })
        );
        assert!(derived.child("Label").is_some());
        assert_eq!(
            fixture.observe(xsd::import_type_base_with_resource_root(
                &root,
                "Derived",
                &fixture.package()
            ))?,
            Some("Base".to_owned())
        );
    }
    Ok(())
}

#[test]
fn escaping_relative_and_absolute_dependencies_refuse_without_silent_string_fallback()
-> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new()?;
    let outside = fixture.0.join("outside/types.xsd");
    std::fs::write(&outside, TYPES)?;
    for location in [
        "../outside/types.xsd".to_owned(),
        outside.display().to_string(),
    ] {
        let root = fixture.root(&location)?;
        // The unrestricted API remains a deliberate host-owned schema importer.
        assert_integer(&fixture.observe(xsd::import_root(&root, Some("Document")))?);
        assert_boundary(
            fixture
                .observe(xsd::import_root_with_resource_root(
                    &root,
                    Some("Document"),
                    &fixture.package(),
                ))
                .unwrap_err(),
        );
        assert_boundary(
            fixture
                .observe(xsd::import_type_with_resource_root(
                    &root,
                    "Derived",
                    &fixture.package(),
                ))
                .unwrap_err(),
        );
        assert_boundary(
            fixture
                .observe(xsd::import_type_base_with_resource_root(
                    &root,
                    "Derived",
                    &fixture.package(),
                ))
                .unwrap_err(),
        );
    }
    Ok(())
}

#[test]
fn two_hop_include_keeps_original_authorizing_root() -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new()?;
    std::fs::write(fixture.0.join("outside/types.xsd"), TYPES)?;
    std::fs::write(
        fixture.package().join("sub/bridge.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:include schemaLocation="../../outside/types.xsd"/></xs:schema>"#,
    )?;
    let root = fixture.root("sub/bridge.xsd")?;
    assert_integer(&fixture.observe(xsd::import(&root))?);
    assert_boundary(
        fixture
            .observe(xsd::import_root_with_resource_root(
                &root,
                None,
                &fixture.package(),
            ))
            .unwrap_err(),
    );
    Ok(())
}

#[test]
fn namespace_import_and_selected_external_root_are_confined() -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new()?;
    let content = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema" targetNamespace="urn:neutral:types">
      <xs:element name="Count" type="xs:int"/>
    </xs:schema>"#;
    let main = fixture.package().join("root.xsd");
    for (location, inside) in [("types.xsd", true), ("../outside/types.xsd", false)] {
        let leaf = if inside {
            fixture.package().join("types.xsd")
        } else {
            fixture.0.join("outside/types.xsd")
        };
        std::fs::write(leaf, content)?;
        std::fs::write(
            &main,
            format!(
                r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:import namespace="urn:neutral:types" schemaLocation="{location}"/></xs:schema>"#
            ),
        )?;
        let actual = fixture.observe(xsd::import_root_with_resource_root(
            &main,
            Some("{urn:neutral:types}Count"),
            &fixture.package(),
        ));
        if inside {
            assert_eq!(
                actual?.kind,
                SchemaKind::Scalar {
                    ty: ScalarType::Int
                }
            );
        } else {
            assert_boundary(actual.unwrap_err());
        }
    }
    Ok(())
}

#[cfg(unix)]
#[test]
fn canonical_file_and_directory_symlinks_accept_only_contained_targets()
-> Result<(), Box<dyn Error>> {
    use std::os::unix::fs::symlink;
    let fixture = Fixture::new()?;
    std::fs::write(fixture.package().join("types.xsd"), TYPES)?;
    std::fs::write(fixture.0.join("outside/types.xsd"), TYPES)?;
    symlink(
        fixture.package().join("types.xsd"),
        fixture.package().join("inside-link.xsd"),
    )?;
    symlink(
        fixture.0.join("outside/types.xsd"),
        fixture.package().join("outside-link.xsd"),
    )?;
    symlink(
        fixture.0.join("outside"),
        fixture.package().join("outside-directory"),
    )?;
    symlink(fixture.package(), fixture.0.join("root-alias"))?;
    let alias = fixture.0.join("root-alias");
    assert_integer(&fixture.observe(xsd::import_root_with_resource_root(
        &fixture.root("inside-link.xsd")?,
        None,
        &alias,
    ))?);
    for location in ["outside-link.xsd", "outside-directory/types.xsd"] {
        assert_boundary(
            fixture
                .observe(xsd::import_root_with_resource_root(
                    &fixture.root(location)?,
                    None,
                    &fixture.package(),
                ))
                .unwrap_err(),
        );
    }
    Ok(())
}

#[test]
fn top_level_resource_cannot_cross_root() -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new()?;
    let outside = fixture.0.join("outside/root.xsd");
    std::fs::write(
        &outside,
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Value" type="xs:int"/></xs:schema>"#,
    )?;
    assert_boundary(
        fixture
            .observe(xsd::import_root_with_resource_root(
                &outside,
                None,
                &fixture.package(),
            ))
            .unwrap_err(),
    );
    Ok(())
}
