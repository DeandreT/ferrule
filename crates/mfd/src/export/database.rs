//! Export of one MapForce database component that owns both read and write ports.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use ir::{SchemaKind, SchemaNode};

use crate::MfdError;

use super::concatenation::TargetBranches;
use super::schema::{
    DbLayout, PortTree, RenderedSchemaComponent, db_datasource_name, db_layout, db_selections_xml,
    db_type_name, db_wrapper_attr, xml_escape,
};

pub(super) fn local_relation_elements(schema: &SchemaNode) -> Result<BTreeSet<String>, MfdError> {
    let mut elements = BTreeSet::new();
    if schema.repeating {
        let parent = physical_table(&schema.name)?;
        collect_local_relations(schema, parent, &mut elements)?;
        return Ok(elements);
    }
    let SchemaKind::Group { children, .. } = &schema.kind else {
        return Ok(elements);
    };
    for table in children.iter().filter(|table| table.repeating) {
        let parent = physical_table(&table.name)?;
        collect_local_relations(table, parent, &mut elements)?;
    }
    Ok(elements)
}

fn collect_local_relations(
    table: &SchemaNode,
    parent_table: &str,
    elements: &mut BTreeSet<String>,
) -> Result<(), MfdError> {
    let SchemaKind::Group { children, .. } = &table.kind else {
        return Ok(());
    };
    for child in children
        .iter()
        .filter(|child| matches!(child.kind, SchemaKind::Group { .. }))
    {
        let child_table = physical_table(&child.name)?;
        if let Some(relation) = &child.database_relation {
            if !child.database_relation_is_valid() {
                return Err(MfdError::Unsupported(format!(
                    "database relation `{}` has inconsistent endpoint metadata",
                    child.name
                )));
            }
            let (source_table, source_column, destination_table, destination_column) =
                match relation.foreign_key_side {
                    ir::DatabaseForeignKeySide::Parent => (
                        parent_table,
                        relation.parent_column.as_str(),
                        child_table,
                        relation.child_column.as_str(),
                    ),
                    ir::DatabaseForeignKeySide::Child => (
                        child_table,
                        relation.child_column.as_str(),
                        parent_table,
                        relation.parent_column.as_str(),
                    ),
                };
            let name = format!(
                "ferrule-{}-{}-{}",
                source_table, destination_table, source_column
            );
            elements.insert(format!(
                "\t\t\t\t\t\t<LocalRelationElement name=\"{}\">\n\
                 \t\t\t\t\t\t\t<SourceTable><PathElement Name=\"main\" Kind=\"Database\"/><PathElement Name=\"{}\" Kind=\"Table\"/></SourceTable>\n\
                 \t\t\t\t\t\t\t<SourceColumns><Column name=\"{}\" kind=\"Column\"/></SourceColumns>\n\
                 \t\t\t\t\t\t\t<DestinationTable><PathElement Name=\"main\" Kind=\"Database\"/><PathElement Name=\"{}\" Kind=\"Table\"/></DestinationTable>\n\
                 \t\t\t\t\t\t\t<DestinationColumns><Column name=\"{}\" kind=\"Column\"/></DestinationColumns>\n\
                 \t\t\t\t\t\t</LocalRelationElement>\n",
                xml_escape(&name),
                xml_escape(source_table),
                xml_escape(source_column),
                xml_escape(destination_table),
                xml_escape(destination_column),
            ));
        }
        collect_local_relations(child, child_table, elements)?;
    }
    Ok(())
}

fn physical_table(name: &str) -> Result<&str, MfdError> {
    let table = name.split_once('|').map_or(name, |(table, _)| table);
    if table.is_empty() {
        return Err(MfdError::Unsupported(
            "database relation has an empty physical table name".into(),
        ));
    }
    Ok(table)
}

pub(super) struct RenderMixedArgs<'a> {
    pub(super) schema: &'a SchemaNode,
    pub(super) source_ports: &'a PortTree,
    pub(super) target_ports: &'a PortTree,
    pub(super) target_branches: &'a TargetBranches,
    pub(super) instance_path: Option<&'a str>,
    pub(super) component_name: &'a str,
    pub(super) component_uid: u32,
    pub(super) default_output: bool,
    pub(super) used_ports: &'a BTreeSet<u32>,
}

