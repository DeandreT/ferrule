use super::*;

#[test]
fn object_one_of_subtypes_import_execute_and_select_xml_types() {
    let imported = mfd::import(&fixture("json-alternatives.mfd")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(engine::validate(&imported.project).is_empty());

    let source_address = imported
        .project
        .source
        .child("Rows")
        .and_then(|row| row.child("address"))
        .unwrap();
    assert_eq!(source_address.alternatives().len(), 2);
    assert!(source_address.child("state").is_some());
    assert!(source_address.child("postcode").is_some());

    let target_address = imported
        .project
        .target
        .child("Row")
        .and_then(|row| row.child("Address"))
        .unwrap();
    assert_eq!(target_address.alternatives().len(), 3);
    assert!(
        target_address
            .alternatives()
            .iter()
            .any(|alternative| alternative.name.ends_with("}Address"))
    );
    let address_scope = imported.project.root.children[0]
        .children
        .iter()
        .find(|scope| scope.target_field == "Address")
        .unwrap();
    assert_eq!(
        address_scope
            .bindings
            .iter()
            .filter(|binding| binding.target_field == "name")
            .count(),
        1
    );

    let source =
        format_json::read(&fixture("json-alternatives.json"), &imported.project.source).unwrap();
    let output = engine::run(&imported.project, &source).unwrap();
    let rows = output.field("Row").and_then(Instance::as_repeated).unwrap();
    assert_eq!(rows.len(), 2);
    let domestic = rows[0].field("Address").unwrap();
    assert_eq!(scalar(domestic, "name"), Value::String("Ada".into()));
    assert_eq!(scalar(domestic, "state"), Value::String("WA".into()));
    assert_eq!(scalar(domestic, "postcode"), Value::Null);
    let international = rows[1].field("Address").unwrap();
    assert_eq!(scalar(international, "name"), Value::String("Lin".into()));
    assert_eq!(scalar(international, "state"), Value::Null);
    assert_eq!(
        scalar(international, "postcode"),
        Value::String("SW1".into())
    );

    let xml = format_xml::to_string(&imported.project.target, &output).unwrap();
    assert!(xml.contains("xsi:type=\"ft:Domestic\""), "{xml}");
    assert!(xml.contains("xsi:type=\"ft:International\""), "{xml}");
    assert!(xml.contains("<state>WA</state>"), "{xml}");
    assert!(xml.contains("<postcode>SW1</postcode>"), "{xml}");
}
