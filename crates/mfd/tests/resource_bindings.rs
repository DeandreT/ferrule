//! Original generic public-API controls. No vendor/customer fixtures or payload
//! execution. These tests import schemas; they do not execute a mapping.
use std::error::Error;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use ir::{ScalarType, SchemaKind};
use mfd::{
    ImportOptions, ResourceBindingKindV1 as Kind, ResourceBindingV1 as Binding, ResourceReadSetV1,
};
use sha2::{Digest, Sha256};

const ROOT_XSD: &str = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema" xmlns:b="urn:binding" targetNamespace="urn:binding" elementFormDefault="qualified"><xs:include schemaLocation="types/value.xsd"/><xs:element name="Packet"><xs:complexType><xs:sequence><xs:element name="Quantity" type="b:Count"/></xs:sequence></xs:complexType></xs:element></xs:schema>"#;
const TYPES: &str = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema" targetNamespace="urn:binding"><xs:simpleType name="Count"><xs:restriction base="xs:int"/></xs:simpleType></xs:schema>"#;
const TARGET: &str = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Result"><xs:complexType><xs:sequence><xs:element name="Quantity" type="xs:int"/></xs:sequence></xs:complexType></xs:element></xs:schema>"#;

fn hash(bytes: impl AsRef<[u8]>) -> [u8; 32] {
    Sha256::digest(bytes.as_ref()).into()
}

struct Fixture {
    root: PathBuf,
    package: PathBuf,
    catalog: PathBuf,
    mapping: PathBuf,
}

