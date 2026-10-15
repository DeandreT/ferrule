use super::*;

#[derive(Clone, Copy)]
enum CsvRawKind {
    Mapping,
    Wrapper,
    Structure,
    Children,
    Literal,
    Text,
    LiteralData,
    TextData,
    Properties,
    Targets,
    Datapoint,
    Constant,
    Root,
    Entry,
    CsvText,
    EdiText,
    Settings,
    Names,
    Field,
    Messages,
    Message,
    Connections,
    SavedEdge,
    Graph,
    Vertices,
    Vertex,
    Edges,
    ModernEdge,
}

/// This proof is deliberately narrower than ordinary repair-oriented import.
/// Unknown raw structure quietly disables ONLY the new CSV occurrence path.
/// No normalized feed, diagnostic text or literal reader establishes membership.
fn whole_raw_membership(structure: roxmltree::Node<'_, '_>) -> bool {
    use CsvRawKind as K;
    const MAX_RAW_ELEMENTS: usize = 16_384;
    const MAX_RAW_DEPTH: usize = 64;
    let Some(wrapper) = structure.parent() else {
        return false;
    };
    let Some(mapping) = wrapper.parent() else {
        return false;
    };
    if !mapping.has_tag_name("mapping") || mapping.id() != structure.document().root_element().id()
    {
        return false;
    }
    let selected = structure
        .children()
        .filter(|n| n.has_tag_name("children"))
        .collect::<Vec<_>>();
    let [children] = selected.as_slice() else {
        return false;
    };
    let one = |node: roxmltree::Node<'_, '_>, tag: &str| {
        node.children().filter(|n| n.has_tag_name(tag)).count() == 1
    };
    let mut stack = vec![(mapping, K::Mapping, 1usize)];
    let mut visited = Vec::new();
    let mut components = 0usize;
    let mut edges = 0usize;
    while let Some((node, kind, depth)) = stack.pop() {
        if depth > MAX_RAW_DEPTH
            || visited.len() == MAX_RAW_ELEMENTS
            || node.tag_name().namespace().is_some()
            || node.attributes().any(|a| a.namespace().is_some())
        {
            return false;
        }
        visited.push(node.id());
        // Every authority key belongs to one explicit saved position.
        // Hidden/dangling key attributes on annotations are never ignored.
        for attribute in node.attributes() {
            let allowed = match attribute.name() {
                "outkey" | "inpkey" => matches!(kind, K::Entry),
                "key" => matches!(kind, K::Datapoint),
                "vertexkey" => matches!(kind, K::Vertex | K::ModernEdge),
                "from" | "to" => matches!(kind, K::SavedEdge),
                _ => continue,
            };
            if !allowed {
                return false;
            }
        }
        match kind {
            K::Mapping if !one(node, "component") => return false,
            K::Wrapper
                if !one(node, "structure")
                    || node
                        .children()
                        .filter(|n| n.has_tag_name("properties"))
                        .count()
                        > 1
                    || node
                        .children()
                        .filter(|n| n.has_tag_name("connections"))
                        .count()
                        > 1 =>
            {
                return false;
            }
            K::Structure
                if !one(node, "children")
                    || node
                        .children()
                        .filter(|n| n.has_tag_name("connections"))
                        .count()
                        > 1
                    || node.children().filter(|n| n.has_tag_name("graph")).count() > 1 =>
            {
                return false;
            }
            K::Literal
                if !one(node, "data")
                    || !one(node, "targets")
                    || node
                        .children()
                        .filter(|n| n.has_tag_name("properties"))
                        .count()
                        > 1 =>
            {
                return false;
            }
            K::Text
                if !one(node, "data")
                    || node
                        .children()
                        .filter(|n| n.has_tag_name("properties"))
                        .count()
                        > 1 =>
            {
                return false;
            }
            K::LiteralData if !one(node, "constant") => return false,
            K::TextData if !one(node, "root") || !one(node, "text") => return false,
            K::CsvText if !one(node, "settings") => return false,
            K::EdiText if !one(node, "messages") => return false,
            K::Settings if !one(node, "names") => return false,
            K::Graph if !one(node, "vertices") => return false,
            K::Vertex if !one(node, "edges") => return false,
            _ => {}
        }
        for child in node.children() {
            if !child.is_element() {
                if child.is_text() && child.text().is_some_and(|s| !s.trim().is_empty()) {
                    return false;
                }
                continue;
            }
            let child_kind = match (kind, child.tag_name().name()) {
                (K::Mapping, "component") if child.id() == wrapper.id() => K::Wrapper,
                (K::Wrapper, "structure") if child.id() == structure.id() => K::Structure,
                (K::Wrapper, "properties") => K::Properties,
                (K::Wrapper | K::Structure, "connections") => K::Connections,
                (K::Structure, "children") if child.id() == children.id() => K::Children,
                (K::Structure, "graph") => K::Graph,
                (K::Children, "component") => {
                    components += 1;
                    if components > MAX_BINDINGS + 2 {
                        return false;
                    }
                    match (child.attribute("library"), child.attribute("kind")) {
                        (Some("core"), Some("2")) => K::Literal,
                        (Some("text"), Some("16")) => K::Text,
                        _ => return false,
                    }
                }
                (K::Literal | K::Text, "properties") => K::Properties,
                (K::Literal, "data") => K::LiteralData,
                (K::Literal, "targets") => K::Targets,
                (K::Targets, "datapoint") => K::Datapoint,
                (K::LiteralData, "constant") => K::Constant,
                (K::Text, "data") => K::TextData,
                (K::TextData, "root") => K::Root,
                (K::TextData, "text") => match (child.attribute("type"), child.attribute("kind")) {
                    (Some("csv"), None) => K::CsvText,
                    (Some("edi"), Some("EDIX12")) => K::EdiText,
                    _ => return false,
                },
                (K::Root | K::Entry, "entry") => K::Entry,
                (K::CsvText, "settings") => K::Settings,
                (K::Settings, "names") => K::Names,
                (K::Names, tag)
                    if tag.strip_prefix("field").is_some_and(|n| {
                        !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit())
                    }) =>
                {
                    K::Field
                }
                (K::EdiText, "messages") => K::Messages,
                (K::Messages, "message") => K::Message,
                (K::Connections, "edge") => {
                    edges += 1;
                    K::SavedEdge
                }
                (K::Graph, "vertices") => K::Vertices,
                (K::Vertices, "vertex") => K::Vertex,
                (K::Vertex, "edges") => K::Edges,
                (K::Edges, "edge") => {
                    edges += 1;
                    K::ModernEdge
                }
                _ => return false,
            };
            if edges > MAX_BINDINGS * 4 {
                return false;
            }
            if visited.len() + stack.len() == MAX_RAW_ELEMENTS {
                return false;
            }
            stack.push((child, child_kind, depth + 1));
        }
    }
    // A structural walk is a membership census, not just a recognized subset.
    // Compare the complete document's element NodeIds, including any extension.
    let mut complete_count = 0usize;
    for node in structure
        .document()
        .root()
        .descendants()
        .filter(|n| n.is_element())
    {
        complete_count += 1;
        if complete_count > MAX_RAW_ELEMENTS || !visited.contains(&node.id()) {
            return false;
        }
    }
    complete_count == visited.len()
}

