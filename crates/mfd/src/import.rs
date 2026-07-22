//! `.mfd` -> `mapping::Project` conversion.
//!
//! The importer never fails on unsupported constructs: it converts what it
//! can and records a warning per skipped piece, because a partial import
//! the user finishes by hand still beats redrawing the mapping.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use ir::SchemaKind;
use mapping::{
    Graph, NamedSource, NamedTarget, Project, Scope, ScopeIteration, ScopeSequence, SequenceExpr,
};

use crate::{MfdError, canonical_function};

mod aggregate;
mod alternatives;
mod db_query;
mod db_where;
mod dynamic_json;
mod dynamic_xml_variable;
mod exception;
mod external_scalar;
mod external_udf;
mod external_xslt;
mod feed;
mod flextext_parser;
mod function;
mod generated_occurrence;
mod graph;
mod group_projection;
mod iteration;
mod join;
mod json_parser;
mod json_serializer;
mod materialize;
mod mixed_content;
mod output_parameter;
mod protobuf_target;
mod recursive;
mod scalar_anchor;
mod scalar_function;
mod schema;
mod scope;
mod sequence_scalar;
mod source;
mod source_node_function;
mod target_iteration;
mod target_mixed_content;
mod target_node_default;
mod target_node_function;
mod target_type_cast;
mod udf;

