//! `.mfd` -> `mapping::Project` conversion.
//!
//! The importer never fails on unsupported constructs: it converts what it
//! can and records a warning per skipped piece, because a partial import
//! the user finishes by hand still beats redrawing the mapping.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use ir::{SchemaKind, SchemaNode, Value};
use mapping::{AggregateOp, Graph, NamedSource, Node, NodeId, Project, Scope, SequenceExpr};

use crate::MfdError;

mod function;
mod graph;
mod iteration;
mod schema;
mod scope;
mod udf;

use function::{
    aggregate_op, is_distinct_values as is_distinct_values_component,
    is_filter as is_filter_component, is_first_items as is_first_items_component,
    is_input as is_input_component, is_sequence_producer, is_sort as is_sort_component,
    map_name as map_function_name, parse_constant, read as read_fn_component,
};
use graph::{GraphBuilder, read_edges};
use iteration::{
    IntermediateFeed, IterationFeed, compatible_collection, note_iteration_control_order,
    split_at_innermost_repeating,
};
use schema::{
    ComponentFormat, SchemaComponent, collect_matching_scalar_paths, note_skipped_library,
    read_csv_component, read_db_component, read_edi_component, read_json_component,
    read_schema_component, schema_node_at,
};
use scope::{IterationNodes, ScopeBuilder, TargetLeaf};
use udf::{Call as UdfCall, Registry as UdfRegistry};

pub struct Imported {
    pub project: Project,
    pub warnings: Vec<String>,
}

