use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use ir::{ScalarType, SchemaKind, SchemaNode, XML_TEXT_FIELD};
use mapping::{FormatOptions, XbrlBoundaryOptions};

use super::{ComponentFormat, SchemaComponent, is_default_output, normalize_xml_entry_name};

mod namespace;
mod sps;

pub(super) fn read(
    component: &roxmltree::Node<'_, '_>,
    mfd_path: &Path,
    warnings: &mut Vec<String>,
) -> Result<SchemaComponent, String> {
    if component.attribute("kind") != Some("27") {
        return Err("only kind=27 XBRL document components are supported".to_string());
    }
    let name = component.attribute("name").unwrap_or_default().to_string();
    let data = child(component, "data").ok_or("XBRL component has no data block")?;
    let metadata = child(&data, "xbrl").ok_or("XBRL component has no xbrl metadata")?;
    let taxonomy = required_attribute(&metadata, "schema", "taxonomy schema")?;
    let roots = data
        .children()
        .filter(|node| node.has_tag_name("root"))
        .collect::<Vec<_>>();
    let [root] = roots.as_slice() else {
        return Err("XBRL component must expose exactly one entry-tree root".to_string());
    };
    let payload = document_payload(root)?;
    let namespace_bindings = namespace::bindings(root, &payload)?;

    let mut state = PortState::default();
    let mut schema = build_root_schema(&payload, &mut state)?;
    let input_ancestors = input_port_ancestors(&payload, &state.input_keys)?;
    let is_source = match (state.input_keys.is_empty(), state.output_keys.is_empty()) {
        (true, false) => true,
        (false, true) => false,
        _ => {
            return Err("XBRL component must expose ports in exactly one direction".to_string());
        }
    };

    let fact_bindings = if is_source {
        Vec::new()
    } else if let Some(presentation) = metadata.attribute("sps") {
        match sps::fact_bindings(mfd_path, presentation, &schema, &namespace_bindings) {
            Ok(bindings) => bindings,
            Err(error) => {
                warnings.push(format!(
                    "XBRL component `{name}` could not compile numeric fact metadata from `{presentation}` ({error}); numeric target facts require explicit item-type metadata before execution"
                ));
                Vec::new()
            }
        }
    } else {
        Vec::new()
    };

    let boundary = if is_source {
        if metadata.attribute("sps").is_some() {
            return Err("XBRL source presentation metadata is not supported".to_string());
        }
        XbrlBoundaryOptions::external_source(taxonomy)
    } else {
        if metadata.attribute("inputinstance").is_some() {
            return Err("XBRL target cannot declare an input instance".to_string());
        }
        XbrlBoundaryOptions::external_target(taxonomy, metadata.attribute("sps"))
    }
    .and_then(|boundary| boundary.with_namespace_bindings(namespace_bindings))
    .and_then(|boundary| boundary.with_fact_bindings(fact_bindings))
    .map_err(|error| format!("invalid XBRL boundary metadata ({error})"))?;

    if is_source {
        normalize_source_table(&mut schema, &state.output_keys, &state.ports)?;
    } else {
        normalize_target_tables(&mut schema, &mut state)?;
    }

    Ok(SchemaComponent {
        name,
        format: ComponentFormat::Xbrl,
        schema,
        input_instance: is_source
            .then(|| metadata.attribute("inputinstance").map(str::to_string))
            .flatten(),
        output_instance: (!is_source)
            .then(|| metadata.attribute("outputinstance").map(str::to_string))
            .flatten(),
        options: FormatOptions {
            xbrl: Some(boundary),
            ..FormatOptions::default()
        },
        is_source,
        is_default_output: is_default_output(component),
        is_variable: false,
        is_pass_through: false,
        compute_when_key: None,
        ports: state.ports,
        input_ancestors,
        input_keys: state.input_keys,
        output_keys: state.output_keys,
        db_queries: Vec::new(),
        dynamic_json: None,
    })
}