pub(super) fn render_mixed(args: RenderMixedArgs<'_>) -> Result<RenderedSchemaComponent, MfdError> {
    let layout = db_layout(args.schema).ok_or_else(|| {
        MfdError::Unsupported(
            "a bidirectional database component requires a canonical relational schema".into(),
        )
    })?;
    let entries = mixed_entries(&layout, &args)?;
    let selections = db_selections_xml(&layout);
    let wrapper = db_wrapper_attr(&layout);
    let datasource = db_datasource_name(args.instance_path);
    let properties = if args.default_output {
        "<properties XSLTDefaultOutput=\"1\"/>\n\t\t\t\t\t"
    } else {
        ""
    };
    let xml = format!(
        "\t\t\t\t<component name=\"{}\" library=\"db\" uid=\"{}\" kind=\"15\">\n\
         \t\t\t\t\t{properties}<view rbx=\"300\" rby=\"400\"/>\n\
         \t\t\t\t\t<data>\n\
         \t\t\t\t\t\t<root>\n\
         \t\t\t\t\t\t\t<header><namespaces><namespace/></namespaces></header>\n\
         \t\t\t\t\t\t\t<entry name=\"document\" expanded=\"1\"{wrapper}>\n\
         {entries}\
         \t\t\t\t\t\t\t</entry>\n\
         \t\t\t\t\t\t</root>\n\
         \t\t\t\t\t\t<database ref=\"{}\">\n\
         \t\t\t\t\t\t\t<data><selections>\n\
         {selections}\
         \t\t\t\t\t\t\t</selections></data>\n\
         \t\t\t\t\t\t</database>\n\
         \t\t\t\t\t</data>\n\
         \t\t\t\t</component>\n",
        xml_escape(args.component_name),
        args.component_uid,
        xml_escape(&datasource),
    );
    Ok(RenderedSchemaComponent {
        xml,
        siblings: Vec::new(),
    })
}

fn mixed_entries(layout: &DbLayout<'_>, args: &RenderMixedArgs<'_>) -> Result<String, MfdError> {
    let mut output = String::new();
    match layout {
        DbLayout::Table(table) => {
            let path = Vec::new();
            render_if_used(
                table,
                &path,
                Some(RenderDirection {
                    ports: args.source_ports,
                    branch: None,
                    attr: "outkey",
                }),
                Some(RenderDirection {
                    ports: args.target_ports,
                    branch: None,
                    attr: "inpkey",
                }),
                args,
                &mut output,
            )?;
        }
        DbLayout::Database(tables) => {
            let mut occurrences = BTreeMap::<&str, usize>::new();
            let mut rendered_source = BTreeSet::new();
            for table in *tables {
                let path = vec![table.name.clone()];
                let index = *occurrences.entry(&table.name).or_default();
                *occurrences.entry(&table.name).or_default() += 1;
                let branch = args
                    .target_branches
                    .count(&path)
                    .map(|_| (path.as_slice(), index));
                render_if_used(
                    table,
                    &path,
                    rendered_source
                        .insert(table.name.as_str())
                        .then_some(RenderDirection {
                            ports: args.source_ports,
                            branch: None,
                            attr: "outkey",
                        }),
                    Some(RenderDirection {
                        ports: args.target_ports,
                        branch,
                        attr: "inpkey",
                    }),
                    args,
                    &mut output,
                )?;
            }
            for (name, rendered) in occurrences {
                let root = vec![name.to_string()];
                let Some(count) = args.target_branches.count(&root) else {
                    continue;
                };
                let Some(table) = tables.iter().find(|table| table.name == name) else {
                    continue;
                };
                for index in rendered..count {
                    render_if_used(
                        table,
                        &root,
                        None,
                        Some(RenderDirection {
                            ports: args.target_ports,
                            branch: Some((&root, index)),
                            attr: "inpkey",
                        }),
                        args,
                        &mut output,
                    )?;
                }
            }
        }
    }
    Ok(output)
}

#[derive(Clone, Copy)]
struct RenderDirection<'a> {
    ports: &'a PortTree,
    branch: Option<(&'a [String], usize)>,
    attr: &'static str,
}

