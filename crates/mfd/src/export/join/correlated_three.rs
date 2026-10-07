use std::collections::BTreeSet;

use ir::SchemaKind;
use mapping::{JoinSource, JoinSourceCardinality};

use super::super::source::SourceExports;
use super::schema_node_at;

pub(super) fn resolved_correlated_source_paths(
    join_sources: &[&JoinSource],
    anchor: &[String],
    sources: &SourceExports<'_>,
) -> Result<Vec<Vec<String>>, String> {
    if join_sources[0].cardinality() != JoinSourceCardinality::Singleton
        || join_sources[1..]
            .iter()
            .any(|source| source.cardinality() != JoinSourceCardinality::Repeating)
    {
        let reason = if join_sources.len() == 3 {
            "nested three-input join requires a primary singleton followed by two static named repeating sources"
        } else {
            "nested multi-input join requires a primary singleton followed by static named repeating sources"
        };
        return Err(reason.to_string());
    }
    for source in &join_sources[1..] {
        if source.collection().starts_with(anchor) {
            return Err(
                "nested named join collection would change when its enclosing anchor is removed"
                    .to_string(),
            );
        }
    }
    let singleton = join_sources[0];
    if singleton.collection().is_empty() || sources.is_named_extra_path(singleton.collection()) {
        return Err(
            "nested join singleton must be a field of the active primary source item".into(),
        );
    }
    let mut singleton_path = anchor.to_vec();
    singleton_path.extend(singleton.collection().iter().cloned());
    let (singleton_owner, primary, _) = sources.owner_with_index(&singleton_path);
    let (anchor_owner, _, _) = sources.owner_with_index(anchor);
    if singleton_owner != 0 || anchor_owner != 0 {
        return Err(
            "nested join singleton and anchor must belong to the primary source component".into(),
        );
    }
    let singleton_field = sources
        .join_collection(&singleton_path)
        .ok_or("nested join singleton has no exact primary source schema and port")?;
    if singleton_field.schema.repeating
        || !singleton_field.schema.is_scalar()
        || primary.ports.key_for_abs(&singleton_path) != Some(singleton_field.port)
    {
        return Err("nested join singleton must resolve to one primary scalar source port".into());
    }
    let active = schema_node_at(primary.schema, anchor)
        .ok_or("nested join anchor has no primary source schema")?;
    if !matches!(active.kind, SchemaKind::Group { .. }) {
        return Err("nested join anchor must select a primary source group".into());
    }
    let mut component_indices = BTreeSet::new();
    let mut component_uids = BTreeSet::new();
    for source in &join_sources[1..] {
        let collection = source.collection();
        let (index, component, local) = sources.owner_with_index(collection);
        if index == 0 || component.dynamic_path_node.is_some() {
            return Err(
                "nested repeating join inputs must belong to static named source components".into(),
            );
        }
        if !component_indices.insert(index)
            || !component_uids.insert(component.component_uid)
            || component.component_uid == sources.primary_component_uid()
        {
            return Err(
                "nested repeating join inputs must use distinct named source components".into(),
            );
        }
        let resolved = sources
            .join_collection(collection)
            .ok_or("nested named join input has no exact source schema and port")?;
        if !resolved.schema.repeating
            || !matches!(resolved.schema.kind, SchemaKind::Group { .. })
            || component.ports.key_for_abs(local) != Some(resolved.port)
        {
            return Err(
                "nested named join input must resolve to a repeating group source port".into(),
            );
        }
        let name = collection
            .first()
            .ok_or("nested named join collection is empty")?;
        // Every possible primary frame on the anchor is checked conservatively.
        // Runtime lookup chooses the innermost frame owning this first segment.
        for length in 0..=anchor.len() {
            let frame = schema_node_at(primary.schema, &anchor[..length])
                .ok_or("nested join anchor has an unresolved primary source frame")?;
            if frame.dynamic_fields().is_some() {
                return Err(format!(
                    "nested named join source `{name}` may be shadowed by an open primary source frame"
                ));
            }
            if frame.child(name).is_some() {
                return Err(format!(
                    "nested named join source `{name}` is shadowed by a primary source frame"
                ));
            }
        }
    }
    Ok(std::iter::once(singleton_path)
        .chain(
            join_sources[1..]
                .iter()
                .map(|source| source.collection().to_vec()),
        )
        .collect())
}