fn normalize_source_table(
    schema: &mut SchemaNode,
    output_keys: &BTreeSet<u32>,
    ports: &BTreeMap<u32, Vec<String>>,
) -> Result<(), String> {
    let paths = output_keys
        .iter()
        .filter_map(|key| ports.get(key))
        .collect::<Vec<_>>();
    let [first, rest @ ..] = paths.as_slice() else {
        return Err("XBRL source table has no connected output paths".to_string());
    };
    let common_length = rest.iter().fold(first.len(), |length, path| {
        length.min(
            first
                .iter()
                .zip(path.iter())
                .take_while(|(left, right)| left == right)
                .count(),
        )
    });
    let mut row_length = common_length;
    while row_length > 0
        && !schema_at_path(schema, &first[..row_length])
            .is_some_and(|node| matches!(node.kind, SchemaKind::Group { .. }))
    {
        row_length -= 1;
    }
    if row_length == 0 {
        return Err("connected XBRL outputs do not share a table row group".to_string());
    }
    clear_repeating(schema);
    let row_path = &first[..row_length];
    let row = schema_at_path_mut(schema, row_path).ok_or_else(|| {
        format!(
            "connected XBRL table row `{}` is missing from its projected schema",
            row_path.join("/")
        )
    })?;
    if !matches!(row.kind, SchemaKind::Group { .. }) {
        return Err(format!(
            "connected XBRL table row `{}` is not a group",
            row_path.join("/")
        ));
    }
    row.repeating = true;
    Ok(())
}

fn normalize_target_tables(schema: &mut SchemaNode, state: &mut PortState) -> Result<(), String> {
    let SchemaKind::Group { children, .. } = &schema.kind else {
        return Err("XBRL target document root is not a group".to_string());
    };
    let row_paths = children
        .iter()
        .filter(|branch| !branch.attribute && contains_repeating_group(branch))
        .filter_map(|branch| {
            let paths = state
                .input_keys
                .iter()
                .filter_map(|key| state.ports.get(key))
                .filter(|path| path.first() == Some(&branch.name))
                .filter(|path| !path.iter().any(|segment| segment == "defaults"))
                .map(Vec::as_slice)
                .collect::<Vec<_>>();
            common_group_path(schema, &paths)
        })
        .collect::<Vec<_>>();

    for row_path in row_paths {
        let structural_keys = state
            .input_keys
            .iter()
            .filter(|key| !state.scalar_value_keys.contains(key))
            .filter(|key| {
                state
                    .ports
                    .get(key)
                    .is_some_and(|path| path.starts_with(&row_path))
            })
            .copied()
            .collect::<Vec<_>>();
        let branch_path = &row_path[..1];
        let branch = schema_at_path_mut(schema, branch_path).ok_or_else(|| {
            format!(
                "connected XBRL table branch `{}` is missing from its projected schema",
                branch_path.join("/")
            )
        })?;
        clear_repeating(branch);

        let row = schema_at_path_mut(schema, &row_path).ok_or_else(|| {
            format!(
                "connected XBRL table row `{}` is missing from its projected schema",
                row_path.join("/")
            )
        })?;
        if !matches!(row.kind, SchemaKind::Group { .. }) {
            return Err(format!(
                "connected XBRL table row `{}` is not a group",
                row_path.join("/")
            ));
        }
        row.repeating = true;
        for key in structural_keys {
            state.ports.insert(key, row_path.clone());
        }
    }
    Ok(())
}

fn common_group_path(schema: &SchemaNode, paths: &[&[String]]) -> Option<Vec<String>> {
    let [first, rest @ ..] = paths else {
        return None;
    };
    let common_length = rest.iter().fold(first.len(), |length, path| {
        length.min(
            first
                .iter()
                .zip(path.iter())
                .take_while(|(left, right)| left == right)
                .count(),
        )
    });
    (1..=common_length).rev().find_map(|length| {
        let path = &first[..length];
        schema_at_path(schema, path)
            .is_some_and(|node| matches!(node.kind, SchemaKind::Group { .. }))
            .then(|| path.to_vec())
    })
}

fn contains_repeating_group(schema: &SchemaNode) -> bool {
    if schema.repeating && matches!(schema.kind, SchemaKind::Group { .. }) {
        return true;
    }
    let SchemaKind::Group { children, .. } = &schema.kind else {
        return false;
    };
    children.iter().any(contains_repeating_group)
}