impl Fixture {
    fn new() -> Result<Self, Box<dyn Error>> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "ferrule_binding_v1_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&root)?;
        let package = root.join("package");
        let catalog = root.join("catalog");
        std::fs::create_dir_all(package.join("maps"))?;
        std::fs::create_dir_all(catalog.join("types"))?;
        let mapping = package.join("maps/design.mfd");
        std::fs::write(package.join("maps/target.xsd"), TARGET)?;
        std::fs::write(catalog.join("input.xsd"), ROOT_XSD)?;
        std::fs::write(catalog.join("types/value.xsd"), TYPES)?;
        let fixture = Self {
            root,
            package,
            catalog,
            mapping,
        };
        fixture.xml_mapping("RelocatedInput.xsd")?;
        Ok(fixture)
    }

    fn xml_mapping(&self, reference: &str) -> Result<(), std::io::Error> {
        std::fs::write(
            &self.mapping,
            format!(
                r#"<mapping><component name="defaultmap" uid="1"><structure><children>
          <component name="input" library="xml" uid="2" kind="14"><data><root><entry name="Packet"><entry name="Quantity" outkey="10"/></entry></root><document schema="{reference}" instanceroot="{{urn:binding}}Packet"/></data></component>
          <component name="output" library="xml" uid="3" kind="14"><properties XSLTDefaultOutput="1"/><data><root><entry name="Result"><entry name="Quantity" inpkey="20"/></entry></root><document schema="target.xsd" instanceroot="{{}}Result"/></data></component>
          </children><graph><vertices><vertex vertexkey="10"><edges><edge vertexkey="20"/></edges></vertex></vertices></graph></structure></component></mapping>"#
            ),
        )
    }

    fn binding(
        &self,
        kind: Kind,
        reference: &str,
        digest: [u8; 32],
    ) -> Result<Binding, mfd::MfdError> {
        Binding::new(
            kind,
            &self.mapping,
            reference,
            &self.catalog,
            "input.xsd",
            digest,
        )
    }

    fn options(
        &self,
        bindings: Vec<Binding>,
        readset: ResourceReadSetV1,
    ) -> Result<ImportOptions, mfd::MfdError> {
        ImportOptions::default()
            .with_package_root(&self.package)
            .with_resource_readset_v1(readset)
            .with_resource_bindings_v1(bindings)
    }

    fn edi(&self, reference: &str) -> Result<(), Box<dyn Error>> {
        std::fs::write(
            self.catalog.join("Fields.Segment"),
            r#"<Config><Elements><Data name="F1" type="string"/><Data name="F143" type="string"/><Data name="F999" type="integer"/><Segment name="ISA"><Data ref="F1"/></Segment><Segment name="GS"><Data ref="F1"/></Segment><Segment name="ST"><Data ref="F143"/></Segment><Segment name="BEG"><Data ref="F999"/></Segment><Segment name="SE"><Data ref="F1"/></Segment><Segment name="GE"><Data ref="F1"/></Segment><Segment name="IEA"><Data ref="F1"/></Segment></Elements></Config>"#,
        )?;
        std::fs::write(
            self.catalog.join("Envelope.Config"),
            r#"<Config><Format standard="X12"/><Include href="Fields.Segment"/><Group name="Envelope"><Group name="Interchange" maxOccurs="unbounded"><Segment ref="ISA"/><Group name="Group" maxOccurs="unbounded"><Segment ref="GS"/><Select field="ST/F143" maxOccurs="unbounded"/><Segment ref="GE" minOccurs="0"/></Group><Segment ref="IEA" minOccurs="0"/></Group></Group></Config>"#,
        )?;
        std::fs::write(
            self.catalog.join("850.Config"),
            r#"<Config><Format standard="X12"/><Include href="Fields.Segment"/><Message><MessageType>850</MessageType><Group name="Message_850" maxOccurs="unbounded"><Segment ref="ST"/><Segment ref="BEG"/><Segment ref="SE"/></Group></Message></Config>"#,
        )?;
        std::fs::write(
            &self.mapping,
            format!(
                r#"<mapping><resources/><component name="defaultmap" uid="1"><structure><children>
          <component name="input" library="text" uid="2" kind="16"><data><root><entry name="FileInstance"><entry name="document"><entry name="Envelope"><entry name="Interchange"><entry name="Group"><entry name="Message_850"><entry name="BEG"><entry name="F999" outkey="10"/></entry></entry></entry></entry></entry></entry></entry></root><text type="edi" kind="EDIX12" config="{reference}"><messages><message type="850"/></messages></text></data></component>
          <component name="output" library="xml" uid="3" kind="14"><properties XSLTDefaultOutput="1"/><data><root><entry name="Result"><entry name="Quantity" inpkey="20"/></entry></root><document schema="target.xsd" instanceroot="{{}}Result"/></data></component>
          </children><graph><vertices><vertex vertexkey="10"><edges><edge vertexkey="20"/></edges></vertex></vertices></graph></structure></component></mapping>"#
            ),
        )?;
        Ok(())
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn integer_source(imported: &mfd::Imported) {
    assert_eq!(
        imported.project.source.child("Quantity").map(|n| &n.kind),
        Some(&SchemaKind::Scalar {
            ty: ScalarType::Int
        })
    );
}

fn refusal(result: Result<mfd::Imported, mfd::MfdError>, contains: &str) {
    let message = match result {
        Ok(_) => panic!("binding failure published a partial project"),
        Err(e) => e.to_string(),
    };
    assert!(message.contains(contains), "{message}");
}

#[test]
fn xml_relocation_with_include_uses_public_import_and_keeps_private_readset()
-> Result<(), Box<dyn Error>> {
    let f = Fixture::new()?;
    let original = std::fs::read(&f.mapping)?;
    let reads = ResourceReadSetV1::default();
    let binding = f
        .binding(Kind::XmlSchema, "RelocatedInput.xsd", hash(ROOT_XSD))?
        .with_origin_sha256(hash(&original))
        .with_dependency_pin("types/value.xsd", hash(TYPES))?;
    let options = f.options(vec![binding], reads.clone())?;
    let imported = mfd::import_with_options(&f.mapping, &options)?;
    integer_source(&imported);
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert_eq!(std::fs::read(&f.mapping)?, original);
    let records = reads.records();
    assert_eq!(records.len(), 2);
    assert!(
        records
            .iter()
            .all(|r| r.original_reference == "RelocatedInput.xsd"
                && r.delivered
                && r.expected_sha256 == Some(r.sha256))
    );
    assert!(
        records
            .iter()
            .all(|r| r.resolved_resource.is_absolute() && r.resolved_resource.file_name().is_some())
    );
    assert!(
        records
            .iter()
            .all(|r| r.original_origin_sha256 == hash(&original))
    );
    let resolved = records
        .iter()
        .find(|r| r.resolved_resource.ends_with("input.xsd"))
        .unwrap();
    let escaped = serde_json::to_string(
        &resolved
            .resolved_resource
            .parent()
            .unwrap()
            .to_string_lossy(),
    )?;
    assert!(!serde_json::to_string(&imported.project)?.contains(&escaped[1..escaped.len() - 1]));
    Ok(())
}

#[test]
fn an_explicit_origin_pin_checks_the_actual_parsed_mfd_snapshot() -> Result<(), Box<dyn Error>> {
    let f = Fixture::new()?;
    let reads = ResourceReadSetV1::default();
    let binding = f
        .binding(Kind::XmlSchema, "RelocatedInput.xsd", hash(ROOT_XSD))?
        .with_origin_sha256([0; 32]);
    refusal(
        mfd::import_with_options(&f.mapping, &f.options(vec![binding], reads.clone())?),
        "original MFD SHA-256 mismatch",
    );
    assert!(reads.records().is_empty());
    Ok(())
}

#[test]
fn prepared_root_and_include_prove_consumed_bytes_are_not_reopened_live_paths()
-> Result<(), Box<dyn Error>> {
    let f = Fixture::new()?;
    let reads = ResourceReadSetV1::default();
    let binding = f
        .binding(Kind::XmlSchema, "RelocatedInput.xsd", hash(ROOT_XSD))?
        .with_dependency_pin("types/value.xsd", hash(TYPES))?
        .prepare()?;
    // A hash-then-import(path) implementation would parse these invalid bytes.
    std::fs::write(f.catalog.join("input.xsd"), b"replacement, not XML")?;
    std::fs::write(f.catalog.join("types/value.xsd"), b"different replacement")?;
    let imported = mfd::import_with_options(&f.mapping, &f.options(vec![binding], reads.clone())?)?;
    integer_source(&imported);
    let records = reads.records();
    assert_eq!(records.len(), 2);
    assert!(
        records.iter().all(|r| r.delivered
            && r.from_prepared_snapshot
            && r.expected_sha256 == Some(r.sha256))
    );
    assert!(records.iter().any(|r| r.sha256 == hash(ROOT_XSD)));
    assert!(records.iter().any(|r| r.sha256 == hash(TYPES)));
    // All native handles must have closed before the caller regains control.
    std::fs::rename(&f.catalog, f.root.join("released-catalog"))?;
    Ok(())
}

#[test]
fn root_only_pin_records_changed_unpinned_dependency_without_claiming_sealed_closure()
-> Result<(), Box<dyn Error>> {
    let f = Fixture::new()?;
    let reads = ResourceReadSetV1::default();
    let binding = f
        .binding(Kind::XmlSchema, "RelocatedInput.xsd", hash(ROOT_XSD))?
        .prepare()?;
    let changed = TYPES.replace("xs:int", "xs:long");
    std::fs::write(f.catalog.join("types/value.xsd"), &changed)?;
    let imported = mfd::import_with_options(&f.mapping, &f.options(vec![binding], reads.clone())?)?;
    integer_source(&imported);
    let records = reads.records();
    assert_eq!(records.len(), 2);
    let child = records
        .iter()
        .find(|r| r.resolved_resource.ends_with("types/value.xsd"))
        .unwrap();
    assert_eq!(child.expected_sha256, None);
    assert_eq!(child.sha256, hash(changed));
    assert!(!child.from_prepared_snapshot);
    Ok(())
}

#[test]
fn root_digest_mismatch_refuses_every_public_profile_even_with_package_fallback()
-> Result<(), Box<dyn Error>> {
    for profile in [
        mfd::ImportProfile::BestEffort,
        mfd::ImportProfile::Executable,
    ] {
        let f = Fixture::new()?;
        std::fs::write(f.package.join("maps/RelocatedInput.xsd"), ROOT_XSD)?;
        std::fs::create_dir_all(f.package.join("maps/types"))?;
        std::fs::write(f.package.join("maps/types/value.xsd"), TYPES)?;
        let reads = ResourceReadSetV1::default();
        let binding = f.binding(Kind::XmlSchema, "RelocatedInput.xsd", [0; 32])?;
        let options = f.options(vec![binding], reads.clone())?;
        let result = mfd::import_with_profile(&f.mapping, &options, profile);
        let message = match result {
            Ok(_) => panic!("mismatched binding was published"),
            Err(e) => e.to_string(),
        };
        assert!(message.contains("SHA-256 mismatch"), "{message}");
        let records = reads.records();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].sha256, hash(ROOT_XSD));
        assert!(!records[0].delivered);
    }
    Ok(())
}

