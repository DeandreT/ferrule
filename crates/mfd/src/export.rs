//! `mapping::Project` -> `.mfd` conversion for the supported subset, with
//! generated schemas and component families selected from instance paths.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::Path;

use ir::{SchemaKind, SchemaNode};
use mapping::{FormatOptions, IterationOutput, NodeId, Project, Scope, ScopeConstruction};

use crate::MfdError;

mod artifact;
mod auto_number;
mod concatenation;
mod database;
mod dynamic_json;
mod edi;
mod exception;
mod external_source;
mod flextext;
mod function;
mod join;
mod mapped_sequence;
mod node;
mod pdf;
mod position;
mod preflight;
mod protobuf;
mod recursive;
mod schema;
mod scope;
mod sequence;
mod source;
#[cfg(test)]
mod tests;
mod udf;
mod wsdl;
mod xbrl;
mod xlsx;

use artifact::write_artifacts;
use mapped_sequence::{ScopePlans, preflight_mapped_sequences, render_edge_metadata};
use position::connect_position_roots;
use schema::{
    KeyAlloc, PortTree, RenderedSchemaComponent, Side, SideFormat, db_datasource_name,
    render_schema_component, side_format, xml_escape,
};

struct TargetExport<'a> {
    component_name: &'a str,
    schema: &'a SchemaNode,
    path: Option<&'a str>,
    options: &'a FormatOptions,
    root: &'a Scope,
    format: SideFormat,
    root_iterable: bool,
    force_root_port: bool,
    ports: PortTree,
    branches: concatenation::TargetBranches,
    mapped_scope_plans: ScopePlans,
    dynamic_json: Option<dynamic_json::TargetPlan>,
    component_uid: u32,
    sibling_suffix: String,
    default_output: bool,
    document_path_port: Option<u32>,
}

struct TargetSpec<'a> {
    component_name: &'a str,
    schema: &'a SchemaNode,
    path: &'a Option<String>,
    options: &'a FormatOptions,
    root: &'a Scope,
}

impl<'a> TargetExport<'a> {
    fn build(
        project: &'a Project,
        sources: &source::SourceExports<'_>,
        spec: TargetSpec<'a>,
        index: usize,
        keys: &mut KeyAlloc,
    ) -> Result<Self, MfdError> {
        let mut format = side_format(spec.path, spec.options);
        let dynamic_json =
            dynamic_json::TargetPlan::build(spec.schema, spec.root, sources, &project.graph, keys)?;
        if dynamic_json.is_some() && format != SideFormat::Json {
            if spec.path.is_some() || !dynamic_json::target_format_is_implicit(spec.options) {
                return Err(MfdError::Unsupported(
                    "computed JSON property targets conflict with a non-JSON format".into(),
                ));
            }
            format = SideFormat::Json;
        }
        let mapped_scope_plans = match dynamic_json.as_ref() {
            Some(plan) => plan.static_root().map_or_else(
                || Ok(ScopePlans::default()),
                |root| {
                    preflight_mapped_sequences(&project.graph, sources, spec.schema, root, format)
                },
            )?,
            None => {
                preflight_mapped_sequences(&project.graph, sources, spec.schema, spec.root, format)?
            }
        };
        let component_uid = u32::try_from(sources.len() + index + 2).map_err(|_| {
            MfdError::Unsupported("too many target components for .mfd export".to_string())
        })?;
        let document_path_port = spec.root.output_path().map(|_| keys.next());
        let copy_document_root = spec.root.construction == ScopeConstruction::CopyCurrentSource;
        let force_root_port = copy_document_root
            || recursive::requires_root_port(spec.root)
            || spec.root.iteration_output == IterationOutput::First
                && spec.root.source() == Some(&[]);
        let mut explicit_text_ports = explicit_scope_text_ports(spec.schema, spec.root);
        explicit_text_ports.extend(mapped_scope_plans.explicit_text_ports());
        explicit_text_ports.extend(xbrl::explicit_text_ports(spec.schema, spec.options));
        Ok(Self {
            component_name: spec.component_name,
            schema: spec.schema,
            path: spec.path.as_deref(),
            options: spec.options,
            root: spec.root,
            format,
            root_iterable: matches!(
                format,
                SideFormat::Csv | SideFormat::FixedWidth | SideFormat::Xlsx | SideFormat::Db
            ) || (format == SideFormat::Json && spec.schema.repeating),
            force_root_port,
            ports: PortTree::build_with_explicit_text(spec.schema, keys, &explicit_text_ports),
            branches: concatenation::TargetBranches::build(
                spec.schema,
                spec.root,
                &project.graph,
                keys,
                &explicit_text_ports,
            ),
            mapped_scope_plans,
            dynamic_json,
            component_uid,
            sibling_suffix: if index == 0 {
                "target".to_string()
            } else {
                format!("target-{}", index + 1)
            },
            default_output: index == 0,
            document_path_port,
        })
    }
}

