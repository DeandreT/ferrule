//! Request-specific cache authority, import isolation, and native graph limits.
use super::*;

fn two_request_mapping(f: &Fixture) -> Result<(), Box<dyn Error>> {
    std::fs::write(
        f.package.join("maps/target.xsd"),
        TARGET.replace(
            "</xs:sequence>",
            "<xs:element name=\"Other\" type=\"xs:int\"/></xs:sequence>",
        ),
    )?;
    std::fs::write(
        &f.mapping,
        r#"<mapping><component name="defaultmap" uid="1"><structure><children>
      <component name="first" library="xml" uid="2" kind="14"><data><root><entry name="Packet"><entry name="Quantity" outkey="10"/></entry></root><document schema="FirstLegacy.xsd" inputinstance="first.xml" instanceroot="{}Packet"/></data></component>
      <component name="second" library="xml" uid="4" kind="14"><data><root><entry name="Packet"><entry name="Quantity" outkey="11"/></entry></root><document schema="SecondLegacy.xsd" inputinstance="second.xml" instanceroot="{}Packet"/></data></component>
      <component name="output" library="xml" uid="3" kind="14"><properties XSLTDefaultOutput="1"/><data><root><entry name="Result"><entry name="Quantity" inpkey="20"/><entry name="Other" inpkey="21"/></entry></root><document schema="target.xsd" instanceroot="{}Result"/></data></component>
      </children><graph><vertices><vertex vertexkey="10"><edges><edge vertexkey="20"/></edges></vertex><vertex vertexkey="11"><edges><edge vertexkey="21"/></edges></vertex></vertices></graph></structure></component></mapping>"#,
    )?;
    Ok(())
}

#[test]
fn conflicting_hash_selectors_for_one_path_are_verified_per_request() -> Result<(), Box<dyn Error>>
{
    let f = Fixture::new()?;
    two_request_mapping(&f)?;
    let a = TARGET.replace("name=\"Result\"", "name=\"Packet\"");
    let b = format!("{a}<!-- another approved byte sequence -->");
    std::fs::write(f.catalog.join("input.xsd"), &a)?;
    let first = f.binding(Kind::XmlSchema, "FirstLegacy.xsd", hash(&a))?;
    let second = f.binding(Kind::XmlSchema, "SecondLegacy.xsd", hash(&b))?;
    let reads = ResourceReadSetV1::default();
    let options = f.options(vec![first, second], reads.clone())?;
    refusal(
        mfd::import_with_options(&f.mapping, &options),
        "SHA-256 mismatch",
    );
    let records = reads.records();
    assert!(
        records
            .iter()
            .any(|r| r.original_reference == "FirstLegacy.xsd"
                && r.delivered
                && r.sha256 == hash(&a))
    );
    assert!(
        records
            .iter()
            .any(|r| r.original_reference == "SecondLegacy.xsd"
                && !r.delivered
                && r.sha256 == hash(&a)
                && r.expected_sha256 == Some(hash(&b)))
    );
    Ok(())
}

#[test]
fn distinct_approved_snapshots_of_one_path_have_independent_request_authority()
-> Result<(), Box<dyn Error>> {
    let f = Fixture::new()?;
    two_request_mapping(&f)?;
    let a = TARGET.replace("name=\"Result\"", "name=\"Packet\"");
    let b = format!("{a}<!-- second snapshot -->");
    std::fs::write(f.catalog.join("input.xsd"), &a)?;
    let first = f
        .binding(Kind::XmlSchema, "FirstLegacy.xsd", hash(&a))?
        .prepare()?;
    std::fs::write(f.catalog.join("input.xsd"), &b)?;
    let second = f.binding(Kind::XmlSchema, "SecondLegacy.xsd", hash(&b))?;
    let reads = ResourceReadSetV1::default();
    let imported =
        mfd::import_with_options(&f.mapping, &f.options(vec![first, second], reads.clone())?)?;
    integer_source(&imported);
    let records = reads.records();
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].resolved_resource, records[1].resolved_resource);
    assert!(
        records
            .iter()
            .any(|r| r.original_reference == "FirstLegacy.xsd"
                && r.sha256 == hash(&a)
                && r.from_prepared_snapshot)
    );
    assert!(
        records
            .iter()
            .any(|r| r.original_reference == "SecondLegacy.xsd"
                && r.sha256 == hash(&b)
                && !r.from_prepared_snapshot)
    );
    assert!(
        records
            .iter()
            .all(|r| r.delivered && r.expected_sha256 == Some(r.sha256))
    );
    Ok(())
}