#[test]
fn dependency_mismatch_is_sticky_across_optional_xsd_searches() -> Result<(), Box<dyn Error>> {
    let f = Fixture::new()?;
    let reads = ResourceReadSetV1::default();
    let binding = f
        .binding(Kind::XmlSchema, "RelocatedInput.xsd", hash(ROOT_XSD))?
        .with_dependency_pin("types/value.xsd", [0; 32])?;
    refusal(
        mfd::import_with_options(&f.mapping, &f.options(vec![binding], reads.clone())?),
        "SHA-256 mismatch",
    );
    assert!(
        reads
            .records()
            .iter()
            .any(|r| r.resolved_resource.ends_with("types/value.xsd") && !r.delivered)
    );
    Ok(())
}

#[test]
fn pins_cover_raw_utf16_bytes_not_reencoded_text() -> Result<(), Box<dyn Error>> {
    let f = Fixture::new()?;
    let mut raw = vec![0xff, 0xfe];
    for unit in ROOT_XSD.encode_utf16() {
        raw.extend_from_slice(&unit.to_le_bytes());
    }
    std::fs::write(f.catalog.join("input.xsd"), &raw)?;
    let wrong = f.binding(Kind::XmlSchema, "RelocatedInput.xsd", hash(ROOT_XSD))?;
    refusal(
        mfd::import_with_options(
            &f.mapping,
            &f.options(vec![wrong], ResourceReadSetV1::default())?,
        ),
        "SHA-256 mismatch",
    );
    let reads = ResourceReadSetV1::default();
    let correct = f.binding(Kind::XmlSchema, "RelocatedInput.xsd", hash(&raw))?;
    let imported = mfd::import_with_options(&f.mapping, &f.options(vec![correct], reads.clone())?)?;
    integer_source(&imported);
    assert!(
        reads
            .records()
            .iter()
            .any(|r| r.expected_sha256 == Some(hash(&raw)) && r.sha256 == hash(&raw))
    );
    Ok(())
}