fn input_port_ancestors(
    root: &roxmltree::Node<'_, '_>,
    input_keys: &BTreeSet<u32>,
) -> Result<BTreeMap<u32, Vec<u32>>, String> {
    fn collect(
        entry: &roxmltree::Node<'_, '_>,
        input_keys: &BTreeSet<u32>,
        ancestors: &mut Vec<u32>,
        result: &mut BTreeMap<u32, Vec<u32>>,
        next_branch: &mut u32,
    ) -> Result<(), String> {
        let key = entry
            .attribute("inpkey")
            .and_then(|raw| raw.parse::<u32>().ok())
            .filter(|key| input_keys.contains(key));
        if let Some(key) = key {
            result.insert(key, ancestors.clone());
            ancestors.push(key);
        }
        let children = entry
            .children()
            .filter(|node| node.has_tag_name("entry"))
            .collect::<Vec<_>>();
        let mut name_counts = BTreeMap::new();
        for child in &children {
            let normalized = normalize_xml_entry_name(child.attribute("name").unwrap_or_default());
            *name_counts.entry(normalized).or_insert(0usize) += 1;
        }
        for child in children {
            let normalized = normalize_xml_entry_name(child.attribute("name").unwrap_or_default());
            let duplicate_branch = name_counts.get(&normalized).copied().unwrap_or_default() > 1;
            if duplicate_branch {
                let branch = *next_branch;
                *next_branch = next_branch.checked_add(1).ok_or(
                    "XBRL target has too many cloned entry branches to preserve their identity",
                )?;
                ancestors.push(branch);
            }
            collect(&child, input_keys, ancestors, result, next_branch)?;
            if duplicate_branch {
                ancestors.pop();
            }
        }
        if key.is_some() {
            ancestors.pop();
        }
        Ok(())
    }

    let mut result = BTreeMap::new();
    let mut next_branch = input_keys
        .last()
        .copied()
        .unwrap_or_default()
        .checked_add(1)
        .ok_or("XBRL target input port range leaves no room for cloned branch identities")?;
    collect(
        root,
        input_keys,
        &mut Vec::new(),
        &mut result,
        &mut next_branch,
    )?;
    Ok(result)
}

fn schema_at_path<'a>(mut schema: &'a SchemaNode, path: &[String]) -> Option<&'a SchemaNode> {
    for segment in path {
        let SchemaKind::Group { children, .. } = &schema.kind else {
            return None;
        };
        schema = children
            .iter()
            .find(|child| child.name == *segment && !child.attribute)
            .or_else(|| children.iter().find(|child| child.name == *segment))?;
    }
    Some(schema)
}

fn clear_repeating(schema: &mut SchemaNode) {
    schema.repeating = false;
    if let SchemaKind::Group { children, .. } = &mut schema.kind {
        for child in children {
            clear_repeating(child);
        }
    }
}

fn schema_at_path_mut<'a>(
    mut schema: &'a mut SchemaNode,
    path: &[String],
) -> Option<&'a mut SchemaNode> {
    for segment in path {
        let SchemaKind::Group { children, .. } = &mut schema.kind else {
            return None;
        };
        let index = children
            .iter()
            .position(|child| child.name == *segment && !child.attribute)
            .or_else(|| children.iter().position(|child| child.name == *segment))?;
        schema = &mut children[index];
    }
    Some(schema)
}

#[derive(Default)]
struct PortState {
    ports: BTreeMap<u32, Vec<String>>,
    input_keys: BTreeSet<u32>,
    output_keys: BTreeSet<u32>,
    scalar_value_keys: BTreeSet<u32>,
    promotable_value_keys: BTreeSet<u32>,
}

impl PortState {
    fn record(
        &mut self,
        entry: &roxmltree::Node<'_, '_>,
        path: &[String],
    ) -> Result<[Option<u32>; 2], String> {
        let input = self.record_attribute(entry, "inpkey", path, true)?;
        let output = self.record_attribute(entry, "outkey", path, false)?;
        Ok([input, output])
    }

    fn record_value(
        &mut self,
        entry: &roxmltree::Node<'_, '_>,
        path: &[String],
        attribute: bool,
    ) -> Result<(), String> {
        let keys = self.record(entry, path)?;
        for key in keys.into_iter().flatten() {
            self.scalar_value_keys.insert(key);
            if !attribute {
                self.promotable_value_keys.insert(key);
            }
        }
        Ok(())
    }

