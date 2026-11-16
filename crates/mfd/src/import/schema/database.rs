use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use ir::{ScalarType, SchemaKind, SchemaNode};
use mapping::FormatOptions;

use crate::resource::ResourceResolver;

use super::{
    ComponentFormat, SchemaComponent, database_relation, database_xml, entry_key_sets,
    input_port_ancestors, is_default_output, record_entry_keys, resolve_resource_reference,
};

/// Reads a database schema component. A lone flat table preserves the
/// historical flat-row shape: its own port maps to `[]` and its columns map
/// below that. Relational components retain each repeating table level;
/// several top-level tables live below a non-repeating `database` root and
/// nested relationship names keep the reference application's `PhysicalTable|JoinColumn`
/// convention understood by `format_db::read_instance`.
pub(in super::super) fn read_db_component_in_package(
    component: &roxmltree::Node,
    mapping_el: &roxmltree::Node,
    resources: &ResourceResolver,
    warnings: &mut Vec<String>,
) -> Option<SchemaComponent> {
    read_db_component_resolved(component, mapping_el, resources, warnings)
}

fn read_db_component_resolved(
    component: &roxmltree::Node,
    mapping_el: &roxmltree::Node,
    resources: &ResourceResolver,
    warnings: &mut Vec<String>,
) -> Option<SchemaComponent> {
    let mfd_path = resources.mapping_path();
    let name = component.attribute("name").unwrap_or_default().to_string();
    match super::db_query::read_embedded_catalog(component, mapping_el, mfd_path) {
        Ok(Some(catalog)) => return Some(catalog),
        Ok(None) => {}
        Err(reason) => warnings.push(format!(
            "database component `{name}` has an unsupported inline query: {reason}; parent table fields were imported"
        )),
    }
    if component.attribute("kind") == Some("28") {
        return match super::db_query::read_component(component, mapping_el, mfd_path) {
            Ok(query) => Some(query),
            Err(reason) => {
                warnings.push(format!(
                    "skipped database query component `{name}`: {reason}"
                ));
                None
            }
        };
    }
    let data = component
        .children()
        .find(|n| n.is_element() && n.tag_name().name() == "data")?;
    let root_el = data
        .children()
        .find(|n| n.is_element() && n.tag_name().name() == "root")?;

    // Descend through the synthetic FileInstance/document levels to the
    // level whose entries are the tables.
    let mut container = root_el;
    loop {
        let mut entries = container
            .children()
            .filter(|n| n.is_element() && n.tag_name().name() == "entry");
        let (first, second) = (entries.next(), entries.next());
        match (first, second) {
            (Some(entry), None)
                if matches!(
                    entry.attribute("name"),
                    Some("FileInstance") | Some("document")
                ) =>
            {
                container = entry;
            }
            _ => break,
        }
    }
    let entries: Vec<roxmltree::Node> = container
        .children()
        .filter(|n| n.is_element() && n.tag_name().name() == "entry")
        .collect();
    let all_tables: Vec<roxmltree::Node> = entries
        .iter()
        .copied()
        .filter(|entry| entry.attribute("type") == Some("table"))
        .collect();
    if all_tables.is_empty() {
        warnings.push(format!(
            "skipped database component `{name}`: it contains no table entries"
        ));
        return None;
    }
    let connected_tables = all_tables
        .iter()
        .copied()
        .filter(|table| {
            let (inputs, outputs) = entry_key_sets(table);
            !inputs.is_empty() || !outputs.is_empty()
        })
        .collect::<Vec<_>>();
    // Database designers retain selected but disconnected tables in the
    // entry tree. Once any connected branch exists, those empty branches
    // have no mapping semantics and must not become invalid runtime tables.
    let tables = if connected_tables.is_empty() {
        all_tables
    } else {
        connected_tables
    };
    let single_plain_table = tables.len() == 1
        && !tables[0]
            .children()
            .any(|n| n.attribute("type") == Some("table"));
    let canonical_database_wrapper = tables.len() == 1
        && !single_plain_table
        && container.attribute("ferrule-database-wrapper") == Some("1");
    let db_xml_columns = database_xml::collect(
        &tables,
        tables.len() > 1 || canonical_database_wrapper,
        resources,
        &name,
        warnings,
    );
    if container.descendants().any(|entry| {
        entry.has_tag_name("entry")
            && entry
                .attribute("type")
                .is_some_and(|entry_type| !matches!(entry_type, "table" | "doc-xml"))
    }) {
        warnings.push(format!(
            "database component `{name}` contains non-table database entries; those entries were skipped"
        ));
    }

    let mut ports = BTreeMap::new();
    let mut out_count = 0usize;
    let mut in_count = 0usize;
    for table in &tables {
        let mut path = if single_plain_table || tables.len() == 1 && !canonical_database_wrapper {
            Vec::new()
        } else {
            vec![table.attribute("name").unwrap_or_default().to_string()]
        };
        collect_db_ports(table, &mut path, &mut ports, &mut out_count, &mut in_count);
    }
    for (&input, column) in &db_xml_columns {
        ports.insert(input, column.path.clone());
        in_count += 1;
    }
    if out_count == 0 && in_count == 0 {
        warnings.push(format!("component `{name}` has no connected ports"));
    }
    let is_source = out_count >= in_count;
    let (input_keys, output_keys) = entry_key_sets(&root_el);
    let input_ancestors = input_port_ancestors(&root_el, &input_keys);
    // The connection string lives in the mapping's datasource registry,
    // linked from the component by name.
    let connection_node = data
        .children()
        .find(|n| n.is_element() && n.tag_name().name() == "database")
        .and_then(|db| db.attribute("ref"))
        .and_then(|r| {
            mapping_el
                .descendants()
                .find(|n| n.has_tag_name("database_connection") && n.attribute("name") == Some(r))
        });
    let connection = connection_node
        .as_ref()
        .and_then(|c| c.attribute("ConnectionString"))
        .map(str::to_string);
    if connection.is_none() {
        warnings.push(format!(
            "database component `{name}` has no resolvable connection; the \
             project needs an instance path filled in"
        ));
    }

    let embedded_types = embedded_db_column_types(&tables);

    for generation in root_el.descendants().filter(|entry| {
        entry.has_tag_name("entry") && entry.attribute("valuekeygeneration").is_some()
    }) {
        if generation.attribute("valuekeygeneration") != Some("maxnumber") {
            warnings.push(format!(
                "component `{name}`: database column `{}` uses unsupported value generation `{}`; a mapped value is required",
                generation.attribute("name").unwrap_or_default(),
                generation.attribute("valuekeygeneration").unwrap_or_default()
            ));
        }
    }

    let db_path = connection.as_deref().and_then(|conn| {
        match resolve_resource_reference(mfd_path, Some(resources), conn, "database") {
            Ok(path) => Some(path),
            Err(error) => {
                if embedded_types.is_none() {
                    warnings.push(format!(
                        "component `{name}`: database `{conn}` could not be resolved ({error}); \
                         falling back to untyped columns"
                    ));
                }
                None
            }
        }
    });

    // Keep the exact existing flat-table behavior, including exposing every
    // introspected column. Relational entry trees instead select their own
    // columns and use introspection only to recover each selected leaf type.
    let mut schema = if single_plain_table {
        let table = tables[0];
        let table_name = table.attribute("name").unwrap_or_default();
        let introspected =
            db_path
                .as_deref()
                .and_then(|path| match format_db::introspect(path, table_name) {
                    Ok(schema) => Some(schema),
                    Err(error) => {
                        warnings.push(format!(
                            "component `{name}`: could not introspect `{}` ({error}); \
                         falling back to untyped columns",
                            connection.as_deref().unwrap_or_default()
                        ));
                        None
                    }
                });
        if let Some(mut schema) = introspected {
            apply_db_value_generation(&table, &mut schema);
            schema
        } else {
            let empty = BTreeMap::new();
            db_table_schema(&table, embedded_types.as_ref().unwrap_or(&empty))
        }
    } else {
        let mut introspected_types = BTreeMap::new();
        let mut introspected_generation = BTreeMap::new();
        if let Some(path) = db_path.as_deref() {
            collect_db_column_types(
                path,
                &tables,
                &name,
                warnings,
                &mut introspected_types,
                &mut introspected_generation,
            );
        }
        let types = if db_path.is_some() {
            &introspected_types
        } else {
            embedded_types.as_ref().unwrap_or(&introspected_types)
        };
        let mut schema = if tables.len() == 1 && !canonical_database_wrapper {
            db_table_schema(&tables[0], types)
        } else {
            SchemaNode::group(
                "database",
                tables
                    .iter()
                    .map(|table| db_table_schema(table, types))
                    .collect(),
            )
        };
        if db_path.is_some() {
            apply_introspected_value_generation(&tables, &mut schema, &introspected_generation);
        }
        schema
    };
    database_relation::apply(connection_node.as_ref(), &mut schema, &name, warnings);
    if !single_plain_table
        && let Some(path) = db_path.as_deref()
        && let Err(error) = format_db::validate_relational_schema(path, &schema)
    {
        warnings.push(format!(
            "component `{name}`: relational schema does not match SQLite foreign-key metadata \
             ({error}); execution is disabled until the relationship is corrected"
        ));
    }

    Some(SchemaComponent {
        name,
        format: ComponentFormat::Db,
        schema,
        input_instance: connection.clone(),
        output_instance: connection,
        options: FormatOptions::default(),
        is_source,
        is_default_output: is_default_output(component),
        is_variable: false,
        is_pass_through: false,
        compute_when_key: None,
        ports,
        input_ancestors,
        input_branch_paths: Vec::new(),
        ambiguous_branch_inputs: Default::default(),
        has_target_node_controls: false,
        input_keys,
        output_keys,
        json_schema_authoritative: false,
        port_declarations: None,
        db_queries: Vec::new(),
        db_xml_columns,
        dynamic_json: None,
        target_schema_hints: None,
    })
}