#[allow(clippy::too_many_arguments)]
fn render_if_used(
    table: &SchemaNode,
    path: &[String],
    source: Option<RenderDirection<'_>>,
    target: Option<RenderDirection<'_>>,
    args: &RenderMixedArgs<'_>,
    output: &mut String,
) -> Result<(), MfdError> {
    let source = source
        .filter(|direction| subtree_used(table, path, direction.ports, direction.branch, args));
    let target = target
        .filter(|direction| subtree_used(table, path, direction.ports, direction.branch, args));
    if source.is_some() || target.is_some() {
        render_table(table, &mut path.to_vec(), source, target, 9, args, output)?;
    }
    Ok(())
}

fn subtree_used(
    node: &SchemaNode,
    path: &[String],
    ports: &PortTree,
    branch: Option<(&[String], usize)>,
    args: &RenderMixedArgs<'_>,
) -> bool {
    port_key(ports, branch, path, args).is_some_and(|key| args.used_ports.contains(&key))
        || match &node.kind {
            SchemaKind::Group { children, .. } => children.iter().any(|child| {
                let mut child_path = path.to_vec();
                child_path.push(child.name.clone());
                subtree_used(child, &child_path, ports, branch, args)
            }),
            SchemaKind::Scalar { .. } => false,
        }
}

#[allow(clippy::too_many_arguments)]
fn render_table(
    table: &SchemaNode,
    path: &mut Vec<String>,
    source: Option<RenderDirection<'_>>,
    target: Option<RenderDirection<'_>>,
    indent: usize,
    args: &RenderMixedArgs<'_>,
    output: &mut String,
) -> Result<(), MfdError> {
    let mut keys = String::new();
    for direction in [source, target].into_iter().flatten() {
        let key = port_key(direction.ports, direction.branch, path, args).ok_or_else(|| {
            MfdError::Unsupported(format!(
                "internal bidirectional database port `{}` was not allocated",
                path.join("/")
            ))
        })?;
        let _ = write!(keys, " {}=\"{key}\"", direction.attr);
    }
    let pad = "\t".repeat(indent);
    let clone = if target
        .and_then(|direction| direction.branch)
        .is_some_and(|(_, index)| index > 0)
    {
        " clone=\"1\""
    } else {
        ""
    };
    let _ = writeln!(
        output,
        "{pad}<entry name=\"{}\" type=\"table\"{keys} expanded=\"1\"{clone}>",
        xml_escape(&table.name),
    );
    let SchemaKind::Group { children, .. } = &table.kind else {
        return Err(MfdError::Unsupported(
            "internal bidirectional database table is not a group".into(),
        ));
    };
    for child in children {
        path.push(child.name.clone());
        match child.kind {
            SchemaKind::Scalar { ty } => {
                let mut keys = String::new();
                for direction in [source, target].into_iter().flatten() {
                    if direction.attr == "inpkey" && child.value_generation.is_some() {
                        continue;
                    }
                    let key = port_key(direction.ports, direction.branch, path, args).ok_or_else(
                        || {
                            MfdError::Unsupported(format!(
                                "internal bidirectional database column port `{}` was not allocated",
                                path.join("/")
                            ))
                        },
                    )?;
                    let _ = write!(keys, " {}=\"{key}\"", direction.attr);
                }
                let generation = child
                    .value_generation
                    .map(|generation| match generation {
                        ir::ValueGeneration::MaxNumber => " valuekeygeneration=\"maxnumber\"",
                    })
                    .unwrap_or_default();
                let _ = writeln!(
                    output,
                    "{pad}\t<entry name=\"{}\"{keys}{generation} datatype=\"{}\"/>",
                    xml_escape(&child.name),
                    db_type_name(ty)
                );
            }
            SchemaKind::Group { .. } => {
                render_table(child, path, source, target, indent + 1, args, output)?;
            }
        }
        path.pop();
    }
    let _ = writeln!(output, "{pad}</entry>");
    Ok(())
}

fn port_key(
    ports: &PortTree,
    branch: Option<(&[String], usize)>,
    path: &[String],
    args: &RenderMixedArgs<'_>,
) -> Option<u32> {
    match branch {
        Some((root, index)) => args.target_branches.key_for(ports, root, index, path),
        None => ports.key_for_abs(path),
    }
}