    fn record_attribute(
        &mut self,
        entry: &roxmltree::Node<'_, '_>,
        attribute: &str,
        path: &[String],
        input: bool,
    ) -> Result<Option<u32>, String> {
        let Some(raw) = entry.attribute(attribute) else {
            return Ok(None);
        };
        let key = raw
            .parse::<u32>()
            .map_err(|_| format!("XBRL {attribute} `{raw}` is not a valid port key"))?;
        if let Some(existing) = self.ports.insert(key, path.to_vec()) {
            return Err(format!(
                "XBRL port key `{key}` is duplicated at `{}` and `{}`",
                existing.join("/"),
                path.join("/")
            ));
        }
        if input {
            self.input_keys.insert(key);
        } else {
            self.output_keys.insert(key);
        }
        Ok(Some(key))
    }

    fn promote_scalar_path(&mut self, path: &[String]) {
        for key in &self.promotable_value_keys {
            if let Some(port_path) = self.ports.get_mut(key)
                && port_path == path
            {
                port_path.push(XML_TEXT_FIELD.to_string());
            }
        }
    }
}

fn document_payload<'a, 'input>(
    root: &roxmltree::Node<'a, 'input>,
) -> Result<roxmltree::Node<'a, 'input>, String> {
    let mut entry = child(root, "entry").ok_or("XBRL root has no entry tree")?;
    for wrapper in ["FileInstance", "document"] {
        let (name, attribute) =
            normalize_xml_entry_name(entry.attribute("name").unwrap_or_default());
        if name == wrapper && !attribute {
            entry = child(&entry, "entry")
                .ok_or_else(|| format!("XBRL `{wrapper}` wrapper has no payload entry"))?;
        }
    }
    let (name, attribute) = normalize_xml_entry_name(entry.attribute("name").unwrap_or_default());
    if name != "xbrl" || attribute {
        return Err("XBRL entry tree has no xbrl document payload".to_string());
    }
    Ok(entry)
}

fn build_root_schema(
    root: &roxmltree::Node<'_, '_>,
    state: &mut PortState,
) -> Result<SchemaNode, String> {
    let _ = state.record(root, &[])?;
    let mut children = Vec::new();
    let mut path = Vec::new();
    let mut unit_index = 0usize;
    for entry in root.children().filter(|node| node.has_tag_name("entry")) {
        let (name, attribute) =
            normalize_xml_entry_name(entry.attribute("name").unwrap_or_default());
        let override_name = (name == "unit" && !attribute).then(|| {
            unit_index += 1;
            format!("{}{unit_index}", mapping::XBRL_UNIT_FIELD_PREFIX)
        });
        if let Some(node) = build_entry_named(&entry, &mut path, state, override_name)? {
            merge_child(&mut children, node, state, &path)?;
        }
    }
    if children.is_empty() {
        return Err("XBRL entry tree has no connected fields".to_string());
    }
    Ok(SchemaNode::group("xbrl", children))
}

fn build_entry(
    entry: &roxmltree::Node<'_, '_>,
    path: &mut Vec<String>,
    state: &mut PortState,
) -> Result<Option<SchemaNode>, String> {
    build_entry_named(entry, path, state, None)
}

fn build_entry_named(
    entry: &roxmltree::Node<'_, '_>,
    path: &mut Vec<String>,
    state: &mut PortState,
    override_name: Option<String>,
) -> Result<Option<SchemaNode>, String> {
    if !has_connected_port(entry) {
        return Ok(None);
    }
    let raw_name = entry.attribute("name").unwrap_or_default();
    let (name, legacy_attribute) = normalize_xml_entry_name(raw_name);
    if name.is_empty() {
        return Err("connected XBRL entry has no name".to_string());
    }
    let name = override_name.unwrap_or_else(|| name.to_string());
    path.push(name.clone());

    let mut children = Vec::new();
    for child_entry in entry.children().filter(|node| node.has_tag_name("entry")) {
        if let Some(child) = build_entry(&child_entry, path, state)? {
            merge_child(&mut children, child, state, path)?;
        }
    }

    let attribute = legacy_attribute || entry.attribute("type") == Some("attribute");
    if attribute && !children.is_empty() {
        return Err(format!(
            "XBRL attribute entry `{}` has connected child fields",
            path.join("/")
        ));
    }
    let has_non_attribute_child = children.iter().any(|child| !child.attribute);
    let has_own_port = entry.attribute("inpkey").is_some() || entry.attribute("outkey").is_some();
    let simple_content = !attribute
        && has_own_port
        && !children.is_empty()
        && children.iter().all(|child| child.attribute);
    if simple_content {
        path.push(XML_TEXT_FIELD.to_string());
        state.record_value(entry, path, false)?;
        path.pop();
        children.push(SchemaNode::scalar(XML_TEXT_FIELD, ScalarType::String).text());
    } else if children.is_empty() || attribute {
        state.record_value(entry, path, attribute)?;
    } else {
        let _ = state.record(entry, path)?;
    }
    path.pop();

    let mut node = if children.is_empty() || attribute {
        SchemaNode::scalar(name, ScalarType::String)
    } else {
        SchemaNode::group(name, children)
    };
    node.attribute = attribute;
    node.repeating = !attribute && has_non_attribute_child && has_own_port;
    Ok(Some(node))
}

