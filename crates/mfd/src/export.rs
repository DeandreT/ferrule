//! `mapping::Project` -> `.mfd` conversion for the supported subset, with
//! generated schemas and component families selected from instance paths.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::Path;

use ir::SchemaNode;
use mapping::{FormatOptions, IterationOutput, NodeId, Project, Scope, ScopeConstruction};

use crate::MfdError;

mod artifact;
mod concatenation;
mod external_source;
mod flextext;
mod function;
mod join;
mod mapped_sequence;
mod node;
mod position;
mod preflight;
mod schema;
mod scope;
mod sequence;
#[cfg(test)]
mod tests;
mod xbrl;

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
    component_uid: u32,
    sibling_suffix: String,
    default_output: bool,
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
        spec: TargetSpec<'a>,
        index: usize,
        keys: &mut KeyAlloc,
    ) -> Result<Self, MfdError> {
        let format = side_format(spec.path, spec.options);
        let mapped_scope_plans = preflight_mapped_sequences(
            &project.graph,
            &project.source,
            spec.schema,
            spec.root,
            format,
        )?;
        let component_uid = u32::try_from(index + 3).map_err(|_| {
            MfdError::Unsupported("too many target components for .mfd export".to_string())
        })?;
        let copy_document_root = spec.root.construction == ScopeConstruction::CopyCurrentSource;
        let force_root_port = copy_document_root
            || spec.root.iteration_output == IterationOutput::First
                && spec.root.source() == Some(&[]);
        let mut explicit_text_ports = mapped_scope_plans.explicit_text_ports();
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
                keys,
                &explicit_text_ports,
            ),
            mapped_scope_plans,
            component_uid,
            sibling_suffix: if index == 0 {
                "target".to_string()
            } else {
                format!("target-{}", index + 1)
            },
            default_output: index == 0,
        })
    }
}