#[test]
fn wrong_kind_has_no_body_read_and_cannot_fall_back_to_valid_original() -> Result<(), Box<dyn Error>>
{
    let f = Fixture::new()?;
    std::fs::write(f.package.join("maps/RelocatedInput.xsd"), ROOT_XSD)?;
    std::fs::create_dir_all(f.package.join("maps/types"))?;
    std::fs::write(f.package.join("maps/types/value.xsd"), TYPES)?;
    let reads = ResourceReadSetV1::default();
    let binding = f.binding(Kind::EdiConfig, "RelocatedInput.xsd", hash(ROOT_XSD))?;
    refusal(
        mfd::import_with_options(&f.mapping, &f.options(vec![binding], reads.clone())?),
        "kind",
    );
    assert!(reads.records().is_empty());
    Ok(())
}

#[test]
fn duplicates_including_cross_kind_are_errors_before_any_target_open() -> Result<(), Box<dyn Error>>
{
    let f = Fixture::new()?;
    let binding = f.binding(Kind::XmlSchema, "RelocatedInput.xsd", hash(ROOT_XSD))?;
    assert!(
        f.options(
            vec![binding.clone(), binding.clone()],
            ResourceReadSetV1::default()
        )
        .is_err()
    );
    let other = Binding::new(
        Kind::EdiConfig,
        &f.mapping,
        "RelocatedInput.xsd",
        &f.catalog,
        "input.xsd",
        [1; 32],
    )?;
    std::fs::remove_dir_all(&f.catalog)?;
    assert!(
        f.options(
            vec![binding.clone(), binding.clone()],
            ResourceReadSetV1::default()
        )
        .is_err()
    );
    assert!(
        f.options(vec![binding, other], ResourceReadSetV1::default())
            .is_err()
    );
    Ok(())
}

#[test]
fn unrelated_origin_and_raw_reference_do_not_capture_an_unbound_request()
-> Result<(), Box<dyn Error>> {
    let f = Fixture::new()?;
    std::fs::write(f.package.join("maps/RelocatedInput.xsd"), ROOT_XSD)?;
    std::fs::create_dir_all(f.package.join("maps/types"))?;
    std::fs::write(f.package.join("maps/types/value.xsd"), TYPES)?;
    let baseline = mfd::import_with_options(
        &f.mapping,
        &ImportOptions::default().with_package_root(&f.package),
    )?;
    let other = f.package.join("maps/other.mfd");
    std::fs::copy(&f.mapping, &other)?;
    let a = Binding::new(
        Kind::XmlSchema,
        &other,
        "RelocatedInput.xsd",
        &f.catalog,
        "input.xsd",
        [0; 32],
    )?;
    let b = f.binding(Kind::XmlSchema, ".\\RelocatedInput.xsd", [0; 32])?;
    let reads = ResourceReadSetV1::default();
    let imported = mfd::import_with_options(&f.mapping, &f.options(vec![a, b], reads.clone())?)?;
    assert_eq!(
        serde_json::to_value(&baseline.project)?,
        serde_json::to_value(&imported.project)?
    );
    assert_eq!(baseline.warnings, imported.warnings);
    assert!(reads.records().is_empty());
    Ok(())
}

#[test]
fn empty_opt_in_preserves_unbound_missing_schema_behavior() -> Result<(), Box<dyn Error>> {
    let f = Fixture::new()?;
    let baseline = mfd::import_with_options(
        &f.mapping,
        &ImportOptions::default().with_package_root(&f.package),
    )?;
    let reads = ResourceReadSetV1::default();
    let imported = mfd::import_with_options(&f.mapping, &f.options(Vec::new(), reads.clone())?)?;
    assert_eq!(
        serde_json::to_value(&baseline.project)?,
        serde_json::to_value(&imported.project)?
    );
    assert_eq!(baseline.warnings, imported.warnings);
    assert!(reads.records().is_empty());
    Ok(())
}

