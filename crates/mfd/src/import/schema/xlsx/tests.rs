use super::*;

fn minimal_grid(
    header_start: u32,
    data_start: u32,
    header_row_key: Option<u32>,
    value_ports: bool,
) -> String {
    let header_row_key = header_row_key
        .map(|key| format!(r#" outkey="{key}""#))
        .unwrap_or_default();
    let header_value_key = if value_ports { r#" outkey="1""# } else { "" };
    let data_value_key = if value_ports { r#" outkey="4""# } else { "" };
    format!(
        r#"
        <component name="Grid">
          <data>
            <root><entry name="Workbook"><entry name="Worksheet">
              <ranges>
                <range id="1" start="{header_start}" count="1"/>
                <range id="2" start="{data_start}"/>
              </ranges>
              <entry name="Row"{header_row_key}>
                <condition><function name="is-range-id"><constant value="1"/></function></condition>
                <entry name="Cell"{header_value_key}><entry name="n" outkey="2"/></entry>
              </entry>
              <entry name="Row" outkey="3">
                <condition><function name="is-range-id"><constant value="2"/></function></condition>
                <entry name="Cell"{data_value_key}><entry name="n" outkey="5"/></entry>
              </entry>
            </entry></entry></root>
            <excel inputinstance="grid.xlsx"/>
          </data>
        </component>
        "#
    )
}

fn read_minimal_grid(xml: &str) -> (SchemaComponent, Vec<String>) {
    let document = roxmltree::Document::parse(xml).unwrap();
    let mut warnings = Vec::new();
    let component = read(&document.root_element(), &mut warnings).unwrap();
    (component, warnings)
}

#[test]
fn unsupported_composite_shape_falls_back_to_its_supported_table() {
    let document = roxmltree::Document::parse(
        r#"
        <component name="MixedWorkbook"><data><root><entry name="FileInstance"><entry name="document"><entry name="Workbook">
          <entry name="Worksheet">
            <condition><expression><function name="equal-ignorecase" library="xlsx"><expression><attribute name="Name"/></expression><expression><constant value="Sales"/></expression></function></expression></condition>
            <ranges><range id="fixed" start="1" count="1"/><range id="row" start="2" count="1"/></ranges>
            <entry name="Row"><condition><expression><function name="is-range-id"><expression><constant value="fixed"/></expression></function></expression></condition>
              <entry name="Cell" outkey="101" annotation="Year" datatype="string"><condition><expression><function name="equal" library="core"><expression><attribute name="n"/></expression><expression><constant value="1" datatype="long"/></expression></function></expression></condition></entry>
            </entry>
            <entry name="Row"><condition><expression><function name="is-range-id"><expression><constant value="row"/></expression></function></expression></condition>
              <entry name="Cell" outkey="102" annotation="Month" datatype="string"><entry name="n" outkey="103"/></entry>
            </entry>
          </entry>
        </entry></entry></entry></root><excel inputinstance="mixed.xlsx"/></data></component>
        "#,
    )
    .unwrap();
    let mut warnings = Vec::new();
    let component = read(&document.root_element(), &mut warnings).unwrap();

    assert!(component.options.xlsx_composite.is_none());
    assert_eq!(component.options.xlsx_rows, [2]);
    assert!(warnings.iter().any(|warning| warning.contains(
        "combines fixed records with a transposed table; that composite layout is unsupported"
    )));
}

#[test]
fn grid_field_collision_warns_and_falls_back() {
    let document = roxmltree::Document::parse(
        r#"
        <component name="Grid"><data><root><entry name="Workbook"><entry name="Worksheet">
          <ranges><range id="1" start="1" count="1"/><range id="2" start="2"/></ranges>
          <entry name="Row"><condition><function name="is-range-id"><constant value="1"/></function></condition><entry name="Cell" annotation="value" outkey="1"><entry name="n" outkey="2"/></entry></entry>
          <entry name="Row" outkey="3"><condition><function name="is-range-id"><constant value="2"/></function></condition><entry name="Cell" outkey="4"><entry name="n" outkey="5"/></entry></entry>
        </entry></entry></root><excel inputinstance="grid.xlsx"/></data></component>
        "#,
    )
    .unwrap();
    let mut warnings = Vec::new();
    let component = read(&document.root_element(), &mut warnings).unwrap();

    assert!(component.options.xlsx_grid.is_none());
    assert!(warnings.iter().any(|warning| {
        warning.contains("nested-grid header name `value` conflicts with a generated field")
    }));
}

#[test]
fn grid_rejects_runtime_invalid_row_order_and_connected_header_rows() {
    let (component, warnings) = read_minimal_grid(&minimal_grid(2, 1, None, true));
    assert!(component.options.xlsx_grid.is_none());
    assert!(
        warnings
            .iter()
            .any(|warning| warning.contains("data row must start after its header row"))
    );

    let (component, warnings) = read_minimal_grid(&minimal_grid(1, 2, Some(6), true));
    assert!(component.options.xlsx_grid.is_none());
    assert!(
        warnings
            .iter()
            .any(|warning| warning.contains("header Row connections are not supported"))
    );
}

#[test]
fn grid_accepts_position_only_cell_sequences() {
    let (component, warnings) = read_minimal_grid(&minimal_grid(1, 2, None, false));

    assert!(warnings.is_empty(), "{warnings:?}");
    assert!(component.options.xlsx_grid.is_some());
    assert!(!component.ports.contains_key(&1));
    assert!(!component.ports.contains_key(&4));
    assert_eq!(component.ports.get(&2), Some(&vec!["HeaderColumn".into()]));
    assert_eq!(
        component.ports.get(&5),
        Some(&vec!["Rows".into(), "Cells".into(), "CellColumn".into()])
    );
}

#[test]
fn flat_target_retains_existing_workbook_update_mode() {
    let document = roxmltree::Document::parse(
        r#"
        <component name="Report"><data><root><entry name="Workbook"><entry name="Worksheet">
          <condition><function name="equal-ignorecase" library="xlsx"><attribute name="Name"/><constant value="Sales"/></function></condition>
          <ranges><range id="2" start="5"/></ranges>
          <entry name="Row" inpkey="10" enabletitlerow="1"><condition><function name="is-range-id"><constant value="2"/></function></condition>
            <entry name="Cell" inpkey="11" annotation="Month" datatype="string"><condition><function name="equal"><attribute name="n"/><constant value="1"/></function></condition></entry>
          </entry>
        </entry></entry></root><excel outputinstance="report.xlsx" updateexistingfile="1"/></data></component>
        "#,
    )
    .unwrap();
    let mut warnings = Vec::new();
    let component = read(&document.root_element(), &mut warnings).unwrap();

    assert!(warnings.is_empty(), "{warnings:?}");
    assert!(component.options.xlsx_update_existing);
    assert_eq!(component.options.xlsx_sheet.as_deref(), Some("Sales"));
    assert_eq!(component.options.xlsx_start_row, Some(5));
}

#[test]
fn flat_columns_keep_distinct_physical_indexes_when_annotations_repeat() {
    let mut columns = vec![
        Column {
            name: "Phone".into(),
            header: "Phone".into(),
            index: 4,
            ty: ScalarType::String,
            ports: vec![10],
        },
        Column {
            name: "Phone".into(),
            header: "Phone".into(),
            index: 5,
            ty: ScalarType::String,
            ports: vec![11],
        },
        Column {
            name: "Phone_5".into(),
            header: "Phone_5".into(),
            index: 6,
            ty: ScalarType::String,
            ports: vec![12],
        },
    ];

    assert!(!duplicate_column_index(&columns));
    assert!(duplicate_column_name(&columns));
    disambiguate_column_names(&mut columns);
    assert_eq!(
        columns
            .iter()
            .map(|column| column.name.as_str())
            .collect::<Vec<_>>(),
        ["Phone", "Phone_5_2", "Phone_5"]
    );
    assert!(!duplicate_column_name(&columns));
    columns[2].index = 5;
    assert!(duplicate_column_index(&columns));
}

#[test]
fn flat_target_retains_duplicate_headers_behind_unique_field_names() {
    let document = roxmltree::Document::parse(
        r#"
        <component name="Report"><data><root><entry name="Workbook"><entry name="Worksheet">
          <ranges><range id="1" start="1"/></ranges>
          <entry name="Row" inpkey="10" enabletitlerow="1"><condition><function name="is-range-id"><constant value="1"/></function></condition>
            <entry name="Cell" inpkey="11" annotation="Phone"><condition><function name="equal"><attribute name="n"/><constant value="4"/></function></condition></entry>
            <entry name="Cell" inpkey="12" annotation="Phone"><condition><function name="equal"><attribute name="n"/><constant value="5"/></function></condition></entry>
          </entry>
        </entry></entry></root><excel outputinstance="report.xlsx"/></data></component>
        "#,
    )
    .unwrap();
    let mut warnings = Vec::new();
    let component = read(&document.root_element(), &mut warnings)
        .unwrap_or_else(|| panic!("duplicate-header target was skipped: {warnings:?}"));

    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(component.options.xlsx_headers, ["Phone", "Phone"]);
    let ir::SchemaKind::Group { children, .. } = component.schema.kind else {
        panic!("expected flat XLSX group schema");
    };
    assert_eq!(children[0].name, "Phone");
    assert_eq!(children[1].name, "Phone_5");
}
