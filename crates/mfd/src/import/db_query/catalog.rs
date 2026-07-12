use std::path::Path;

use crate::import::schema::SchemaComponent;

use super::{
    correlated, query_connection, query_identity, read_inline_component, same_query_identity,
};

pub(in crate::import) fn read_embedded_catalog(
    component: &roxmltree::Node<'_, '_>,
    mapping: &roxmltree::Node<'_, '_>,
    mfd_path: &Path,
) -> Result<Option<SchemaComponent>, String> {
    if component.attribute("kind") != Some("15") {
        return Ok(None);
    }
    let active = component
        .descendants()
        .filter(|entry| {
            entry.has_tag_name("entry")
                && entry.attribute("type") == Some("routine")
                && entry.attribute("outkey").is_some()
        })
        .collect::<Vec<_>>();
    let [routine] = active.as_slice() else {
        return if active.is_empty() {
            Ok(None)
        } else {
            Err("expected exactly one connected inline query result".to_string())
        };
    };
    let routine_name = routine
        .attribute("name")
        .ok_or_else(|| "inline query result has no name".to_string())?;
    let (query_name, _) = query_identity(routine_name)
        .ok_or_else(|| format!("inline query name `{routine_name}` is invalid"))?;
    if component.descendants().any(|entry| {
        entry.has_tag_name("entry")
            && entry.attribute("type") == Some("routine")
            && entry
                .attribute("name")
                .is_none_or(|name| !same_query_identity(name, routine_name))
    }) {
        return Err("inline catalog contains more than one query identity".to_string());
    }
    let Some(parent) = routine
        .ancestors()
        .find(|entry| entry.has_tag_name("entry") && entry.attribute("type") == Some("table"))
    else {
        return read_inline_component(component, mapping, mfd_path, query_name).map(Some);
    };
    if component
        .descendants()
        .filter(|entry| {
            entry.has_tag_name("entry")
                && entry.attribute("type") == Some("table")
                && !entry.ancestors().any(|ancestor| ancestor == *routine)
        })
        .count()
        != 1
    {
        return Err(
            "inline query catalog must contain exactly one physical parent table".to_string(),
        );
    }
    let connection = query_connection(mapping, query_name)?;
    if !connection
        .attribute("database_kind")
        .is_some_and(|kind| kind.eq_ignore_ascii_case("SQLite"))
        || !connection
            .attribute("import_kind")
            .is_some_and(|kind| kind.eq_ignore_ascii_case("SQLite"))
    {
        return Err("only SQLite query datasources are supported".to_string());
    }
    correlated::read_catalog_component(
        component,
        mapping,
        mfd_path,
        &connection,
        query_name,
        correlated::CatalogRoutine {
            parent,
            routine: *routine,
        },
    )
    .map(Some)
}
