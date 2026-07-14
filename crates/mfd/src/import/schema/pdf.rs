use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use mapping::{
    FormatOptions, PdfAnchorAssignment, PdfAnchorAxis, PdfCapture, PdfCommand, PdfCoordinate,
    PdfEdgeFind, PdfEdgeRows, PdfGroup, PdfLayout, PdfPageSelection, PdfReference, PdfRegion,
    PdfVerticalBoundaryFind,
};

use super::{
    ComponentFormat, SchemaComponent, entry_key_sets, is_default_output, parse_u32, schema_node_at,
};

const MAX_PXT_BYTES: usize = 1024 * 1024;
const MAX_PXT_DEPTH: usize = 64;
const MAX_PXT_ELEMENTS: usize = 4096;

pub(super) fn read(
    component: &roxmltree::Node<'_, '_>,
    mfd_path: &Path,
) -> Result<SchemaComponent, String> {
    if component.attribute("kind") != Some("34") {
        return Err("only kind=34 PDF components are supported".to_string());
    }
    let name = component.attribute("name").unwrap_or_default().to_string();
    let data = child(component, "data").ok_or_else(|| "component has no data block".to_string())?;
    let root =
        child(&data, "root").ok_or_else(|| "component has no visible entry tree".to_string())?;
    let documents = root
        .descendants()
        .filter(|node| node.has_tag_name("entry") && node.attribute("type") == Some("doc-pdf"))
        .collect::<Vec<_>>();
    let [document_entry] = documents.as_slice() else {
        return Err("component must expose exactly one doc-pdf entry".to_string());
    };
    let document = child(document_entry, "document")
        .ok_or_else(|| "doc-pdf entry has no document metadata".to_string())?;
    let payload = child(document_entry, "entry")
        .ok_or_else(|| "doc-pdf entry has no document root".to_string())?;
    let declared_root = document
        .attribute("root")
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "PDF document metadata has no root".to_string())?;
    let visible_root = payload.attribute("name").unwrap_or_default();
    if visible_root != declared_root {
        return Err(format!(
            "PDF document root `{declared_root}` does not match visible root `{visible_root}`"
        ));
    }

    let (declared_inputs, declared_outputs) = entry_key_sets(&root);
    if !declared_inputs.is_empty() {
        return Err("PDF target or mixed-direction components are not supported".to_string());
    }
    if declared_outputs.is_empty() {
        return Err("PDF source has no output ports".to_string());
    }

    let schema_file = document
        .attribute("schemafile")
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "PDF document metadata has no schemafile".to_string())?;
    let schema_path = resolve_reference(mfd_path, schema_file, "PDF template")?;
    let layout = parse_layout(&schema_path, declared_root)?;
    let schema = layout.schema();
    let ports = collect_ports(&root, document_entry, &payload, &declared_outputs, &schema)?;
    if ports.is_empty() {
        return Err("PDF source has no output ports matching its template".to_string());
    }

    let input_file = root
        .descendants()
        .filter(|node| node.has_tag_name("file") && node.attribute("role") == Some("inputinstance"))
        .collect::<Vec<_>>();
    let [input_file] = input_file.as_slice() else {
        return Err("PDF source must declare exactly one input instance file".to_string());
    };
    let input_name = input_file
        .attribute("name")
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "PDF input instance has no file name".to_string())?;
    let input_path = resolve_reference(mfd_path, input_name, "PDF input instance")?;
    let input_instance = portable_instance_path(mfd_path, &input_path);
    let output_keys = ports.keys().copied().collect();

    Ok(SchemaComponent {
        name,
        format: ComponentFormat::Pdf,
        schema,
        input_instance: Some(input_instance),
        output_instance: None,
        options: FormatOptions {
            pdf: Some(layout),
            ..FormatOptions::default()
        },
        is_source: true,
        is_default_output: is_default_output(component),
        is_variable: false,
        compute_when_key: None,
        ports,
        input_keys: BTreeSet::new(),
        output_keys,
        db_queries: Vec::new(),
        dynamic_json: None,
    })
}

