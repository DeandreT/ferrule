use std::collections::{BTreeMap, BTreeSet};
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};

use mapping::{
    FormatOptions, PdfAnchorAssignment, PdfAnchorAxis, PdfCapture, PdfCommand, PdfCoordinate,
    PdfEdgeFind, PdfEdgeRows, PdfGroup, PdfLayout, PdfMerge, PdfMergeComposition, PdfMergeSource,
    PdfPageSelection, PdfPages, PdfReference, PdfRegion, PdfVerticalBoundaryFind,
};

use super::{
    ComponentFormat, SchemaComponent, entry_key_sets, is_default_output, parse_u32, schema_node_at,
};

mod text;

use text::{parse_object_find_splitter, parse_text_find_splitter};

const MAX_PXT_BYTES: usize = 1024 * 1024;
const MAX_PXT_DEPTH: usize = 64;
const MAX_PXT_ELEMENTS: usize = 4096;

pub(super) fn read(
    component: &roxmltree::Node<'_, '_>,
    mfd_path: &Path,
    warnings: &mut Vec<String>,
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
    let (layout, layout_warnings) = parse_layout(&schema_path, declared_root)?;
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
    let input_instance = resolve_reference(mfd_path, input_name, "PDF input instance")
        .map(|path| portable_instance_path(mfd_path, &path))
        .unwrap_or_else(|_| input_name.replace('\\', "/"));
    let output_keys = ports.keys().copied().collect();
    warnings.extend(
        layout_warnings
            .into_iter()
            .map(|warning| format!("PDF component `{name}`: {warning}")),
    );

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
        input_ancestors: BTreeMap::new(),
        input_keys: BTreeSet::new(),
        output_keys,
        db_queries: Vec::new(),
        dynamic_json: None,
    })
}

fn parse_layout(path: &Path, expected_root: &str) -> Result<(PdfLayout, Vec<String>), String> {
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
    if let Some(layout) = child(&root, "FerruleLayout") {
        return parse_canonical_layout(&layout, expected_root);
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
    let mut merge_sources = BTreeMap::new();
    collect_merge_sources(&children, &mut merge_sources)?;
    let mut context = ParseContext {
        merge_sources,
        merge_targets: BTreeSet::new(),
    };
    let commands = parse_commands(&children, true, &mut context)?;
    if let Some(name) = context.merge_sources.keys().next() {
        return Err(format!(
            "PDF merge source `{name}` has no matching MergeTarget"
        ));
    }
    let layout = PdfLayout::new(root_name, PdfPageSelection::All, commands)
        .map_err(|error| format!("invalid PDF extraction layout ({error})"))?;
    Ok((layout, Vec::new()))
}

fn parse_canonical_layout(
    node: &roxmltree::Node<'_, '_>,
    expected_root: &str,
) -> Result<(PdfLayout, Vec<String>), String> {
    if node.attribute("version") != Some("1") {
        return Err("PDF FerruleLayout has an unsupported version".to_string());
    }
    let encoded = node
        .text()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "PDF FerruleLayout is empty".to_string())?;
    let layout = serde_json::from_str::<PdfLayout>(encoded)
        .map_err(|error| format!("invalid PDF FerruleLayout ({error})"))?;
    if layout.root_name() != expected_root {
        return Err(format!(
            "PDF template root `{}` does not match document root `{expected_root}`",
            layout.root_name()
        ));
    }
    Ok((layout, Vec::new()))
}

struct ParseContext {
    merge_sources: BTreeMap<String, Vec<PdfMergeSource>>,
    merge_targets: BTreeSet<String>,
}

fn parse_commands(
    children: &roxmltree::Node<'_, '_>,
    document_level: bool,
    context: &mut ParseContext,
) -> Result<Vec<PdfCommand>, String> {
    let mut commands = Vec::new();
    for node in children.children().filter(roxmltree::Node::is_element) {
        commands.extend(parse_command(&node, document_level, context)?);
    }
    Ok(commands)
}

