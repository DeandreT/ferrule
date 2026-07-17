use std::collections::BTreeMap;
use std::path::Path;

use ir::SchemaNode;
use mapping::FormatOptions;

use super::{ComponentFormat, SchemaComponent};

pub(super) fn read(
    component: &roxmltree::Node,
    mfd_path: &Path,
    warnings: &mut Vec<String>,
) -> Option<SchemaComponent> {
    match component.attribute("library") {
        Some("xml") => super::read_schema_component(component, mfd_path, warnings),
        Some("text")
            if component
                .descendants()
                .any(|node| node.has_tag_name("text") && node.attribute("type") == Some("edi")) =>
        {
            super::edi::read(component, mfd_path, warnings, false)
        }
        Some("db") => read_db(component, warnings),
        _ => None,
    }
}

fn read_db(component: &roxmltree::Node, warnings: &mut Vec<String>) -> Option<SchemaComponent> {
    let name = component.attribute("name").unwrap_or_default().to_string();
    let data = component
        .children()
        .find(|node| node.is_element() && node.has_tag_name("data"))?;
    let root = data
        .children()
        .find(|node| node.is_element() && node.has_tag_name("root"))?;
    let mut container = root;
    loop {
        let mut entries = container
            .children()
            .filter(|node| node.is_element() && node.has_tag_name("entry"));
        match (entries.next(), entries.next()) {
            (Some(entry), None)
                if matches!(
                    entry.attribute("name"),
                    Some("FileInstance") | Some("document")
                ) =>
            {
                container = entry;
            }
            _ => break,
        }
    }
    let tables = container
        .children()
        .filter(|node| {
            node.is_element()
                && node.has_tag_name("entry")
                && node.attribute("type") == Some("table")
        })
        .collect::<Vec<_>>();
    if tables.is_empty() {
        warnings.push(format!(
            "structured database parameter `{name}` contains no table entries"
        ));
        return None;
    }
    let single_plain_table = tables.len() == 1
        && !tables[0]
            .children()
            .any(|node| node.attribute("type") == Some("table"));
    let mut ports = BTreeMap::new();
    let mut out_count = 0usize;
    let mut in_count = 0usize;
    for table in &tables {
        let mut path = if single_plain_table || tables.len() == 1 {
            Vec::new()
        } else {
            vec![table.attribute("name").unwrap_or_default().to_string()]
        };
        super::collect_db_ports(table, &mut path, &mut ports, &mut out_count, &mut in_count);
    }
    let schema = if tables.len() == 1 {
        super::db_table_schema(&tables[0], &BTreeMap::new())
    } else {
        SchemaNode::group(
            "database",
            tables
                .iter()
                .map(|table| super::db_table_schema(table, &BTreeMap::new()))
                .collect(),
        )
    };
    let (input_keys, output_keys) = super::entry_key_sets(&root);
    Some(SchemaComponent {
        name,
        format: ComponentFormat::Db,
        schema,
        input_instance: None,
        output_instance: None,
        options: FormatOptions::default(),
        is_source: out_count >= in_count,
        is_default_output: false,
        is_variable: false,
        compute_when_key: None,
        ports,
        input_ancestors: BTreeMap::new(),
        input_keys,
        output_keys,
        db_queries: Vec::new(),
        dynamic_json: None,
    })
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::super::ComponentFormat;

    #[test]
    fn internal_edi_and_database_parameters_do_not_emit_boundary_warnings() {
        let edi = roxmltree::Document::parse(
            r#"<component name="Items" library="text" kind="16"><data>
              <root><entry name="document"><entry name="Item" outkey="1"><entry name="Code" outkey="2"/></entry></entry></root>
              <text type="edi" kind="SWIFTMT"/><parameter usageKind="input" name="Items"/>
            </data></component>"#,
        )
        .unwrap();
        let db = roxmltree::Document::parse(
            r#"<component name="Rows" library="db" kind="15"><data>
              <root><entry name="document"><entry name="Row" type="table" inpkey="3"><entry name="Code" inpkey="4"/></entry></entry></root>
              <database ref="ignored"/><parameter usageKind="output" name="Rows"/>
            </data></component>"#,
        )
        .unwrap();
        let mut warnings = Vec::new();
        let edi =
            super::read(&edi.root_element(), Path::new("mapping.mfd"), &mut warnings).unwrap();
        let db = super::read(&db.root_element(), Path::new("mapping.mfd"), &mut warnings).unwrap();

        assert!(warnings.is_empty(), "{warnings:?}");
        assert!(edi.format == ComponentFormat::Edi);
        assert_eq!(edi.ports.get(&1), Some(&Vec::new()));
        assert!(db.format == ComponentFormat::Db);
        assert!(db.schema.repeating);
        assert_eq!(db.ports.get(&3), Some(&Vec::new()));
    }
}