#[test]
fn an_unbound_alias_of_the_same_file_does_not_borrow_bound_root_authority()
-> Result<(), Box<dyn Error>> {
    let f = Fixture::new()?;
    std::fs::create_dir(f.root.join("external"))?;
    std::fs::write(f.root.join("external/types.xsd"), TYPES)?;
    let shared = ROOT_XSD.replace("types/value.xsd", "../../external/types.xsd");
    std::fs::write(f.package.join("maps/shared.xsd"), &shared)?;
    std::fs::write(
        f.package.join("maps/target.xsd"),
        TARGET.replace(
            "</xs:sequence>",
            "<xs:element name=\"Other\" type=\"xs:string\"/></xs:sequence>",
        ),
    )?;
    std::fs::write(
        &f.mapping,
        r#"<mapping><component name="defaultmap" uid="1"><structure><children>
      <component name="authorized" library="xml" uid="2" kind="14"><data><root><entry name="Packet"><entry name="Quantity" outkey="10"/></entry></root><document schema="RelocatedInput.xsd" instanceroot="{urn:binding}Packet"/></data></component>
      <component name="unbound" library="xml" uid="4" kind="14"><data><root><entry name="Packet"><entry name="Quantity" outkey="11"/></entry></root><document schema="shared.xsd" instanceroot="{urn:binding}Packet"/></data></component>
      <component name="output" library="xml" uid="3" kind="14"><properties XSLTDefaultOutput="1"/><data><root><entry name="Result"><entry name="Quantity" inpkey="20"/><entry name="Other" inpkey="21"/></entry></root><document schema="target.xsd" instanceroot="{}Result"/></data></component>
      </children><graph><vertices><vertex vertexkey="10"><edges><edge vertexkey="20"/></edges></vertex><vertex vertexkey="11"><edges><edge vertexkey="21"/></edges></vertex></vertices></graph></structure></component></mapping>"#,
    )?;
    let reads = ResourceReadSetV1::default();
    let binding = Binding::new(
        Kind::XmlSchema,
        &f.mapping,
        "RelocatedInput.xsd",
        &f.root,
        "package/maps/shared.xsd",
        hash(shared),
    )?;
    let imported = mfd::import_with_options(&f.mapping, &f.options(vec![binding], reads.clone())?)?;
    integer_source(&imported);
    assert_eq!(imported.project.extra_sources.len(), 1);
    assert_eq!(
        imported.project.extra_sources[0]
            .schema
            .child("Quantity")
            .map(|n| &n.kind),
        Some(&SchemaKind::Scalar {
            ty: ScalarType::String
        })
    );
    assert!(
        imported
            .warnings
            .iter()
            .any(|w| w.contains("outside authorizing root"))
    );
    assert!(
        reads
            .records()
            .iter()
            .all(|r| r.original_reference == "RelocatedInput.xsd")
    );
    Ok(())
}

#[test]
fn an_xml_binding_does_not_dispatch_to_another_format_by_filename_or_contents()
-> Result<(), Box<dyn Error>> {
    let f = Fixture::new()?;
    let wrong = "<Config><Format standard=\"X12\"/></Config>";
    std::fs::write(f.catalog.join("input.xsd"), wrong)?;
    let binding = f.binding(Kind::XmlSchema, "RelocatedInput.xsd", hash(wrong))?;
    refusal(
        mfd::import_with_options(
            &f.mapping,
            &f.options(vec![binding], ResourceReadSetV1::default())?,
        ),
        "bound resource",
    );
    Ok(())
}

#[test]
fn legacy_edi_reference_uses_exact_selected_bytes_via_public_import() -> Result<(), Box<dyn Error>>
{
    let f = Fixture::new()?;
    let reference = "../../../RetiredInstall/X12/Envelope.Config";
    f.edi(reference)?;
    let original = std::fs::read(&f.mapping)?;
    let root = std::fs::read(f.catalog.join("Envelope.Config"))?;
    let reads = ResourceReadSetV1::default();
    let binding = Binding::new(
        Kind::EdiConfig,
        &f.mapping,
        reference,
        &f.catalog,
        "Envelope.Config",
        hash(root),
    )?;
    let imported = mfd::import_with_options(&f.mapping, &f.options(vec![binding], reads.clone())?)?;
    let field = imported
        .project
        .source
        .child("Interchange")
        .and_then(|n| n.child("Group"))
        .and_then(|n| n.child("Message_850"))
        .and_then(|n| n.child("BEG"))
        .and_then(|n| n.child("F999"));
    assert_eq!(
        field.map(|n| &n.kind),
        Some(&SchemaKind::Scalar {
            ty: ScalarType::Int
        })
    );
    assert!(imported.project.runtime_dependencies().is_empty());
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let records = reads.records();
    assert_eq!(records.len(), 3);
    assert!(
        records
            .iter()
            .all(|r| r.original_reference == reference && r.delivered)
    );
    assert_eq!(
        records
            .iter()
            .filter(|r| r.expected_sha256.is_some())
            .count(),
        1
    );
    assert_eq!(std::fs::read(&f.mapping)?, original);
    Ok(())
}

