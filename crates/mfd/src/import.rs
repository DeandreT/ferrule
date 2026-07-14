//! `.mfd` -> `mapping::Project` conversion.
//!
//! The importer never fails on unsupported constructs: it converts what it
//! can and records a warning per skipped piece, because a partial import
//! the user finishes by hand still beats redrawing the mapping.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use ir::{SchemaKind, Value};
use mapping::{Graph, NamedSource, Node, NodeId, Project, RuntimeValue, Scope, SequenceExpr};

use crate::MfdError;

mod aggregate;
mod alternatives;
mod db_query;
mod db_where;
mod dynamic_json;
mod function;
mod generated_occurrence;
mod graph;
mod group_projection;
mod iteration;
mod join;
mod materialize;
mod output_parameter;
mod schema;
mod scope;
mod sequence_scalar;
mod source;
mod target_iteration;
mod udf;

use db_query::is_routine_catalog;
use function::{
    aggregate_op, is_db_function_component, is_db_where as is_db_where_component,
    is_distinct_values as is_distinct_values_component, is_filter as is_filter_component,
    is_first_items as is_first_items_component, is_group_into_blocks, is_group_starting_with,
    is_input as is_input_component, is_sequence_producer, is_sort as is_sort_component,
    map_name as map_function_name, parse_constant, read as read_fn_component,
};
use graph::{GraphBuilder, read_copy_all_targets, read_edges};
use iteration::{
    IntermediateFeed, IterationFeed, note_iteration_control_order, split_at_innermost_repeating,
};
use schema::{
    SchemaComponent, note_skipped_library, read_csv_component, read_db_component,
    read_edi_component, read_fixed_width_component, read_json_component, read_schema_component,
    read_xlsx_component, schema_node_at,
};
use scope::{ScopeBuilder, TargetLeaf};
use source::{SourcePath, primary_index, runtime_names};
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
    let mut output_parameters = Vec::new();
    let mut udf_registry = UdfRegistry::read(&mapping_el, path, &mut warnings);
    let mut udf_calls = Vec::new();
    let mut pending_joins = join::PendingJoins::default();
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
                "xlsx" if component.attribute("kind") == Some("26") => {
                    match read_xlsx_component(&component, &mut warnings) {
                        Some(sc) => schema_components.push(sc),
                        None => {
                            note_skipped_library(&mut skipped_libraries, "xlsx");
                            warnings.push(format!("skipped xlsx component `{name}`"));
                        }
                    }
                }
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
                    } else if flavor == "flf" {
                        let string_parse = text_el.is_some_and(|text| {
                            text.parent().is_some_and(|data| {
                                data.children().any(|node| {
                                    node.has_tag_name("parameter")
                                        && node.attribute("usageKind") == Some("stringparse")
                                })
                            })
                        });
                        if string_parse {
                            note_skipped_library(&mut skipped_libraries, "text/flf-stringparse");
                            warnings.push(format!(
                                "skipped fixed-length component `{name}`: string-parse parameters consume a run-time string, which ferrule file inputs cannot represent"
                            ));
                        } else {
                            let warning_count = warnings.len();
                            match read_fixed_width_component(&component, &mut warnings) {
                                Some(sc) => schema_components.push(sc),
                                None if warnings.len() == warning_count => warnings
                                    .push(format!("skipped fixed-length component `{name}`")),
                                None => {}
                            }
                        }
                    } else if flavor == "txt"
                        && text_el.and_then(|text| text.attribute("config")).is_some()
                    {
                        note_skipped_library(&mut skipped_libraries, "text/flextext");
                        warnings.push(format!(
                            "skipped FlexText component `{name}`: external `.mft` configurations are not embedded in the design and cannot be imported"
                        ));
                    } else {
                        let label = if flavor.is_empty() {
                            "text".to_string()
                        } else {
                            format!("text/{flavor}")
                        };
                        note_skipped_library(&mut skipped_libraries, &label);
                        warnings.push(format!(
                            "skipped component `{name}`: text flavor `{flavor}` is \
                             not supported yet (inline csv, fixed-length, and edi text components import)"
                        ));
                    }
                }
                "db" if is_db_function_component(&component) => {
                    fn_components.push(read_fn_component(&component));
                }
                "db" if is_routine_catalog(&component, &children) => {}
                "db" => match read_db_component(&component, &mapping_el, path, &mut warnings) {
                    Some(sc) => schema_components.push(sc),
                    None => note_skipped_library(&mut skipped_libraries, "db"),
                },
                "core" if component.attribute("kind") == Some("7") => {
                    output_parameters.push(output_parameter::read(&component));
                }
                "core" if component.attribute("kind") == Some("32") => {
                    pending_joins.read(component, &mut warnings);
                }
                "core" | "lang" => fn_components.push(read_fn_component(&component)),
                "edifact" if name == "to-datetime" => {
                    fn_components.push(read_fn_component(&component));
                }
                "xpath2" if map_function_name(&name).is_some() => {
                    fn_components.push(read_fn_component(&component));
                }
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
                                 (only xml/json/csv/fixed-length/edi/db/xlsx, scalar user-defined functions, and \
                                 core/lang function components and supported XPath 2 functions import)"
                            ));
                        }
                    }
                }
            }
        }
    }
    // UDF-owned static catalogs are secondary to ordinary mapping sources.
    // Keeping them last also preserves the document source as the default in
    // scalar-only mappings, where repetition-based primary scoring is tied.
    schema_components.extend(udf_registry.take_sources());

    // Edges are indexed as to-key -> from-key; each input has at most one feed.
    let edge_from = read_edges(&structure, Some(&wrapper));
    let copy_all_targets = read_copy_all_targets(&structure);

    let output_failed = output_parameter::install_fallback(
        &mut schema_components,
        output_parameters,
        &edge_from,
        &mut warnings,
    );

    let mut sources: Vec<&SchemaComponent> = schema_components
        .iter()
        .filter(|c| !c.is_variable && c.is_source)
        .collect();
    let targets: Vec<&SchemaComponent> = schema_components
        .iter()
        .filter(|c| !c.is_variable && !c.is_source)
        .collect();
    let intermediates: Vec<&SchemaComponent> =
        schema_components.iter().filter(|c| c.is_variable).collect();
    let unsupported =
        |side: &str| output_parameter::missing_error(side, &skipped_libraries, output_failed);
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
    if sources.is_empty() {
        return Err(unsupported("source"));
    }
    let primary_source = primary_index(&sources, target, &edge_from, &fn_components);
    sources.swap(0, primary_source);
    let source_names = runtime_names(&sources);
    let primary = sources[0];
    let joins = pending_joins.resolve(&edge_from, &sources, &source_names, &mut warnings);

    let mut builder = GraphBuilder {
        graph: Graph::default(),
        next_id: 0,
        fn_nodes: BTreeMap::new(),
        sequence_items: BTreeMap::new(),
        sequence_scope_components: BTreeSet::new(),
        sequence_predicate_components: BTreeSet::new(),
        warned_sequence_uses: BTreeSet::new(),
        warned_scalar_filters: BTreeSet::new(),
        warned_join_controls: BTreeSet::new(),
        rejected_join_paths: BTreeSet::new(),
        source_fields: BTreeMap::new(),
        query_scope_sources: BTreeSet::new(),
        warned_unscoped_queries: BTreeSet::new(),
        edge_from: &edge_from,
        sources: &sources,
        source_names: &source_names,
        intermediates: &intermediates,
        fn_components: &fn_components,
        fn_by_output: BTreeMap::new(),
        udf_nodes: BTreeMap::new(),
        udf_by_output: BTreeMap::new(),
        udf_calls: &udf_calls,
        udf_registry: &udf_registry,
        joins,
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
    let dynamic_target = dynamic_json::prepare_target(target, &mut builder);
    let mut iterations = Vec::new();
    let mut bindings = Vec::new();
    let mut group_projections = Vec::new();
    let mut structured_udf_targets = Vec::new();
    for (&inpkey, target_path) in &target.ports {
        let Some(&from) = edge_from.get(&inpkey) else {
            continue;
        };
        let node_kind = schema_node_at(&target.schema, target_path);
        match node_kind {
            Some(node) if matches!(node.kind, SchemaKind::Group { .. }) => {
                if udf::structured::accept_target(target, target_path, node, inpkey, from, &builder)
                {
                    structured_udf_targets.push((target_path.clone(), from));
                    continue;
                }
                group_projection::classify_target_connection(
                    target,
                    group_projection::TargetConnection {
                        target_path,
                        target_node: node,
                        input_key: inpkey,
                        feed: from,
                        copy_all_targets: &copy_all_targets,
                    },
                    &mut builder,
                    &mut iterations,
                    &mut group_projections,
                )
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
    udf::structured::prepare_target_frames(&structured_udf_targets, &mut builder);
    generated_occurrence::infer(target, &mut builder, &mut iterations);
    iterations.sort_by_key(|iteration| iteration.target_path.len());
    join::prepare_iterations(&iterations, &mut builder, &mut scope_builder);
    for iteration in &iterations {
        let feed = builder.resolve_iteration_feed(iteration.feed);
        if let Some(idx) = feed.sequence_component {
            builder.sequence_scope_components.insert(idx);
        }
        if let Some(source_path) = builder.iteration_source_path(&feed) {
            builder.note_framed_prefixes(&source_path);
        }
    }
    materialize::eager_functions(&mut builder);
    let mut skipped_iteration_paths = target_iteration::build(
        iterations,
        target,
        &bindings,
        &mut builder,
        &mut scope_builder,
    );
    let structured_udf_paths = structured_udf_targets
        .iter()
        .map(|(path, _)| path.clone())
        .collect::<Vec<_>>();
    udf::structured::build_targets(
        structured_udf_targets,
        target,
        &mut builder,
        &mut scope_builder,
        &mut skipped_iteration_paths,
    );
    group_projection::build(
        group_projections,
        target,
        &skipped_iteration_paths,
        &mut builder,
        &mut scope_builder,
    );
    for (target, from) in bindings {
        let target_path = target.path();
        if builder.join_dependency_rejected(from) {
            continue;
        }
        if structured_udf_paths
            .iter()
            .any(|path| target_path.starts_with(path))
            && builder.is_structured_recipe(from)
        {
            continue;
        }
        if skipped_iteration_paths
            .iter()
            .any(|path| target_path.starts_with(path))
        {
            continue;
        }
        let Some(node) = builder.binding_node(from, &target_path) else {
            continue;
        };
        scope_builder.add_binding(target, node);
    }
    dynamic_json::build_target(
        dynamic_target,
        target,
        &mut builder,
        &mut scope_builder.root,
    );

    let mut extra_sources = Vec::new();
    for (index, extra) in sources.iter().enumerate().skip(1) {
        let has_dynamic_input = extra.db_queries.is_empty()
            && extra
                .input_keys
                .iter()
                .any(|key| edge_from.contains_key(key));
        if has_dynamic_input {
            builder.warnings.push(format!(
                "extra source `{}` has a connected run-time input; the stored instance path is \
                 used until dynamic sources are supported",
                source_names[index]
            ));
        } else if extra.input_instance.is_none() {
            builder.warnings.push(format!(
                "extra source `{}` has no input instance path; the imported project needs one \
                 before it can run",
                source_names[index]
            ));
        }
        extra_sources.push(NamedSource {
            name: source_names[index].clone(),
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
    pub(super) fn value_node(&mut self, key: u32) -> Option<NodeId> {
        if let Some(node) = self.join_field_node(key) {
            return Some(node);
        }
        // A source schema entry?
        for (idx, source) in self.sources.iter().enumerate() {
            if let Some(abs) = source.ports.get(&key).cloned() {
                return self.source_field_at(&SourcePath {
                    source: idx,
                    path: abs,
                });
            }
        }
        // A transparent output of a variable schema component?
        if let Some(intermediate) = self.intermediate_feed(key) {
            if intermediate.suffix.is_empty() {
                return self.value_node(intermediate.feed);
            }
            let mut source_path = self.sequence_source_path(intermediate.feed)?;
            source_path.path.extend(intermediate.suffix);
            return self.source_field_at(&source_path);
        }
        if let Some(&(call_idx, component_id)) = self.udf_by_output.get(&key) {
            return self.udf_output_node(key, call_idx, component_id);
        }
        // A function output?
        let idx = *self.fn_by_output.get(&key)?;
        if is_filter_component(&self.fn_components[idx]) {
            if let Some(node) = self.scalar_filter_lookup_node(idx) {
                return Some(node);
            }
            // A filter feeding a value position is pass-through of its
            // node input for our purposes; treat the value as whatever
            // feeds the filter's first input.
            let feed = self.input_feed(idx, 0)?;
            return self.value_node(feed);
        }
        if is_db_where_component(&self.fn_components[idx]) {
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
            if !(self.sequence_scope_components.contains(&idx)
                || self.sequence_predicate_components.contains(&idx))
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
        if is_group_into_blocks(&self.fn_components[idx]) {
            return self
                .input_feed(idx, 0)
                .and_then(|feed| self.value_node(feed));
        }
        if is_group_starting_with(&self.fn_components[idx]) {
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
        if name == "exists"
            && self.fn_components[idx].library == "core"
            && self.fn_components[idx].kind == 5
            && let Some(node) = self.sequence_exists_node(idx)
        {
            self.graph.nodes.insert(id, node);
            return id;
        }
        if let Some(op) = aggregate_op(&name).filter(|_| self.fn_components[idx].kind == 5) {
            let node = match self.aggregate_node(op, idx) {
                Ok(Some(node)) => node,
                Ok(None) => self.unsupported_aggregate_call(
                    &name,
                    idx,
                    "has an unresolvable sequence input",
                ),
                Err(reason) => self.unsupported_aggregate_call(
                    &name,
                    idx,
                    &format!("cannot import its sequence: {reason}"),
                ),
            };
            self.graph.nodes.insert(id, node);
            return id;
        }
        if name == "position" && self.fn_components[idx].kind == 5 {
            let node = self
                .join_position_node(idx)
                .unwrap_or_else(|| Node::Position {
                    collection: self.position_collection(idx),
                });
            self.graph.nodes.insert(id, node);
            return id;
        }
        let fc = &self.fn_components[idx];
        let numeric_inputs = matches!(fc.name.as_str(), "add" | "subtract" | "multiply" | "divide");

        let mut input_ids = Vec::with_capacity(fc.inputs.len());
        for input in fc.inputs.clone() {
            let feed = input.and_then(|k| self.edge_from.get(&k).copied());
            let node = feed.and_then(|from| {
                numeric_inputs
                    .then(|| self.numeric_string_constant(from))
                    .flatten()
                    .or_else(|| self.value_node(from))
            });
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
            ("mfd-filepath", _) => Node::RuntimeValue {
                value: RuntimeValue::MappingFilePath,
            },
            ("main-mfd-filepath", _) => Node::RuntimeValue {
                value: RuntimeValue::MainMappingFilePath,
            },
            ("now", _) => Node::RuntimeValue {
                value: RuntimeValue::CurrentDateTime,
            },
            ("set-xsi-nil", _) => Node::Const {
                value: Value::xml_nil(),
            },
            ("if-else", _) => Node::If {
                condition: input_or_null(self, 0),
                then: input_or_null(self, 1),
                else_: input_or_null(self, 2),
            },
            ("value-map", _) => {
                let value_map = fc.valuemap.clone().unwrap_or_default();
                Node::ValueMap {
                    input: input_or_null(self, 0),
                    input_type: value_map.input_type,
                    table: value_map.table,
                    default: value_map.default,
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

    fn numeric_string_constant(&mut self, feed: u32) -> Option<NodeId> {
        let component = self
            .fn_by_output
            .get(&feed)
            .and_then(|index| self.fn_components.get(*index))?;
        let (text, datatype) = component.constant.as_ref()?;
        if datatype != "string" {
            return None;
        }
        let value = text
            .trim()
            .parse::<i64>()
            .map(Value::Int)
            .or_else(|_| text.trim().parse::<f64>().map(Value::Float))
            .ok()
            .filter(|value| !matches!(value, Value::Float(value) if !value.is_finite()))?;
        Some(self.alloc(Node::Const { value }))
    }

    fn position_collection(&self, idx: usize) -> Vec<String> {
        let Some(source_path) = self
            .input_feed(idx, 0)
            .and_then(|feed| self.sequence_source_path(feed))
        else {
            return Vec::new();
        };
        let Some(source) = self.sources.get(source_path.source) else {
            return Vec::new();
        };
        let collection_abs = split_at_innermost_repeating(&source.schema, &source_path.path).0;
        self.collection_path(source_path.source, &collection_abs)
            .unwrap_or_default()
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

    /// Follows an iteration feed through sequence controls back to the
    /// underlying source entry, collecting their expressions on the way.
    pub(super) fn resolve_iteration_feed(&self, from: u32) -> IterationFeed {
        self.resolve_iteration_feed_inner(from, 0)
    }

    fn resolve_iteration_feed_inner(&self, mut from: u32, depth: usize) -> IterationFeed {
        let mut filter_expr = None;
        let mut has_filter = false;
        let mut group_key = None;
        let mut has_key_grouping = false;
        let mut group_starting_with = None;
        let mut has_start_grouping = false;
        let mut block_size = None;
        let mut has_block_grouping = false;
        let mut distinct_key = None;
        let mut order_issue = None;
        let mut nearest_control = None;
        let mut sort_expr = None;
        let mut has_sort = false;
        let mut sort_descending = false;
        let mut take_expr = None;
        let mut take_default_one = false;
        let mut projects_whole_group = false;
        let mut projections = BTreeMap::new();
        let mut source_suffix = Vec::new();
        let mut sequence_component = None;
        let mut db_where_component = None;
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
                    has_filter |= control.has_filter;
                    let grouping_count = [
                        group_key,
                        distinct_key,
                        group_starting_with,
                        block_size,
                        control.group_key,
                        control.distinct_key,
                        control.group_starting_with,
                        control.block_size,
                    ]
                    .into_iter()
                    .flatten()
                    .count();
                    if grouping_count > 1 {
                        order_issue.get_or_insert(
                            "combines multiple grouping controls, which cannot be represented exactly",
                        );
                    }
                    group_key = group_key.or(control.group_key);
                    has_key_grouping |= control.has_key_grouping;
                    group_starting_with = group_starting_with.or(control.group_starting_with);
                    has_start_grouping |= control.has_start_grouping;
                    block_size = block_size.or(control.block_size);
                    has_block_grouping |= control.has_block_grouping;
                    distinct_key = distinct_key.or(control.distinct_key);
                    order_issue = order_issue.or(control.order_issue);
                    if sort_expr.is_none() && control.sort_expr.is_some() {
                        sort_expr = control.sort_expr;
                        sort_descending = control.sort_descending;
                    }
                    has_sort |= control.has_sort;
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
            } else if is_db_where_component(fc) {
                let Some(nodes_feed) = self.input_feed(idx, 0) else {
                    db_where_component = Some(idx);
                    break;
                };
                if db_where_component.replace(idx).is_some() {
                    order_issue.get_or_insert(
                        "chains multiple database where/order controls, which cannot be represented exactly",
                    );
                }
                from = nodes_feed;
            } else if is_filter_component(fc) {
                has_filter = true;
                let Some(node_feed) = self.input_feed(idx, 0) else {
                    break;
                };
                note_iteration_control_order(1, &mut nearest_control, &mut order_issue);
                filter_expr = filter_expr.or_else(|| self.input_feed(idx, 1));
                from = node_feed;
            } else if is_sort_component(fc) {
                has_sort = true;
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
                if group_key.is_none()
                    && group_starting_with.is_none()
                    && block_size.is_none()
                    && take_expr.is_none()
                    && !take_default_one
                {
                    take_expr = self.input_feed(idx, 1);
                    take_default_one = take_expr.is_none();
                }
                from = nodes_feed;
            } else if is_group_starting_with(fc) {
                has_start_grouping = true;
                let Some(nodes_feed) = self.input_feed(idx, 0) else {
                    break;
                };
                note_iteration_control_order(2, &mut nearest_control, &mut order_issue);
                if group_key.is_some()
                    || group_starting_with.is_some()
                    || block_size.is_some()
                    || distinct_key.is_some()
                {
                    order_issue.get_or_insert(
                        "combines group-starting-with with another grouping control, which cannot be represented exactly",
                    );
                } else {
                    group_starting_with = group_starting_with.or_else(|| self.input_feed(idx, 1));
                }
                from = nodes_feed;
            } else if is_group_into_blocks(fc) {
                has_block_grouping = true;
                let Some(nodes_feed) = self.input_feed(idx, 0) else {
                    break;
                };
                note_iteration_control_order(2, &mut nearest_control, &mut order_issue);
                if group_key.is_some()
                    || group_starting_with.is_some()
                    || block_size.is_some()
                    || distinct_key.is_some()
                {
                    order_issue.get_or_insert(
                        "combines group-into-blocks with another grouping control, which cannot be represented exactly",
                    );
                } else {
                    block_size = block_size.or_else(|| self.input_feed(idx, 1));
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
                } else if group_starting_with.is_some() {
                    Some("group-starting-with")
                } else if block_size.is_some() {
                    Some("group-into-blocks")
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
                        "group-starting-with" => "applies distinct-values before group-starting-with, which cannot be represented exactly",
                        "group-into-blocks" => "applies distinct-values before group-into-blocks, which cannot be represented exactly",
                        _ => "chains multiple distinct-values components, which cannot be represented exactly",
                    });
                }
                distinct_key.get_or_insert(values_feed);
                from = values_feed;
            } else {
                match fc.name.as_str() {
                    "group-by" if fc.outputs.first() == Some(&from) => {
                        has_key_grouping = true;
                        let Some(nodes_feed) = self.input_feed(idx, 0) else {
                            break;
                        };
                        note_iteration_control_order(2, &mut nearest_control, &mut order_issue);
                        if distinct_key.is_some() {
                            order_issue.get_or_insert(
                                "applies group-by before distinct-values, which cannot be represented exactly",
                            );
                        }
                        if group_key.is_some()
                            || block_size.is_some()
                            || group_starting_with.is_some()
                        {
                            order_issue.get_or_insert(
                                "combines multiple grouping controls, which cannot be represented exactly",
                            );
                        } else {
                            group_key = group_key.or_else(|| self.input_feed(idx, 1));
                        }
                        from = nodes_feed;
                    }
                    _ => break,
                }
            }
        }
        IterationFeed {
            source_key: from,
            sequence_component,
            db_where_component,
            source_suffix,
            filter_expr,
            has_filter,
            group_key,
            has_key_grouping,
            group_starting_with,
            has_start_grouping,
            block_size,
            has_block_grouping,
            distinct_key,
            order_issue,
            sort_expr,
            has_sort,
            sort_descending,
            take_expr,
            take_default_one,
            projects_whole_group,
            projections,
        }
    }

    fn sequence_expr(&mut self, idx: usize) -> Option<SequenceExpr> {
        let item = self.sequence_item(idx);
        Some(match self.fn_components[idx].name.as_str() {
            "tokenize" => {
                let input = self
                    .input_feed(idx, 0)
                    .and_then(|feed| self.sequence_scalar_input(feed))?;
                let delimiter = self
                    .input_feed(idx, 1)
                    .and_then(|feed| self.sequence_scalar_input(feed))?;
                SequenceExpr::Tokenize {
                    input,
                    delimiter,
                    item,
                }
            }
            "tokenize-by-length" => {
                let input = self
                    .input_feed(idx, 0)
                    .and_then(|feed| self.sequence_scalar_input(feed))?;
                let length = self
                    .input_feed(idx, 1)
                    .and_then(|feed| self.sequence_scalar_input(feed))?;
                SequenceExpr::TokenizeByLength {
                    input,
                    length,
                    item,
                }
            }
            "generate-sequence" => {
                let from = self
                    .input_feed(idx, 0)
                    .and_then(|feed| self.sequence_scalar_input(feed));
                let to = self
                    .input_feed(idx, 1)
                    .and_then(|feed| self.sequence_scalar_input(feed))?;
                SequenceExpr::Generate { from, to, item }
            }
            _ => return None,
        })
    }
}