/// Reads ferrule's canonical self-describing database entry metadata. The
/// metadata is trusted only when every selected scalar entry has a supported
/// type and repeated declarations agree, so ordinary database designs still
/// require live introspection.
fn embedded_db_column_types(
    tables: &[roxmltree::Node<'_, '_>],
) -> Option<BTreeMap<String, BTreeMap<String, ScalarType>>> {
    fn collect(
        table: &roxmltree::Node<'_, '_>,
        types: &mut BTreeMap<String, BTreeMap<String, ScalarType>>,
        leaf_count: &mut usize,
    ) -> Option<()> {
        let name = table.attribute("name")?;
        let physical_table = name.split_once('|').map_or(name, |(table, _)| table);
        if physical_table.is_empty() {
            return None;
        }
        for entry in table.children().filter(|node| node.has_tag_name("entry")) {
            match entry.attribute("type") {
                Some("table") => collect(&entry, types, leaf_count)?,
                None => {
                    let column = entry.attribute("name")?;
                    if column.is_empty() {
                        return None;
                    }
                    let ty = match entry.attribute("datatype")? {
                        "string" => ScalarType::String,
                        "integer" | "int" | "long" => ScalarType::Int,
                        "decimal" | "double" | "float" | "number" => ScalarType::Float,
                        "boolean" => ScalarType::Bool,
                        _ => return None,
                    };
                    let columns = types.entry(physical_table.to_string()).or_default();
                    if let Some(existing) = columns
                        .iter()
                        .find(|(name, _)| name.eq_ignore_ascii_case(column))
                        .map(|(_, ty)| *ty)
                        && existing != ty
                    {
                        return None;
                    }
                    columns.insert(column.to_string(), ty);
                    *leaf_count += 1;
                }
                Some(_) => return None,
            }
        }
        Some(())
    }

    let mut types = BTreeMap::new();
    let mut leaf_count = 0;
    for table in tables {
        collect(table, &mut types, &mut leaf_count)?;
    }
    (leaf_count > 0).then_some(types)
}