#[test]
fn bound_edi_will_not_scan_for_a_message_replacement() -> Result<(), Box<dyn Error>> {
    let f = Fixture::new()?;
    let reference = "../../../RetiredInstall/X12/Envelope.Config";
    f.edi(reference)?;
    std::fs::rename(f.catalog.join("850.Config"), f.catalog.join("trap.Config"))?;
    let digest = hash(std::fs::read(f.catalog.join("Envelope.Config"))?);
    let reads = ResourceReadSetV1::default();
    let binding = Binding::new(
        Kind::EdiConfig,
        &f.mapping,
        reference,
        &f.catalog,
        "Envelope.Config",
        digest,
    )?;
    refusal(
        mfd::import_with_options(&f.mapping, &f.options(vec![binding], reads.clone())?),
        "bound resource",
    );
    assert!(
        reads
            .records()
            .iter()
            .all(|r| !r.resolved_resource.ends_with("trap.Config"))
    );
    Ok(())
}

#[test]
fn escaping_include_is_not_delivered_or_silently_defaulted() -> Result<(), Box<dyn Error>> {
    let f = Fixture::new()?;
    let root = ROOT_XSD.replace("types/value.xsd", "../outside.xsd");
    std::fs::write(f.catalog.join("input.xsd"), &root)?;
    std::fs::write(f.root.join("outside.xsd"), TYPES)?;
    let reads = ResourceReadSetV1::default();
    let binding = f.binding(Kind::XmlSchema, "RelocatedInput.xsd", hash(root))?;
    refusal(
        mfd::import_with_options(&f.mapping, &f.options(vec![binding], reads.clone())?),
        "bound resource",
    );
    assert_eq!(reads.records().len(), 1);
    Ok(())
}

#[test]
fn path_and_request_limits_are_enforced_before_target_reads() -> Result<(), Box<dyn Error>> {
    let f = Fixture::new()?;
    for path in [
        "../outside.xsd",
        "C:/outside.xsd",
        "\\\\server\\share",
        "file:stream",
        "NUL",
        "a/.. ",
        "types/value.xsd.",
    ] {
        assert!(
            Binding::new(
                Kind::XmlSchema,
                &f.mapping,
                "RelocatedInput.xsd",
                &f.catalog,
                path,
                [0; 32]
            )
            .is_err(),
            "{path}"
        );
    }
    assert!(
        f.binding(Kind::XmlSchema, &"x".repeat(4097), [0; 32])
            .is_err()
    );
    let binding = f.binding(Kind::XmlSchema, "RelocatedInput.xsd", hash(ROOT_XSD))?;
    assert!(
        f.options(vec![binding; 65], ResourceReadSetV1::default())
            .is_err()
    );
    Ok(())
}

#[test]
fn noncanonical_pin_and_include_aliases_cannot_bypass_an_expected_digest()
-> Result<(), Box<dyn Error>> {
    let f = Fixture::new()?;
    let reads = ResourceReadSetV1::default();
    let binding = f
        .binding(Kind::XmlSchema, "RelocatedInput.xsd", hash(ROOT_XSD))?
        .with_dependency_pin("Types/value.xsd", [0; 32])?;
    refusal(
        mfd::import_with_options(&f.mapping, &f.options(vec![binding], reads.clone())?),
        "pin identity",
    );
    assert!(reads.records().is_empty());
    let root = ROOT_XSD.replace("types/value.xsd", "TYPES/value.xsd");
    std::fs::write(f.catalog.join("input.xsd"), &root)?;
    let binding = f
        .binding(Kind::XmlSchema, "RelocatedInput.xsd", hash(root))?
        .with_dependency_pin("types/value.xsd", [0; 32])?;
    refusal(
        mfd::import_with_options(&f.mapping, &f.options(vec![binding], reads.clone())?),
        "bound resource open refused",
    );
    assert_eq!(reads.records().len(), 1);
    Ok(())
}