fn parse_layout(path: &Path, expected_root: &str) -> Result<PdfLayout, String> {
    let bytes = std::fs::read(path)
        .map_err(|error| format!("could not read PDF template `{}` ({error})", path.display()))?;
    if bytes.len() > MAX_PXT_BYTES {
        return Err(format!(
            "PDF template `{}` exceeds the {MAX_PXT_BYTES}-byte limit",
            path.display()
        ));
    }
    let source = String::from_utf8(bytes)
        .map_err(|_| format!("PDF template `{}` is not valid UTF-8", path.display()))?;
    if source.contains("<!DOCTYPE") {
        return Err(format!(
            "PDF template `{}` uses a document type declaration",
            path.display()
        ));
    }
    let document = roxmltree::Document::parse(&source).map_err(|error| {
        format!(
            "could not parse PDF template `{}` ({error})",
            path.display()
        )
    })?;
    let elements = document
        .descendants()
        .filter(roxmltree::Node::is_element)
        .collect::<Vec<_>>();
    if elements.len() > MAX_PXT_ELEMENTS {
        return Err(format!(
            "PDF template `{}` exceeds the {MAX_PXT_ELEMENTS}-element limit",
            path.display()
        ));
    }
    if elements.iter().any(|element| {
        element
            .ancestors()
            .filter(roxmltree::Node::is_element)
            .count()
            > MAX_PXT_DEPTH
    }) {
        return Err(format!(
            "PDF template `{}` exceeds the {MAX_PXT_DEPTH}-level depth limit",
            path.display()
        ));
    }

    let root = document.root_element();
    if !root.has_tag_name("Document") {
        return Err("PDF template root is not <Document>".to_string());
    }
    let model_root = child(&root, "Template")
        .and_then(|template| child(&template, "Model"))
        .and_then(|model| child(&model, "Root"))
        .ok_or_else(|| "PDF template has no Template/Model/Root command".to_string())?;
    let root_name = required_text_child(&model_root, "Label")?;
    if root_name != expected_root {
        return Err(format!(
            "PDF template root `{root_name}` does not match document root `{expected_root}`"
        ));
    }
    let children = child(&model_root, "Children")
        .ok_or_else(|| "PDF template root has no Children block".to_string())?;
    let commands = parse_commands(&children)?;
    PdfLayout::new(root_name, PdfPageSelection::All, commands)
        .map_err(|error| format!("invalid PDF extraction layout ({error})"))
}

fn parse_commands(children: &roxmltree::Node<'_, '_>) -> Result<Vec<PdfCommand>, String> {
    let mut commands = Vec::new();
    for node in children.children().filter(roxmltree::Node::is_element) {
        commands.extend(parse_command(&node)?);
    }
    Ok(commands)
}

fn parse_command(node: &roxmltree::Node<'_, '_>) -> Result<Vec<PdfCommand>, String> {
    match node.tag_name().name() {
        "Capture" => Ok(vec![PdfCommand::Capture(PdfCapture {
            name: required_text_child(node, "Label")?,
            region: parse_required_region(node)?,
        })]),
        "Grouping" => parse_group(node),
        "Splitter" => {
            let children = child(node, "Children")
                .ok_or_else(|| "PDF Splitter has no Children block".to_string())?;
            Ok(vec![PdfCommand::EdgeRows(PdfEdgeRows {
                region: parse_required_region(node)?,
                find: parse_edge_find(node)?,
                minimum_extent: parse_minimum_extent(node)?,
                children: parse_commands(&children)?,
            })])
        }
        "VerticalAnchorAssignment" | "HorizontalAnchorAssignment" => {
            let axis = if node.has_tag_name("VerticalAnchorAssignment") {
                PdfAnchorAxis::Vertical
            } else {
                PdfAnchorAxis::Horizontal
            };
            Ok(vec![PdfCommand::Anchor(PdfAnchorAssignment {
                name: required_text_child(node, "Name")?,
                axis,
                at: parse_coordinate(&required_text_child(node, "Expression")?)?,
            })])
        }
        "BoundaryFindVertical" => Ok(vec![PdfCommand::BoundaryFindVertical(
            PdfVerticalBoundaryFind {
                region: parse_optional_region(node)?,
                begin_anchor: required_text_child(node, "NameBegin")?,
                end_anchor: required_text_child(node, "NameEnd")?,
                find: parse_edge_find(node)?,
            },
        )]),
        other => Err(format!("unsupported PDF template command <{other}>")),
    }
}