fn explicit_scope_text_ports(schema: &SchemaNode, root: &Scope) -> BTreeSet<Vec<String>> {
    fn schema_node_at<'a>(schema: &'a SchemaNode, path: &[String]) -> Option<&'a SchemaNode> {
        path.iter()
            .try_fold(schema, |node, segment| node.child(segment))
    }

    fn collect(
        schema: &SchemaNode,
        scope: &Scope,
        path: &mut Vec<String>,
        ports: &mut BTreeSet<Vec<String>>,
    ) {
        let pushed = !scope.target_field.is_empty();
        if pushed {
            path.push(scope.target_field.clone());
        }
        let maps_text = scope
            .bindings
            .iter()
            .any(|binding| binding.target_field == ir::XML_TEXT_FIELD);
        let has_element_content =
            schema_node_at(schema, path).is_some_and(|node| match &node.kind {
                SchemaKind::Group { children, .. } => {
                    children.iter().any(|child| !child.text && !child.attribute)
                }
                SchemaKind::Scalar { .. } => false,
            });
        if maps_text && has_element_content {
            let mut text = path.clone();
            text.push(ir::XML_TEXT_FIELD.to_string());
            ports.insert(text);
        }
        for child in &scope.children {
            collect(schema, child, path, ports);
        }
        if pushed {
            path.pop();
        }
    }

    let mut ports = BTreeSet::new();
    collect(schema, root, &mut Vec::new(), &mut ports);
    ports
}