pub fn import(path: &Path) -> Result<Imported, MfdError> {
    let text = std::fs::read_to_string(path)?;
    let doc = roxmltree::Document::parse(&text)?;
    let mapping_el = doc.root_element();
    if mapping_el.tag_name().name() != "mapping" {
        return Err(MfdError::NotMfd("root element is not <mapping>"));
    }
    let wrapper = mapping_el
        .children()
        .find(|n| n.is_element() && n.tag_name().name() == "component")
        .ok_or(MfdError::NotMfd("no wrapper component"))?;
    let structure = wrapper
        .children()
        .find(|n| n.is_element() && n.tag_name().name() == "structure")
        .ok_or(MfdError::NotMfd("wrapper has no structure"))?;

    let mut warnings = Vec::new();
    let mut schema_components = Vec::new();
    let mut fn_components = Vec::new();
    let udf_registry = UdfRegistry::read(&mapping_el);
    let mut udf_calls = Vec::new();
    let mut skipped_libraries: Vec<String> = Vec::new();

    if let Some(children) = structure
        .children()
        .find(|n| n.is_element() && n.tag_name().name() == "children")
    {
        for component in children
            .children()
            .filter(|n| n.is_element() && n.tag_name().name() == "component")
        {
            let library = component.attribute("library").unwrap_or_default();
            let name = component.attribute("name").unwrap_or_default().to_string();
            match library {
                "xml" => match read_schema_component(&component, path, &mut warnings) {
                    Some(sc) => schema_components.push(sc),
                    None => warnings.push(format!("skipped xml component `{name}`")),
                },
                "json" => match read_json_component(&component, path, &mut warnings) {
                    Some(sc) => schema_components.push(sc),
                    None => warnings.push(format!("skipped json component `{name}`")),
                },
                "text" => {
                    let text_el = component
                        .children()
                        .find(|n| n.is_element() && n.tag_name().name() == "data")
                        .and_then(|d| {
                            d.children()
                                .find(|n| n.is_element() && n.tag_name().name() == "text")
                        });
                    let flavor = text_el.and_then(|t| t.attribute("type")).unwrap_or("");
                    if flavor == "csv" {
                        match read_csv_component(&component, &mut warnings) {
                            Some(sc) => schema_components.push(sc),
                            None => warnings.push(format!("skipped csv component `{name}`")),
                        }
                    } else if flavor == "edi" {
                        match read_edi_component(&component, &mut warnings) {
                            Some(sc) => schema_components.push(sc),
                            None => warnings.push(format!("skipped edi component `{name}`")),
                        }
                    } else {
                        let label = if flavor.is_empty() {
                            "text".to_string()
                        } else {
                            format!("text/{flavor}")
                        };
                        note_skipped_library(&mut skipped_libraries, &label);
                        warnings.push(format!(
                            "skipped component `{name}`: text flavor `{flavor}` is \
                             not supported yet (only csv and edi text components import)"
                        ));
                    }
                }
                "db" => match read_db_component(&component, &mapping_el, path, &mut warnings) {
                    Some(sc) => schema_components.push(sc),
                    None => note_skipped_library(&mut skipped_libraries, "db"),
                },
                "core" | "lang" => fn_components.push(read_fn_component(&component)),
                other => {
                    if let Some(definition) = udf_registry.supported(other, &name) {
                        if let Some(shape) = udf_registry.definition(definition) {
                            match UdfCall::read(&component, definition, shape) {
                                Ok(call) => udf_calls.push(call),
                                Err(reason) => warnings.push(format!(
                                    "skipped user-defined function `{name}`: {reason}"
                                )),
                            }
                        }
                    } else {
                        note_skipped_library(&mut skipped_libraries, other);
                        if let Some(reason) = udf_registry.unsupported_reason(other, &name) {
                            warnings
                                .push(format!("skipped user-defined function `{name}`: {reason}"));
                        } else {
                            warnings.push(format!(
                                "skipped component `{name}`: unsupported library `{other}` \
                                 (only xml/json/csv/edi/db, scalar user-defined functions, and \
                                 core/lang function components import)"
                            ));
                        }
                    }
                }
            }
        }
    }

    // Edges are indexed as to-key -> from-key; each input has at most one feed.
    let edge_from = read_edges(&structure, Some(&wrapper));

    let sources: Vec<&SchemaComponent> = schema_components
        .iter()
        .filter(|c| !c.is_variable && c.is_source)
        .collect();
    let targets: Vec<&SchemaComponent> = schema_components
        .iter()
        .filter(|c| !c.is_variable && !c.is_source)
        .collect();
    let intermediates: Vec<&SchemaComponent> =
        schema_components.iter().filter(|c| c.is_variable).collect();
    let unsupported = |side: &str| {
        MfdError::Unsupported(if skipped_libraries.is_empty() {
            format!("no importable {side} component (xml/json/csv/edi/db) found in this design")
        } else {
            format!(
                "no importable {side} component (xml/json/csv/edi/db) found; this design \
                 uses {} components, which ferrule cannot import yet",
                skipped_libraries.join("/")
            )
        })
    };
    let primary = sources.first().ok_or_else(|| unsupported("source"))?;
    let default_target = targets
        .iter()
        .copied()
        .find(|component| component.is_default_output);
    let target = default_target
        .or_else(|| targets.first().copied())
        .ok_or_else(|| unsupported("target"))?;
    let drops_connected_target = targets.iter().copied().any(|component| {
        !std::ptr::eq(component, target)
            && component
                .ports
                .keys()
                .any(|key| edge_from.contains_key(key))
    });
    if targets.len() > 1 && (default_target.is_none() || drops_connected_target) {
        warnings.push(format!(
            "multiple target components; only `{}` imported",
            target.name
        ));
    }

    let mut builder = GraphBuilder {
        graph: Graph::default(),
        next_id: 0,
        fn_nodes: BTreeMap::new(),
        sequence_items: BTreeMap::new(),
        sequence_scope_components: BTreeSet::new(),
        warned_sequence_uses: BTreeSet::new(),
        source_fields: BTreeMap::new(),
        edge_from: &edge_from,
        sources: &sources,
        intermediates: &intermediates,
        fn_components: &fn_components,
        fn_by_output: BTreeMap::new(),
        udf_nodes: BTreeMap::new(),
        udf_by_output: BTreeMap::new(),
        udf_calls: &udf_calls,
        udf_registry: &udf_registry,
        framed: std::collections::BTreeSet::new(),
        warnings: Vec::new(),
    };
    for (i, fc) in fn_components.iter().enumerate() {
        for &out in &fc.outputs {
            builder.fn_by_output.insert(out, i);
        }
    }
    for (call_idx, call) in udf_calls.iter().enumerate() {
        for (&output, &component_id) in &call.outputs {
            builder
                .udf_by_output
                .insert(output, (call_idx, component_id));
        }
    }
    // Scopes and bindings from the target's connected ports.
    let mut scope_builder = ScopeBuilder {
        root: Scope::default(),
        anchors: BTreeMap::new(),
    };
    let mut iterations = Vec::new();
    let mut bindings = Vec::new();
    for (&inpkey, target_path) in &target.ports {
        let Some(&from) = edge_from.get(&inpkey) else {
            continue;
        };
        let node_kind = schema_node_at(&target.schema, target_path);
        match node_kind {
            Some(node) if matches!(node.kind, SchemaKind::Group { .. }) => {
                // Iteration connection (or filtered iteration). An empty
                // path is a document-level connection: for row/array-shaped
                // targets (a CSV block, a repeating JSON root) it iterates
                // the root scope; for document-shaped targets the root runs
                // exactly once anyway, so it carries no information.
                if target_path.is_empty() {
                    let row_shaped =
                        matches!(target.format, ComponentFormat::Csv | ComponentFormat::Db)
                            || (target.format == ComponentFormat::Json && node.repeating);
                    if row_shaped {
                        iterations.push((target_path.clone(), from));
                    }
                    continue;
                }
                if !node.repeating {
                    let descendants_are_connected = target.ports.iter().any(|(key, path)| {
                        path.len() > target_path.len()
                            && path.starts_with(target_path)
                            && edge_from.contains_key(key)
                    });
                    if !descendants_are_connected {
                        builder.warnings.push(format!(
                            "connection into non-repeating group `{}` ignored",
                            target_path.join("/")
                        ));
                    }
                    continue;
                }
                iterations.push((target_path.clone(), from));
            }
            Some(_) => match TargetLeaf::from_path(target_path) {
                Some(target) => bindings.push((target, from)),
                None => builder.warnings.push(
                    "connection into a scalar document root is not supported; binding skipped"
                        .to_string(),
                ),
            },
            None => builder.warnings.push(format!(
                "target port path `{}` not found in schema",
                target_path.join("/")
            )),
        }
    }
    // Iterations first (outer before inner), so anchors exist for bindings.
    iterations.sort_by_key(|(path, _)| path.len());
    // SourceField paths are relative to the enclosing iteration frames, so
    // the builder must know which repeating levels the scopes will iterate
    // before any function component materializes a SourceField.
    for (_, from) in &iterations {
        let feed = builder.resolve_iteration_feed(*from);
        if let Some(idx) = feed.sequence_component {
            builder.sequence_scope_components.insert(idx);
        }
        if let Some(abs) = builder.iteration_source_path(&feed) {
            builder.note_framed_prefixes(&abs);
        }
    }
    // Materialize aggregates first so computed sequence functions are built
    // under their per-item collection frame rather than as outer expressions.
    for (i, fc) in fn_components.iter().enumerate() {
        if fc.kind == 5 && aggregate_op(&fc.name).is_some() {
            builder.fn_node(i);
        }
    }
    // Materialize every remaining value-producing function up front
    // (filters and group-bys are handled at the scope stage instead).
    // Outputless core components are annotations such as comments.
    for (i, fc) in fn_components.iter().enumerate() {
        if !(fc.outputs.is_empty()
            || is_filter_component(fc)
            || is_input_component(fc)
            || is_sort_component(fc)
            || is_first_items_component(fc)
            || is_distinct_values_component(fc)
            || is_sequence_producer(fc)
            || fc.name == "group-by"
            || fc.kind == 5 && aggregate_op(&fc.name).is_some())
        {
            builder.fn_node(i);
        }
    }
    let connected_bindings: BTreeSet<Vec<String>> =
        bindings.iter().map(|(target, _)| target.path()).collect();
    for (target_path, from) in iterations {
        let feed = builder.resolve_iteration_feed(from);
        if let Some(issue) = feed.order_issue {
            builder.warnings.push(format!(
                "sequence into `{}` {issue}; imported using ferrule's sequence order",
                target_path.join("/")
            ));
        }
        let source_abs = builder.iteration_source_path(&feed);
        let sequence = feed
            .sequence_component
            .and_then(|idx| builder.sequence_expr(idx));
        if source_abs.is_none() && sequence.is_none() {
            builder.warnings.push(format!(
                "iteration into `{}` comes from an unsupported feed; skipped",
                target_path.join("/")
            ));
            continue;
        }
        let mut filter_node = feed.filter_expr.and_then(|key| builder.value_node(key));
        let distinct_node = feed.distinct_key.and_then(|key| builder.value_node(key));
        let group_node = feed
            .group_key
            .and_then(|key| builder.value_node(key))
            .or(distinct_node);
        if let Some(distinct_node) = distinct_node {
            let exists = builder.alloc(Node::Call {
                function: "exists".into(),
                args: vec![distinct_node],
            });
            filter_node = Some(match filter_node {
                Some(filter) => builder.alloc(Node::Call {
                    function: "and".into(),
                    args: vec![filter, exists],
                }),
                None => exists,
            });
        }
        let sort_node = feed.sort_expr.and_then(|key| builder.value_node(key));
        let take_node = feed
            .take_expr
            .and_then(|key| builder.value_node(key))
            .or_else(|| {
                feed.take_default_one.then(|| {
                    builder.alloc(Node::Const {
                        value: Value::Int(1),
                    })
                })
            });
        let nodes = IterationNodes {
            filter: filter_node,
            group_by: group_node,
            sort_by: sort_node,
            sort_descending: feed.sort_descending,
            take: take_node,
        };
        if let Some(sequence) = sequence {
            scope_builder.add_sequence(&target_path, sequence, nodes);
        } else if let Some(source_abs) = &source_abs {
            scope_builder.add_iteration(&target_path, source_abs, nodes);
        }
        if feed.projects_whole_group
            && let Some(source_abs) = &source_abs
            && let (Some(source_group), Some(target_group)) = (
                schema_node_at(&primary.schema, source_abs),
                schema_node_at(&target.schema, &target_path),
            )
        {
            let mut relative_paths = Vec::new();
            collect_matching_scalar_paths(
                source_group,
                target_group,
                &mut Vec::new(),
                &mut relative_paths,
            );
            for relative in relative_paths {
                let mut target_leaf = target_path.clone();
                target_leaf.extend(relative.iter().cloned());
                if connected_bindings.contains(&target_leaf)
                    || feed.projections.contains_key(&relative)
                {
                    continue;
                }
                let mut source_leaf = source_abs.clone();
                source_leaf.extend(relative);
                if let (Some(target), Some(node)) = (
                    TargetLeaf::from_path(&target_leaf),
                    builder.primary_source_field(&source_leaf),
                ) {
                    scope_builder.add_binding(target, node);
                }
            }
        }
        let mut projection_paths = Vec::new();
        if let Some(target_group) = schema_node_at(&target.schema, &target_path) {
            collect_matching_scalar_paths(
                target_group,
                target_group,
                &mut Vec::new(),
                &mut projection_paths,
            );
        }
        for relative in projection_paths {
            let Some(value_feed) = feed.projections.get(&relative) else {
                continue;
            };
            let mut target_leaf = target_path.clone();
            target_leaf.extend(relative.iter().cloned());
            if connected_bindings.contains(&target_leaf)
                || !schema_node_at(&target.schema, &target_leaf)
                    .is_some_and(|node| matches!(node.kind, SchemaKind::Scalar { .. }))
            {
                continue;
            }
            if let Some(node) = builder.value_node(*value_feed)
                && let Some(target) = TargetLeaf::from_path(&target_leaf)
            {
                scope_builder.add_binding(target, node);
            }
        }
    }
    for (target, from) in bindings {
        let Some(node) = builder.value_node(from) else {
            builder.warnings.push(format!(
                "binding for `{}` comes from an unsupported feed; skipped",
                target.path().join("/")
            ));
            continue;
        };
        scope_builder.add_binding(target, node);
    }

    let mut extra_sources = Vec::new();
    for extra in sources.iter().skip(1) {
        builder.warnings.push(format!(
            "extra source `{}` imported as a named source; cross-source \
             connections usually need manual lookup/scope fixes",
            extra.name
        ));
        extra_sources.push(NamedSource {
            name: extra.name.clone(),
            path: extra.input_instance.clone().unwrap_or_default(),
            schema: extra.schema.clone(),
            options: extra.options.clone(),
        });
    }

    warnings.extend(builder.warnings);
    Ok(Imported {
        project: Project {
            source: primary.schema.clone(),
            target: target.schema.clone(),
            source_path: primary.input_instance.clone(),
            target_path: target
                .output_instance
                .clone()
                .or_else(|| target.input_instance.clone()),
            source_options: primary.options.clone(),
            target_options: target.options.clone(),
            extra_sources,
            graph: builder.graph,
            root: scope_builder.root,
        },
        warnings,
    })
}