fn parse_group(node: &roxmltree::Node<'_, '_>) -> Result<Vec<PdfCommand>, String> {
    let filter = child(node, "Filter")
        .and_then(|value| value.text())
        .map(str::trim)
        .unwrap_or_default();
    if !matches!(filter, "" | "1") {
        return Err(format!("PDF grouping filter `{filter}` is not supported"));
    }
    let one_per_page =
        child(node, "Kind").is_some_and(|kind| child(&kind, "OneGroupPerPage").is_some());
    if !one_per_page {
        return Err("PDF grouping kind is not OneGroupPerPage".to_string());
    }
    let children =
        child(node, "Children").ok_or_else(|| "PDF Grouping has no Children block".to_string())?;
    let children = parse_commands(&children)?;
    let name = child(node, "Label")
        .and_then(|label| label.text())
        .map(str::trim)
        .unwrap_or_default();
    if name.is_empty() {
        return Ok(children);
    }
    Ok(vec![PdfCommand::GroupPerPage(PdfGroup {
        name: name.to_string(),
        region: parse_optional_region(node)?,
        children,
    })])
}

fn parse_edge_find(node: &roxmltree::Node<'_, '_>) -> Result<PdfEdgeFind, String> {
    let feature = child(node, "FeatureFind")
        .and_then(|feature| child(&feature, "EdgeFind"))
        .ok_or_else(|| format!("PDF {} has no EdgeFind feature", node.tag_name().name()))?;
    if let Some(resolution) = child(&feature, "Resolution")
        && !resolution
            .children()
            .filter(roxmltree::Node::is_element)
            .any(|resolution| resolution.has_tag_name("Standard"))
    {
        return Err("only standard-resolution PDF edge finding is supported".to_string());
    }
    Ok(PdfEdgeFind {
        fill: parse_points(&required_text_child(&feature, "Fill")?)?,
        prominence: parse_f64(&required_text_child(&feature, "Prominence")?)?,
    })
}

fn parse_minimum_extent(node: &roxmltree::Node<'_, '_>) -> Result<Option<f64>, String> {
    let Some(post_process) = child(node, "PostProcess") else {
        return Ok(None);
    };
    let discards = child(&post_process, "Behavior")
        .is_some_and(|behavior| child(&behavior, "discard").is_some());
    if !discards {
        return Err("only discard PDF splitter post-processing is supported".to_string());
    }
    let value = child(&post_process, "MinimumExtent")
        .and_then(|extent| extent.text())
        .map(str::trim)
        .unwrap_or_default();
    if value.is_empty() {
        Ok(None)
    } else {
        parse_points(value).map(Some)
    }
}

fn parse_required_region(node: &roxmltree::Node<'_, '_>) -> Result<PdfRegion, String> {
    let value = required_text_child(node, "Region")?;
    if value.trim().is_empty() {
        return Err(format!(
            "PDF {} has an empty Region",
            node.tag_name().name()
        ));
    }
    parse_region(&value)
}

fn parse_optional_region(node: &roxmltree::Node<'_, '_>) -> Result<PdfRegion, String> {
    match child(node, "Region").and_then(|region| region.text()) {
        Some(value) if !value.trim().is_empty() => parse_region(value),
        _ => Ok(current_region()),
    }
}

fn parse_region(value: &str) -> Result<PdfRegion, String> {
    let value = value.trim();
    let body = value
        .strip_prefix('{')
        .and_then(|value| value.strip_suffix('}'))
        .ok_or_else(|| format!("invalid PDF region `{value}`"))?;
    let mut edges = BTreeMap::new();
    for part in split_region_parts(body)? {
        let (name, expression) = part
            .split_once(':')
            .ok_or_else(|| format!("invalid PDF region edge `{part}`"))?;
        let name = name.trim();
        if !matches!(name, "Left" | "Top" | "Right" | "Bottom") {
            return Err(format!("unknown PDF region edge `{name}`"));
        }
        if edges
            .insert(name, parse_coordinate(expression.trim())?)
            .is_some()
        {
            return Err(format!("duplicate PDF region edge `{name}`"));
        }
    }
    Ok(PdfRegion {
        left: take_edge(&mut edges, "Left")?,
        top: take_edge(&mut edges, "Top")?,
        right: take_edge(&mut edges, "Right")?,
        bottom: take_edge(&mut edges, "Bottom")?,
    })
}