/// Writes a MapForce design and generated schema siblings, returning warnings
/// for project features that have no export representation.
pub fn export(project: &Project, path: &Path) -> Result<Vec<String>, MfdError> {
    let mut warnings = Vec::new();

    preflight::validate(project)?;

    let mut keys = KeyAlloc { next: 1 };
    let sources = source::SourceExports::build(project, &mut keys)?;
    let dynamic_sources = dynamic_json::SourcePlan::build(project, &sources, &mut keys)?;
    let mut targets = Vec::with_capacity(project.extra_targets.len() + 1);
    targets.push(TargetExport::build(
        project,
        &sources,
        TargetSpec {
            component_name: &project.target.name,
            schema: &project.target,
            path: &project.target_path,
            options: &project.target_options,
            root: &project.root,
        },
        0,
        &mut keys,
    )?);
    for (index, target) in project.extra_targets.iter().enumerate() {
        targets.push(TargetExport::build(
            project,
            &sources,
            TargetSpec {
                component_name: &target.name,
                schema: &target.schema,
                path: &target.path,
                options: &target.options,
                root: &target.root,
            },
            index + 1,
            &mut keys,
        )?);
    }
    let primary_target = &targets[0];
    let mixed_database_pairs = pair_mixed_databases(&sources, &targets);

    let mut node_out_key: BTreeMap<NodeId, u32> = BTreeMap::new();
    let mut components = String::new();
    let mut edges: Vec<(u32, u32)> = Vec::new();
    let mut structural_edges = BTreeSet::new();
    let mut uid = u32::try_from(sources.len() + targets.len() + 2)
        .map_err(|_| MfdError::Unsupported("too many components for .mfd export".to_string()))?
        .max(100);
    let user_functions = udf::Exports::build(project, &mut keys, &mut uid)?;
    let joins = join::render(join::RenderJoinArgs {
        project,
        sources: &sources,
        target_ports: &primary_target.ports,
        target_root_iterable: primary_target.root_iterable,
        keys: &mut keys,
        uid: &mut uid,
        node_out_key: &mut node_out_key,
        components: &mut components,
        edges: &mut edges,
        warnings: &mut warnings,
    });
    let mut blocked_nodes = dynamic_sources.owned_nodes().clone();
    for target in &targets {
        blocked_nodes.extend(target.mapped_scope_plans.absorbed_nodes());
    }
    recursive::seed_context_fields(project, &sources, &mut node_out_key);
    let node::RenderedNodes {
        position_inputs,
        sequence_exists_pins,
        siblings: node_siblings,
    } = node::render(node::RenderArgs {
        project,
        sources: &sources,
        joins: &joins,
        keys: &mut keys,
        uid: &mut uid,
        node_out_key: &mut node_out_key,
        components: &mut components,
        edges: &mut edges,
        structural_edges: &mut structural_edges,
        warnings: &mut warnings,
        blocked_nodes: &blocked_nodes,
        mfd_path: path,
        user_functions: &user_functions,
    });
    dynamic_sources.render_nodes(
        &mut keys,
        &mut uid,
        &mut node_out_key,
        &mut components,
        &mut edges,
    )?;
    for target in &targets {
        let (Some(node), Some(to)) = (target.root.output_path(), target.document_path_port) else {
            continue;
        };
        let from = node_out_key.get(&node).copied().ok_or_else(|| {
            MfdError::Unsupported(format!(
                "dynamic target path references unexported node {node}"
            ))
        })?;
        edges.push((from, to));
    }
    for source in sources.iter() {
        let Some(node) = source.dynamic_path_node else {
            continue;
        };
        let from = node_out_key.get(&node).copied().ok_or_else(|| {
            MfdError::Unsupported(format!(
                "dynamic extra source `{}` references an unexported path node {node}",
                source.name
            ))
        })?;
        let to = source
            .ports
            .required_key_for_abs(&[], "dynamic source path input")?;
        edges.push((from, to));
    }

    let mut scope_components = String::new();
    let mut position_contexts: BTreeMap<NodeId, Option<u32>> = BTreeMap::new();
    for pins in sequence_exists_pins {
        match node_out_key.get(&pins.predicate) {
            Some(&predicate_output) => edges.push((predicate_output, pins.filter_predicate)),
            None => warnings.push(format!(
                "sequence-exists predicate references unexported node {}; connection skipped",
                pins.predicate
            )),
        }
        connect_position_roots(
            [pins.predicate],
            None,
            true,
            pins.sequence_output,
            &project.graph,
            &position_inputs,
            &mut position_contexts,
            &mut edges,
            &mut warnings,
        );
    }
    let mut exception_branches = exception::Branches::new(project);
    for (target_index, target) in targets.iter().enumerate() {
        let prior_position_contexts = position_contexts.clone();
        let static_root = target
            .dynamic_json
            .as_ref()
            .map_or(Some(target.root), dynamic_json::TargetPlan::static_root);
        if let Some(static_root) = static_root {
            recursive::render_construction(recursive::RenderArgs {
                scope: static_root,
                sources: &sources,
                target_ports: &target.ports,
                node_out_key: &node_out_key,
                keys: &mut keys,
                uid: &mut uid,
                components: &mut scope_components,
                edges: &mut edges,
            })?;
            scope::connect(scope::ConnectArgs {
                scope: static_root,
                sources: &sources,
                target_ports: &target.ports,
                target_root_iterable: target.root_iterable,
                graph: &project.graph,
                node_out_key: &node_out_key,
                position_inputs: &position_inputs,
                position_contexts: &mut position_contexts,
                keys: &mut keys,
                uid: &mut uid,
                components: &mut scope_components,
                edges: &mut edges,
                warnings: &mut warnings,
                structural_edges: &mut structural_edges,
                mapped_scope_plans: &target.mapped_scope_plans,
                joins: &joins,
                target_branches: &target.branches,
                exception_branches: &mut exception_branches,
            });
        }
        if let Some(plan) = &target.dynamic_json {
            plan.connect(dynamic_json::ConnectArgs {
                sources: &sources,
                node_out_key: &node_out_key,
                keys: &mut keys,
                uid: &mut uid,
                components: &mut scope_components,
                edges: &mut edges,
            })?;
        }
        if target_index > 0
            && position_contexts.iter().any(|(node, context)| {
                context.is_none()
                    && prior_position_contexts
                        .get(node)
                        .is_none_or(|prior| prior.is_some())
            })
        {
            return Err(MfdError::Unsupported(
                "an additional target evaluates a position node in more than one iteration context"
                    .to_string(),
            ));
        }
    }
    exception_branches.render(exception::RenderArgs {
        graph: &project.graph,
        node_out_key: &node_out_key,
        position_contexts: &position_contexts,
        keys: &mut keys,
        uid: &mut uid,
        components: &mut scope_components,
        edges: &mut edges,
    })?;
    for (id, input) in &position_inputs {
        if !position_contexts.contains_key(id) {
            warnings.push(format!(
                "position node {id} has no matching iteration scope; its context input {input} is unconnected"
            ));
        }
    }
    components.push_str(&scope_components);

    // Database components reference a mapping-level datasource.
    let mut datasources: Vec<(String, String, BTreeSet<String>)> = Vec::new();
    for (format, instance, schema) in sources
        .iter()
        .map(|source| (source.format, source.path, source.schema))
        .chain(
            targets
                .iter()
                .map(|target| (target.format, target.path, target.schema)),
        )
    {
        if format == SideFormat::Db
            && let Some(conn) = instance
        {
            let name = db_datasource_name(Some(conn));
            let relations = database::local_relation_elements(schema)?;
            if let Some((_, _, existing)) = datasources
                .iter_mut()
                .find(|(existing, _, _)| *existing == name)
            {
                existing.extend(relations);
            } else {
                datasources.push((name, conn.to_string(), relations));
            }
        }
    }
    let resources = if datasources.is_empty() {
        "\t<resources/>\n".to_string()
    } else {
        let mut resources = String::from("\t<resources>\n\t\t<datasources>\n");
        for (name, conn, relations) in &datasources {
            let connection = if relations.is_empty() {
                format!(
                    "\t\t\t\t<database_connection database_kind=\"SQLite\" import_kind=\"SQLite\" ConnectionString=\"{}\" name=\"{}\" path=\"{}\"/>\n",
                    xml_escape(conn),
                    xml_escape(name),
                    xml_escape(name),
                )
            } else {
                format!(
                    "\t\t\t\t<database_connection database_kind=\"SQLite\" import_kind=\"SQLite\" ConnectionString=\"{}\" name=\"{}\" path=\"{}\">\n\
                     \t\t\t\t\t<LocalRelationsStorage>\n{}\
                     \t\t\t\t\t</LocalRelationsStorage>\n\
                     \t\t\t\t</database_connection>\n",
                    xml_escape(conn),
                    xml_escape(name),
                    xml_escape(name),
                    relations.iter().cloned().collect::<String>(),
                )
            };
            let _ = write!(
                resources,
                "\t\t\t<datasource name=\"{0}\">\n\
                 \t\t\t\t<properties JDBCDriver=\"org.sqlite.JDBC\" JDBCDatabaseURL=\"jdbc:sqlite:{1}\" DBDataSource=\"{1}\" DBCatalog=\"main\"/>\n\
                 {connection}\
                 \t\t\t</datasource>\n",
                xml_escape(name),
                xml_escape(conn),
            );
        }
        resources.push_str("\t\t</datasources>\n\t</resources>\n");
        resources
    };
    let used_ports = edges
        .iter()
        .flat_map(|(from, to)| [*from, *to])
        .collect::<BTreeSet<_>>();

    let mut out = String::new();
    let _ = write!(
        out,
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <mapping version=\"22\" ferrule-primary-source=\"{}\">\n\
         {resources}\
         \t<component name=\"defaultmap\" uid=\"1\" editable=\"1\">\n\
         \t\t{}\n\
         \t\t<structure>\n\
         \t\t\t<children>\n",
        sources.primary_component_uid(),
        wsdl::mapping_properties(project)
            .unwrap_or_else(|| "<properties SelectedLanguage=\"builtin\"/>".to_string())
    );
    let mut source_components = Vec::with_capacity(sources.len());
    for (source_index, source) in sources.iter().enumerate() {
        if mixed_database_pairs
            .iter()
            .any(|(paired_source, _)| *paired_source == source_index)
        {
            continue;
        }
        let rendered = if let Some(rendered) =
            dynamic_json::render_source(dynamic_json::RenderSourceArgs {
                plan: &dynamic_sources,
                source_index,
                schema: source.schema,
                ports: &source.ports,
                instance_path: source.path,
                json_lines: source.options.json_lines,
                mfd_path: path,
                component_name: source.name,
                component_uid: source.component_uid,
                sibling_suffix: &source.sibling_suffix,
            }) {
            rendered
        } else if source.options.external_source.is_some() {
            if matches!(
                source
                    .options
                    .external_source
                    .as_ref()
                    .map(mapping::ExternalSourceOptions::origin),
                Some(mapping::ExternalSourceOrigin::UserFunction { .. })
            ) {
                external_source::render_user_function(external_source::RenderUserFunctionArgs {
                    component_name: source.name,
                    schema: source.schema,
                    ports: &source.ports,
                    options: source.options,
                    instance_path: source.path,
                    mfd_path: path,
                    sibling_suffix: &source.sibling_suffix,
                    uid: source.component_uid,
                })?
            } else {
                let request_suffix = format!("{}-request", source.sibling_suffix);
                let request_schema =
                    external_source::request_schema_artifact(source.options, path, &request_suffix);
                let xml = external_source::render_http_post(external_source::RenderHttpPostArgs {
                    component_name: source.name,
                    response_schema: source.schema,
                    response_ports: &source.ports,
                    request_ports: source.request_ports.as_ref(),
                    request_schema_file: request_schema
                        .as_ref()
                        .map(|artifact| artifact.file_name.as_str()),
                    options: source.options,
                    url: source.path,
                    uid: source.component_uid,
                })?;
                RenderedSchemaComponent {
                    xml,
                    siblings: request_schema
                        .into_iter()
                        .map(|artifact| schema::GeneratedSibling {
                            path: artifact.path,
                            contents: artifact.contents,
                        })
                        .collect(),
                }
            }
        } else {
            let root_key = source.ports.key_for_abs(&[]);
            render_schema_component(
                source.schema,
                source.format,
                &source.ports,
                Side::Source,
                source.path,
                source.options,
                path,
                root_key.is_some_and(|key| used_ports.contains(&key)),
                source.dynamic_path_node.is_some(),
                None,
                source.name,
                source.component_uid,
                &source.sibling_suffix,
                false,
                &used_ports,
                source.document_path_port,
            )?
        };
        out.push_str(&rendered.xml);
        source_components.push(rendered);
    }
    for &(source_index, target_index) in &mixed_database_pairs {
        let source = sources.iter().nth(source_index).ok_or_else(|| {
            MfdError::Unsupported("internal mixed database source is missing".into())
        })?;
        let target = targets.get(target_index).ok_or_else(|| {
            MfdError::Unsupported("internal mixed database target is missing".into())
        })?;
        let rendered = database::render_mixed(database::RenderMixedArgs {
            schema: source.schema,
            source_ports: &source.ports,
            target_ports: &target.ports,
            target_branches: &target.branches,
            instance_path: source.path,
            component_name: target.component_name,
            component_uid: source.component_uid,
            default_output: target.default_output,
            used_ports: &used_ports,
        })?;
        out.push_str(&rendered.xml);
        source_components.push(rendered);
    }
    let mut target_components = Vec::with_capacity(targets.len());
    for (target_index, target) in targets.iter().enumerate() {
        if mixed_database_pairs
            .iter()
            .any(|(_, paired_target)| *paired_target == target_index)
        {
            continue;
        }
        let rendered = if let Some(plan) = &target.dynamic_json {
            dynamic_json::render_target(dynamic_json::RenderTargetArgs {
                plan,
                schema: target.schema,
                ports: &target.ports,
                instance_path: target.path,
                json_lines: target.options.json_lines,
                mfd_path: path,
                component_name: target.component_name,
                component_uid: target.component_uid,
                sibling_suffix: &target.sibling_suffix,
                default_output: target.default_output,
            })
        } else {
            render_schema_component(
                target.schema,
                target.format,
                &target.ports,
                Side::Target,
                target.path,
                target.options,
                path,
                target.force_root_port,
                false,
                Some(&target.branches),
                target.component_name,
                target.component_uid,
                &target.sibling_suffix,
                target.default_output,
                &used_ports,
                target.document_path_port,
            )?
        };
        out.push_str(&rendered.xml);
        target_components.push(rendered);
    }
    out.push_str(&components);
    let (structural_edge_keys, edge_metadata) = render_edge_metadata(&structural_edges, &mut keys);
    let _ = write!(
        out,
        "\t\t\t</children>\n\t\t\t<graph directed=\"1\">\n{edge_metadata}\t\t\t\t<vertices>\n"
    );
    let mut by_from: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
    for (from, to) in edges {
        by_from.entry(from).or_default().push(to);
    }
    for (from, tos) in by_from {
        let _ = write!(
            out,
            "\t\t\t\t\t<vertex vertexkey=\"{from}\">\n\t\t\t\t\t\t<edges>\n"
        );
        for to in tos {
            if let Some(edge_key) = structural_edge_keys.get(&(from, to)) {
                let _ = writeln!(
                    out,
                    "\t\t\t\t\t\t\t<edge vertexkey=\"{to}\" edgekey=\"{edge_key}\"/>"
                );
            } else {
                let _ = writeln!(out, "\t\t\t\t\t\t\t<edge vertexkey=\"{to}\"/>");
            }
        }
        out.push_str("\t\t\t\t\t\t</edges>\n\t\t\t\t\t</vertex>\n");
    }
    out.push_str("\t\t\t\t</vertices>\n\t\t\t</graph>\n\t\t</structure>\n\t</component>\n");
    out.push_str(user_functions.declarations());
    out.push_str("</mapping>\n");

    let mut artifacts = Vec::new();
    artifacts.extend(
        node_siblings
            .into_iter()
            .map(|sibling| (sibling.path, sibling.contents)),
    );
    for sibling in source_components
        .into_iter()
        .chain(target_components)
        .flat_map(|component| component.siblings)
    {
        artifacts.push((sibling.path, sibling.contents));
    }
    // Publish the design after its schema siblings reach their final paths.
    artifacts.push((path.to_path_buf(), out));
    write_artifacts(artifacts)?;
    Ok(warnings)
}