#[test]
fn a_failed_import_does_not_poison_a_later_import_using_the_same_options()
-> Result<(), Box<dyn Error>> {
    let f = Fixture::new()?;
    let changed = format!("{ROOT_XSD}<!-- changed -->");
    std::fs::write(f.catalog.join("input.xsd"), &changed)?;
    let reads = ResourceReadSetV1::default();
    let binding = f.binding(Kind::XmlSchema, "RelocatedInput.xsd", hash(ROOT_XSD))?;
    let options = f.options(vec![binding], reads.clone())?;
    refusal(
        mfd::import_with_options(&f.mapping, &options),
        "SHA-256 mismatch",
    );
    std::fs::write(f.catalog.join("input.xsd"), ROOT_XSD)?;
    integer_source(&mfd::import_with_options(&f.mapping, &options)?);
    assert!(
        reads
            .records()
            .iter()
            .any(|r| !r.delivered && r.sha256 == hash(&changed))
    );
    assert!(
        reads
            .records()
            .iter()
            .any(|r| r.delivered && r.sha256 == hash(ROOT_XSD))
    );
    Ok(())
}

#[cfg(any(unix, windows))]
#[test]
fn a_linked_trusted_root_is_not_silently_canonicalized_into_authority() -> Result<(), Box<dyn Error>>
{
    let f = Fixture::new()?;
    let link = f.root.join("linked-catalog");
    directory_link(&f.catalog, &link)?;
    assert!(
        Binding::new(
            Kind::XmlSchema,
            &f.mapping,
            "RelocatedInput.xsd",
            &link,
            "input.xsd",
            hash(ROOT_XSD)
        )
        .is_err()
    );
    Ok(())
}

#[test]
fn dependency_cycles_cache_once_and_file_limits_remain_terminal() -> Result<(), Box<dyn Error>> {
    let f = Fixture::new()?;
    let cycle = TYPES.replace(
        "<xs:simpleType",
        "<xs:include schemaLocation=\"../input.xsd\"/><xs:simpleType",
    );
    std::fs::write(f.catalog.join("types/value.xsd"), &cycle)?;
    let reads = ResourceReadSetV1::default();
    let binding = f
        .binding(Kind::XmlSchema, "RelocatedInput.xsd", hash(ROOT_XSD))?
        .with_dependency_pin("types/value.xsd", hash(&cycle))?;
    integer_source(&mfd::import_with_options(
        &f.mapping,
        &f.options(vec![binding], reads.clone())?,
    )?);
    assert_eq!(reads.records().len(), 2);

    std::fs::create_dir(f.catalog.join("chain"))?;
    let main = ROOT_XSD.replace("types/value.xsd", "chain/step0.xsd");
    std::fs::write(f.catalog.join("input.xsd"), &main)?;
    for index in 0..65 {
        let content = if index == 64 {
            TYPES.to_string()
        } else {
            format!(
                "<xs:schema xmlns:xs=\"http://www.w3.org/2001/XMLSchema\" targetNamespace=\"urn:binding\"><xs:include schemaLocation=\"step{}.xsd\"/></xs:schema>",
                index + 1
            )
        };
        std::fs::write(f.catalog.join(format!("chain/step{index}.xsd")), content)?;
    }
    let reads = ResourceReadSetV1::default();
    let binding = f.binding(Kind::XmlSchema, "RelocatedInput.xsd", hash(&main))?;
    refusal(
        mfd::import_with_options(&f.mapping, &f.options(vec![binding], reads.clone())?),
        "bound resource",
    );
    assert_eq!(reads.records().len(), 64);
    Ok(())
}