fn split_region_parts(value: &str) -> Result<Vec<&str>, String> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut parentheses = 0_u32;
    let mut brackets = 0_u32;
    for (index, character) in value.char_indices() {
        match character {
            '(' => parentheses += 1,
            ')' => {
                parentheses = parentheses
                    .checked_sub(1)
                    .ok_or_else(|| "unbalanced PDF region parentheses".to_string())?;
            }
            '[' => brackets += 1,
            ']' => {
                brackets = brackets
                    .checked_sub(1)
                    .ok_or_else(|| "unbalanced PDF region brackets".to_string())?;
            }
            ',' if parentheses == 0 && brackets == 0 => {
                parts.push(value[start..index].trim());
                start = index + 1;
            }
            _ => {}
        }
    }
    if parentheses != 0 || brackets != 0 {
        return Err("unbalanced PDF region expression".to_string());
    }
    parts.push(value[start..].trim());
    if parts.iter().any(|part| part.is_empty()) {
        return Err("PDF region contains an empty edge".to_string());
    }
    Ok(parts)
}

fn parse_coordinate(value: &str) -> Result<PdfCoordinate, String> {
    let mut value = value.trim();
    while let Some(inner) = value
        .strip_prefix('(')
        .and_then(|value| value.strip_suffix(')'))
    {
        value = inner.trim();
    }
    let mut bracket_depth = 0_u32;
    let mut operator = None;
    for (index, character) in value.char_indices() {
        match character {
            '[' => bracket_depth += 1,
            ']' => {
                bracket_depth = bracket_depth
                    .checked_sub(1)
                    .ok_or_else(|| format!("unbalanced PDF coordinate `{value}`"))?;
            }
            '+' | '-' if index > 0 && bracket_depth == 0 => {
                operator = Some((index, character));
                break;
            }
            _ => {}
        }
    }
    if bracket_depth != 0 {
        return Err(format!("unbalanced PDF coordinate `{value}`"));
    }
    let (reference, offset) = match operator {
        Some((index, operator)) => {
            let reference = value[..index].trim();
            let magnitude = parse_points(value[index + operator.len_utf8()..].trim())?;
            (
                reference,
                if operator == '-' {
                    -magnitude
                } else {
                    magnitude
                },
            )
        }
        None => (value, 0.0),
    };
    let reference = match reference {
        "Left" => PdfReference::Left,
        "Top" => PdfReference::Top,
        "Right" => PdfReference::Right,
        "Bottom" => PdfReference::Bottom,
        anchor if anchor.starts_with('[') && anchor.ends_with(']') => {
            let name = anchor[1..anchor.len() - 1].trim();
            if name.is_empty() {
                return Err("PDF coordinate has an empty anchor name".to_string());
            }
            PdfReference::Anchor(name.to_string())
        }
        _ => return Err(format!("unsupported PDF coordinate `{value}`")),
    };
    Ok(PdfCoordinate { reference, offset })
}

fn parse_points(value: &str) -> Result<f64, String> {
    let number = value
        .trim()
        .strip_suffix("pt")
        .ok_or_else(|| format!("PDF dimension `{value}` is not in points"))?;
    parse_f64(number)
}

fn parse_f64(value: &str) -> Result<f64, String> {
    let number = value
        .trim()
        .parse::<f64>()
        .map_err(|_| format!("invalid PDF number `{value}`"))?;
    if !number.is_finite() {
        return Err(format!("PDF number `{value}` is not finite"));
    }
    Ok(number)
}

fn take_edge(
    edges: &mut BTreeMap<&str, PdfCoordinate>,
    name: &'static str,
) -> Result<PdfCoordinate, String> {
    edges
        .remove(name)
        .ok_or_else(|| format!("PDF region has no {name} edge"))
}

fn current_region() -> PdfRegion {
    PdfRegion {
        left: PdfCoordinate {
            reference: PdfReference::Left,
            offset: 0.0,
        },
        top: PdfCoordinate {
            reference: PdfReference::Top,
            offset: 0.0,
        },
        right: PdfCoordinate {
            reference: PdfReference::Right,
            offset: 0.0,
        },
        bottom: PdfCoordinate {
            reference: PdfReference::Bottom,
            offset: 0.0,
        },
    }
}

fn collect_ports(
    root: &roxmltree::Node<'_, '_>,
    document_entry: &roxmltree::Node<'_, '_>,
    payload: &roxmltree::Node<'_, '_>,
    declared_outputs: &BTreeSet<u32>,
    schema: &ir::SchemaNode,
) -> Result<BTreeMap<u32, Vec<String>>, String> {
    let mut ports = BTreeMap::new();
    for wrapper in document_entry
        .ancestors()
        .take_while(|node| *node != *root)
        .filter(|node| node.has_tag_name("entry"))
        .chain(std::iter::once(*document_entry))
        .chain(std::iter::once(*payload))
    {
        if let Some(key) = parse_u32(wrapper.attribute("outkey")) {
            ports.insert(key, Vec::new());
        }
    }
    collect_descendant_ports(
        payload,
        &mut Vec::new(),
        declared_outputs,
        schema,
        &mut ports,
    )?;
    Ok(ports)
}

