use std::collections::BTreeMap;

use ir::SchemaNode;
use mapping::FormatOptions;

use super::{ComponentFormat, SchemaComponent};

pub(super) fn read(
    component: &roxmltree::Node,
    warnings: &mut Vec<String>,
    runtime_boundary: bool,
) -> Option<SchemaComponent> {
    let name = component.attribute("name").unwrap_or_default().to_string();
    let data = component
        .children()
        .find(|node| node.has_tag_name("data"))?;
    let text = data
        .children()
        .find(|node| node.has_tag_name("text") && node.attribute("type") == Some("edi"))?;
    let kind = text.attribute("kind").unwrap_or_default();
    let root = data.children().find(|node| node.has_tag_name("root"))?;
    let mut entry = root.children().find(|node| node.has_tag_name("entry"))?;
    while matches!(
        entry.attribute("name"),
        Some("FileInstance") | Some("document")
    ) {
        entry = entry.children().find(|node| node.has_tag_name("entry"))?;
    }

    let mut schema = entry_tree_schema(&entry, true, false)?;
    schema.name = match kind {
        "EDIX12" => "MFD-X12",
        "EDIFACT" => "MFD-EDIFACT",
        "EDIHL7" => "HL7",
        "EDIFIXED" => "IDOC",
        "SWIFTMT" => "SWIFT",
        "EDITRADACOMS" => "TRADACOMS",
        _ if kind.is_empty() => "EDI",
        other => other,
    }
    .to_string();

    let mut ports = BTreeMap::new();
    let mut out_count = 0usize;
    let mut in_count = 0usize;
    let (input_keys, output_keys) = super::entry_key_sets(&root);
    super::record_entry_keys(&entry, &[], &mut ports, &mut out_count, &mut in_count);
    super::collect_entry_ports(
        &entry,
        &mut Vec::new(),
        &mut ports,
        &mut out_count,
        &mut in_count,
    );
    if out_count == 0 && in_count == 0 {
        warnings.push(format!("component `{name}` has no connected ports"));
    }

    if runtime_boundary {
        warnings.push(format!(
            "EDI component `{name}` uses an entry-tree schema inferred without its external `{kind}` \
             configuration; its mapping graph was imported, but execution is disabled until a schema \
             with element positions, scalar types, fixed qualifiers, and cardinalities is supplied"
        ));
        if !matches!(kind, "EDIX12" | "EDIFACT") {
            warnings.push(format!(
                "EDI component `{name}` uses runtime dialect `{kind}`; its mapping graph was imported, \
                 but ferrule currently executes only EDIX12 and EDIFACT instances"
            ));
        }
    }

    Some(SchemaComponent {
        name,
        format: ComponentFormat::Edi,
        schema,
        input_instance: text.attribute("inputinstance").map(str::to_string),
        output_instance: text.attribute("outputinstance").map(str::to_string),
        options: FormatOptions {
            lenient_segments: true,
            ..FormatOptions::default()
        },
        is_source: out_count >= in_count,
        is_default_output: super::is_default_output(component),
        is_variable: false,
        compute_when_key: None,
        ports,
        input_keys,
        output_keys,
        db_queries: Vec::new(),
        dynamic_json: None,
    })
}

fn entry_tree_schema(
    entry: &roxmltree::Node,
    is_root: bool,
    parent_is_segment: bool,
) -> Option<SchemaNode> {
    let name = entry.attribute("name").unwrap_or("EDI");
    let is_segment = is_inferred_segment(name);
    let child_entries = entry
        .children()
        .filter(|node| node.has_tag_name("entry"))
        .collect::<Vec<_>>();
    let children = child_entries
        .iter()
        .filter_map(|child| entry_tree_schema(child, false, is_segment))
        .collect::<Vec<_>>();
    let connected = entry.attribute("inpkey").is_some() || entry.attribute("outkey").is_some();

    if child_entries.is_empty() {
        return connected.then(|| SchemaNode::scalar(name, ir::ScalarType::String));
    }
    if children.is_empty() && !connected && !is_root {
        return None;
    }

    let node = SchemaNode::group(name, children);
    if !is_root && !parent_is_segment && (connected || !is_segment) {
        Some(node.repeating())
    } else {
        Some(node)
    }
}

fn is_inferred_segment(name: &str) -> bool {
    let name = name.strip_prefix("MF_").unwrap_or(name);
    (2..=3).contains(&name.len())
        && name
            .chars()
            .next()
            .is_some_and(|character| character.is_ascii_uppercase())
        && name
            .chars()
            .all(|character| character.is_ascii_uppercase() || character.is_ascii_digit())
        && !name.strip_prefix("SG").is_some_and(|suffix| {
            !suffix.is_empty() && suffix.bytes().all(|byte| byte.is_ascii_digit())
        })
}
