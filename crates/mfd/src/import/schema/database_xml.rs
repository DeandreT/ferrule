use std::collections::BTreeMap;
use std::path::Path;

use ir::SchemaNode;

use super::{parse_u32, read_xml_schema_file, resolve_xml_schema_reference};

pub(in crate::import) struct Column {
    pub(in crate::import) path: Vec<String>,
    pub(in crate::import) schema: SchemaNode,
    pub(in crate::import) namespace: Option<String>,
}

pub(super) fn collect(
    tables: &[roxmltree::Node<'_, '_>],
    include_table_name: bool,
    mfd_path: &Path,
    component_name: &str,
    warnings: &mut Vec<String>,
) -> BTreeMap<u32, Column> {
    let mut columns = BTreeMap::new();
    for table in tables {
        let mut path = if include_table_name {
            vec![table.attribute("name").unwrap_or_default().to_string()]
        } else {
            Vec::new()
        };
        collect_table(
            table,
            &mut path,
            mfd_path,
            component_name,
            warnings,
            &mut columns,
        );
    }
    columns
}

fn collect_table(
    table: &roxmltree::Node<'_, '_>,
    path: &mut Vec<String>,
    mfd_path: &Path,
    component_name: &str,
    warnings: &mut Vec<String>,
    columns: &mut BTreeMap<u32, Column>,
) {
    for entry in table.children().filter(|node| node.has_tag_name("entry")) {
        let name = entry.attribute("name").unwrap_or_default();
        path.push(name.to_string());
        if entry.attribute("type") == Some("table") {
            collect_table(&entry, path, mfd_path, component_name, warnings, columns);
            path.pop();
            continue;
        }

        let documents = entry
            .children()
            .filter(|node| node.has_tag_name("entry") && node.attribute("type") == Some("doc-xml"))
            .collect::<Vec<_>>();
        if documents.is_empty() {
            path.pop();
            continue;
        }
        let result = match documents.as_slice() {
            [document] => read_column(document, path, mfd_path),
            _ => Err("column contains more than one XML document declaration".to_string()),
        };
        match result {
            Ok((input, column)) => match columns.entry(input) {
                std::collections::btree_map::Entry::Vacant(entry) => {
                    entry.insert(column);
                }
                std::collections::btree_map::Entry::Occupied(_) => warnings.push(format!(
                    "database component `{component_name}` reuses XML column input port {input}; the duplicate was skipped"
                )),
            },
            Err(reason) => warnings.push(format!(
                "database component `{component_name}` XML column `{}` is unsupported: {reason}",
                path.join("/")
            )),
        }
        path.pop();
    }
}

fn read_column(
    entry: &roxmltree::Node<'_, '_>,
    path: &[String],
    mfd_path: &Path,
) -> Result<(u32, Column), String> {
    let metadata = entry
        .children()
        .find(|node| node.has_tag_name("document"))
        .ok_or_else(|| "document metadata is missing".to_string())?;
    if metadata
        .attribute("encoding")
        .is_some_and(|encoding| !encoding.eq_ignore_ascii_case("UTF-8"))
    {
        return Err("only UTF-8 XML documents are supported".to_string());
    }
    let payloads = entry
        .children()
        .filter(|node| node.has_tag_name("entry"))
        .collect::<Vec<_>>();
    let [payload] = payloads.as_slice() else {
        return Err("expected exactly one XML document root entry".to_string());
    };
    let input = parse_u32(payload.attribute("inpkey"))
        .ok_or_else(|| "XML document root has no input port".to_string())?;
    if payload.attribute("outkey").is_some() {
        return Err("XML-valued database source columns are not supported".to_string());
    }
    let schema_file = metadata
        .attribute("schemafile")
        .ok_or_else(|| "document schema file is missing".to_string())?;
    let root = metadata
        .attribute("root")
        .and_then(local_name)
        .unwrap_or_else(|| payload.attribute("name").unwrap_or_default());
    if root.is_empty() {
        return Err("document root name is missing".to_string());
    }
    let schema_path = resolve_xml_schema_reference(mfd_path, schema_file)?;
    let schema =
        read_xml_schema_file(&schema_path, Some(root)).map_err(|error| error.to_string())?;
    let namespace = metadata
        .attribute("root")
        .and_then(expanded_namespace)
        .map(str::to_string);
    Ok((
        input,
        Column {
            path: path.to_vec(),
            schema,
            namespace,
        },
    ))
}

fn local_name(name: &str) -> Option<&str> {
    let local = name.rsplit('}').next().unwrap_or(name);
    (!local.is_empty()).then_some(local)
}

fn expanded_namespace(name: &str) -> Option<&str> {
    let rest = name.strip_prefix('{')?;
    let (namespace, _) = rest.split_once('}')?;
    (!namespace.is_empty()).then_some(namespace)
}

#[cfg(test)]
mod tests {
    use super::{expanded_namespace, local_name};

    #[test]
    fn expanded_root_names_are_split_without_affecting_plain_names() {
        assert_eq!(local_name("{urn:catalog}Item"), Some("Item"));
        assert_eq!(local_name("Item"), Some("Item"));
        assert_eq!(expanded_namespace("{urn:catalog}Item"), Some("urn:catalog"));
        assert_eq!(expanded_namespace("{}Item"), None);
        assert_eq!(expanded_namespace("Item"), None);
    }
}
