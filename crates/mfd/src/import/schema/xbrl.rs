use std::collections::{BTreeMap, BTreeSet};

use ir::{ScalarType, SchemaKind, SchemaNode, XML_TEXT_FIELD};
use mapping::{FormatOptions, XbrlBoundaryOptions};

use super::{ComponentFormat, SchemaComponent, is_default_output, normalize_xml_entry_name};

pub(super) fn read(
    component: &roxmltree::Node<'_, '_>,
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

    let mut state = PortState::default();
    let schema = build_root_schema(&payload, &mut state)?;
    let is_source = match (state.input_keys.is_empty(), state.output_keys.is_empty()) {
        (true, false) => true,
        (false, true) => false,
        _ => {
            return Err("XBRL component must expose ports in exactly one direction".to_string());
        }
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
    .map_err(|error| format!("invalid XBRL boundary metadata ({error})"))?;

    let direction = if is_source { "source" } else { "target" };
    warnings.push(format!(
        "XBRL component `{name}` was imported as a typed external {direction} boundary; XBRL taxonomy and table runtime is unavailable, so file execution remains disabled"
    ));

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
        compute_when_key: None,
        ports: state.ports,
        input_keys: state.input_keys,
        output_keys: state.output_keys,
        db_queries: Vec::new(),
        dynamic_json: None,
    })
}

#[derive(Default)]
struct PortState {
    ports: BTreeMap<u32, Vec<String>>,
    input_keys: BTreeSet<u32>,
    output_keys: BTreeSet<u32>,
    scalar_value_keys: BTreeSet<u32>,
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
        if !attribute {
            self.scalar_value_keys.extend(keys.into_iter().flatten());
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
        for key in &self.scalar_value_keys {
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
    for entry in root.children().filter(|node| node.has_tag_name("entry")) {
        if let Some(node) = build_entry(&entry, &mut path, state)? {
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
    if !has_connected_port(entry) {
        return Ok(None);
    }
    let raw_name = entry.attribute("name").unwrap_or_default();
    let (name, legacy_attribute) = normalize_xml_entry_name(raw_name);
    if name.is_empty() {
        return Err("connected XBRL entry has no name".to_string());
    }
    let name = name.to_string();
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
        let component = read(&document.root_element(), &mut warnings)?;
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
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("external source boundary"));
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
        assert!(warnings[0].contains("external target boundary"));
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