/// Selected raw owners cannot be inferred from the normalized feed map alone.
/// The initial contract admits one CSV source, one X12 target and literals only.
pub(super) fn raw_authority(
    structure: &roxmltree::Node<'_, '_>,
    feeds: &BTreeMap<u32, u32>,
) -> bool {
    if !whole_raw_membership(*structure) {
        return false;
    }
    let Some(wrapper) = structure.parent() else {
        return false;
    };
    let Some(mapping) = wrapper.parent() else {
        return false;
    };
    if !mapping.has_tag_name("mapping")
        || mapping
            .children()
            .filter(|node| node.has_tag_name("component"))
            .count()
            != 1
    {
        return false;
    }
    let containers = structure
        .children()
        .filter(|node| node.has_tag_name("children"))
        .collect::<Vec<_>>();
    let [children] = containers.as_slice() else {
        return false;
    };
    let components = children
        .children()
        .filter(|node| node.is_element())
        .collect::<Vec<_>>();
    if components.len() > MAX_BINDINGS + 2 {
        return false;
    }
    let mut csv = 0;
    let mut x12 = 0;
    let mut outputs = BTreeSet::new();
    let mut inputs = BTreeSet::new();
    for component in components {
        if !component.has_tag_name("component") {
            return false;
        }
        if component
            .descendants()
            .filter(|node| node.is_element())
            .any(|node| {
                node.tag_name().namespace().is_some()
                    || node
                        .attributes()
                        .any(|attribute| attribute.namespace().is_some())
            })
        {
            return false;
        }
        if component
            .descendants()
            .skip(1)
            .any(|node| node.has_tag_name("component"))
        {
            return false;
        }
        match (component.attribute("library"), component.attribute("kind")) {
            (Some("core"), Some("2")) => {
                let literal = crate::import::function::read(&component);
                if !literal.raw_literal_valid
                    || !literal.raw_single_output_valid
                    || !literal.bounded_scalar_pins_valid
                    || !literal.inputs.is_empty()
                {
                    return false;
                }
            }
            (Some("text"), Some("16")) => {
                let data = component
                    .children()
                    .filter(|node| node.has_tag_name("data"))
                    .collect::<Vec<_>>();
                let [data] = data.as_slice() else {
                    return false;
                };
                let texts = data
                    .children()
                    .filter(|node| node.has_tag_name("text"))
                    .collect::<Vec<_>>();
                let [text] = texts.as_slice() else {
                    return false;
                };
                match (text.attribute("type"), text.attribute("kind")) {
                    (Some("csv"), None)
                        if component
                            .children()
                            .filter(|node| node.is_element())
                            .all(|node| node.has_tag_name("data"))
                            && raw_csv_block(data, text) =>
                    {
                        csv += 1
                    }
                    (Some("edi"), Some("EDIX12")) => x12 += 1,
                    _ => return false,
                }
            }
            _ => return false,
        }
        for entry in component
            .descendants()
            .filter(|node| node.has_tag_name("entry"))
        {
            for (attribute, keys) in [("outkey", &mut outputs), ("inpkey", &mut inputs)] {
                if let Some(raw) = entry.attribute(attribute) {
                    let Ok(key) = raw.parse::<u32>() else {
                        return false;
                    };
                    if !keys.insert(key) {
                        return false;
                    }
                }
            }
        }
        for targets in component
            .children()
            .filter(|node| node.has_tag_name("targets"))
        {
            for pin in targets
                .descendants()
                .filter(|node| node.has_tag_name("datapoint"))
            {
                let Some(raw) = pin.attribute("key") else {
                    return false;
                };
                let Ok(key) = raw.parse::<u32>() else {
                    return false;
                };
                if !outputs.insert(key) {
                    return false;
                }
            }
        }
    }
    csv == 1
        && x12 == 1
        && outputs.is_disjoint(&inputs)
        && feeds
            .iter()
            .all(|(input, output)| inputs.contains(input) && outputs.contains(output))
        && complete_raw_edges(structure, feeds)
}

