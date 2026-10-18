use super::*;

const ROOT: &str = r##"{"type":"object","additionalProperties":false,"required":["Quantity"],"properties":{"Quantity":{"$ref":"types/count.json#/$defs/count"}}}"##;
const COUNT: &str = r#"{"$defs":{"count":{"type":"integer","minimum":2}}}"#;
const REFERENCE: &str = r"old\location\Input.json";

fn fixture() -> Result<Fixture, Box<dyn Error>> {
    let f = Fixture::new()?;
    std::fs::write(f.catalog.join("input.json"), ROOT)?;
    std::fs::write(f.catalog.join("types/count.json"), COUNT)?;
    std::fs::write(
        &f.mapping,
        format!(
            r#"<mapping><component name="defaultmap" uid="1"><structure><children>
      <component name="input" library="json" uid="2" kind="31"><data><root><entry name="object"><entry name="Quantity" type="json-property"><entry name="number" outkey="10"/></entry></entry></root><json schema="{REFERENCE}"/></data></component>
      <component name="output" library="xml" uid="3" kind="14"><properties XSLTDefaultOutput="1"/><data><root><entry name="Result"><entry name="Quantity" inpkey="20"/></entry></root><document schema="target.xsd" instanceroot="{{}}Result"/></data></component>
      </children><graph><vertices><vertex vertexkey="10"><edges><edge vertexkey="20"/></edges></vertex></vertices></graph></structure></component></mapping>"#
        ),
    )?;
    Ok(f)
}

fn binding(f: &Fixture) -> Result<Binding, mfd::MfdError> {
    Binding::new(
        Kind::JsonSchema,
        &f.mapping,
        REFERENCE,
        &f.catalog,
        "input.json",
        hash(ROOT),
    )?
    .with_dependency_pin("types/count.json", hash(COUNT))
}

