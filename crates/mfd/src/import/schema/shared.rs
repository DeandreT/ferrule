use std::collections::BTreeSet;
use std::path::Path;

use ir::SchemaNode;

pub(in crate::import) fn read_xml_schema_file(
    schema_path: &Path,
    root: Option<&str>,
) -> Result<SchemaNode, String> {
    let extension = schema_path
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default();
    if extension.eq_ignore_ascii_case("dtd") {
        format_xml::dtd::import_root(schema_path, root).map_err(|error| error.to_string())
    } else {
        format_xml::xsd::import_root(schema_path, root).map_err(|error| error.to_string())
    }
}

pub(in crate::import) fn parse_u32(attr: Option<&str>) -> Option<u32> {
    attr.and_then(|attribute| attribute.parse().ok())
}

pub(in crate::import) fn entry_key_sets(root: &roxmltree::Node) -> (BTreeSet<u32>, BTreeSet<u32>) {
    let mut inputs = BTreeSet::new();
    let mut outputs = BTreeSet::new();
    for entry in root.descendants().filter(|node| node.has_tag_name("entry")) {
        if let Some(key) = parse_u32(entry.attribute("inpkey")) {
            inputs.insert(key);
        }
        if let Some(key) = parse_u32(entry.attribute("outkey")) {
            outputs.insert(key);
        }
    }
    (inputs, outputs)
}

pub(in crate::import) fn is_default_output(component: &roxmltree::Node) -> bool {
    component
        .children()
        .find(|node| node.has_tag_name("properties"))
        .and_then(|properties| properties.attribute("XSLTDefaultOutput"))
        == Some("1")
}