#[test]
fn missing_or_nondirectory_trusted_roots_refuse_without_delivery() -> Result<(), Box<dyn Error>> {
    for as_file in [false, true] {
        let f = Fixture::new()?;
        let binding = f.binding(Kind::XmlSchema, "RelocatedInput.xsd", hash(ROOT_XSD))?;
        std::fs::remove_dir_all(&f.catalog)?;
        if as_file {
            std::fs::write(&f.catalog, b"not a directory")?;
        }
        let reads = ResourceReadSetV1::default();
        refusal(
            mfd::import_with_options(&f.mapping, &f.options(vec![binding], reads.clone())?),
            "trusted root",
        );
        assert!(reads.records().is_empty());
    }
    Ok(())
}

#[cfg(any(unix, windows))]
#[test]
fn escaping_directory_link_is_not_a_transitive_authorization() -> Result<(), Box<dyn Error>> {
    let f = Fixture::new()?;
    let outside = f.root.join("outside-directory");
    std::fs::create_dir(&outside)?;
    std::fs::write(outside.join("value.xsd"), TYPES)?;
    std::fs::remove_dir_all(f.catalog.join("types"))?;
    directory_link(&outside, &f.catalog.join("types"))?;
    let reads = ResourceReadSetV1::default();
    let binding = f.binding(Kind::XmlSchema, "RelocatedInput.xsd", hash(ROOT_XSD))?;
    refusal(
        mfd::import_with_options(&f.mapping, &f.options(vec![binding], reads.clone())?),
        "bound resource open refused",
    );
    assert_eq!(reads.records().len(), 1);
    Ok(())
}

#[cfg(any(unix, windows))]
fn linked_file_fixture(target: &Path, link: &Path) -> std::io::Result<PathBuf> {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, link)?;
        Ok(link.to_path_buf())
    }
    #[cfg(windows)]
    {
        // A real directory reparse boundary is available without changing
        // Developer Mode or token privileges. The returned file path traverses
        // that boundary; following it would reach the outside authored bytes.
        // This does not claim coverage of privilege-gated file-symlink tags.
        let junction = link.with_extension("junction");
        directory_link(
            target
                .parent()
                .ok_or_else(|| std::io::Error::other("target parent missing"))?,
            &junction,
        )?;
        Ok(junction.join(
            target
                .file_name()
                .ok_or_else(|| std::io::Error::other("target filename missing"))?,
        ))
    }
}

#[cfg(any(unix, windows))]
fn directory_link(target: &Path, link: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, link)
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        for path in [target, link] {
            if path.to_string_lossy().chars().any(|c| {
                matches!(
                    c,
                    '&' | '|' | '<' | '>' | '^' | '%' | '!' | '"' | '\r' | '\n'
                )
            }) {
                return Err(std::io::Error::other(
                    "junction fixture path contains shell metacharacters",
                ));
            }
        }
        let cmd = PathBuf::from(
            std::env::var_os("SystemRoot")
                .ok_or_else(|| std::io::Error::other("SystemRoot missing"))?,
        )
        .join("System32/cmd.exe");
        let outcome = std::process::Command::new(cmd)
            .args(["/d", "/c", "mklink", "/J"])
            .arg(link)
            .arg(target)
            .output()?;
        if !outcome.status.success() {
            return Err(std::io::Error::other(format!(
                "junction fixture failed: {}",
                String::from_utf8_lossy(&outcome.stderr)
            )));
        }
        if std::fs::symlink_metadata(link)?.file_attributes() & 0x400 == 0 {
            return Err(std::io::Error::other(
                "junction fixture did not create a reparse point",
            ));
        }
        eprintln!(
            "WINDOWS_REPARSE_FIXTURE=directory-junction; file-symlink-tag coverage is separate"
        );
        Ok(())
    }
}

#[cfg(any(unix, windows))]
#[test]
fn escaping_and_dangling_target_links_refuse_public_import_before_delivery()
-> Result<(), Box<dyn Error>> {
    // Unix exercises file symlinks; Windows exercises actual directory reparse
    // paths without pretending the file-symlink privilege is available.
    for dangling in [false, true] {
        let f = Fixture::new()?;
        let outside_directory = f.root.join("outside-target");
        std::fs::create_dir(&outside_directory)?;
        let outside = outside_directory.join("outside.xsd");
        std::fs::write(&outside, ROOT_XSD)?;
        std::fs::remove_file(f.catalog.join("input.xsd"))?;
        let effective_target = linked_file_fixture(&outside, &f.catalog.join("input.xsd"))?;
        if dangling {
            std::fs::remove_dir_all(&outside_directory)?;
        }
        let reads = ResourceReadSetV1::default();
        let binding = Binding::new(
            Kind::XmlSchema,
            &f.mapping,
            "RelocatedInput.xsd",
            &f.catalog,
            effective_target.strip_prefix(&f.catalog)?,
            hash(ROOT_XSD),
        )?;
        refusal(
            mfd::import_with_options(&f.mapping, &f.options(vec![binding], reads.clone())?),
            "bound resource open refused",
        );
        assert!(reads.records().is_empty());
    }
    Ok(())
}

