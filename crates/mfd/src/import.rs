//! `.mfd` -> `mapping::Project` conversion.
//!
//! The importer never fails on unsupported constructs: it converts what it
//! can and records a warning per skipped piece, because a partial import
//! the user finishes by hand still beats redrawing the mapping.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use ir::SchemaKind;
use mapping::{
    Graph, NamedSource, NamedTarget, NodeId, Project, Scope, ScopeIteration, ScopeSequence,
    SequenceExpr,
};

use crate::MfdError;

mod aggregate;
mod alternatives;
mod db_query;
mod db_where;
mod dynamic_json;
mod external_udf;
mod flextext_parser;
mod function;
mod generated_occurrence;
mod graph;
mod group_projection;
mod iteration;
mod join;
mod json_serializer;
mod materialize;
mod output_parameter;
mod protobuf_target;
mod scalar_function;
mod schema;
mod scope;
mod sequence_scalar;
mod source;
mod target_iteration;
mod udf;

use db_query::is_routine_catalog;
use function::{
    is_db_function_component, is_db_where as is_db_where_component,
    is_distinct_values as is_distinct_values_component, is_filter as is_filter_component,
    is_first_items as is_first_items_component, is_group_into_blocks, is_group_starting_with,
    is_input as is_input_component, is_isbn_converter_component, is_sequence_producer,
    is_sort as is_sort_component, is_xbrl_measure_component, map_name as map_function_name,
    read as read_fn_component, read_isbn_converter_component,
};
use graph::{GraphBuilder, read_copy_all_targets, read_edges};
use iteration::{
    IntermediateFeed, IterationFeed, note_iteration_control_order, split_at_innermost_repeating,
};
use schema::{
    SchemaComponent, note_skipped_library, read_csv_component, read_db_component,
    read_edi_component, read_fixed_width_component, read_flextext_component,
    read_http_get_component, read_json_component, read_pdf_component, read_protobuf_component,
    read_schema_component, read_xbrl_component, read_xlsx_component, schema_node_at,
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
    let mut json_serializers = Vec::new();
    let mut flextext_parsers = Vec::new();
    let mut output_parameters = Vec::new();
    let mut udf_registry = UdfRegistry::read(&mapping_el, path, &mut warnings);
    let mut udf_calls = Vec::new();
    let mut external_udf_candidates = Vec::new();
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
                    Some(sc) => match json_serializer::read(&component, &sc) {
                        Ok(Some(serializer)) => json_serializers.push(serializer),
                        Ok(None) => schema_components.push(sc),
                        Err(reason) => {
                            warnings.push(format!(
                                "JSON string serializer `{name}` is unsupported: {reason}"
                            ));
                            schema_components.push(sc);
                        }
                    },
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
                        let string_parse = text_el.is_some_and(|text| {
                            text.parent().is_some_and(|data| {
                                data.children().any(|node| {
                                    node.has_tag_name("parameter")
                                        && node.attribute("usageKind") == Some("stringparse")
                                })
                            })
                        });
                        if string_parse {
                            match read_flextext_component(&component, path)
                                .and_then(|schema| flextext_parser::read(&component, schema))
                            {
                                Ok(parser) => flextext_parsers.push(parser),
                                Err(reason) => {
                                    note_skipped_library(
                                        &mut skipped_libraries,
                                        "text/flextext-stringparse",
                                    );
                                    warnings.push(format!(
                                        "skipped FlexText component `{name}`: {reason}"
                                    ));
                                }
                            }
                        } else {
                            match read_flextext_component(&component, path) {
                                Ok(schema) => schema_components.push(schema),
                                Err(reason) => {
                                    note_skipped_library(&mut skipped_libraries, "text/flextext");
                                    warnings.push(format!(
                                        "skipped FlexText component `{name}`: {reason}"
                                    ));
                                }
                            }
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
                "webservice" => match read_http_get_component(&component, path, &mut warnings) {
                    Ok(component) => schema_components.push(component),
                    Err(reason) => {
                        note_skipped_library(&mut skipped_libraries, "webservice");
                        warnings.push(format!("skipped web-service component `{name}`: {reason}"));
                    }
                },
                "binary" if component.attribute("kind") == Some("33") => {
                    match read_protobuf_component(&component, path, &mut warnings) {
                        Ok(component) => schema_components.push(component),
                        Err(reason) => {
                            note_skipped_library(&mut skipped_libraries, "binary/protobuf");
                            warnings.push(format!("skipped protobuf component `{name}`: {reason}"));
                        }
                    }
                }
                "pdf" if component.attribute("kind") == Some("34") => {
                    match read_pdf_component(&component, path, &mut warnings) {
                        Ok(component) => schema_components.push(component),
                        Err(reason) => {
                            note_skipped_library(&mut skipped_libraries, "pdf");
                            warnings.push(format!("skipped PDF component `{name}`: {reason}"));
                        }
                    }
                }
                "xbrl" if component.attribute("kind") == Some("27") => {
                    match read_xbrl_component(&component, &mut warnings) {
                        Ok(component) => schema_components.push(component),
                        Err(reason) => {
                            note_skipped_library(&mut skipped_libraries, "xbrl");
                            warnings.push(format!("skipped XBRL component `{name}`: {reason}"));
                        }
                    }
                }
                "xbrl" if is_xbrl_measure_component(&component) => {
                    fn_components.push(read_fn_component(&component));
                }
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
                "IsbnConverterService" if is_isbn_converter_component(&component) => {
                    match read_isbn_converter_component(&component) {
                        Ok(function) => fn_components.push(function),
                        Err(reason) => warnings.push(format!(
                            "skipped ISBN converter component `{name}`: {reason}"
                        )),
                    }
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
                        if !external_udf::capture_or_warn(
                            &component,
                            udf_registry.unsupported_reason(other, &name),
                            &mut external_udf_candidates,
                            &mut warnings,
                        ) {
                            warnings.push(format!(
                                "skipped component `{name}`: unsupported library `{other}` \
                                 (only xml/json/csv/fixed-length/flextext/edi/db/xlsx/protobuf/pdf-source, requestless HTTP GET XML, scalar user-defined functions, and \
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
    udf::refine_source_schemas(
        &mut schema_components,
        &udf_calls,
        &udf_registry,
        &edge_from,
    );
    let copy_all_targets = read_copy_all_targets(&structure);

    let output_failed = output_parameter::install_fallback(
        &mut schema_components,
        output_parameters,
        &edge_from,
        &mut warnings,
    );

    let target_inputs =
        external_udf::selected_target_inputs(&schema_components).ok_or_else(|| {
            output_parameter::missing_error("target", &skipped_libraries, output_failed)
        })?;
    external_udf::install_fallback(
        &mut schema_components,
        external_udf_candidates,
        &target_inputs,
        &edge_from,
        &fn_components,
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
    let connected_targets = std::iter::once(target)
        .chain(targets.iter().copied().filter(|component| {
            !std::ptr::eq(*component, target)
                && component
                    .ports
                    .keys()
                    .any(|key| edge_from.contains_key(key))
        }))
        .collect::<Vec<_>>();
    let target_names = runtime_names(&connected_targets);
    if sources.is_empty() {
        return Err(unsupported("source"));
    }
    let primary_source = primary_index(&sources, target, &edge_from, &fn_components);
    sources.swap(0, primary_source);
    let source_names = runtime_names(&sources);
    let primary = sources[0];
    let joins = pending_joins.resolve(&edge_from, &sources, &source_names, &mut warnings);

    let xml_type_conditions = alternatives::conditioned_port_types(&structure);
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
        json_serializer_nodes: BTreeMap::new(),
        flextext_parser_nodes: BTreeMap::new(),
        claimed_dynamic_ports: BTreeSet::new(),
        query_scope_sources: BTreeSet::new(),
        warned_unscoped_queries: BTreeSet::new(),
        xml_type_conditions,
        edge_from: &edge_from,
        sources: &sources,
        source_names: &source_names,
        intermediates: &intermediates,
        json_serializers: &json_serializers,
        flextext_parsers: &flextext_parsers,
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

    // Dynamic document inputs must establish their driver frames before any
    // target expression is materialized. A filename expression can also feed
    // an ordinary target binding, and SourceField nodes created there need the
    // same framed suffix as the loader path expression.
    let mut dynamic_source_inputs: Vec<Option<(u32, SourcePath)>> = vec![None; sources.len()];
    for (index, extra) in sources.iter().enumerate().skip(1) {
        if !extra.db_queries.is_empty() {
            continue;
        }
        let connected = extra
            .input_keys
            .iter()
            .filter_map(|key| edge_from.get(key).copied())
            .collect::<Vec<_>>();
        match connected.as_slice() {
            [] => {}
            [feed] => {
                if let Some(driver) = builder.computed_iteration_source(*feed) {
                    builder.note_framed_prefixes(&driver);
                    dynamic_source_inputs[index] = Some((*feed, driver));
                } else {
                    builder.warnings.push(format!(
                        "extra source `{}` has a connected run-time path that does not have one representable source iteration; the stored instance path is used",
                        source_names[index]
                    ));
                }
            }
            _ => builder.warnings.push(format!(
                "extra source `{}` has multiple connected run-time paths; the stored instance path is used",
                source_names[index]
            )),
        }
    }

    let root = build_target_scope(target, &edge_from, &copy_all_targets, &mut builder);
    let mut extra_targets = Vec::new();
    for (index, extra) in connected_targets.iter().copied().enumerate().skip(1) {
        extra_targets.push(NamedTarget {
            name: target_names[index].clone(),
            path: extra
                .output_instance
                .clone()
                .or_else(|| extra.input_instance.clone()),
            schema: extra.schema.clone(),
            options: extra.options.clone(),
            root: build_target_scope(extra, &edge_from, &copy_all_targets, &mut builder),
        });
    }

    let mut extra_sources = Vec::new();
    for (index, extra) in sources.iter().enumerate().skip(1) {
        let dynamic_path = dynamic_source_inputs[index]
            .as_ref()
            .and_then(|(feed, driver)| {
                builder
                    .binding_node(*feed, &[])
                    .map(|node| mapping::DynamicSourcePath {
                        node,
                        iteration: builder.context_path(driver),
                    })
            });
        if dynamic_path.is_none() && extra.input_instance.is_none() {
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
            dynamic_path,
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
            extra_targets,
            graph: builder.graph,
            root,
        },
        warnings,
    })
}

fn build_target_scope(
    target: &SchemaComponent,
    edge_from: &BTreeMap<u32, u32>,
    copy_all_targets: &BTreeSet<u32>,
    builder: &mut GraphBuilder<'_>,
) -> Scope {
    builder.rejected_join_paths.clear();
    let mut scopes = ScopeBuilder {
        root: Scope::default(),
        anchors: BTreeMap::new(),
    };
    let dynamic_target = dynamic_json::prepare_target(target, builder);
    let mut iterations = Vec::new();
    let mut bindings = Vec::new();
    let mut group_projections = Vec::new();
    let mut structured_udf_targets = Vec::new();
    let mut csv_singleton_bindings = BTreeMap::new();
    for (&inpkey, target_path) in &target.ports {
        let Some(&from) = edge_from.get(&inpkey) else {
            continue;
        };
        if let Some((position, field)) = schema::split_singleton_port(target_path) {
            csv_singleton_bindings
                .entry(position)
                .or_insert_with(Vec::new)
                .push((field.to_string(), from));
            continue;
        }
        let node_kind = schema_node_at(&target.schema, target_path);
        match node_kind {
            Some(node) if matches!(node.kind, SchemaKind::Group { .. }) => {
                if udf::structured::accept_target(target, target_path, node, inpkey, from, builder)
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
                        copy_all_targets,
                    },
                    builder,
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
    udf::structured::prepare_target_frames(&structured_udf_targets, builder);
    generated_occurrence::infer(target, builder, &mut iterations);
    iterations.sort_by_key(|iteration| iteration.target_path.len());
    let explicit_iteration_paths = iterations
        .iter()
        .map(|iteration| iteration.target_path.clone())
        .collect::<BTreeSet<_>>();
    join::prepare_iterations(&iterations, builder, &mut scopes);
    for iteration in &iterations {
        for feed_key in
            std::iter::once(iteration.feed).chain(iteration.additional_feeds.iter().copied())
        {
            let feed = builder.resolve_iteration_feed(feed_key);
            if let Some(idx) = feed.sequence_component {
                builder.sequence_scope_components.insert(idx);
            }
            if let Some(source_path) = builder.iteration_source_path(&feed) {
                builder.note_framed_prefixes(&source_path);
            }
        }
    }
    materialize::eager_functions(builder);
    let mut skipped_iteration_paths =
        target_iteration::build(iterations, target, &bindings, builder, &mut scopes);
    protobuf_target::infer_singleton_messages(
        target,
        &bindings,
        &explicit_iteration_paths,
        builder,
        &mut scopes,
        &mut skipped_iteration_paths,
    );
    let structured_udf_paths = structured_udf_targets
        .iter()
        .map(|(path, _)| path.clone())
        .collect::<Vec<_>>();
    udf::structured::build_targets(
        structured_udf_targets,
        target,
        builder,
        &mut scopes,
        &mut skipped_iteration_paths,
    );
    group_projection::build(
        group_projections,
        target,
        &skipped_iteration_paths,
        builder,
        &mut scopes,
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
        scopes.add_binding(target, node);
    }
    dynamic_json::build_target(dynamic_target, target, builder, &mut scopes);
    compose_csv_target_rows(csv_singleton_bindings, builder, &mut scopes);
    scopes.root
}

fn compose_csv_target_rows(
    singleton_bindings: BTreeMap<schema::CsvSingletonPosition, Vec<(String, u32)>>,
    builder: &mut GraphBuilder<'_>,
    scopes: &mut ScopeBuilder,
) {
    if singleton_bindings.is_empty() {
        return;
    }
    let repeated = std::mem::take(&mut scopes.root);
    let mut before = Vec::new();
    let mut after = Vec::new();
    for (position, bindings) in singleton_bindings {
        let mut segment = Scope::default();
        for (field, feed) in bindings {
            let target_path = vec![field.clone()];
            let Some(node) = builder.binding_node(feed, &target_path) else {
                continue;
            };
            segment.bindings.push(mapping::Binding {
                target_field: field,
                node,
            });
        }
        match position {
            schema::CsvSingletonPosition::Before(index) => before.push((index, segment)),
            schema::CsvSingletonPosition::After(index) => after.push((index, segment)),
        }
    }
    before.sort_by_key(|(index, _)| *index);
    after.sort_by_key(|(index, _)| *index);
    let mut segments = before
        .into_iter()
        .map(|(_, scope)| scope)
        .chain(std::iter::once(repeated))
        .chain(after.into_iter().map(|(_, scope)| scope));
    let Some(first) = segments.next() else {
        return;
    };
    scopes.root.iteration =
        ScopeIteration::Concatenate(ScopeSequence::new(first, segments.collect()));
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
        if self
            .json_serializers
            .iter()
            .any(|serializer| serializer.output == key)
        {
            return self.json_serializer_node(key);
        }
        if self
            .flextext_parsers
            .iter()
            .any(|parser| parser.outputs.contains_key(&key))
        {
            return self.flextext_parser_node(key);
        }
        // A source schema entry?
        for (idx, source) in self.sources.iter().enumerate() {
            if let Some(abs) = source.ports.get(&key).cloned() {
                if schema::split_json_dynamic_port(&abs).is_some() {
                    if self.claimed_dynamic_ports.contains(&key) {
                        return Some(self.const_null());
                    }
                    self.warnings.push(format!(
                        "dynamic JSON source port {key} is supported only as a paired property-name equality and boolean value"
                    ));
                    return None;
                }
                let source_path = self.source_value_path(idx, abs);
                return self.source_field_at(&source_path);
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
        let mut udf_filters = Vec::new();
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
            if let Some(input) = self.flextext_parser_input(from) {
                let Some(feed) = self.edge_from.get(&input).copied() else {
                    break;
                };
                from = feed;
                continue;
            }
            if let Some(intermediate) = self.intermediate_feed(from) {
                projects_whole_group |= intermediate.suffix.is_empty();
                projections.extend(intermediate.projections);
                if let Some(control) = intermediate.control
                    && depth < 12
                {
                    let control = self.resolve_iteration_feed_inner(control, depth + 1);
                    filter_expr = filter_expr.or(control.filter_expr);
                    udf_filters.extend(control.udf_filters);
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
            if let Some(nodes_feed) = self.udf_iteration_filter_source(from) {
                has_filter = true;
                note_iteration_control_order(1, &mut nearest_control, &mut order_issue);
                udf_filters.push(from);
                from = nodes_feed;
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
        let direct_group_source = self.source_abs_path(from).is_some_and(|source| {
            self.schema_node(&source)
                .is_some_and(|node| matches!(node.kind, SchemaKind::Group { .. }))
        });
        let computed_source = (!direct_group_source)
            .then(|| self.computed_iteration_source(from))
            .flatten();
        let sort_filter_order = if order_issue
            == Some("applies sort after filter, which cannot be represented exactly")
        {
            order_issue = None;
            mapping::SortFilterOrder::FilterThenSort
        } else {
            mapping::SortFilterOrder::SortThenFilter
        };
        IterationFeed {
            source_key: from,
            computed_source,
            sequence_component,
            db_where_component,
            source_suffix,
            filter_expr,
            udf_filters,
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
            sort_filter_order,
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