#[test]
fn json_relocation_records_root_and_dependency_and_preserves_schema_constraints()
-> Result<(), Box<dyn Error>> {
    let f = fixture()?;
    let original = std::fs::read(&f.mapping)?;
    let reads = ResourceReadSetV1::default();
    let options = f.options(
        vec![binding(&f)?.with_origin_sha256(hash(&original))],
        reads.clone(),
    )?;
    let imported =
        mfd::import_with_profile(&f.mapping, &options, mfd::ImportProfile::Executable)?.imported;
    integer_source(&imported);
    assert!(format_json::from_str(r#"{"Quantity":2}"#, &imported.project.source).is_ok());
    assert!(format_json::from_str(r#"{"Quantity":1}"#, &imported.project.source).is_err());
    let records = reads.records();
    assert_eq!(records.len(), 2);
    assert!(records.iter().all(|r| r.kind == Kind::JsonSchema
        && r.delivered
        && r.original_reference == REFERENCE
        && r.original_origin_sha256 == hash(&original)
        && r.expected_sha256 == Some(r.sha256)));
    assert!(records.iter().any(|r| r.sha256 == hash(ROOT)));
    assert!(records.iter().any(|r| r.sha256 == hash(COUNT)));
    let project = serde_json::to_string(&imported.project)?;
    assert!(!project.contains("old\\\\location"));
    assert!(!project.contains("resolved_resource"));
    assert_eq!(std::fs::read(&f.mapping)?, original);
    Ok(())
}

#[test]
fn prepared_json_bytes_are_parsed_after_live_schema_replacement() -> Result<(), Box<dyn Error>> {
    let f = fixture()?;
    let reads = ResourceReadSetV1::default();
    let prepared = binding(&f)?.prepare()?;
    std::fs::write(f.catalog.join("input.json"), b"not JSON")?;
    std::fs::write(f.catalog.join("types/count.json"), b"also not JSON")?;
    let imported =
        mfd::import_with_options(&f.mapping, &f.options(vec![prepared], reads.clone())?)?;
    integer_source(&imported);
    let records = reads.records();
    assert_eq!(records.len(), 2);
    assert!(
        records.iter().all(|r| r.delivered
            && r.from_prepared_snapshot
            && r.expected_sha256 == Some(r.sha256))
    );
    std::fs::rename(&f.catalog, f.root.join("released-json-catalog"))?;
    Ok(())
}

#[test]
fn json_pin_failures_are_authoritative_in_both_profiles() -> Result<(), Box<dyn Error>> {
    for profile in [
        mfd::ImportProfile::BestEffort,
        mfd::ImportProfile::Executable,
    ] {
        for dependency in [false, true] {
            let f = fixture()?;
            let fallback = f.mapping.parent().unwrap().join("old/location");
            std::fs::create_dir_all(fallback.join("types"))?;
            std::fs::write(fallback.join("Input.json"), ROOT)?;
            std::fs::write(fallback.join("types/count.json"), COUNT)?;
            let reads = ResourceReadSetV1::default();
            let pin = Binding::new(
                Kind::JsonSchema,
                &f.mapping,
                REFERENCE,
                &f.catalog,
                "input.json",
                if dependency { hash(ROOT) } else { [0; 32] },
            )?
            .with_dependency_pin(
                "types/count.json",
                if dependency { [0; 32] } else { hash(COUNT) },
            )?;
            let error = mfd::import_with_profile(
                &f.mapping,
                &f.options(vec![pin], reads.clone())?,
                profile,
            )
            .err()
            .expect("pin failure must not publish a project");
            assert!(error.to_string().contains("SHA-256 mismatch"), "{error}");
            let records = reads.records();
            assert_eq!(records.len(), if dependency { 2 } else { 1 });
            assert!(!records.last().unwrap().delivered);
        }
    }
    Ok(())
}

#[test]
fn json_parse_and_reference_failures_cannot_publish_entry_tree_fallback()
-> Result<(), Box<dyn Error>> {
    for text in [
        "not JSON",
        r#"{"$ref":"../outside.json"}"#,
        r#"{"$ref":"missing.json"}"#,
    ] {
        let f = fixture()?;
        std::fs::write(f.catalog.join("input.json"), text)?;
        let reads = ResourceReadSetV1::default();
        let pin = Binding::new(
            Kind::JsonSchema,
            &f.mapping,
            REFERENCE,
            &f.catalog,
            "input.json",
            hash(text),
        )?;
        refusal(
            mfd::import_with_options(&f.mapping, &f.options(vec![pin], reads.clone())?),
            "bound resource",
        );
        assert_eq!(reads.records().len(), 1);
    }
    Ok(())
}

#[test]
fn wrong_json_binding_kind_has_no_body_delivery() -> Result<(), Box<dyn Error>> {
    let f = fixture()?;
    let reads = ResourceReadSetV1::default();
    let pin = Binding::new(
        Kind::XmlSchema,
        &f.mapping,
        REFERENCE,
        &f.catalog,
        "input.json",
        hash(ROOT),
    )?;
    refusal(
        mfd::import_with_options(&f.mapping, &f.options(vec![pin], reads.clone())?),
        "kind does not match",
    );
    assert!(reads.records().is_empty());
    Ok(())
}

#[test]
fn root_only_json_binding_records_unpinned_live_dependency() -> Result<(), Box<dyn Error>> {
    let f = fixture()?;
    let reads = ResourceReadSetV1::default();
    let prepared = Binding::new(
        Kind::JsonSchema,
        &f.mapping,
        REFERENCE,
        &f.catalog,
        "input.json",
        hash(ROOT),
    )?
    .prepare()?;
    let changed = COUNT.replace('2', "3");
    std::fs::write(f.catalog.join("types/count.json"), &changed)?;
    let imported =
        mfd::import_with_options(&f.mapping, &f.options(vec![prepared], reads.clone())?)?;
    assert!(format_json::from_str(r#"{"Quantity":2}"#, &imported.project.source).is_err());
    let records = reads.records();
    let dependency = records
        .iter()
        .find(|r| r.resolved_resource.ends_with("types/count.json"))
        .unwrap();
    assert_eq!(dependency.sha256, hash(changed));
    assert_eq!(dependency.expected_sha256, None);
    assert!(!dependency.from_prepared_snapshot);
    Ok(())
}

#[test]
fn dormant_parser_schema_still_consumes_bound_bytes_and_refuses_bad_pins()
-> Result<(), Box<dyn Error>> {
    let f = Fixture::new()?;
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/dormant-json-parser");
    std::fs::copy(fixture.join("mapping.mfd"), &f.mapping)?;
    for name in ["document.schema.json", "payload.schema.json"] {
        std::fs::copy(fixture.join(name), f.mapping.parent().unwrap().join(name))?;
    }
    let payload = std::fs::read(fixture.join("payload.schema.json"))?;
    std::fs::write(f.catalog.join("payload.json"), &payload)?;
    for good_pin in [true, false] {
        let reads = ResourceReadSetV1::default();
        let pin = Binding::new(
            Kind::JsonSchema,
            &f.mapping,
            "payload.schema.json",
            &f.catalog,
            "payload.json",
            if good_pin { hash(&payload) } else { [0; 32] },
        )?;
        let result = mfd::import_with_profile(
            &f.mapping,
            &f.options(vec![pin], reads.clone())?,
            mfd::ImportProfile::Executable,
        );
        assert_eq!(result.is_ok(), good_pin);
        let records = reads.records();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].delivered, good_pin);
        assert_eq!(records[0].sha256, hash(&payload));
    }
    Ok(())
}