fn parse_command(
    node: &roxmltree::Node<'_, '_>,
    document_level: bool,
    context: &mut ParseContext,
) -> Result<Vec<PdfCommand>, String> {
    match node.tag_name().name() {
        "Capture" => Ok(vec![PdfCommand::Capture(PdfCapture {
            name: required_text_child(node, "Label")?,
            region: parse_required_region(node)?,
        })]),
        "Grouping" => parse_group(node, document_level, context),
        "Splitter" => {
            let feature = child(node, "FeatureFind")
                .ok_or_else(|| "PDF Splitter has no FeatureFind block".to_string())?;
            if child(&feature, "EdgeFind").is_some() {
                let children = child(node, "Children")
                    .ok_or_else(|| "PDF Splitter has no Children block".to_string())?;
                let children = parse_commands(&children, false, context)?;
                Ok(vec![PdfCommand::EdgeRows(PdfEdgeRows {
                    region: parse_optional_region(node)?,
                    find: parse_edge_find(node)?,
                    minimum_extent: parse_minimum_extent(node)?,
                    fallback_anchor: last_capture_region(&children),
                    children,
                })])
            } else if let Some(text_find) = child(&feature, "TextFind") {
                parse_text_find_splitter(node, &text_find, context)
            } else if let Some(object_find) = child(&feature, "ObjectFind") {
                parse_object_find_splitter(node, &object_find, context)
            } else {
                Err("PDF Splitter uses an unsupported feature finder".to_string())
            }
        }
        "MergeSource" => Ok(Vec::new()),
        "MergeTarget" => parse_merge_target(node, document_level, context),
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

fn parse_group(
    node: &roxmltree::Node<'_, '_>,
    document_level: bool,
    context: &mut ParseContext,
) -> Result<Vec<PdfCommand>, String> {
    if let Some(selection) = parse_groups_from_list(node)? {
        if !document_level {
            return Err("PDF GroupsFromList is only supported at document level".to_string());
        }
        require_transparent_merge_group(node)?;
        let children = child(node, "Children")
            .ok_or_else(|| "PDF Grouping has no Children block".to_string())?;
        let commands = parse_commands(&children, true, context)?;
        if commands.is_empty()
            || commands
                .iter()
                .any(|command| !matches!(command, PdfCommand::Merge(_)))
        {
            return Err(
                "PDF GroupsFromList currently supports only a named merge source and target"
                    .to_string(),
            );
        }
        if commands.iter().any(|command| {
            matches!(
                command,
                PdfCommand::Merge(merge)
                    if merge.sources.iter().any(|source| source.page_selection != selection)
            )
        }) {
            return Err(
                "PDF GroupsFromList merge sources use incompatible page selections".to_string(),
            );
        }
        return Ok(commands);
    }
    require_one_group_per_page(node)?;
    let selection = parse_page_filter(node)?;
    if selection.is_some() && !document_level {
        return Err("numeric PDF page filters are only supported at document level".to_string());
    }
    let children =
        child(node, "Children").ok_or_else(|| "PDF Grouping has no Children block".to_string())?;
    let children = parse_commands(&children, false, context)?;
    let name = child(node, "Label")
        .and_then(|label| label.text())
        .map(str::trim)
        .unwrap_or_default();
    let commands = if name.is_empty() {
        children
    } else {
        vec![PdfCommand::GroupPerPage(PdfGroup {
            name: name.to_string(),
            region: parse_optional_region(node)?,
            children,
        })]
    };
    match selection {
        Some(selection) if !commands.is_empty() => Ok(vec![PdfCommand::Pages(PdfPages {
            selection,
            children: commands,
        })]),
        _ => Ok(commands),
    }
}

fn parse_merge_target(
    node: &roxmltree::Node<'_, '_>,
    document_level: bool,
    context: &mut ParseContext,
) -> Result<Vec<PdfCommand>, String> {
    if !document_level {
        return Err("PDF MergeTarget is only supported at document level".to_string());
    }
    let name = required_text_child(node, "Name")?;
    if !context.merge_targets.insert(name.clone()) {
        return Err(format!("duplicate PDF MergeTarget `{name}`"));
    }
    let sources = context
        .merge_sources
        .remove(&name)
        .ok_or_else(|| format!("PDF MergeTarget `{name}` has no MergeSource"))?;
    let children = child(node, "Children")
        .ok_or_else(|| format!("PDF MergeTarget `{name}` has no Children block"))?;
    let elements = children
        .children()
        .filter(roxmltree::Node::is_element)
        .collect::<Vec<_>>();
    let (composition, commands) = match elements.as_slice() {
        [collage] if collage.has_tag_name("Collage") => {
            let collage_children = child(collage, "Children")
                .ok_or_else(|| "PDF Collage has no Children block".to_string())?;
            (
                PdfMergeComposition::VerticalCollage,
                parse_commands(&collage_children, false, context)?,
            )
        }
        elements
            if elements
                .iter()
                .any(|element| element.has_tag_name("Collage")) =>
        {
            return Err(
                "PDF MergeTarget Collage must be its only direct child command".to_string(),
            );
        }
        _ => (
            PdfMergeComposition::Independent,
            parse_commands(&children, false, context)?,
        ),
    };
    Ok(vec![PdfCommand::Merge(PdfMerge {
        name,
        composition,
        sources,
        children: commands,
    })])
}

fn collect_merge_sources(
    children: &roxmltree::Node<'_, '_>,
    sources: &mut BTreeMap<String, Vec<PdfMergeSource>>,
) -> Result<(), String> {
    for node in children.children().filter(roxmltree::Node::is_element) {
        match node.tag_name().name() {
            "Grouping" => {
                let group_children = child(&node, "Children")
                    .ok_or_else(|| "PDF Grouping has no Children block".to_string())?;
                let direct_sources = group_children
                    .children()
                    .filter(|child| child.has_tag_name("MergeSource"))
                    .collect::<Vec<_>>();
                if direct_sources.is_empty() {
                    if contains_merge_source(&group_children) {
                        return Err(
                            "PDF MergeSource must be directly inside a document-level grouping"
                                .to_string(),
                        );
                    }
                    continue;
                }
                let selection = if let Some(selection) = parse_groups_from_list(&node)? {
                    selection
                } else {
                    require_one_group_per_page(&node)?;
                    parse_page_filter(&node)?.unwrap_or(PdfPageSelection::All)
                };
                require_transparent_merge_group(&node)?;
                for source in direct_sources {
                    insert_merge_source(&source, selection, sources)?;
                }
            }
            "MergeSource" => {
                insert_merge_source(&node, PdfPageSelection::All, sources)?;
            }
            _ => {
                if contains_merge_source(&node) {
                    return Err(
                        "PDF MergeSource must be page-relative, not nested in another command"
                            .to_string(),
                    );
                }
            }
        }
    }
    Ok(())
}

fn insert_merge_source(
    node: &roxmltree::Node<'_, '_>,
    selection: PdfPageSelection,
    sources: &mut BTreeMap<String, Vec<PdfMergeSource>>,
) -> Result<(), String> {
    let target = required_text_child(node, "Target")?;
    sources.entry(target).or_default().push(PdfMergeSource {
        page_selection: selection,
        region: parse_required_region(node)?,
    });
    Ok(())
}

fn contains_merge_source(node: &roxmltree::Node<'_, '_>) -> bool {
    node.descendants()
        .skip(1)
        .any(|descendant| descendant.has_tag_name("MergeSource"))
}

fn last_capture_region(commands: &[PdfCommand]) -> Option<PdfRegion> {
    commands.iter().rev().find_map(|command| match command {
        PdfCommand::Capture(capture) if region_is_candidate_relative(&capture.region) => {
            Some(capture.region.clone())
        }
        PdfCommand::Capture(_) => None,
        PdfCommand::GroupPerPage(group) if group.region == PdfRegion::full() => {
            last_capture_region(&group.children)
        }
        PdfCommand::GroupPerPage(_)
        | PdfCommand::EdgeRows(_)
        | PdfCommand::TextGroups(_)
        | PdfCommand::TextRows(_)
        | PdfCommand::Pages(_)
        | PdfCommand::Merge(_)
        | PdfCommand::Anchor(_)
        | PdfCommand::BoundaryFindVertical(_) => None,
    })
}

fn region_is_candidate_relative(region: &PdfRegion) -> bool {
    !matches!(&region.left.reference, PdfReference::Anchor(_))
        && !matches!(&region.right.reference, PdfReference::Anchor(_))
        && matches!(&region.top.reference, PdfReference::Top)
        && region.top.offset == 0.0
        && matches!(&region.bottom.reference, PdfReference::Bottom)
        && region.bottom.offset == 0.0
}

fn require_one_group_per_page(node: &roxmltree::Node<'_, '_>) -> Result<(), String> {
    let one_per_page =
        child(node, "Kind").is_some_and(|kind| child(&kind, "OneGroupPerPage").is_some());
    if one_per_page {
        Ok(())
    } else {
        Err("PDF grouping kind is not OneGroupPerPage".to_string())
    }
}

fn require_transparent_merge_group(node: &roxmltree::Node<'_, '_>) -> Result<(), String> {
    let name = child(node, "Label")
        .and_then(|label| label.text())
        .map(str::trim)
        .unwrap_or_default();
    let region_is_scoped = child(node, "Region")
        .and_then(|region| region.text())
        .is_some_and(|region| !region.trim().is_empty());
    if !name.is_empty() || region_is_scoped {
        return Err("PDF MergeSource grouping must be transparent and page-relative".to_string());
    }
    Ok(())
}

fn parse_groups_from_list(
    node: &roxmltree::Node<'_, '_>,
) -> Result<Option<PdfPageSelection>, String> {
    let Some(groups) = child(node, "Kind").and_then(|kind| child(&kind, "GroupsFromList")) else {
        return Ok(None);
    };
    let filter = child(node, "Filter")
        .and_then(|value| value.text())
        .map(str::trim)
        .unwrap_or_default();
    if !filter.is_empty() {
        return Err("PDF GroupsFromList cannot be combined with a grouping Filter".to_string());
    }
    let pages = required_text_child(&groups, "Pages")?;
    let first = pages
        .strip_suffix('-')
        .and_then(|value| value.trim().parse::<u32>().ok())
        .and_then(NonZeroU32::new)
        .ok_or_else(|| {
            format!("PDF GroupsFromList page expression `{pages}` is unsupported; expected N-")
        })?;
    Ok(Some(PdfPageSelection::From { first }))
}

fn parse_page_filter(node: &roxmltree::Node<'_, '_>) -> Result<Option<PdfPageSelection>, String> {
    let filter = child(node, "Filter")
        .and_then(|value| value.text())
        .map(str::trim)
        .unwrap_or_default();
    if filter.is_empty() {
        return Ok(None);
    }
    let page = filter
        .parse::<u32>()
        .ok()
        .and_then(NonZeroU32::new)
        .ok_or_else(|| format!("PDF grouping filter `{filter}` is not an exact page number"))?;
    if page.get() == 1 {
        Ok(Some(PdfPageSelection::First))
    } else {
        Ok(Some(PdfPageSelection::Range {
            first: page,
            last: page,
        }))
    }
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
    use std::collections::BTreeMap;

    use super::{
        collect_merge_sources, last_capture_region, parse_coordinate, parse_groups_from_list,
        parse_page_filter, parse_region,
    };
    use mapping::{PdfCapture, PdfCommand, PdfCoordinate, PdfGroup, PdfReference, PdfRegion};

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

    #[test]
    fn page_filters_require_one_exact_positive_page() {
        let Ok(valid) = roxmltree::Document::parse(
            "<Grouping><Filter>2</Filter><Kind><OneGroupPerPage/></Kind></Grouping>",
        ) else {
            panic!("valid page filter XML must parse");
        };
        assert!(matches!(
            parse_page_filter(&valid.root_element()),
            Ok(Some(mapping::PdfPageSelection::Range { first, last }))
                if first.get() == 2 && last.get() == 2
        ));

        let Ok(open) = roxmltree::Document::parse(
            "<Grouping><Filter>2-</Filter><Kind><OneGroupPerPage/></Kind></Grouping>",
        ) else {
            panic!("open page filter XML must parse");
        };
        assert!(matches!(
            parse_page_filter(&open.root_element()),
            Err(message) if message.contains("not an exact page number")
        ));

        let Ok(zero) = roxmltree::Document::parse(
            "<Grouping><Filter>0</Filter><Kind><OneGroupPerPage/></Kind></Grouping>",
        ) else {
            panic!("zero page filter XML must parse");
        };
        assert!(parse_page_filter(&zero.root_element()).is_err());
    }

    #[test]
    fn groups_from_list_accepts_only_open_positive_ranges() {
        let Ok(valid) = roxmltree::Document::parse(
            "<Grouping><Kind><GroupsFromList><Pages>2-</Pages></GroupsFromList></Kind><Filter/></Grouping>",
        ) else {
            panic!("valid page-list XML must parse");
        };
        assert!(matches!(
            parse_groups_from_list(&valid.root_element()),
            Ok(Some(mapping::PdfPageSelection::From { first })) if first.get() == 2
        ));

        let Ok(disjoint) = roxmltree::Document::parse(
            "<Grouping><Kind><GroupsFromList><Pages>2,4</Pages></GroupsFromList></Kind><Filter/></Grouping>",
        ) else {
            panic!("unsupported page-list XML must still parse");
        };
        assert!(matches!(
            parse_groups_from_list(&disjoint.root_element()),
            Err(message) if message.contains("expected N-")
        ));
    }

    #[test]
    fn merge_sources_must_remain_in_page_relative_document_groups() {
        let Ok(document) = roxmltree::Document::parse(
            "<Children><Splitter><Children><MergeSource><Region>{ Left: Left, Top: Top, Right: Right, Bottom: Bottom }</Region><Target>Rows</Target></MergeSource></Children></Splitter></Children>",
        ) else {
            panic!("nested merge-source XML must parse");
        };
        let mut sources = BTreeMap::new();
        assert!(matches!(
            collect_merge_sources(&document.root_element(), &mut sources),
            Err(message) if message.contains("page-relative")
        ));
        assert!(sources.is_empty());
    }

    #[test]
    fn row_anchor_inference_does_not_cross_a_narrowed_group_region() {
        let command = PdfCommand::GroupPerPage(PdfGroup {
            name: "Row".into(),
            region: PdfRegion {
                left: PdfCoordinate::new(PdfReference::Left, 20.0),
                ..PdfRegion::full()
            },
            children: vec![PdfCommand::Capture(PdfCapture {
                name: "Value".into(),
                region: PdfRegion::full(),
            })],
        });
        assert!(last_capture_region(&[command]).is_none());
    }
}
