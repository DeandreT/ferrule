//! `mapping::Project` -> `.mfd` conversion for the supported subset, with
//! generated schemas and component families selected from instance paths.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::Path;

use mapping::{NodeId, Project, ScopeConstruction};

use crate::MfdError;

mod artifact;
mod concatenation;
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

use artifact::write_artifacts;
use mapped_sequence::{preflight_mapped_sequences, render_edge_metadata};
use position::connect_position_roots;
use schema::{
    KeyAlloc, PortTree, Side, SideFormat, db_datasource_name, render_schema_component, side_format,
    xml_escape,
};

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
    let target_format = side_format(&project.target_path, &project.target_options);
    let copy_document_root = project.root.construction == ScopeConstruction::CopyCurrentSource;
    let target_root_iterable = matches!(
        target_format,
        SideFormat::Csv | SideFormat::FixedWidth | SideFormat::Xlsx | SideFormat::Db
    ) || (target_format == SideFormat::Json && project.target.repeating);
    let mapped_scope_plans = preflight_mapped_sequences(project, target_format)?;

    let mut keys = KeyAlloc { next: 1 };
    let source_ports = PortTree::build(&project.source, &mut keys);
    let target_ports = PortTree::build(&project.target, &mut keys);
    let target_branches =
        concatenation::TargetBranches::build(&project.target, &project.root, &mut keys);

    let mut node_out_key: BTreeMap<NodeId, u32> = BTreeMap::new();
    let mut components = String::new();
    let mut edges: Vec<(u32, u32)> = Vec::new();
    let mut structural_edges = BTreeSet::new();
    let mut uid = 100u32;
    let joins = join::render(join::RenderJoinArgs {
        project,
        source_ports: &source_ports,
        target_ports: &target_ports,
        target_root_iterable,
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
    scope::connect(scope::ConnectArgs {
        scope: &project.root,
        source_ports: &source_ports,
        target_ports: &target_ports,
        target_root_iterable,
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
        mapped_scope_plans: &mapped_scope_plans,
        joins: &joins,
        target_branches: &target_branches,
    });
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
    for (format, instance) in [
        (source_format, project.source_path.as_deref()),
        (target_format, project.target_path.as_deref()),
    ] {
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
    let source_component = render_schema_component(
        &project.source,
        source_format,
        &source_ports,
        Side::Source,
        project.source_path.as_deref(),
        &project.source_options,
        path,
        copy_document_root || concatenation::needs_source_root_port(&project.root),
        None,
    )?;
    let target_component = render_schema_component(
        &project.target,
        target_format,
        &target_ports,
        Side::Target,
        project.target_path.as_deref(),
        &project.target_options,
        path,
        copy_document_root,
        Some(&target_branches),
    )?;
    out.push_str(&source_component.xml);
    out.push_str(&target_component.xml);
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
    for sibling in [source_component.sibling, target_component.sibling]
        .into_iter()
        .flatten()
    {
        artifacts.push((sibling.path, sibling.contents));
    }
    // Publish the design after its schema siblings reach their final paths.
    artifacts.push((path.to_path_buf(), out));
    write_artifacts(artifacts)?;
    Ok(warnings)
}