fn collect_descendant_ports(
    entry: &roxmltree::Node<'_, '_>,
    path: &mut Vec<String>,
    declared_outputs: &BTreeSet<u32>,
    schema: &ir::SchemaNode,
    ports: &mut BTreeMap<u32, Vec<String>>,
) -> Result<(), String> {
    for child in entry.children().filter(|node| node.has_tag_name("entry")) {
        path.push(child.attribute("name").unwrap_or_default().to_string());
        if let Some(key) = parse_u32(child.attribute("outkey"))
            && declared_outputs.contains(&key)
        {
            if schema_node_at(schema, path).is_none() {
                return Err(format!(
                    "PDF output port {key} targets unknown template path `{}`",
                    path.join("/")
                ));
            }
            ports.insert(key, path.clone());
        }
        collect_descendant_ports(&child, path, declared_outputs, schema, ports)?;
        path.pop();
    }
    Ok(())
}

fn resolve_reference(
    mfd_path: &Path,
    relative: &str,
    description: &str,
) -> Result<PathBuf, String> {
    let portable = relative.replace('\\', "/");
    let base = mfd_path.parent().unwrap_or_else(|| Path::new("."));
    let direct = if Path::new(&portable).is_absolute() {
        PathBuf::from(&portable)
    } else {
        base.join(&portable)
    };
    if direct.is_file() {
        return Ok(direct);
    }
    let file_name = direct
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| format!("{description} path `{relative}` has no file name"))?;
    let directory = direct.parent().unwrap_or(base);
    let entries = std::fs::read_dir(directory)
        .map_err(|error| format!("could not resolve {description} `{relative}` ({error})"))?;
    let mut matched = None;
    for entry in entries {
        let entry = entry
            .map_err(|error| format!("could not resolve {description} `{relative}` ({error})"))?;
        if !entry
            .file_name()
            .to_str()
            .is_some_and(|name| name.eq_ignore_ascii_case(file_name))
        {
            continue;
        }
        let file_type = entry
            .file_type()
            .map_err(|error| format!("could not inspect {description} `{relative}` ({error})"))?;
        if !file_type.is_file() {
            continue;
        }
        if matched.is_some() {
            return Err(format!(
                "{description} `{relative}` has multiple case-insensitive sibling matches"
            ));
        }
        matched = Some(entry.path());
    }
    matched.ok_or_else(|| format!("{description} `{relative}` was not found"))
}

fn portable_instance_path(mfd_path: &Path, instance_path: &Path) -> String {
    let base = mfd_path.parent().unwrap_or_else(|| Path::new("."));
    instance_path
        .strip_prefix(base)
        .unwrap_or(instance_path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn child<'a, 'input>(
    node: &roxmltree::Node<'a, 'input>,
    name: &str,
) -> Option<roxmltree::Node<'a, 'input>> {
    node.children().find(|child| child.has_tag_name(name))
}

fn required_text_child(node: &roxmltree::Node<'_, '_>, name: &str) -> Result<String, String> {
    child(node, name)
        .and_then(|child| child.text())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .ok_or_else(|| format!("PDF {} has no {name}", node.tag_name().name()))
}

#[cfg(test)]
mod tests {
    use super::{parse_coordinate, parse_region};
    use mapping::PdfReference;

    #[test]
    fn parses_absolute_and_anchor_coordinates() {
        let coordinate = parse_coordinate("(Left + 18.5pt)").unwrap();
        assert_eq!(coordinate.reference, PdfReference::Left);
        assert_eq!(coordinate.offset, 18.5);

        let coordinate = parse_coordinate("[Column] - 2pt").unwrap();
        assert_eq!(coordinate.reference, PdfReference::Anchor("Column".into()));
        assert_eq!(coordinate.offset, -2.0);
    }

    #[test]
    fn parses_complete_regions_in_any_edge_order() {
        let region =
            parse_region("{ Top: Top + 2pt, Left: Left, Bottom: [End], Right: Right - 3pt }")
                .unwrap();
        assert_eq!(region.left.reference, PdfReference::Left);
        assert_eq!(region.top.offset, 2.0);
        assert_eq!(region.right.offset, -3.0);
        assert_eq!(region.bottom.reference, PdfReference::Anchor("End".into()));
    }
}