fn one_entry<'a, 'input>(
    owner: roxmltree::Node<'a, 'input>,
    name: &str,
) -> Option<roxmltree::Node<'a, 'input>> {
    let entries = owner
        .children()
        .filter(|node| node.is_element())
        .collect::<Vec<_>>();
    let [entry] = entries.as_slice() else {
        return None;
    };
    (entry.has_tag_name("entry")
        && entry.attribute("name") == Some(name)
        && entry
            .attributes()
            .all(|attribute| attribute.name() == "name"))
    .then_some(*entry)
}

/// The ordinary CSV reader has no JSON port census. Retain an independent raw
/// flat block/column census and compare its normalized shape at admission.
fn raw_csv_block(data: &roxmltree::Node<'_, '_>, text: &roxmltree::Node<'_, '_>) -> bool {
    if data.children().filter(|node| node.is_element()).count() != 2
        || text.attribute("outputinstance").is_some()
        || text.attributes().any(|attribute| {
            !matches!(
                attribute.name(),
                "type" | "inputinstance" | "encoding" | "byteorder" | "byteordermark"
            )
        })
    {
        return false;
    }
    let roots = data
        .children()
        .filter(|node| node.has_tag_name("root"))
        .collect::<Vec<_>>();
    let [root] = roots.as_slice() else {
        return false;
    };
    if root.attributes().len() != 0 {
        return false;
    }
    let settings = text
        .children()
        .filter(|node| node.is_element())
        .collect::<Vec<_>>();
    let [settings] = settings.as_slice() else {
        return false;
    };
    if !settings.has_tag_name("settings")
        || settings.attributes().any(|attribute| {
            !matches!(
                attribute.name(),
                "separator" | "firstrownames" | "removeempty" | "quote"
            )
        })
    {
        return false;
    }
    let names = settings
        .children()
        .filter(|node| node.is_element())
        .collect::<Vec<_>>();
    let [names] = names.as_slice() else {
        return false;
    };
    if !names.has_tag_name("names")
        || names
            .attributes()
            .any(|attribute| !matches!(attribute.name(), "root" | "block"))
    {
        return false;
    }
    let Some(block_name) = names.attribute("block").filter(|name| !name.is_empty()) else {
        return false;
    };
    let Some(file) = one_entry(*root, "FileInstance") else {
        return false;
    };
    let Some(document) = one_entry(file, "document") else {
        return false;
    };
    let blocks = document
        .children()
        .filter(|node| node.is_element())
        .collect::<Vec<_>>();
    let [block] = blocks.as_slice() else {
        return false;
    };
    if !block.has_tag_name("entry")
        || block.attribute("name") != Some(block_name)
        || block
            .attribute("outkey")
            .and_then(|key| key.parse::<u32>().ok())
            .is_none()
        || block
            .attributes()
            .any(|attribute| !matches!(attribute.name(), "name" | "outkey"))
    {
        return false;
    }
    let declarations = names
        .children()
        .filter(|node| node.is_element())
        .collect::<Vec<_>>();
    let columns = block
        .children()
        .filter(|node| node.is_element())
        .collect::<Vec<_>>();
    if declarations.is_empty()
        || declarations.len() > MAX_BINDINGS
        || columns.len() != declarations.len()
    {
        return false;
    }
    let mut declared_names = BTreeSet::new();
    for (index, field) in declarations.iter().enumerate() {
        let Some(name) = field.attribute("name").filter(|name| !name.is_empty()) else {
            return false;
        };
        if field.tag_name().name() != format!("field{index}")
            || !declared_names.insert(name)
            || !matches!(
                field.attribute("type"),
                Some("string" | "boolean" | "integer" | "double" | "float")
            )
            || field
                .attributes()
                .any(|attribute| !matches!(attribute.name(), "name" | "type"))
            || field.children().any(|node| node.is_element())
        {
            return false;
        }
    }
    let mut column_names = BTreeSet::new();
    columns.iter().all(|column| {
        column.has_tag_name("entry")
            && column
                .attribute("name")
                .is_some_and(|name| declared_names.contains(name) && column_names.insert(name))
            && column
                .attribute("outkey")
                .and_then(|key| key.parse::<u32>().ok())
                .is_some()
            && column
                .attributes()
                .all(|attribute| matches!(attribute.name(), "name" | "outkey"))
            && !column.children().any(|node| node.is_element())
    }) && column_names == declared_names
}