fn merge_child(
    children: &mut Vec<SchemaNode>,
    incoming: SchemaNode,
    state: &mut PortState,
    parent_path: &[String],
) -> Result<(), String> {
    let Some(existing) = children
        .iter_mut()
        .find(|child| child.name == incoming.name && child.attribute == incoming.attribute)
    else {
        children.push(incoming);
        return Ok(());
    };
    let mut path = parent_path.to_vec();
    path.push(incoming.name.clone());
    existing.repeating |= incoming.repeating;
    match (&mut existing.kind, incoming.kind) {
        (SchemaKind::Scalar { ty: existing }, SchemaKind::Scalar { ty: incoming }) => {
            if *existing != incoming {
                return Err(format!(
                    "XBRL duplicate entry `{}` has incompatible scalar types",
                    path.join("/")
                ));
            }
        }
        (
            SchemaKind::Group {
                children: existing_children,
                ..
            },
            SchemaKind::Group {
                children: incoming_children,
                ..
            },
        ) => {
            for child in incoming_children {
                merge_child(existing_children, child, state, &path)?;
            }
        }
        (
            SchemaKind::Scalar { .. },
            SchemaKind::Group {
                children: incoming_children,
                ..
            },
        ) => {
            state.promote_scalar_path(&path);
            let mut merged = vec![SchemaNode::scalar(XML_TEXT_FIELD, ScalarType::String).text()];
            for child in incoming_children {
                merge_child(&mut merged, child, state, &path)?;
            }
            existing.kind = SchemaNode::group("", merged).kind;
        }
        (SchemaKind::Group { children, .. }, SchemaKind::Scalar { .. }) => {
            state.promote_scalar_path(&path);
            merge_child(
                children,
                SchemaNode::scalar(XML_TEXT_FIELD, ScalarType::String).text(),
                state,
                &path,
            )?;
        }
    }
    Ok(())
}

fn has_connected_port(entry: &roxmltree::Node<'_, '_>) -> bool {
    entry.descendants().any(|node| {
        node.has_tag_name("entry")
            && (node.attribute("inpkey").is_some() || node.attribute("outkey").is_some())
    })
}

fn required_attribute<'a, 'input>(
    node: &roxmltree::Node<'a, 'input>,
    attribute: &str,
    description: &str,
) -> Result<&'a str, String> {
    node.attribute(attribute)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| format!("XBRL component has no {description}"))
}

fn child<'a, 'input>(
    node: &roxmltree::Node<'a, 'input>,
    name: &str,
) -> Option<roxmltree::Node<'a, 'input>> {
    node.children().find(|child| child.has_tag_name(name))
}

#[cfg(test)]
mod tests {
    use ir::{SchemaKind, XML_TEXT_FIELD};

    use super::read;

    fn parse(xml: &str) -> Result<(super::SchemaComponent, Vec<String>), String> {
        let document = roxmltree::Document::parse(xml).map_err(|error| error.to_string())?;
        let mut warnings = Vec::new();
        let component = read(
            &document.root_element(),
            std::path::Path::new("mapping.mfd"),
            &mut warnings,
        )?;
        Ok((component, warnings))
    }

    fn duplicate_shape_component(entries: &str) -> String {
        format!(
            r#"<component name="Report" library="xbrl" kind="27"><data>
              <root><entry name="xbrl"><entry name="Report">
                <entry name="@Metric" outkey="13"/>{entries}
              </entry></entry></root>
              <xbrl schema="report.xsd" inputinstance="facts.xbrl"/>
            </data></component>"#
        )
    }