pub(super) fn collect_db_ports(
    table: &roxmltree::Node,
    path: &mut Vec<String>,
    ports: &mut BTreeMap<u32, Vec<String>>,
    out_count: &mut usize,
    in_count: &mut usize,
) {
    record_entry_keys(table, path, ports, out_count, in_count);
    for child in table
        .children()
        .filter(|node| node.is_element() && node.tag_name().name() == "entry")
    {
        path.push(child.attribute("name").unwrap_or_default().to_string());
        match child.attribute("type") {
            Some("table") => collect_db_ports(&child, path, ports, out_count, in_count),
            None => record_entry_keys(&child, path, ports, out_count, in_count),
            Some(_) => {}
        }
        path.pop();
    }
}

fn collect_db_column_types(
    db_path: &Path,
    tables: &[roxmltree::Node<'_, '_>],
    component_name: &str,
    warnings: &mut Vec<String>,
    types: &mut BTreeMap<String, BTreeMap<String, ir::ScalarType>>,
    generated: &mut BTreeMap<String, BTreeSet<String>>,
) {
    for entry in tables {
        let physical_table = entry
            .attribute("name")
            .unwrap_or_default()
            .split_once('|')
            .map_or_else(
                || entry.attribute("name").unwrap_or_default(),
                |(table, _)| table,
            );
        if !types.contains_key(physical_table) {
            match format_db::introspect(db_path, physical_table) {
                Ok(schema) => {
                    let mut generated_columns = BTreeSet::new();
                    let column_types = match schema.kind {
                        SchemaKind::Group { children, .. } => children
                            .into_iter()
                            .filter_map(|column| match column.kind {
                                SchemaKind::Scalar { ty } => {
                                    if column.value_generation.is_some() {
                                        generated_columns.insert(column.name.clone());
                                    }
                                    Some((column.name, ty))
                                }
                                SchemaKind::ScalarUnion { .. } | SchemaKind::Group { .. } => None,
                            })
                            .collect(),
                        SchemaKind::Scalar { .. } | SchemaKind::ScalarUnion { .. } => {
                            BTreeMap::new()
                        }
                    };
                    types.insert(physical_table.to_string(), column_types);
                    generated.insert(physical_table.to_string(), generated_columns);
                }
                Err(error) => {
                    warnings.push(format!(
                        "component `{component_name}`: could not introspect table \
                         `{physical_table}` ({error}); its columns are untyped"
                    ));
                    types.insert(physical_table.to_string(), BTreeMap::new());
                }
            }
        }
        let nested = entry
            .children()
            .filter(|node| node.has_tag_name("entry") && node.attribute("type") == Some("table"))
            .collect::<Vec<_>>();
        collect_db_column_types(db_path, &nested, component_name, warnings, types, generated);
    }
}