/// Writes a MapForce design and generated schema siblings, returning warnings
/// for project features that have no export representation.
pub fn export(project: &Project, path: &Path) -> Result<Vec<String>, MfdError> {
    let mut warnings = Vec::new();

    preflight::validate(project)?;

    if !project.extra_sources.is_empty() {
        warnings.push(
            "extra sources are not exported; MapForce multi-input wiring must be redone"
                .to_string(),
        );
    }

    let source_format = if project.source_options.http_get.is_some() {
        SideFormat::Xml
    } else {
        side_format(&project.source_path, &project.source_options)
    };
    let mut keys = KeyAlloc { next: 1 };
    let source_explicit_text = xbrl::explicit_text_ports(&project.source, &project.source_options);
    let source_ports =
        PortTree::build_with_explicit_text(&project.source, &mut keys, &source_explicit_text);
    let external_request_ports = external_source::request_ports(&project.source_options, &mut keys);
    let mut targets = Vec::with_capacity(project.extra_targets.len() + 1);
    targets.push(TargetExport::build(
        project,
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

    let mut node_out_key: BTreeMap<NodeId, u32> = BTreeMap::new();
    let mut components = String::new();
    let mut edges: Vec<(u32, u32)> = Vec::new();
    let mut structural_edges = BTreeSet::new();
    let mut uid = u32::try_from(project.extra_targets.len() + 3)
        .map_err(|_| MfdError::Unsupported("too many components for .mfd export".to_string()))?
        .max(100);
    let joins = join::render(join::RenderJoinArgs {
        project,
        source_ports: &source_ports,
        target_ports: &primary_target.ports,
        target_root_iterable: primary_target.root_iterable,
        keys: &mut keys,
        uid: &mut uid,
        node_out_key: &mut node_out_key,
        components: &mut components,
        edges: &mut edges,
        warnings: &mut warnings,
    });
    let node::RenderedNodes {
        position_inputs,
        sequence_exists_pins,
    } = node::render(node::RenderArgs {
        project,
        source_ports: &source_ports,
        joins: &joins,
        keys: &mut keys,
        uid: &mut uid,
        node_out_key: &mut node_out_key,
        components: &mut components,
        edges: &mut edges,
        warnings: &mut warnings,
    });

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
    for (target_index, target) in targets.iter().enumerate() {
        let prior_position_contexts = position_contexts.clone();
        scope::connect(scope::ConnectArgs {
            scope: target.root,
            source_ports: &source_ports,
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
        });
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
    for (id, input) in &position_inputs {
        if !position_contexts.contains_key(id) {
            warnings.push(format!(
                "position node {id} has no matching iteration scope; its context input {input} is unconnected"
            ));
        }
    }
    components.push_str(&scope_components);

    // Database components reference a mapping-level datasource.
    let mut datasources: Vec<(String, String)> = Vec::new();
    for (format, instance) in std::iter::once((source_format, project.source_path.as_deref()))
        .chain(targets.iter().map(|target| (target.format, target.path)))
    {
        if format == SideFormat::Db
            && let Some(conn) = instance
        {
            let name = db_datasource_name(Some(conn));
            if !datasources.iter().any(|(n, _)| *n == name) {
                datasources.push((name, conn.to_string()));
            }
        }
    }
    let resources = if datasources.is_empty() {
        "\t<resources/>\n".to_string()
    } else {
        let mut resources = String::from("\t<resources>\n\t\t<datasources>\n");
        for (name, conn) in &datasources {
            let _ = write!(
                resources,
                "\t\t\t<datasource name=\"{0}\">\n\
                 \t\t\t\t<properties JDBCDriver=\"org.sqlite.JDBC\" JDBCDatabaseURL=\"jdbc:sqlite:{1}\" DBDataSource=\"{1}\" DBCatalog=\"main\"/>\n\
                 \t\t\t\t<database_connection database_kind=\"SQLite\" import_kind=\"SQLite\" ConnectionString=\"{1}\" name=\"{0}\" path=\"{0}\"/>\n\
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
         <mapping version=\"22\">\n\
         {resources}\
         \t<component name=\"defaultmap\" uid=\"1\" editable=\"1\">\n\
         \t\t<properties SelectedLanguage=\"builtin\"/>\n\
         \t\t<structure>\n\
         \t\t\t<children>\n"
    );
    let source_component = if project.source_options.external_source.is_some() {
        let request_schema = external_source::request_schema_artifact(
            &project.source_options,
            path,
            "source-request",
        );
        let xml = external_source::render_http_post(external_source::RenderHttpPostArgs {
            component_name: &project.source.name,
            response_schema: &project.source,
            response_ports: &source_ports,
            request_ports: external_request_ports.as_ref(),
            request_schema_file: request_schema
                .as_ref()
                .map(|artifact| artifact.file_name.as_str()),
            options: &project.source_options,
            url: project.source_path.as_deref(),
            uid: 2,
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
    } else {
        render_schema_component(
            &project.source,
            source_format,
            &source_ports,
            Side::Source,
            project.source_path.as_deref(),
            &project.source_options,
            path,
            targets.iter().any(|target| target.force_root_port)
                || targets
                    .iter()
                    .any(|target| concatenation::needs_source_root_port(target.root)),
            None,
            &project.source.name,
            2,
            "source",
            false,
            &used_ports,
        )?
    };
    out.push_str(&source_component.xml);
    let mut target_components = Vec::with_capacity(targets.len());
    for target in &targets {
        let rendered = render_schema_component(
            target.schema,
            target.format,
            &target.ports,
            Side::Target,
            target.path,
            target.options,
            path,
            target.force_root_port,
            Some(&target.branches),
            target.component_name,
            target.component_uid,
            &target.sibling_suffix,
            target.default_output,
            &used_ports,
        )?;
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
    out.push_str(
        "\t\t\t\t</vertices>\n\t\t\t</graph>\n\t\t</structure>\n\t</component>\n</mapping>\n",
    );

    let mut artifacts = Vec::new();
    for sibling in source_component.siblings.into_iter().chain(
        target_components
            .into_iter()
            .flat_map(|component| component.siblings),
    ) {
        artifacts.push((sibling.path, sibling.contents));
    }
    // Publish the design after its schema siblings reach their final paths.
    artifacts.push((path.to_path_buf(), out));
    write_artifacts(artifacts)?;
    Ok(warnings)
}