    fn assert_scalar_group_duplicate(entries: &str) {
        let xml = duplicate_shape_component(entries);
        let component = match parse(&xml) {
            Ok((component, _)) => component,
            Err(reason) => panic!("compatible scalar/group XBRL duplicates must merge: {reason}"),
        };
        assert_eq!(
            component.ports.get(&10).map(|path| path.join("/")),
            Some("Report/Metric/#text".to_string())
        );
        assert_eq!(
            component.ports.get(&11).map(|path| path.join("/")),
            Some("Report/Metric/Unit".to_string())
        );
        assert_eq!(
            component.ports.get(&12).map(|path| path.join("/")),
            Some("Report/Metric".to_string())
        );
        assert_eq!(
            component.ports.get(&13).map(|path| path.join("/")),
            Some("Report/Metric".to_string())
        );
        let Some(report) = component.schema.child("Report") else {
            panic!("merged XBRL schema must retain Report");
        };
        let SchemaKind::Group { children, .. } = &report.kind else {
            panic!("merged XBRL Report must remain a group");
        };
        let Some(metric) = children
            .iter()
            .find(|child| child.name == "Metric" && !child.attribute)
        else {
            panic!("merged XBRL schema must retain Metric");
        };
        assert!(
            children
                .iter()
                .any(|child| child.name == "Metric" && child.attribute)
        );
        assert!(metric.repeating);
        assert!(metric.child(XML_TEXT_FIELD).is_some_and(|text| text.text));
        assert!(metric.child("Unit").is_some());
    }

    #[test]
    fn source_boundary_merges_paths_and_preserves_repeating_groups() {
        let xml = r#"<component name="Report" library="xbrl" kind="27">
            <properties/>
            <data>
              <root><entry name="FileInstance"><entry name="document"><entry name="xbrl">
                <entry name="IncomeStatement">
                  <entry name="rows" outkey="10">
                    <entry name="Period"><entry name="Start" outkey="11"/></entry>
                    <entry name="Amount" outkey="12"/>
                  </entry>
                  <entry name="rows"><entry name="Amount" outkey="13"/></entry>
                </entry>
              </entry></entry></entry></root>
              <xbrl schema="taxonomy/report.xsd" inputinstance="facts.xbrl"/>
            </data>
          </component>"#;
        let Ok((component, warnings)) = parse(xml) else {
            panic!("self-authored XBRL source boundary must import");
        };
        assert!(component.is_source);
        assert_eq!(component.input_instance.as_deref(), Some("facts.xbrl"));
        assert!(component.options.xbrl.is_some());
        assert!(component.options.xbrl.as_ref().is_some_and(|boundary| {
            boundary.mode() == mapping::XbrlBoundaryMode::ExternalSource
                && boundary.taxonomy() == "taxonomy/report.xsd"
                && boundary.presentation().is_none()
        }));
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(component.output_keys.len(), 4);
        assert!(component.input_keys.is_empty());
        assert_eq!(
            component.ports.get(&13).map(|path| path.join("/")),
            Some("IncomeStatement/rows/Amount".to_string())
        );
        let Some(statement) = component.schema.child("IncomeStatement") else {
            panic!("XBRL source schema must contain its connected statement");
        };
        let Some(rows) = statement.child("rows") else {
            panic!("XBRL source schema must merge its row branches");
        };
        assert!(rows.repeating);
        let SchemaKind::Group { children, .. } = &rows.kind else {
            panic!("XBRL row port must remain a group");
        };
        assert_eq!(
            children
                .iter()
                .filter(|child| child.name == "Amount")
                .count(),
            1
        );
    }