fn apply_introspected_value_generation(
    table_entries: &[roxmltree::Node<'_, '_>],
    schema: &mut SchemaNode,
    generated: &BTreeMap<String, BTreeSet<String>>,
) {
    if table_entries.len() == 1 && schema.repeating {
        apply_table_generation(&table_entries[0], schema, generated);
        return;
    }
    let SchemaKind::Group { children, .. } = &mut schema.kind else {
        return;
    };
    for table in table_entries {
        let Some(name) = table.attribute("name") else {
            continue;
        };
        if let Some(child) = children.iter_mut().find(|child| child.name == name) {
            apply_table_generation(table, child, generated);
        }
    }
}

fn apply_table_generation(
    table: &roxmltree::Node<'_, '_>,
    schema: &mut SchemaNode,
    generated: &BTreeMap<String, BTreeSet<String>>,
) {
    let physical_table = table
        .attribute("name")
        .unwrap_or_default()
        .split_once('|')
        .map_or_else(
            || table.attribute("name").unwrap_or_default(),
            |(table, _)| table,
        );
    let generated_columns = generated.iter().find_map(|(table, columns)| {
        table
            .eq_ignore_ascii_case(physical_table)
            .then_some(columns)
    });
    let entries = table
        .children()
        .filter(|entry| entry.is_element() && entry.has_tag_name("entry"))
        .collect::<Vec<_>>();
    let SchemaKind::Group { children, .. } = &mut schema.kind else {
        return;
    };
    for child in children {
        if matches!(child.kind, SchemaKind::Scalar { .. }) {
            let explicitly_exposed = entries.iter().any(|entry| {
                entry.attribute("type") != Some("table")
                    && entry
                        .attribute("name")
                        .is_some_and(|name| name.eq_ignore_ascii_case(&child.name))
            });
            if !explicitly_exposed
                && generated_columns.is_some_and(|columns| {
                    columns
                        .iter()
                        .any(|column| column.eq_ignore_ascii_case(&child.name))
                })
            {
                child.value_generation = Some(ir::ValueGeneration::MaxNumber);
            }
            continue;
        }
        let Some(entry) = entries.iter().find(|entry| {
            entry.attribute("type") == Some("table")
                && entry.attribute("name") == Some(child.name.as_str())
        }) else {
            continue;
        };
        apply_table_generation(entry, child, generated);
    }
}