#[cfg(any(unix, windows))]
#[test]
fn escaping_include_link_cannot_yield_an_authoritative_default_schema() -> Result<(), Box<dyn Error>>
{
    let f = Fixture::new()?;
    let outside_directory = f.root.join("outside-include");
    std::fs::create_dir(&outside_directory)?;
    std::fs::write(outside_directory.join("value.xsd"), TYPES)?;
    std::fs::remove_file(f.catalog.join("types/value.xsd"))?;
    let effective_target = linked_file_fixture(
        &outside_directory.join("value.xsd"),
        &f.catalog.join("types/value.xsd"),
    )?;
    let effective_reference = effective_target
        .strip_prefix(&f.catalog)?
        .to_string_lossy()
        .replace('\\', "/");
    let root = ROOT_XSD.replace("types/value.xsd", &effective_reference);
    std::fs::write(f.catalog.join("input.xsd"), &root)?;
    let reads = ResourceReadSetV1::default();
    let binding = f.binding(Kind::XmlSchema, "RelocatedInput.xsd", hash(root))?;
    refusal(
        mfd::import_with_options(&f.mapping, &f.options(vec![binding], reads.clone())?),
        "bound resource open refused",
    );
    assert_eq!(reads.records().len(), 1);
    Ok(())
}

#[test]
fn byte_budget_refuses_before_publication() -> Result<(), Box<dyn Error>> {
    let f = Fixture::new()?;
    let reference = "../../../RetiredInstall/X12/Envelope.Config";
    f.edi(reference)?;
    std::fs::write(
        f.catalog.join("Envelope.Config"),
        vec![b' '; 8 * 1024 * 1024 + 1],
    )?;
    let binding = Binding::new(
        Kind::EdiConfig,
        &f.mapping,
        reference,
        &f.catalog,
        "Envelope.Config",
        [0; 32],
    )?;
    refusal(
        mfd::import_with_options(
            &f.mapping,
            &f.options(vec![binding], ResourceReadSetV1::default())?,
        ),
        "byte limit",
    );
    Ok(())
}

#[test]
fn pipeline_public_api_uses_bound_bytes_and_refuses_a_mismatch() -> Result<(), Box<dyn Error>> {
    for wrong in [false, true] {
        let f = Fixture::new()?;
        let source = TARGET.replace("name=\"Result\"", "name=\"Packet\"");
        let buffer = TARGET.replace("name=\"Result\"", "name=\"Buffer\"");
        std::fs::write(f.catalog.join("input.xsd"), &source)?;
        std::fs::write(f.package.join("maps/buffer.xsd"), buffer)?;
        std::fs::write(
            &f.mapping,
            r#"<mapping><component name="map" uid="1"><structure><children>
          <component name="input" library="xml" uid="2" kind="14"><data><root><entry name="Packet"><entry name="Quantity" outkey="10"/></entry></root><document schema="RelocatedInput.xsd" inputinstance="source.xml" instanceroot="{}Packet"/></data></component>
          <component name="buffer" library="xml" uid="4" kind="14"><properties PassThrough="1"/><data><root><entry name="Buffer"><entry name="Quantity" inpkey="20" outkey="30"/></entry></root><document schema="buffer.xsd" instanceroot="{}Buffer"/></data></component>
          <component name="output" library="xml" uid="3" kind="14"><properties XSLTDefaultOutput="1"/><data><root><entry name="Result"><entry name="Quantity" inpkey="40"/></entry></root><document schema="target.xsd" instanceroot="{}Result"/></data></component>
          </children><graph><vertices><vertex vertexkey="10"><edges><edge vertexkey="20"/></edges></vertex><vertex vertexkey="30"><edges><edge vertexkey="40"/></edges></vertex></vertices></graph></structure></component></mapping>"#,
        )?;
        let reads = ResourceReadSetV1::default();
        let binding = f.binding(
            Kind::XmlSchema,
            "RelocatedInput.xsd",
            if wrong { [0; 32] } else { hash(source) },
        )?;
        let result = mfd::import_pipeline_with_options(
            &f.mapping,
            &f.options(vec![binding], reads.clone())?,
        );
        if wrong {
            let error = match result {
                Ok(_) => panic!("pipeline published mismatched bytes"),
                Err(e) => e.to_string(),
            };
            assert!(error.contains("SHA-256 mismatch"), "{error}");
        } else {
            let imported = result?;
            assert_eq!(imported.pipeline.stages.len(), 2);
            assert_eq!(reads.records().len(), 1);
            assert!(reads.records()[0].delivered);
        }
    }
    Ok(())
}
