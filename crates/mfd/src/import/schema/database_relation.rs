use ir::{DatabaseForeignKeySide, DatabaseRelation, SchemaKind, SchemaNode};

const MAX_LOCAL_RELATIONS: usize = 4096;

struct DeclaredRelation {
    source_table: String,
    source_column: String,
    destination_table: String,
    destination_column: String,
}

pub(super) fn apply(
    connection: Option<&roxmltree::Node<'_, '_>>,
    schema: &mut SchemaNode,
    component_name: &str,
    warnings: &mut Vec<String>,
) {
    let Some(connection) = connection else {
        return;
    };
    let declarations = connection
        .descendants()
        .filter(|node| node.has_tag_name("LocalRelationElement"))
        .take(MAX_LOCAL_RELATIONS + 1)
        .collect::<Vec<_>>();
    if declarations.len() > MAX_LOCAL_RELATIONS {
        warnings.push(format!(
            "component `{component_name}` declares more than {MAX_LOCAL_RELATIONS} local database relations; excess declarations were skipped"
        ));
    }
    let mut relations = Vec::new();
    for declaration in declarations.into_iter().take(MAX_LOCAL_RELATIONS) {
        match read(&declaration) {
            Some(relation) => relations.push(relation),
            None => warnings.push(format!(
                "component `{component_name}` contains a malformed local database relation; that declaration was skipped"
            )),
        }
    }
    if relations.is_empty() {
        return;
    }

    if schema.repeating {
        let parent = physical_table(&schema.name).to_string();
        apply_table(schema, &parent, &relations, component_name, warnings);
        return;
    }
    let SchemaKind::Group { children, .. } = &mut schema.kind else {
        return;
    };
    for table in children {
        let parent = physical_table(&table.name).to_string();
        apply_table(table, &parent, &relations, component_name, warnings);
    }
}

fn apply_table(
    table: &mut SchemaNode,
    parent_table: &str,
    relations: &[DeclaredRelation],
    component_name: &str,
    warnings: &mut Vec<String>,
) {
    let SchemaKind::Group { children, .. } = &mut table.kind else {
        return;
    };
    for child in children
        .iter_mut()
        .filter(|child| matches!(child.kind, SchemaKind::Group { .. }))
    {
        let Some((child_table, join_column)) = child.name.split_once('|') else {
            continue;
        };
        let child_table = child_table.to_string();
        let join_column = join_column.to_string();
        let candidates = relations
            .iter()
            .filter_map(|relation| declared_metadata(parent_table, &child_table, relation))
            .collect::<Vec<_>>();
        match candidates.as_slice() {
            [] => {}
            [relation] => {
                let expected_join = match relation.foreign_key_side {
                    DatabaseForeignKeySide::Parent => &relation.parent_column,
                    DatabaseForeignKeySide::Child => &relation.child_column,
                };
                if !join_column.eq_ignore_ascii_case(expected_join) {
                    warnings.push(format!(
                        "component `{component_name}` local relation for `{parent_table}` and `{child_table}` uses `{expected_join}`, but the nested entry joins through `{join_column}`; the declaration was skipped"
                    ));
                } else {
                    child.database_relation = Some(relation.clone());
                }
            }
            _ => warnings.push(format!(
                "component `{component_name}` declares multiple local relations for nested tables `{parent_table}` and `{child_table}`; the ambiguous declarations were skipped"
            )),
        }
        apply_table(child, &child_table, relations, component_name, warnings);
    }
}

fn declared_metadata(
    parent_table: &str,
    child_table: &str,
    relation: &DeclaredRelation,
) -> Option<DatabaseRelation> {
    if relation.source_table.eq_ignore_ascii_case(child_table)
        && relation
            .destination_table
            .eq_ignore_ascii_case(parent_table)
    {
        return Some(DatabaseRelation {
            parent_column: relation.destination_column.clone(),
            child_column: relation.source_column.clone(),
            foreign_key_side: DatabaseForeignKeySide::Child,
        });
    }
    if relation.source_table.eq_ignore_ascii_case(parent_table)
        && relation.destination_table.eq_ignore_ascii_case(child_table)
    {
        return Some(DatabaseRelation {
            parent_column: relation.source_column.clone(),
            child_column: relation.destination_column.clone(),
            foreign_key_side: DatabaseForeignKeySide::Parent,
        });
    }
    None
}

fn read(element: &roxmltree::Node<'_, '_>) -> Option<DeclaredRelation> {
    Some(DeclaredRelation {
        source_table: table_name(element, "SourceTable")?,
        source_column: column_name(element, "SourceColumns")?,
        destination_table: table_name(element, "DestinationTable")?,
        destination_column: column_name(element, "DestinationColumns")?,
    })
}

fn table_name(element: &roxmltree::Node<'_, '_>, owner: &str) -> Option<String> {
    let owner = element.children().find(|node| node.has_tag_name(owner))?;
    let tables = owner
        .children()
        .filter(|node| node.has_tag_name("PathElement") && node.attribute("Kind") == Some("Table"))
        .filter_map(|node| node.attribute("Name"))
        .collect::<Vec<_>>();
    let [table] = tables.as_slice() else {
        return None;
    };
    (!table.is_empty()).then(|| (*table).to_string())
}

fn column_name(element: &roxmltree::Node<'_, '_>, owner: &str) -> Option<String> {
    let owner = element.children().find(|node| node.has_tag_name(owner))?;
    let columns = owner
        .children()
        .filter(|node| node.has_tag_name("Column"))
        .filter_map(|node| node.attribute("name"))
        .collect::<Vec<_>>();
    let [column] = columns.as_slice() else {
        return None;
    };
    (!column.is_empty()).then(|| (*column).to_string())
}

fn physical_table(name: &str) -> &str {
    name.split_once('|').map_or(name, |(table, _)| table)
}