use db_query::is_routine_catalog;
use function::{
    is_db_function_component, is_isbn_converter_component, is_xbrl_measure_component,
    map_name as map_function_name, read as read_fn_component, read_isbn_converter_component,
};
use graph::{GraphBuilder, read_copy_all_targets, read_edges};
use schema::{
    ComponentFormat, SchemaComponent, note_skipped_library, read_csv_component, read_db_component,
    read_edi_component, read_fixed_width_component, read_flextext_component,
    read_http_get_component, read_json_component, read_pdf_component, read_protobuf_component,
    read_schema_component, read_wsdl_component, read_xbrl_component, read_xlsx_component,
    refine_wsdl_target_schemas, schema_node_at,
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
    let selected_language = wrapper
        .children()
        .find(|node| node.has_tag_name("properties"))
        .and_then(|properties| properties.attribute("SelectedLanguage"))
        .unwrap_or_default()
        .to_ascii_lowercase();

    let mut warnings = Vec::new();
    let mut schema_components = Vec::new();
    let mut fn_components = Vec::new();
    let mut json_serializers = Vec::new();
    let mut json_parsers = Vec::new();
    let mut flextext_parsers = Vec::new();
    let mut output_parameters = Vec::new();
    let mut udf_registry = UdfRegistry::read(&mapping_el, path, &mut warnings);
    let mut udf_calls = Vec::new();
    let mut external_udf_candidates = Vec::new();
    let mut external_scalar_recipes = Vec::new();
    let mut external_xslt_aggregates = Vec::new();
    let mut exception_recipes = Vec::new();
    let mut pending_joins = join::PendingJoins::default();
    let mut skipped_libraries: Vec<String> = Vec::new();
    let source_node_functions = source_node_function::read(&mapping_el);

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
                        Ok(None) => match json_parser::read(&component, &sc) {
                            Ok(Some(parser)) => json_parsers.push(parser),
                            Ok(None) => schema_components.push(sc),
                            Err(reason) => warnings.push(format!(
                                "JSON string parser `{name}` is unsupported: {reason}"
                            )),
                        },
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
                        match read_edi_component(&component, path, &mut warnings) {
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
                            let warning_count = warnings.len();
                            match read_fixed_width_component(&component, &mut warnings)
                                .ok_or_else(|| {
                                    if warnings.len() == warning_count {
                                        "component could not be compiled".to_string()
                                    } else {
                                        "component has invalid inline settings".to_string()
                                    }
                                })
                                .and_then(|schema| {
                                    flextext_parser::read_fixed_width(&component, schema)
                                }) {
                                Ok(parser) => flextext_parsers.push(parser),
                                Err(reason) => {
                                    note_skipped_library(
                                        &mut skipped_libraries,
                                        "text/flf-stringparse",
                                    );
                                    warnings.push(format!(
                                        "skipped fixed-length string parser `{name}`: {reason}"
                                    ));
                                }
                            }
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
                "wsdl" => match read_wsdl_component(&component, path, &mut warnings) {
                    Ok(component) => schema_components.push(component),
                    Err(reason) => {
                        note_skipped_library(&mut skipped_libraries, "wsdl");
                        warnings.push(format!("skipped WSDL message component `{name}`: {reason}"));
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
                    match read_xbrl_component(&component, path, &mut warnings) {
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
                "core" if component.attribute("kind") == Some("18") => {
                    exception_recipes.push(exception::read(&component));
                }
                "core"
                    if component.attribute("kind") == Some("29")
                        && component.descendants().any(|node| {
                            node.has_tag_name("parameter")
                                && node.attribute("usageKind") == Some("variable")
                        }) =>
                {
                    match read_schema_component(&component, path, &mut warnings) {
                        Some(variable) => schema_components.push(variable),
                        None => warnings.push(format!(
                            "skipped core structure variable `{name}`: missing entry tree"
                        )),
                    }
                }
                "core" | "lang" => fn_components.push(read_fn_component(&component)),
                "ferrule"
                    if component.attribute("kind") == Some("5")
                        && canonical_function::is_internal(&name) =>
                {
                    fn_components.push(read_fn_component(&component));
                }
                "ferrule" if recursive::is_component(&component) => {
                    match recursive::read_component(&component) {
                        Ok(function) => fn_components.push(function),
                        Err(reason) => {
                            warnings.push(format!(
                                "skipped ferrule recursive component `{name}`: {reason}"
                            ));
                            let mut function = read_fn_component(&component);
                            function.recursive = Some(function::RecursiveComponent::Invalid);
                            fn_components.push(function);
                        }
                    }
                }
                "edifact" if name == "to-datetime" => {
                    fn_components.push(read_fn_component(&component));
                }
                "xpath2" if map_function_name(&name).is_some() => {
                    fn_components.push(read_fn_component(&component));
                }
                "xpath2"
                    if name == "current-dateTime" && component.attribute("kind") == Some("5") =>
                {
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
                        match external_scalar::read(&component, path, &selected_language) {
                            Ok(Some(recipe)) => external_scalar_recipes.push(recipe),
                            Err(reason) => {
                                note_skipped_library(&mut skipped_libraries, other);
                                warnings.push(format!(
                                    "external {selected_language} function `{name}` is unsupported: {reason}"
                                ));
                            }
                            Ok(None) => match external_xslt::read(&component, path) {
                                Ok(Some(recipe)) => external_xslt_aggregates.push(recipe),
                                Err(reason) => {
                                    note_skipped_library(&mut skipped_libraries, other);
                                    warnings.push(format!(
                                        "external XSLT function `{name}` is unsupported: {reason}"
                                    ));
                                }
                                Ok(None) => {
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
                            },
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
    refine_database_roles(&mut schema_components, &edge_from);
    udf::refine_source_schemas(
        &mut schema_components,
        &udf_calls,
        &udf_registry,
        &edge_from,
    );
    refine_wsdl_target_schemas(&mut schema_components, &fn_components, &edge_from);
    schema::restore_connected_structural_ports(&mut schema_components, &edge_from);
    let copy_all_targets = read_copy_all_targets(&structure, Some(&wrapper));
    refine_copied_json_root_schemas(&mut schema_components, &edge_from, &copy_all_targets);

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

    let connected_outputs = edge_from.values().copied().collect::<BTreeSet<_>>();
    let mut sources: Vec<&SchemaComponent> = schema_components
        .iter()
        .filter(|c| {
            !c.is_variable
                && (c.is_source
                    || c.format == ComponentFormat::Db
                        && c.output_keys
                            .iter()
                            .any(|key| connected_outputs.contains(key)))
        })
        .collect();
    let targets: Vec<&SchemaComponent> = schema_components
        .iter()
        .filter(|component| component.is_target())
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
        .or_else(|| {
            targets
                .iter()
                .copied()
                .find(|component| !component.is_pass_through)
        })
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
        external_scalar_nodes: BTreeMap::new(),
        external_xslt_nodes: BTreeMap::new(),
        json_parser_nodes: BTreeMap::new(),
        flextext_parser_nodes: BTreeMap::new(),
        source_node_function_nodes: BTreeMap::new(),
        claimed_dynamic_ports: BTreeSet::new(),
        query_scope_sources: BTreeSet::new(),
        warned_unscoped_queries: BTreeSet::new(),
        xml_type_conditions,
        edge_from: &edge_from,
        sources: &sources,
        source_names: &source_names,
        intermediates: &intermediates,
        json_serializers: &json_serializers,
        external_scalar_recipes: &external_scalar_recipes,
        external_xslt_aggregates: &external_xslt_aggregates,
        json_parsers: &json_parsers,
        flextext_parsers: &flextext_parsers,
        source_node_functions: &source_node_functions,
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
        if extra.format == ComponentFormat::Db
            || !extra.db_queries.is_empty()
            || extra.options.external_source.is_some()
        {
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

    let root = build_target_scope(
        &mapping_el,
        target,
        &structure,
        path,
        &edge_from,
        &copy_all_targets,
        &mut builder,
    );
    let mut extra_targets = Vec::new();
    for (index, extra) in connected_targets.iter().copied().enumerate().skip(1) {
        let extra_root = build_target_scope(
            &mapping_el,
            extra,
            &structure,
            path,
            &edge_from,
            &copy_all_targets,
            &mut builder,
        );
        let extra_path = if extra_root.output_path().is_some() {
            None
        } else {
            extra
                .output_instance
                .clone()
                .or_else(|| extra.input_instance.clone())
                .or_else(|| default_pass_through_output_path(extra))
        };
        extra_targets.push(NamedTarget {
            name: target_names[index].clone(),
            path: extra_path,
            schema: runtime_target_schema(extra, &edge_from),
            options: extra.options.clone(),
            root: extra_root,
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

    let source_path = primary
        .input_instance
        .clone()
        .or_else(|| builder.static_component_input_path(primary));
    let target_path = if root.output_path().is_some() {
        None
    } else {
        target
            .output_instance
            .clone()
            .or_else(|| target.input_instance.clone())
            .or_else(|| builder.static_target_document_path(target))
            .or_else(|| default_pass_through_output_path(target))
    };
    let failure_rules = exception::lower(exception_recipes, &mut builder);
    warnings.extend(builder.warnings);
    let mut project = Project {
        source: primary.schema.clone(),
        target: runtime_target_schema(target, &edge_from),
        source_path,
        target_path,
        source_options: primary.options.clone(),
        target_options: target.options.clone(),
        extra_sources,
        extra_targets,
        failure_rules,
        graph: builder.graph,
        root,
    };
    project.prune_unreachable_nodes();
    Ok(Imported { project, warnings })
}

fn default_pass_through_output_path(component: &SchemaComponent) -> Option<String> {
    if !component.is_pass_through {
        return None;
    }
    let stem = if component.name.trim().is_empty() {
        &component.schema.name
    } else {
        &component.name
    };
    Some(format!("{stem}.xml"))
}

/// Database components can expose read and write ports in one visual component.
/// Their entry counts alone cannot determine the role when both sides have the
/// same shape, so connected table inputs decide target ownership once the graph
/// is available. A connected output still admits the same component as a source.
fn refine_database_roles(components: &mut [SchemaComponent], edge_from: &BTreeMap<u32, u32>) {
    for component in components {
        if component.format == ComponentFormat::Db
            && component.db_queries.is_empty()
            && component
                .input_keys
                .iter()
                .any(|key| edge_from.contains_key(key))
        {
            component.is_source = false;
        }
    }
}

fn runtime_target_schema(
    component: &SchemaComponent,
    edge_from: &BTreeMap<u32, u32>,
) -> ir::SchemaNode {
    if component.format != ComponentFormat::Db || component.schema.repeating {
        return component.schema.clone();
    }
    let selected_tables = component
        .input_keys
        .iter()
        .filter(|key| edge_from.contains_key(key))
        .filter_map(|key| component.ports.get(key).and_then(|path| path.first()))
        .cloned()
        .collect::<BTreeSet<_>>();
    if selected_tables.is_empty() {
        return component.schema.clone();
    }
    let mut schema = component.schema.clone();
    let SchemaKind::Group { children, .. } = &mut schema.kind else {
        return schema;
    };
    let mut retained = BTreeSet::new();
    children.retain(|child| {
        selected_tables.contains(&child.name) && retained.insert(child.name.clone())
    });
    schema
}

fn refine_copied_json_root_schemas(
    components: &mut [SchemaComponent],
    edge_from: &BTreeMap<u32, u32>,
    copy_all_targets: &BTreeSet<u32>,
) {
    let replacements = components
        .iter()
        .enumerate()
        .filter(|(_, target)| {
            !target.is_source
                && target.format == ComponentFormat::Json
                && matches!(target.schema.kind, SchemaKind::Scalar { .. })
        })
        .filter_map(|(target_index, target)| {
            let feed = target
                .ports
                .iter()
                .find(|(input, path)| path.is_empty() && copy_all_targets.contains(input))
                .and_then(|(input, _)| edge_from.get(input))?;
            let source_schema = components.iter().find_map(|source| {
                let path = source.ports.get(feed)?;
                let node = schema_node_at(&source.schema, path)?;
                (!node.repeating && matches!(node.kind, SchemaKind::Group { .. }))
                    .then(|| node.clone())
            })?;
            Some((target_index, source_schema))
        })
        .collect::<Vec<_>>();
    for (target_index, schema) in replacements {
        components[target_index].schema = schema;
    }
}

fn build_target_scope(
    mapping: &roxmltree::Node<'_, '_>,
    target: &SchemaComponent,
    structure: &roxmltree::Node<'_, '_>,
    mfd_path: &Path,
    edge_from: &BTreeMap<u32, u32>,
    copy_all_targets: &BTreeSet<u32>,
    builder: &mut GraphBuilder<'_>,
) -> Scope {
    builder.rejected_join_paths.clear();
    let mut scopes = ScopeBuilder {
        root: Scope::default(),
        anchors: BTreeMap::new(),
    };
    let dynamic_document = target
        .ports
        .iter()
        .find(|(_, path)| path.as_slice() == [schema::TARGET_DOCUMENT_PATH_PORT])
        .and_then(|(input, _)| edge_from.get(input).copied())
        .and_then(|feed| {
            if builder.static_string_feed(feed).is_some() {
                return None;
            }
            let driver = builder.computed_iteration_source(feed);
            match driver {
                Some(driver) => {
                    builder.note_framed_prefixes(&driver);
                    let context = builder.context_path(&driver);
                    scopes.add_iteration(
                        &[],
                        &context,
                        scope::IterationNodes::default(),
                        mapping::IterationOutput::Repeated,
                    );
                    Some(feed)
                }
                None => None,
            }
        });
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
        if target_path.as_slice() == [schema::TARGET_DOCUMENT_PATH_PORT] {
            continue;
        }
        if let Some((position, field)) = schema::split_singleton_port(target_path) {
            csv_singleton_bindings
                .entry(position)
                .or_insert_with(Vec::new)
                .push((field.to_string(), from));
            continue;
        }
        let node_kind = schema_node_at(&target.schema, target_path);
        if let Some(node) = node_kind
            && recursive::accept_target(target_path, node, from, builder, &mut scopes)
        {
            continue;
        }
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
                Some(target) => bindings.push((target, from, inpkey)),
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
    order_repeating_scalar_bindings(target, &mut bindings);
    udf::structured::prepare_target_frames(&structured_udf_targets, builder);
    generated_occurrence::infer(target, builder, &mut iterations);
    iterations.sort_by_key(|iteration| iteration.target_path.len());
    let explicit_iteration_paths = iterations
        .iter()
        .map(|iteration| iteration.target_path.clone())
        .collect::<BTreeSet<_>>();
    join::prepare_iterations(&iterations, builder, &mut scopes);
    for iteration in &iterations {
        for feed_key in std::iter::once(iteration.feed)
            .chain(iteration.additional_feeds.iter().map(|(feed, _, _)| *feed))
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
    if let Some(feed) = dynamic_document
        && let Some(node) = builder.binding_node_at_anchor(feed, &[], &[])
        && !scopes.root.set_output_path(Some(node))
    {
        builder.warnings.push(
            "target FileInstance path conflicts with another root iteration; dynamic document output was skipped"
                .to_string(),
        );
    }
    let mut skipped_iteration_paths =
        target_iteration::build(iterations, target, &mut bindings, builder, &mut scopes);
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
    target_mixed_content::install(target, builder, &mut scopes);
    for (target_leaf, from, _) in bindings {
        let target_path = target_leaf.path();
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
        if install_repeating_scalar_iteration(target, &target_leaf, from, builder, &mut scopes) {
            continue;
        }
        let active_anchor = scopes.enclosing_anchor(&target_path);
        let Some(node) = builder.binding_node_at_anchor(from, &target_path, &active_anchor) else {
            continue;
        };
        scopes.add_binding(target_leaf, node);
    }
    dynamic_json::build_target(dynamic_target, target, builder, &mut scopes);
    compose_csv_target_rows(csv_singleton_bindings, builder, &mut scopes);
    target_node_default::install(target, structure, builder, &mut scopes);
    target_node_function::install(mapping, target, structure, mfd_path, builder, &mut scopes);
    target_type_cast::install(target, structure, mfd_path, builder, &mut scopes);
    group_projection::install_optional_text_occurrences(target, builder, &mut scopes);
    scopes.root
}

fn install_repeating_scalar_iteration(
    target_component: &SchemaComponent,
    target: &TargetLeaf,
    feed: u32,
    builder: &mut GraphBuilder<'_>,
    scopes: &mut ScopeBuilder,
) -> bool {
    let target_path = target.path();
    if !schema_node_at(&target_component.schema, &target_path)
        .is_some_and(|node| node.repeating && matches!(node.kind, SchemaKind::Scalar { .. }))
    {
        return false;
    }
    let Some(source_path) = builder
        .source_abs_path(feed)
        .map(|path| builder.source_value_path(path.source, path.path))
    else {
        return false;
    };
    if !builder
        .schema_node(&source_path)
        .is_some_and(|node| node.repeating && matches!(node.kind, SchemaKind::Scalar { .. }))
    {
        return false;
    }
    let source_abs = builder.context_path(&source_path);
    builder.note_framed_prefixes(&source_path);
    let value = builder.source_field(Some(source_abs.clone()), Vec::new());
    scopes.add_iteration(
        &target_path,
        &source_abs,
        scope::IterationNodes::default(),
        mapping::IterationOutput::Repeated,
    );
    scopes.ensure_scope(&target_path).construction = mapping::ScopeConstruction::Scalar { value };
    true
}

/// Repeated scalar target entries can be cloned several times under the same
/// schema path. Their numeric pin keys are identifiers, not an occurrence
/// order, so preserve the entry-tree branch order recorded by the schema
/// reader before the bindings are distributed into concatenated scopes.
fn order_repeating_scalar_bindings(
    target: &SchemaComponent,
    bindings: &mut [(TargetLeaf, u32, u32)],
) {
    let mut positions = BTreeMap::<Vec<String>, Vec<usize>>::new();
    for (index, (binding, _, _)) in bindings.iter().enumerate() {
        let path = binding.path();
        if schema_node_at(&target.schema, &path)
            .is_some_and(|node| node.repeating && matches!(node.kind, SchemaKind::Scalar { .. }))
        {
            positions.entry(path).or_default().push(index);
        }
    }
    for positions in positions.values().filter(|positions| positions.len() > 1) {
        let mut ordered = positions
            .iter()
            .map(|index| bindings[*index].clone())
            .collect::<Vec<_>>();
        ordered.sort_by(|left, right| {
            target
                .input_ancestors
                .get(&left.2)
                .cmp(&target.input_ancestors.get(&right.2))
                .then_with(|| left.2.cmp(&right.2))
        });
        for (position, binding) in positions.iter().copied().zip(ordered) {
            bindings[position] = binding;
        }
    }
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
    fn sequence_expr(&mut self, idx: usize) -> Option<SequenceExpr> {
        let item = self.sequence_item(idx);
        if let Some(function::RecursiveComponent::Collect {
            collection,
            children,
            descent_value,
            values,
            value,
        }) = self
            .fn_components
            .get(idx)
            .and_then(|component| component.recursive.clone())
        {
            let source = self
                .input_feed(idx, 0)
                .and_then(|feed| self.source_abs_path(feed))?;
            if self.context_path(&source) != collection {
                return None;
            }
            let prefix = self
                .input_feed(idx, 1)
                .and_then(|feed| self.sequence_scalar_input(feed))?;
            let separator = self
                .input_feed(idx, 2)
                .and_then(|feed| self.sequence_scalar_input(feed))?;
            return Some(SequenceExpr::RecursiveCollect {
                collection,
                children,
                descent_value,
                values,
                value,
                prefix,
                separator,
                item,
            });
        }
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
            "tokenize-regexp" => {
                let input = self
                    .input_feed(idx, 0)
                    .and_then(|feed| self.sequence_scalar_input(feed))?;
                let pattern = self
                    .input_feed(idx, 1)
                    .and_then(|feed| self.sequence_scalar_input(feed))?;
                let flags = self
                    .input_feed(idx, 2)
                    .and_then(|feed| self.sequence_scalar_input(feed));
                SequenceExpr::TokenizeRegex {
                    input,
                    pattern,
                    flags,
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