impl GraphBuilder<'_> {
    fn primary_source_field(&mut self, abs: &[String]) -> Option<NodeId> {
        let schema = &self.sources.first()?.schema;
        let path = self.suffix_after_framed(schema, abs);
        let frame = self.frame_for_field(schema, abs);
        Some(self.source_field(frame, path))
    }

    /// Marks every repeating level along an iterated absolute source path
    /// as getting a run-time context frame.
    fn note_framed_prefixes(&mut self, abs: &[String]) {
        let Some(source) = self.sources.first() else {
            return;
        };
        let mut node = &source.schema;
        for (i, segment) in abs.iter().enumerate() {
            let Some(child) = node.child(segment) else {
                break;
            };
            if child.repeating {
                self.framed.insert(abs[..=i].to_vec());
            }
            node = child;
        }
    }

    /// Path segments after the innermost framed (scope-iterated) repeating
    /// ancestor -- what a `SourceField` must hold so it resolves against
    /// the enclosing scopes' iteration frames.
    fn suffix_after_framed(&self, schema: &SchemaNode, abs: &[String]) -> Vec<String> {
        let mut node = schema;
        let mut suffix_start = 0;
        for (i, segment) in abs.iter().enumerate() {
            let Some(child) = node.child(segment) else {
                break;
            };
            if child.repeating && self.framed.contains(&abs[..=i]) {
                suffix_start = i + 1;
            }
            node = child;
        }
        abs[suffix_start..].to_vec()
    }

    fn frame_for_field(&self, schema: &SchemaNode, abs: &[String]) -> Option<Vec<String>> {
        let mut node = schema;
        let mut frame = None;
        for (i, segment) in abs.iter().enumerate() {
            let Some(child) = node.child(segment) else {
                break;
            };
            if child.repeating && self.framed.contains(&abs[..=i]) {
                frame = Some(abs[..=i].to_vec());
            }
            node = child;
        }
        frame
    }

    /// Resolves one output of a variable schema component to the connected
    /// input that supplies it plus the output's path below that input. An
    /// Connected descendant inputs are returned as scalar projections so a
    /// constructed group can become ordinary target bindings.
    fn intermediate_feed(&self, output_key: u32) -> Option<IntermediateFeed> {
        for component in self.intermediates {
            if !component.output_keys.contains(&output_key) {
                continue;
            }
            let output_path = component.ports.get(&output_key)?;
            let (input_key, input_path) = component
                .ports
                .iter()
                .filter(|(key, path)| {
                    component.input_keys.contains(key)
                        && self.edge_from.contains_key(key)
                        && output_path.starts_with(path)
                })
                .max_by_key(|(_, path)| path.len())?;
            let feed = *self.edge_from.get(input_key)?;
            let control = component
                .compute_when_key
                .and_then(|key| self.edge_from.get(&key).copied());
            let projections = component
                .ports
                .iter()
                .filter_map(|(key, path)| {
                    if component.input_keys.contains(key)
                        && path.len() > output_path.len()
                        && path.starts_with(output_path)
                    {
                        self.edge_from
                            .get(key)
                            .map(|feed| (path[output_path.len()..].to_vec(), *feed))
                    } else {
                        None
                    }
                })
                .collect();
            return Some(IntermediateFeed {
                feed,
                suffix: output_path[input_path.len()..].to_vec(),
                control,
                projections,
            });
        }
        None
    }

    /// The ferrule node producing the value at output-port `key`, creating
    /// SourceField/function nodes on demand. `None` for unsupported feeds.
    fn value_node(&mut self, key: u32) -> Option<NodeId> {
        // A source schema entry?
        for (idx, source) in self.sources.iter().enumerate() {
            if let Some(abs) = source.ports.get(&key).cloned() {
                if idx == 0 {
                    return self.primary_source_field(&abs);
                }
                // Extra sources are addressed by name from the outermost
                // context frame.
                let mut path = vec![self.sources[idx].name.clone()];
                path.extend(abs);
                return Some(self.source_field(None, path));
            }
        }
        // A transparent output of a variable schema component?
        if let Some(intermediate) = self.intermediate_feed(key) {
            if intermediate.suffix.is_empty() {
                return self.value_node(intermediate.feed);
            }
            let mut abs = self.sequence_source_path(intermediate.feed)?;
            abs.extend(intermediate.suffix);
            return self.primary_source_field(&abs);
        }
        if let Some(&(call_idx, component_id)) = self.udf_by_output.get(&key) {
            return self.udf_output_node(key, call_idx, component_id);
        }
        // A function output?
        let idx = *self.fn_by_output.get(&key)?;
        if is_filter_component(&self.fn_components[idx]) {
            // A filter feeding a value position is pass-through of its
            // node input for our purposes; treat the value as whatever
            // feeds the filter's first input.
            let feed = self.input_feed(idx, 0)?;
            return self.value_node(feed);
        }
        if is_input_component(&self.fn_components[idx]) {
            return match self.input_feed(idx, 0) {
                Some(feed) => self.value_node(feed),
                None => Some(self.const_null()),
            };
        }
        if is_distinct_values_component(&self.fn_components[idx]) {
            return self
                .input_feed(idx, 0)
                .and_then(|feed| self.value_node(feed));
        }
        if is_sequence_producer(&self.fn_components[idx]) {
            if !self.sequence_scope_components.contains(&idx)
                && self.warned_sequence_uses.insert(idx)
            {
                self.warnings.push(format!(
                    "sequence function `{}` is not connected to a repeating target; scalar use is unsupported",
                    self.fn_components[idx].name
                ));
            }
            return Some(self.sequence_item(idx));
        }
        if is_first_items_component(&self.fn_components[idx]) {
            return self
                .input_feed(idx, 0)
                .and_then(|feed| self.value_node(feed));
        }
        match self.fn_components[idx].name.as_str() {
            // A group-by's key output is the key expression itself
            // (re-evaluated in the group's context it reads the group's
            // shared key); its groups output passes the nodes through.
            "group-by" => {
                let pos = if self.fn_components[idx].outputs.get(1) == Some(&key) {
                    1
                } else {
                    0
                };
                let feed = self.input_feed(idx, pos)?;
                self.value_node(feed)
            }
            _ => Some(self.fn_node(idx)),
        }
    }

    /// Materializes function component `idx` as a mapping node.
    fn fn_node(&mut self, idx: usize) -> NodeId {
        if let Some(&id) = self.fn_nodes.get(&idx) {
            return id;
        }
        // Reserve the id first so cycles cannot recurse forever.
        let id = self.next_id;
        self.next_id += 1;
        self.fn_nodes.insert(idx, id);

        // Aggregates take a sequence connection, not scalar arguments, so
        // they must not materialize their feeds as SourceFields.
        let name = self.fn_components[idx].name.clone();
        if let Some(op) = aggregate_op(&name).filter(|_| self.fn_components[idx].kind == 5) {
            let node = match self.aggregate_node(op, idx) {
                Some(node) => node,
                None => {
                    self.warnings.push(format!(
                        "aggregate `{name}` has an unresolvable sequence input; \
                         imported as a plain call and will fail at run time until \
                         replaced"
                    ));
                    let args = (0..self.fn_components[idx].inputs.len().max(1))
                        .map(|_| self.const_null())
                        .collect();
                    Node::Call {
                        function: name,
                        args,
                    }
                }
            };
            self.graph.nodes.insert(id, node);
            return id;
        }
        if name == "position" && self.fn_components[idx].kind == 5 {
            let collection = self.position_collection(idx);
            self.graph.nodes.insert(id, Node::Position { collection });
            return id;
        }
        let fc = &self.fn_components[idx];

        let mut input_ids = Vec::with_capacity(fc.inputs.len());
        for input in fc.inputs.clone() {
            let feed = input.and_then(|k| self.edge_from.get(&k).copied());
            let node = feed.and_then(|from| self.value_node(from));
            input_ids.push(node);
        }
        let input_or_null = |builder: &mut Self, i: usize| {
            input_ids
                .get(i)
                .copied()
                .flatten()
                .unwrap_or_else(|| builder.const_null())
        };

        let node = match (fc.name.as_str(), fc.kind) {
            ("constant", _) => {
                let (value, datatype) = fc.constant.clone().unwrap_or_default();
                Node::Const {
                    value: parse_constant(&value, &datatype),
                }
            }
            ("if-else", _) => Node::If {
                condition: input_or_null(self, 0),
                then: input_or_null(self, 1),
                else_: input_or_null(self, 2),
            },
            ("value-map", _) => {
                let (table, default) = fc.valuemap.clone().unwrap_or_default();
                Node::ValueMap {
                    input: input_or_null(self, 0),
                    table: table
                        .into_iter()
                        .map(|(f, t)| (Value::String(f), Value::String(t)))
                        .collect(),
                    default: default.map(Value::String),
                }
            }
            (name, _) => {
                let function = match map_function_name(name) {
                    Some(mapped) => mapped.to_string(),
                    None => {
                        self.warnings.push(format!(
                            "function `{name}` has no ferrule equivalent; imported \
                             as-is and will fail at run time until replaced"
                        ));
                        name.to_string()
                    }
                };
                // MapForce declares the function's full optional pin set even
                // when callers leave its trailing optional arguments unwired.
                // Keep interior pin positions, but do not turn unused trailing
                // pins into ferrule arguments.
                let arity = input_ids
                    .iter()
                    .rposition(Option::is_some)
                    .map_or(1, |last| last + 1);
                let args = (0..arity)
                    .map(|i| {
                        input_ids.get(i).copied().flatten().unwrap_or_else(|| {
                            if function == "format_number" && i == 2 {
                                self.alloc(Node::Const {
                                    value: Value::String(".".into()),
                                })
                            } else {
                                self.const_null()
                            }
                        })
                    })
                    .collect();
                Node::Call { function, args }
            }
        };
        self.graph.nodes.insert(id, node);
        id
    }

    /// Converts an aggregate function component into a [`Node::Aggregate`].
    /// The connected inputs split into source-entry feeds (sequence and,
    /// optionally, an explicit parent-context before it) and scalar feeds
    /// (join's separator, item-at's position). `None` when no input
    /// resolves to a source entry.
    fn aggregate_node(&mut self, op: AggregateOp, idx: usize) -> Option<Node> {
        let fc = &self.fn_components[idx];
        let source_schema = self.sources.first()?.schema.clone();
        let sequence_feed = self.input_feed(idx, 1).or_else(|| {
            (fc.inputs.len() == 1)
                .then(|| self.input_feed(idx, 0))
                .flatten()
        })?;

        let (collection_abs, value, expression) =
            if let Some(path) = self.sequence_source_path(sequence_feed) {
                let (collection, value) = split_at_innermost_repeating(&source_schema, &path);
                (collection, value, None)
            } else {
                let mut dependencies = self.sequence_dependency_paths(sequence_feed);
                if let Some(context) = self
                    .input_feed(idx, 0)
                    .and_then(|feed| self.sequence_source_path(feed))
                {
                    dependencies.push(context);
                }
                let collection = compatible_collection(&source_schema, &dependencies)?;
                let expression = self.value_node_in_collection(sequence_feed, &collection)?;
                (collection, Vec::new(), Some(expression))
            };

        let collection = match collection_abs.split_last() {
            Some((last, prefix)) => {
                let mut relative = self.suffix_after_framed(&source_schema, prefix);
                relative.push(last.clone());
                relative
            }
            None => Vec::new(),
        };
        let arg = self
            .input_feed(idx, 2)
            .and_then(|feed| self.value_node(feed));
        Some(Node::Aggregate {
            function: op,
            collection,
            value,
            expression,
            arg,
        })
    }

    /// Source leaves used by a computed sequence expression. Aggregating
    /// that expression iterates the deepest collection shared by the leaves;
    /// outer leaves broadcast through the engine's normal context fallback.
    fn sequence_dependency_paths(&self, feed: u32) -> Vec<Vec<String>> {
        fn visit(
            builder: &GraphBuilder<'_>,
            feed: u32,
            visited: &mut std::collections::BTreeSet<u32>,
            paths: &mut Vec<Vec<String>>,
        ) {
            if !visited.insert(feed) {
                return;
            }
            if let Some(path) = builder
                .sources
                .first()
                .and_then(|source| source.ports.get(&feed))
            {
                paths.push(path.clone());
                return;
            }
            let Some(&idx) = builder.fn_by_output.get(&feed) else {
                return;
            };
            let component = &builder.fn_components[idx];
            if aggregate_op(&component.name).is_some() && component.kind == 5
                || is_distinct_values_component(component)
            {
                return;
            }
            for key in component.inputs.iter().flatten() {
                if let Some(&input_feed) = builder.edge_from.get(key) {
                    visit(builder, input_feed, visited, paths);
                }
            }
        }

        let mut paths = Vec::new();
        visit(
            self,
            feed,
            &mut std::collections::BTreeSet::new(),
            &mut paths,
        );
        paths
    }

    fn position_collection(&self, idx: usize) -> Vec<String> {
        let Some(source) = self.sources.first() else {
            return Vec::new();
        };
        let Some(path) = self
            .input_feed(idx, 0)
            .and_then(|feed| self.sequence_source_path(feed))
        else {
            return Vec::new();
        };
        let collection_abs = split_at_innermost_repeating(&source.schema, &path).0;
        match collection_abs.split_last() {
            Some((last, prefix)) => {
                let mut relative = self.suffix_after_framed(&source.schema, prefix);
                relative.push(last.clone());
                relative
            }
            None => Vec::new(),
        }
    }

    /// The feed of pin `pos` on function component `idx`, if connected.
    fn input_feed(&self, idx: usize, pos: usize) -> Option<u32> {
        self.fn_components[idx]
            .inputs
            .get(pos)
            .copied()
            .flatten()
            .and_then(|k| self.edge_from.get(&k).copied())
    }

    /// Materializes an expression with `collection` treated as an iteration
    /// frame, then restores the scope-derived frame set for other nodes.
    fn value_node_in_collection(&mut self, key: u32, collection: &[String]) -> Option<NodeId> {
        let inserted = !collection.is_empty() && self.framed.insert(collection.to_vec());
        let node = self.value_node(key);
        if inserted {
            self.framed.remove(collection);
        }
        node
    }

    /// Follows an iteration feed through `filter` and `group-by`
    /// components back to the underlying source entry, collecting the
    /// filter's boolean expression and the group-by's key expression on
    /// the way.
    fn resolve_iteration_feed(&self, from: u32) -> IterationFeed {
        self.resolve_iteration_feed_inner(from, 0)
    }

    fn resolve_iteration_feed_inner(&self, mut from: u32, depth: usize) -> IterationFeed {
        let mut filter_expr = None;
        let mut group_key = None;
        let mut distinct_key = None;
        let mut order_issue = None;
        let mut nearest_control = None;
        let mut sort_expr = None;
        let mut sort_descending = false;
        let mut take_expr = None;
        let mut take_default_one = false;
        let mut projects_whole_group = false;
        let mut projections = BTreeMap::new();
        let mut source_suffix = Vec::new();
        let mut sequence_component = None;
        // Chains are short; the bound only guards against odd cycles.
        for _ in 0..12 {
            if let Some(intermediate) = self.intermediate_feed(from) {
                projects_whole_group |= intermediate.suffix.is_empty();
                projections.extend(intermediate.projections);
                if let Some(control) = intermediate.control
                    && depth < 12
                {
                    let control = self.resolve_iteration_feed_inner(control, depth + 1);
                    filter_expr = filter_expr.or(control.filter_expr);
                    group_key = group_key.or(control.group_key);
                    distinct_key = distinct_key.or(control.distinct_key);
                    order_issue = order_issue.or(control.order_issue);
                    if sort_expr.is_none() && control.sort_expr.is_some() {
                        sort_expr = control.sort_expr;
                        sort_descending = control.sort_descending;
                    }
                    take_expr = take_expr.or(control.take_expr);
                    take_default_one |= control.take_default_one;
                }
                let mut suffix = intermediate.suffix;
                suffix.extend(source_suffix);
                source_suffix = suffix;
                from = intermediate.feed;
                continue;
            }
            let Some(&idx) = self.fn_by_output.get(&from) else {
                break;
            };
            let fc = &self.fn_components[idx];
            if is_sequence_producer(fc) {
                sequence_component = Some(idx);
                break;
            } else if is_filter_component(fc) {
                let Some(node_feed) = self.input_feed(idx, 0) else {
                    break;
                };
                note_iteration_control_order(1, &mut nearest_control, &mut order_issue);
                filter_expr = filter_expr.or_else(|| self.input_feed(idx, 1));
                from = node_feed;
            } else if is_sort_component(fc) {
                let Some(nodes_feed) = self.input_feed(idx, 0) else {
                    break;
                };
                note_iteration_control_order(0, &mut nearest_control, &mut order_issue);
                if sort_expr.is_none() {
                    sort_expr = self.input_feed(idx, 1);
                    sort_descending = fc.sort_descending.unwrap_or(false);
                }
                from = nodes_feed;
            } else if is_first_items_component(fc) {
                let Some(nodes_feed) = self.input_feed(idx, 0) else {
                    break;
                };
                note_iteration_control_order(3, &mut nearest_control, &mut order_issue);
                if distinct_key.is_some() {
                    order_issue.get_or_insert(
                        "applies first-items before distinct-values, which cannot be represented exactly",
                    );
                }
                // A variable driven by group-by uses first-items to select
                // the first member inside each group. Grouped scope frames
                // already expose that member to scalar bindings, so an
                // outer item limit would incorrectly truncate the groups.
                if group_key.is_none() && take_expr.is_none() && !take_default_one {
                    take_expr = self.input_feed(idx, 1);
                    take_default_one = take_expr.is_none();
                }
                from = nodes_feed;
            } else if is_distinct_values_component(fc) {
                let Some(values_feed) = self.input_feed(idx, 0) else {
                    break;
                };
                let unsupported_downstream = if sort_expr.is_some() {
                    Some("sort")
                } else if filter_expr.is_some() {
                    Some("filter")
                } else if group_key.is_some() {
                    Some("group-by")
                } else if distinct_key.is_some() {
                    Some("another distinct-values")
                } else {
                    None
                };
                if let Some(downstream) = unsupported_downstream {
                    order_issue.get_or_insert(match downstream {
                        "sort" => "applies distinct-values before sort, which cannot be represented exactly",
                        "filter" => "applies distinct-values before filter, which cannot be represented exactly",
                        "group-by" => "applies distinct-values before group-by, which cannot be represented exactly",
                        _ => "chains multiple distinct-values components, which cannot be represented exactly",
                    });
                }
                distinct_key.get_or_insert(values_feed);
                from = values_feed;
            } else {
                match fc.name.as_str() {
                    "group-by" if fc.outputs.first() == Some(&from) => {
                        let Some(nodes_feed) = self.input_feed(idx, 0) else {
                            break;
                        };
                        note_iteration_control_order(2, &mut nearest_control, &mut order_issue);
                        if distinct_key.is_some() {
                            order_issue.get_or_insert(
                                "applies group-by before distinct-values, which cannot be represented exactly",
                            );
                        }
                        group_key = group_key.or_else(|| self.input_feed(idx, 1));
                        from = nodes_feed;
                    }
                    _ => break,
                }
            }
        }
        IterationFeed {
            source_key: from,
            sequence_component,
            source_suffix,
            filter_expr,
            group_key,
            distinct_key,
            order_issue,
            sort_expr,
            sort_descending,
            take_expr,
            take_default_one,
            projects_whole_group,
            projections,
        }
    }

    /// Follows supported sequence pass-throughs to the primary-source entry
    /// a sequence connection ultimately reads, for aggregates.
    fn sequence_source_path(&self, mut feed: u32) -> Option<Vec<String>> {
        let mut suffix = Vec::new();
        for _ in 0..12 {
            if let Some(abs) = self.sources.first()?.ports.get(&feed) {
                let mut path = abs.clone();
                path.extend(suffix);
                return Some(path);
            }
            if let Some(intermediate) = self.intermediate_feed(feed) {
                let mut intermediate_suffix = intermediate.suffix;
                intermediate_suffix.extend(suffix);
                suffix = intermediate_suffix;
                feed = intermediate.feed;
                continue;
            }
            let &idx = self.fn_by_output.get(&feed)?;
            let fc = &self.fn_components[idx];
            if is_filter_component(fc) || is_sort_component(fc) || is_first_items_component(fc) {
                feed = self.input_feed(idx, 0)?;
            } else {
                match fc.name.as_str() {
                    "group-by" if fc.outputs.first() == Some(&feed) => {
                        feed = self.input_feed(idx, 0)?;
                    }
                    _ => return None,
                }
            }
        }
        None
    }

    fn iteration_source_path(&self, feed: &IterationFeed) -> Option<Vec<String>> {
        if feed.sequence_component.is_some() {
            return None;
        }
        let mut path = self.source_abs_path(feed.source_key)?;
        path.extend(feed.source_suffix.iter().cloned());
        if feed.distinct_key.is_some() {
            let schema = &self.sources.first()?.schema;
            Some(split_at_innermost_repeating(schema, &path).0)
        } else {
            Some(path)
        }
    }

    fn sequence_expr(&mut self, idx: usize) -> Option<SequenceExpr> {
        let item = self.sequence_item(idx);
        Some(match self.fn_components[idx].name.as_str() {
            "tokenize" => {
                let input = self
                    .input_feed(idx, 0)
                    .and_then(|feed| self.value_node(feed))?;
                let delimiter = self
                    .input_feed(idx, 1)
                    .and_then(|feed| self.value_node(feed))?;
                SequenceExpr::Tokenize {
                    input,
                    delimiter,
                    item,
                }
            }
            "tokenize-by-length" => {
                let input = self
                    .input_feed(idx, 0)
                    .and_then(|feed| self.value_node(feed))?;
                let length = self
                    .input_feed(idx, 1)
                    .and_then(|feed| self.value_node(feed))?;
                SequenceExpr::TokenizeByLength {
                    input,
                    length,
                    item,
                }
            }
            "generate-sequence" => {
                let from = self
                    .input_feed(idx, 0)
                    .and_then(|feed| self.value_node(feed));
                let to = self
                    .input_feed(idx, 1)
                    .and_then(|feed| self.value_node(feed))?;
                SequenceExpr::Generate { from, to, item }
            }
            _ => return None,
        })
    }

    /// The absolute source path behind output-port `key` on the primary
    /// source, if that is what it is.
    fn source_abs_path(&self, key: u32) -> Option<Vec<String>> {
        self.sources.first()?.ports.get(&key).cloned()
    }
}