    #[test]
    fn target_boundary_preserves_attributes_and_presentation() {
        let xml = r#"<component name="Report" library="xbrl" kind="27">
            <properties XSLTDefaultOutput="1"/>
            <data>
              <root><entry name="FileInstance"><entry name="document"><entry name="xbrl">
                <entry name="IncomeStatement"><entry name="rows" inpkey="20">
                  <entry name="Amount" inpkey="21"/>
                  <entry name="decimals" type="attribute" inpkey="22"/>
                  <entry name="identifier" inpkey="23">
                    <entry name="scheme" type="attribute" inpkey="24"/>
                  </entry>
                </entry></entry>
              </entry></entry></entry></root>
              <xbrl schema="taxonomy/report.xsd" sps="income-view.sps" outputinstance="report.xbrl"/>
            </data>
          </component>"#;
        let Ok((component, warnings)) = parse(xml) else {
            panic!("self-authored XBRL target boundary must import");
        };
        assert!(!component.is_source);
        assert!(component.is_default_output);
        assert_eq!(component.output_instance.as_deref(), Some("report.xbrl"));
        assert!(component.options.xbrl.is_some());
        assert!(component.options.xbrl.as_ref().is_some_and(|boundary| {
            boundary.mode() == mapping::XbrlBoundaryMode::ExternalTarget
                && boundary.taxonomy() == "taxonomy/report.xsd"
                && boundary.presentation() == Some("income-view.sps")
        }));
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("could not compile numeric fact metadata"));
        assert_eq!(
            component.input_keys,
            [20, 21, 22, 23, 24].into_iter().collect()
        );
        assert!(component.output_keys.is_empty());
        assert_eq!(
            component.ports.get(&23).map(|path| path.join("/")),
            Some("IncomeStatement/rows/identifier/#text".to_string())
        );
        let Some(decimals) = component
            .schema
            .child("IncomeStatement")
            .and_then(|statement| statement.child("rows"))
            .and_then(|rows| rows.child("decimals"))
        else {
            panic!("XBRL target schema must preserve its connected attribute");
        };
        assert!(decimals.attribute);
        let Some(identifier) = component
            .schema
            .child("IncomeStatement")
            .and_then(|statement| statement.child("rows"))
            .and_then(|rows| rows.child("identifier"))
        else {
            panic!("XBRL target schema must preserve simple content with attributes");
        };
        assert!(!identifier.repeating);
        assert!(
            identifier
                .child(XML_TEXT_FIELD)
                .is_some_and(|text| text.text)
        );
        assert!(
            identifier
                .child("scheme")
                .is_some_and(|scheme| scheme.attribute)
        );
    }

    #[test]
    fn target_tables_lift_descendant_repetition_without_repeating_static_branches() {
        let xml = r#"<component name="Report" library="xbrl" kind="27"><data>
            <root><entry name="xbrl">
              <entry name="IncomeTable"><entry name="tableset">
                <entry name="defaults">
                  <entry name="monetary"><entry name="decimals" type="attribute" inpkey="1"/></entry>
                </entry>
                <entry name="row">
                  <entry name="period" inpkey="2">
                    <entry name="startDate" inpkey="3"/>
                    <entry name="endDate" inpkey="4"/>
                  </entry>
                  <entry name="facts"><entry name="Revenue" inpkey="5"/></entry>
                </entry>
              </entry></entry>
              <entry name="BalanceTable"><entry name="row">
                <entry name="identifier" inpkey="6">
                  <entry name="scheme" type="attribute" inpkey="7"/>
                </entry>
                <entry name="aspects">
                  <entry name="period" inpkey="8"><entry name="instant" inpkey="9"/></entry>
                  <entry name="context" inpkey="10"><entry name="Assets" inpkey="11"/></entry>
                </entry>
              </entry></entry>
              <entry name="unit">
                <entry name="id" type="attribute" inpkey="12"/>
                <entry name="measure" inpkey="13"/>
              </entry>
            </entry></root>
            <xbrl schema="report.xsd" sps="report.sps"/>
          </data></component>"#;
        let Ok((component, warnings)) = parse(xml) else {
            panic!("self-authored XBRL target tables must import");
        };
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("could not compile numeric fact metadata"));

        let Some(income_row) = component
            .schema
            .child("IncomeTable")
            .and_then(|table| table.child("tableset"))
            .and_then(|table| table.child("row"))
        else {
            panic!("income table row must remain projected");
        };
        assert!(income_row.repeating);
        assert!(
            income_row
                .child("period")
                .is_some_and(|period| !period.repeating)
        );

        let Some(balance_row) = component
            .schema
            .child("BalanceTable")
            .and_then(|table| table.child("row"))
        else {
            panic!("balance table row must remain projected");
        };
        assert!(balance_row.repeating);
        let Some(aspects) = balance_row.child("aspects") else {
            panic!("balance table aspects must remain projected");
        };
        assert!(!aspects.repeating);
        assert!(
            aspects
                .child("period")
                .is_some_and(|period| !period.repeating)
        );
        assert!(
            aspects
                .child("context")
                .is_some_and(|context| !context.repeating)
        );

        assert!(
            component
                .schema
                .child("IncomeTable")
                .and_then(|table| table.child("tableset"))
                .and_then(|tableset| tableset.child("defaults"))
                .is_some_and(|defaults| !defaults.repeating)
        );
        assert!(matches!(
            &component.schema.kind,
            SchemaKind::Group { children, .. }
                if children.iter().any(|unit| {
                    unit.name.starts_with(mapping::XBRL_UNIT_FIELD_PREFIX) && !unit.repeating
                })
        ));
        assert_eq!(
            component.ports.get(&2).map(|path| path.join("/")),
            Some("IncomeTable/tableset/row".to_string())
        );
        assert_eq!(
            component.ports.get(&8).map(|path| path.join("/")),
            Some("BalanceTable/row".to_string())
        );
        assert_eq!(
            component.ports.get(&10).map(|path| path.join("/")),
            Some("BalanceTable/row".to_string())
        );
        assert_eq!(
            component.ports.get(&11).map(|path| path.join("/")),
            Some("BalanceTable/row/aspects/context/Assets".to_string())
        );
        assert_eq!(
            component.ports.get(&7).map(|path| path.join("/")),
            Some("BalanceTable/row/identifier/scheme".to_string())
        );
        assert!(
            component
                .input_ancestors
                .get(&11)
                .is_some_and(|ancestors| ancestors.contains(&10))
        );
    }

    #[test]
    fn scalar_then_group_duplicate_promotes_the_scalar_port_to_text() {
        assert_scalar_group_duplicate(
            r#"<entry name="Metric" outkey="10"/>
               <entry name="Metric" outkey="12"><entry name="Unit" outkey="11"/></entry>"#,
        );
    }

    #[test]
    fn group_then_scalar_duplicate_promotes_the_scalar_port_to_text() {
        assert_scalar_group_duplicate(
            r#"<entry name="Metric" outkey="12"><entry name="Unit" outkey="11"/></entry>
               <entry name="Metric" outkey="10"/>"#,
        );
    }

    #[test]
    fn legacy_names_normalize_without_stripping_true_qnames() {
        let xml = r#"<component name="Report" library="xbrl" kind="27"><data>
            <root><entry name="0:FileInstance"><entry name="1:document"><entry name="0:xbrl">
              <entry name="gaap:Report">
                <entry name="0:Amount" outkey="30"/>
                <entry name="12:@decimals" outkey="31"/>
              </entry>
            </entry></entry></entry></root>
            <xbrl schema="report.xsd" inputinstance="facts.xbrl"/>
          </data></component>"#;
        let Ok((component, _)) = parse(xml) else {
            panic!("legacy XBRL entry names must normalize");
        };
        assert_eq!(
            component.ports.get(&30).map(|path| path.join("/")),
            Some("gaap:Report/Amount".to_string())
        );
        assert_eq!(
            component.ports.get(&31).map(|path| path.join("/")),
            Some("gaap:Report/decimals".to_string())
        );
        let Some(report) = component.schema.child("gaap:Report") else {
            panic!("real QName prefix must remain in the schema");
        };
        assert!(report.child("Amount").is_some());
        assert!(
            report
                .child("decimals")
                .is_some_and(|decimals| decimals.attribute)
        );
    }

    #[test]
    fn attribute_with_connected_descendants_is_rejected() {
        let xml = r#"<component name="Report" library="xbrl" kind="27"><data>
            <root><entry name="xbrl"><entry name="Report">
              <entry name="0:@invalid"><entry name="Nested" outkey="1"/></entry>
            </entry></entry></root>
            <xbrl schema="report.xsd" inputinstance="facts.xbrl"/>
          </data></component>"#;
        assert!(
            matches!(parse(xml), Err(reason) if reason.contains("attribute entry `Report/invalid` has connected child fields"))
        );
    }

    #[test]
    fn boundary_rejects_mixed_directions_and_other_kinds() {
        let mixed = r#"<component name="Report" library="xbrl" kind="27"><data>
            <root><entry name="xbrl"><entry name="Left" outkey="1"/><entry name="Right" inpkey="2"/></entry></root>
            <xbrl schema="report.xsd"/>
          </data></component>"#;
        assert!(matches!(parse(mixed), Err(reason) if reason.contains("exactly one direction")));

        let other = r#"<component name="measure" library="xbrl" kind="5"><data>
            <root><entry name="xbrl"><entry name="Value" outkey="1"/></entry></root>
            <xbrl schema="report.xsd"/>
          </data></component>"#;
        assert!(matches!(parse(other), Err(reason) if reason.contains("kind=27")));
    }
}