pub(super) fn db_table_schema(
    table: &roxmltree::Node,
    types: &BTreeMap<String, BTreeMap<String, ir::ScalarType>>,
) -> SchemaNode {
    db_table_schema_with_complete_row(table, types, false)
}

fn apply_db_value_generation(table: &roxmltree::Node<'_, '_>, schema: &mut SchemaNode) {
    let SchemaKind::Group { children, .. } = &mut schema.kind else {
        return;
    };
    for entry in table
        .children()
        .filter(|node| node.is_element() && node.has_tag_name("entry"))
    {
        let Some(name) = entry.attribute("name") else {
            continue;
        };
        let Some(child) = children.iter_mut().find(|child| child.name == name) else {
            continue;
        };
        if entry.attribute("type") == Some("table") {
            apply_db_value_generation(&entry, child);
        } else if entry.attribute("valuekeygeneration") == Some("maxnumber") {
            child.value_generation = Some(ir::ValueGeneration::MaxNumber);
        } else {
            // Live SQLite introspection marks implicit rowid primary keys as
            // generated. An explicitly exposed MFD column remains caller-
            // supplied unless the design itself requests maxnumber.
            child.value_generation = None;
        }
    }
}

fn db_table_schema_with_complete_row(
    table: &roxmltree::Node,
    types: &BTreeMap<String, BTreeMap<String, ir::ScalarType>>,
    parent_is_complete: bool,
) -> SchemaNode {
    let name = table.attribute("name").unwrap_or_default();
    let physical_table = name.split_once('|').map_or(name, |(table, _)| table);
    let columns = types.get(physical_table);
    let is_complete = parent_is_complete
        || table.attribute("inpkey").is_some()
        || table.attribute("outkey").is_some();
    let mut children = table
        .children()
        .filter(|node| node.is_element() && node.tag_name().name() == "entry")
        .filter_map(|entry| match entry.attribute("type") {
            Some("table") => Some(db_table_schema_with_complete_row(
                &entry,
                types,
                is_complete,
            )),
            None => {
                let column = entry.attribute("name").unwrap_or_default();
                let ty = columns
                    .and_then(|columns| {
                        columns
                            .iter()
                            .find(|(name, _)| name.eq_ignore_ascii_case(column))
                            .map(|(_, ty)| *ty)
                    })
                    .unwrap_or(ir::ScalarType::String);
                let mut schema = SchemaNode::scalar(column, ty);
                if entry.attribute("valuekeygeneration") == Some("maxnumber") {
                    schema.value_generation = Some(ir::ValueGeneration::MaxNumber);
                }
                Some(schema)
            }
            Some(_) => None,
        })
        .collect::<Vec<_>>();
    if is_complete {
        for (column, ty) in columns.into_iter().flatten() {
            if !children
                .iter()
                .any(|child| child.name.eq_ignore_ascii_case(column))
            {
                children.push(SchemaNode::scalar(column, *ty));
            }
        }
    }
    SchemaNode::group(name, children).repeating()
}
