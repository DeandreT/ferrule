use std::collections::BTreeMap;
use std::path::Path;

use ir::{ScalarType, SchemaKind};

use super::{
    db_table_schema, instance_root_segments, normalize_xml_entry_name, read_json_component,
    read_schema_component,
};

#[test]
fn oversized_xsd_targets_use_the_connected_entry_projection() {
    use std::fmt::Write as _;

    let dir =
        std::env::temp_dir().join(format!("ferrule_mfd_projected_xsd_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut xsd = String::from(
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
          <xs:complexType name="T0"><xs:sequence><xs:element name="Value" type="xs:string"/></xs:sequence></xs:complexType>"#,
    );
    for level in 1..=20 {
        writeln!(
            xsd,
            r#"<xs:complexType name="T{level}"><xs:sequence><xs:element name="Left" type="T{}"/><xs:element name="Right" type="T{}"/></xs:sequence></xs:complexType>"#,
            level - 1,
            level - 1
        )
        .unwrap();
    }
    xsd.push_str(r#"<xs:element name="Root" type="T20"/></xs:schema>"#);
    std::fs::write(dir.join("large.xsd"), xsd).unwrap();

    let target_xml = r#"<component name="Target"><data>
      <root><entry name="FileInstance"><entry name="document"><entry name="Root">
        <entry name="Projected" type="attribute" inpkey="1"/>
      </entry></entry></entry></root>
      <document schema="large.xsd" outputinstance="out.xml" instanceroot="{}Root"/>
    </data></component>"#;
    let target_doc = roxmltree::Document::parse(target_xml).unwrap();
    let mut target_warnings = Vec::new();
    let target = read_schema_component(
        &target_doc.root_element(),
        &dir.join("mapping.mfd"),
        &mut target_warnings,
    )
    .unwrap();
    assert!(target_warnings.is_empty(), "{target_warnings:?}");
    assert!(
        target
            .schema
            .child("Projected")
            .is_some_and(|node| node.attribute)
    );

    let source_xml = target_xml
        .replace("Target", "Source")
        .replace("inpkey", "outkey")
        .replace("outputinstance", "inputinstance");
    let source_doc = roxmltree::Document::parse(&source_xml).unwrap();
    let mut source_warnings = Vec::new();
    read_schema_component(
        &source_doc.root_element(),
        &dir.join("mapping.mfd"),
        &mut source_warnings,
    )
    .unwrap();
    assert!(
        source_warnings
            .iter()
            .any(|warning| warning.contains("materialization limit")),
        "{source_warnings:?}"
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn instance_root_paths_do_not_split_namespace_uris() {
    assert_eq!(
        instance_root_segments(
            "{http://example.com/people}People/{http://example.com/people}Person"
        ),
        ["People", "Person"]
    );
    assert_eq!(
        instance_root_segments("{}People/{}Person"),
        ["People", "Person"]
    );
}

#[test]
fn indexed_xml_entry_names_are_normalized_without_touching_qnames() {
    assert_eq!(normalize_xml_entry_name("0:Person"), ("Person", false));
    assert_eq!(normalize_xml_entry_name("12:@type"), ("type", true));
    assert_eq!(normalize_xml_entry_name("Person"), ("Person", false));
    assert_eq!(normalize_xml_entry_name("ns:Person"), ("ns:Person", false));
}

#[test]
fn json_lines_component_sets_runtime_format_option_without_a_downgrade_warning() {
    let document = roxmltree::Document::parse(
        r#"<component name="Rows"><data><root><entry name="FileInstance"><entry name="document"><entry name="root" outkey="1"/></entry></entry></root><json jsonlines="1" inputinstance="rows.jsonl"/></data></component>"#,
    )
    .unwrap();
    let mut warnings = Vec::new();
    let component = read_json_component(
        &document.root_element(),
        Path::new("mapping.mfd"),
        &mut warnings,
    )
    .unwrap();

    assert!(component.options.json_lines);
    assert!(
        warnings
            .iter()
            .all(|warning| !warning.contains("JSON Lines")),
        "{warnings:?}"
    );
}

#[test]
fn nullable_json_target_uses_one_typed_property_port() {
    let document = roxmltree::Document::parse(
        r#"
        <component name="Result">
          <data>
            <root>
              <entry name="FileInstance"><entry name="document"><entry name="root">
                <entry name="object">
                  <entry name="Shares" type="json-property">
                    <entry name="number" inpkey="22"/>
                    <entry name="null" inpkey="28"/>
                  </entry>
                </entry>
              </entry></entry></entry>
            </root>
            <json/>
          </data>
        </component>
        "#,
    )
    .unwrap();
    let mut warnings = Vec::new();

    let component = read_json_component(
        &document.root_element(),
        Path::new("mapping.mfd"),
        &mut warnings,
    )
    .unwrap();

    assert_eq!(component.ports.get(&22), Some(&vec!["Shares".into()]));
    assert!(!component.ports.contains_key(&28));
}

#[test]
fn whole_table_ports_include_introspected_columns_missing_from_the_entry_tree() {
    let document = roxmltree::Document::parse(
        r#"<entry name="departments" type="table" outkey="7">
            <entry name="id"/>
            <entry name="people|department_id" type="table"/>
        </entry>"#,
    )
    .unwrap();
    let mut types = BTreeMap::new();
    types.insert(
        "departments".to_string(),
        BTreeMap::from([
            ("id".to_string(), ScalarType::Int),
            ("name".to_string(), ScalarType::String),
        ]),
    );
    types.insert(
        "people".to_string(),
        BTreeMap::from([
            ("id".to_string(), ScalarType::Int),
            ("department_id".to_string(), ScalarType::Int),
        ]),
    );

    let schema = db_table_schema(&document.root_element(), &types);

    assert!(matches!(
        schema.child("id").map(|node| &node.kind),
        Some(SchemaKind::Scalar {
            ty: ScalarType::Int
        })
    ));
    assert!(matches!(
        schema.child("name").map(|node| &node.kind),
        Some(SchemaKind::Scalar {
            ty: ScalarType::String
        })
    ));
    assert!(matches!(
        schema
            .child("people|department_id")
            .and_then(|people| people.child("department_id"))
            .map(|node| &node.kind),
        Some(SchemaKind::Scalar {
            ty: ScalarType::Int
        })
    ));
}