/// Every saved edge must agree with the selected feed, including an otherwise
/// shadowed parent connection. Invalid records never establish authority.
fn complete_raw_edges(structure: &roxmltree::Node<'_, '_>, feeds: &BTreeMap<u32, u32>) -> bool {
    let Some(wrapper) = structure.parent() else {
        return false;
    };
    if wrapper
        .children()
        .filter(|node| node.has_tag_name("structure"))
        .count()
        != 1
    {
        return false;
    }
    let mut count = 0;
    let mut seen = BTreeMap::new();
    for owner in [*structure, wrapper] {
        let containers = owner
            .children()
            .filter(|node| node.has_tag_name("connections"))
            .collect::<Vec<_>>();
        if containers.len() > 1 {
            return false;
        }
        for container in containers {
            for edge in container.children().filter(|node| node.is_element()) {
                if !edge.has_tag_name("edge")
                    || edge
                        .attributes()
                        .any(|attribute| !matches!(attribute.name(), "from" | "to"))
                {
                    return false;
                }
                let (Some(from), Some(to)) = (edge.attribute("from"), edge.attribute("to")) else {
                    return false;
                };
                let (Ok(from), Ok(to)) = (from.parse::<u32>(), to.parse::<u32>()) else {
                    return false;
                };
                count += 1;
                if count > MAX_BINDINGS * 4 || feeds.get(&to) != Some(&from) {
                    return false;
                }
                seen.insert(to, from);
            }
        }
    }
    let graphs = structure
        .children()
        .filter(|node| node.has_tag_name("graph"))
        .collect::<Vec<_>>();
    if graphs.len() > 1 {
        return false;
    }
    for graph in graphs {
        let vertices = graph
            .children()
            .filter(|node| node.is_element())
            .collect::<Vec<_>>();
        let [vertices] = vertices.as_slice() else {
            return false;
        };
        if !vertices.has_tag_name("vertices") {
            return false;
        }
        for vertex in vertices.children().filter(|node| node.is_element()) {
            if !vertex.has_tag_name("vertex") {
                return false;
            }
            let Some(from) = vertex
                .attribute("vertexkey")
                .and_then(|key| key.parse::<u32>().ok())
            else {
                return false;
            };
            let containers = vertex
                .children()
                .filter(|node| node.is_element())
                .collect::<Vec<_>>();
            let [edges] = containers.as_slice() else {
                return false;
            };
            if !edges.has_tag_name("edges") {
                return false;
            }
            for edge in edges.children().filter(|node| node.is_element()) {
                if !edge.has_tag_name("edge")
                    || edge
                        .attributes()
                        .any(|attribute| attribute.name() != "vertexkey")
                {
                    return false;
                }
                let Some(to) = edge
                    .attribute("vertexkey")
                    .and_then(|key| key.parse::<u32>().ok())
                else {
                    return false;
                };
                count += 1;
                if count > MAX_BINDINGS * 4 || feeds.get(&to) != Some(&from) {
                    return false;
                }
                seen.insert(to, from);
            }
        }
    }
    &seen == feeds
}