fn pair_mixed_databases(
    sources: &source::SourceExports<'_>,
    targets: &[TargetExport<'_>],
) -> Vec<(usize, usize)> {
    let mut claimed_targets = BTreeSet::new();
    sources
        .iter()
        .enumerate()
        .filter_map(|(source_index, source)| {
            if source.format != SideFormat::Db || source.dynamic_path_node.is_some() {
                return None;
            }
            let target_index = targets
                .iter()
                .enumerate()
                .find_map(|(target_index, target)| {
                    (!claimed_targets.contains(&target_index)
                        && target.format == SideFormat::Db
                        && source.path == target.path
                        && database_target_view_compatible(source.schema, target.schema)
                        && source.options == target.options)
                        .then_some(target_index)
                })?;
            claimed_targets.insert(target_index);
            Some((source_index, target_index))
        })
        .collect()
}

fn database_target_view_compatible(source: &ir::SchemaNode, target: &ir::SchemaNode) -> bool {
    if source == target {
        return true;
    }
    let Some(source) = schema::db_layout(source) else {
        return false;
    };
    let Some(target) = schema::db_layout(target) else {
        return false;
    };
    let source_tables = match source {
        schema::DbLayout::Table(table) => vec![table],
        schema::DbLayout::Database(tables) => tables.iter().collect(),
    };
    let target_tables = match target {
        schema::DbLayout::Table(table) => vec![table],
        schema::DbLayout::Database(tables) => tables.iter().collect(),
    };
    target_tables.iter().all(|target| {
        source_tables
            .iter()
            .any(|source| source.name == target.name && source.kind == target.kind)
    })
}