/// Flat primary CSV columns are read in the existing row iteration context.
/// String declarations do not promise presence: empty/missing cells may be Null.
pub(super) enum ScalarProof {
    Literal(String),
    Field(Vec<String>),
}

impl ScalarProof {
    pub(super) fn matches_lowered(
        &self,
        node: mapping::NodeId,
        builder: &GraphBuilder<'_>,
    ) -> bool {
        match self {
            Self::Literal(value) => {
                exact_constant_node(node, &ir::Value::String(value.clone()), builder)
            }
            Self::Field(path) => matches!(
                builder.graph.nodes.get(&node),
                Some(mapping::Node::SourceField { path: actual, frame: None }) if actual == path
            ),
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn build(
    target: &SchemaComponent,
    bindings: &[(TargetLeaf, u32, u32)],
    iterations: &[TargetIteration],
    builder: &mut GraphBuilder<'_>,
    scopes: &mut ScopeBuilder,
    claimed: &mut BTreeSet<usize>,
    declared: &mut DeclaredBranches,
) {
    if target.format != ComponentFormat::Edi
        || builder
            .sources
            .first()
            .is_none_or(|source| source.format != ComponentFormat::Csv)
    {
        return;
    }
    let mut paths = BTreeMap::<Vec<String>, Vec<u32>>::new();
    for (marker, path) in &target.input_branch_paths {
        if schema_node_at(&target.schema, path)
            .is_some_and(|node| node.repeating && matches!(node.kind, SchemaKind::Group { .. }))
        {
            paths.entry(path.clone()).or_default().push(*marker);
        }
    }
    for (path, markers) in paths {
        if markers.len() < 2 {
            continue;
        }
        let candidate = (markers.len() <= MAX_BRANCHES
            && path.len() <= MAX_DEPTH
            && !iterations
                .iter()
                .any(|iteration| iteration.target_path.starts_with(&path))
            && !target
                .input_branch_paths
                .iter()
                .any(|(_, child)| child.len() > path.len() && child.starts_with(&path)))
        .then(|| {
            plan(
                &path, &markers, target, bindings, builder, scopes, claimed, declared,
            )
        })
        .flatten()
        .and_then(|plans| {
            lowering::prepare(
                &path, plans, target, bindings, builder, scopes, claimed, declared,
            )
        });
        let Some((graph, candidate_scopes, candidate_claimed, candidate_declared)) = candidate
        else {
            // Keep the complete preexisting CSV fallback and diagnostics.
            continue;
        };
        *builder = graph;
        *scopes = candidate_scopes;
        *claimed = candidate_claimed;
        *declared = candidate_declared;
    }
}

fn ordinary(component: &SchemaComponent, source: bool) -> bool {
    component.is_source == source
        && !component.is_variable
        && !component.is_pass_through
        && component.compute_when_key.is_none()
        && component.dynamic_json.is_none()
        && !component.has_target_node_controls
        && component.db_queries.is_empty()
        && component.db_xml_columns.is_empty()
}

fn admit(target: &SchemaComponent, builder: &GraphBuilder<'_>) -> bool {
    if !builder.csv_occurrence_raw_authority {
        return false;
    }
    let [primary] = builder.sources else {
        return false;
    };
    let SchemaKind::Group { children, .. } = &primary.schema.kind else {
        return false;
    };
    let mut names = BTreeSet::new();
    let source_schema = SchemaNode::group(primary.schema.name.clone(), children.clone());
    let source_options = mapping::FormatOptions {
        tabular_kind: Some(mapping::TabularBoundaryKind::Csv),
        delimiter: primary.options.delimiter,
        csv_quote: primary.options.csv_quote,
        csv_quote_disabled: primary.options.csv_quote_disabled,
        csv_utf8_bom: primary.options.csv_utf8_bom,
        csv_preserve_empty_strings: primary.options.csv_preserve_empty_strings,
        has_header_row: primary.options.has_header_row,
        ..Default::default()
    };
    let delimiter = source_options.delimiter.unwrap_or(',');
    let quote = source_options.csv_quote.unwrap_or('"');
    let target_options = mapping::FormatOptions {
        lenient_segments: true,
        edi_kind: Some(mapping::EdiBoundaryKind::X12),
        edi_implied_decimals: target.options.edi_implied_decimals.clone(),
        edi_lexical_formats: target.options.edi_lexical_formats.clone(),
        edi_value_constraints: target.options.edi_value_constraints.clone(),
        edi_autocomplete: target.options.edi_autocomplete.clone(),
        x12_separators: target.options.x12_separators,
        x12_interchange_version: target.options.x12_interchange_version.clone(),
        ..Default::default()
    };
    ordinary(primary, true)
        && ordinary(target, false)
        && primary.format == ComponentFormat::Csv
        && target.format == ComponentFormat::Edi
        && builder.intermediates.is_empty()
        && builder.source_node_functions.is_empty()
        && primary.options == source_options
        && target.options == target_options
        && primary.schema == source_schema
        && !children.is_empty()
        && children.len() <= MAX_BINDINGS
        && children.iter().all(|child| {
            let SchemaKind::Scalar { ty } = child.kind else {
                return false;
            };
            !child.name.is_empty()
                && names.insert(child.name.clone())
                && matches!(
                    ty,
                    ir::ScalarType::String
                        | ir::ScalarType::Bool
                        | ir::ScalarType::Int
                        | ir::ScalarType::Float
                )
                && child == &SchemaNode::scalar(child.name.clone(), ty)
                && (!source_options.csv_preserve_empty_strings || ty == ir::ScalarType::String)
        })
        && delimiter.is_ascii()
        && !matches!(delimiter, '\0' | '\r' | '\n')
        && (source_options.csv_quote_disabled || quote.is_ascii_graphic() && quote != delimiter)
        && !(source_options.csv_quote_disabled && source_options.csv_quote.is_some())
        && primary.input_keys.is_empty()
        && primary.ports.len() == children.len() + 1
        && primary.ports.values().collect::<BTreeSet<_>>().len() == primary.ports.len()
        && primary.ports.values().all(|path| {
            path.is_empty() || path.len() == 1 && children.iter().any(|child| child.name == path[0])
        })
        && primary
            .ports
            .keys()
            .all(|key| csv_unique_producer(*key, builder))
        && target
            .input_keys
            .iter()
            .all(|key| target.unambiguous_declared_input(*key))
        && !target.schema.repeating
        && bounded_schema(&target.schema, &mut 0, 0)
        && plain_shape(&target.schema, 0)
        && format_edi::dialect_of(&target.schema).ok() == Some(format_edi::Dialect::X12)
}

fn csv_unique_producer(feed: u32, builder: &GraphBuilder<'_>) -> bool {
    builder.csv_occurrence_raw_authority
        && builder
            .sources
            .iter()
            .filter(|source| source.ports.contains_key(&feed))
            .count()
            + builder
                .fn_components
                .iter()
                .filter(|function| function.declared_output_keys.contains(&feed))
                .count()
            == 1
        && !builder.has_source_node_functions(feed)
        && builder.joins.hierarchical_source(feed).is_none()
}

fn bounded_schema(schema: &SchemaNode, count: &mut usize, depth: usize) -> bool {
    *count += 1;
    if *count > MAX_BINDINGS * MAX_BRANCHES || depth > MAX_DEPTH {
        return false;
    }
    match &schema.kind {
        SchemaKind::Group { children, .. } => children
            .iter()
            .all(|child| bounded_schema(child, count, depth + 1)),
        SchemaKind::Scalar { .. } => true,
        _ => false,
    }
}

/// Exactly one raw CSV block output must own a real Source([]) body scope.
/// An absent/default anchor is insufficient and the envelope never iterates.
fn row_frame(
    path: &[String],
    target: &SchemaComponent,
    builder: &GraphBuilder<'_>,
    scopes: &ScopeBuilder,
) -> bool {
    let mut drivers = 0;
    for length in 0..path.len() {
        let prefix = &path[..length];
        let Some(schema) = schema_node_at(&target.schema, prefix) else {
            return false;
        };
        let Some(scope) = scopes.scope(prefix) else {
            if schema.repeating {
                return false;
            }
            continue;
        };
        match &scope.iteration {
            // A constructed noniterating ancestor produces one item even if
            // the physical schema wraps that item in Repeated. Only Source([])
            // below the envelope is permitted to drive the CSV row sequence.
            ScopeIteration::None => {}
            ScopeIteration::Source(source)
                if !prefix.is_empty() && source.is_empty() && schema.repeating =>
            {
                drivers += 1;
                let connected = target
                    .ports
                    .iter()
                    .filter(|(input, field)| {
                        field.as_slice() == prefix && builder.edge_from.contains_key(input)
                    })
                    .collect::<Vec<_>>();
                if connected.len() != 1
                    || scopes
                        .anchors
                        .get(prefix)
                        .is_none_or(|anchor| !anchor.is_empty())
                    || !csv_frame_driver(prefix, *connected[0].0, target, builder)
                    || controlled_input(target, *connected[0].0, builder)
                {
                    return false;
                }
            }
            _ => return false,
        }
    }
    drivers == 1 && plain_parents(path, scopes)
}

fn csv_frame_driver(
    path: &[String],
    input: u32,
    target: &SchemaComponent,
    builder: &GraphBuilder<'_>,
) -> bool {
    if !target.unambiguous_declared_input(input)
        || target.ambiguous_branch_inputs.contains(&input)
        || target.ambiguous_input_affects(input, path)
    {
        return false;
    }
    let Some(feed) = builder.edge_from.get(&input) else {
        return false;
    };
    let resolved = builder.resolve_iteration_feed(*feed);
    *feed == resolved.source_key
        && resolved.computed_source.is_none()
        && resolved.source_suffix.is_empty()
        && builder.sources[0]
            .ports
            .get(feed)
            .is_some_and(|path| path.is_empty())
        && csv_unique_producer(*feed, builder)
}

fn scalar_proof(feed: u32, target: &SchemaNode, builder: &GraphBuilder<'_>) -> Option<ScalarProof> {
    if target.repeating
        || !matches!(
            target.kind,
            SchemaKind::Scalar {
                ty: ir::ScalarType::String
            }
        )
        || !csv_unique_producer(feed, builder)
    {
        return None;
    }
    if let Some(ir::Value::String(value)) = exact_literal(feed, builder) {
        let function = builder
            .fn_components
            .get(*builder.fn_by_output.get(&feed)?)?;
        return function
            .raw_literal_valid
            .then_some(ScalarProof::Literal(value));
    }
    let primary = builder.sources.first()?;
    let path = primary.ports.get(&feed)?;
    (path.len() == 1
        && schema_node_at(&primary.schema, path).is_some_and(|node| {
            matches!(
                node.kind,
                SchemaKind::Scalar {
                    ty: ir::ScalarType::String
                }
            )
        }))
    .then(|| ScalarProof::Field(path.clone()))
}

#[allow(clippy::too_many_arguments)]
fn plan(
    path: &[String],
    markers: &[u32],
    target: &SchemaComponent,
    bindings: &[(TargetLeaf, u32, u32)],
    builder: &GraphBuilder<'_>,
    scopes: &ScopeBuilder,
    claimed: &BTreeSet<usize>,
    declared: &DeclaredBranches,
) -> Option<Vec<BranchPlan>> {
    if !admit(target, builder)
        || !row_frame(path, target, builder, scopes)
        || !plain_shape(schema_node_at(&target.schema, path)?, 0)
        || scopes
            .scope(path)
            .is_some_and(|scope| !json_implicit::replaceable(scope))
        || declared
            .layouts
            .keys()
            .any(|ancestor| path.starts_with(ancestor))
        || target.input_keys.iter().any(|input| {
            builder.edge_from.contains_key(input) && target.ambiguous_input_affects(*input, path)
        })
    {
        return None;
    }
    let mut connected = BTreeSet::new();
    for (input, field) in &target.ports {
        if !field.starts_with(path) || !builder.edge_from.contains_key(input) {
            continue;
        }
        if !schema_node_at(&target.schema, field)?.is_scalar()
            || !target.unambiguous_declared_input(*input)
            || target.ambiguous_branch_inputs.contains(input)
            || controlled_input(target, *input, builder)
        {
            return None;
        }
        connected.insert(*input);
    }
    let mut owned = BTreeMap::<u32, Vec<usize>>::new();
    let mut normalized = BTreeSet::new();
    for (index, (leaf, feed, input)) in bindings.iter().enumerate() {
        let field = leaf.path();
        if !field.starts_with(path) {
            continue;
        }
        if field.len() <= path.len()
            || claimed.contains(&index)
            || !connected.contains(input)
            || !normalized.insert(*input)
            || target.ports.get(input) != Some(&field)
            || builder.edge_from.get(input) != Some(feed)
        {
            return None;
        }
        let mut owners = target
            .input_ancestors
            .get(input)?
            .iter()
            .filter(|owner| markers.contains(owner));
        let owner = *owners.next()?;
        if owners.next().is_some() {
            return None;
        }
        owned.entry(owner).or_default().push(index);
    }
    if normalized != connected || normalized.len() > MAX_BINDINGS {
        return None;
    }
    let mut plans = Vec::new();
    for marker in markers {
        let indices = owned.remove(marker)?;
        let lineage = branch_owners::lineage(target, *marker)?;
        if lineage.as_slice() != [*marker] {
            return None;
        }
        let anchors = declared.enclosing_anchors(path, &lineage, &scopes.anchors)?;
        if !enclosing_anchor(path, &anchors).is_empty() {
            return None;
        }
        let mut assigned = BTreeSet::new();
        let mut groups = BTreeSet::new();
        let mut present = BTreeSet::new();
        let mut proofs = BTreeMap::new();
        for index in &indices {
            let (leaf, feed, _) = &bindings[*index];
            let field = leaf.path();
            if !assigned.insert(field.clone()) {
                return None;
            }
            let proof = scalar_proof(*feed, schema_node_at(&target.schema, &field)?, builder)?;
            let nonempty = matches!(&proof, ScalarProof::Literal(value) if !value.is_empty());
            for length in path.len()..field.len() {
                let group = field[..length].to_vec();
                groups.insert(group.clone());
                if nonempty {
                    present.insert(group);
                }
            }
            proofs.insert(*index, proof);
        }
        if !groups.is_subset(&present) || proofs.is_empty() {
            return None;
        }
        plans.push(BranchPlan {
            marker: *marker,
            lineage,
            anchors,
            driver: BranchDriver::Implicit,
            bindings: indices,
            conditionals: BTreeMap::new(),
            value_maps: BTreeMap::new(),
            parse_dates: BTreeMap::new(),
            json_scalars: BTreeMap::new(),
            csv_scalars: proofs,
        });
    }
    owned.is_empty().then_some(plans)
}

#[cfg(test)]
mod tests;
