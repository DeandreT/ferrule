use std::collections::BTreeSet;

use ir::{SchemaKind, SchemaNode};
use mapping::{JoinSource, JoinSourceCardinality};

use super::super::source::SourceExports;
use super::schema_node_at;

/// Resolves the source paths of a join nested inside one ordinary primary
/// source iteration whose item path is `anchor`.
///
/// Singletons may sit at any input position and come from the active primary
/// item (anchor-relative) or from a static named source. Repeating inputs
/// come from static named sources; a two-input plan also keeps its primary
/// repeating lookup. Plans without a singleton are refused.
pub(super) fn resolved_nested_source_paths(
    join_sources: &[&JoinSource],
    anchor: &[String],
    sources: &SourceExports<'_>,
) -> Result<Vec<Vec<String>>, String> {
    let singletons = join_sources
        .iter()
        .filter(|source| source.cardinality() == JoinSourceCardinality::Singleton)
        .count();
    if join_sources.len() == 2 && singletons != 1 {
        return Err(
            "nested join must contain exactly one correlated singleton and one repeating source"
                .to_string(),
        );
    }
    if singletons == 0 {
        let reason = if join_sources.len() == 3 {
            "nested three-input join requires a primary singleton followed by two static named repeating sources"
        } else {
            "nested multi-input join requires a primary singleton followed by static named repeating sources"
        };
        return Err(reason.to_string());
    }
    if singletons == join_sources.len() {
        return Err("nested join requires at least one repeating source".to_string());
    }

    let inputs = join_sources
        .iter()
        .map(|source| NestedInput::classify(source, sources))
        .collect::<Vec<_>>();
    // Reimport strips the enclosing anchor from every absolute input path, so a
    // source-relative path starting with the anchor would not survive it.
    for input in &inputs {
        if input.kind != InputKind::PrimarySingleton && input.collection.starts_with(anchor) {
            return Err(
                "nested named join collection would change when its enclosing anchor is removed"
                    .to_string(),
            );
        }
    }

    let primary_singletons = inputs
        .iter()
        .filter(|input| input.kind == InputKind::PrimarySingleton)
        .map(|input| {
            if input.collection.is_empty() {
                return Err(
                    "nested join singleton must be a field of the active primary source item"
                        .to_string(),
                );
            }
            let mut path = anchor.to_vec();
            path.extend(input.collection.iter().cloned());
            Ok(path)
        })
        .collect::<Result<Vec<_>, String>>()?;
    let (anchor_owner, primary, _) = sources.owner_with_index(anchor);
    if anchor_owner != 0
        || primary_singletons
            .iter()
            .any(|path| sources.owner_with_index(path).0 != 0)
    {
        return Err(
            "nested join singleton and anchor must belong to the primary source component".into(),
        );
    }
    for path in &primary_singletons {
        let field = sources
            .join_collection(path)
            .ok_or("nested join singleton has no exact primary source schema and port")?;
        if field.schema.repeating
            || !field.schema.is_scalar()
            || primary.ports.key_for_abs(path) != Some(field.port)
        {
            return Err(
                "nested join singleton must resolve to one primary scalar source port".into(),
            );
        }
    }
    let active = schema_node_at(primary.schema, anchor)
        .ok_or("nested join anchor has no primary source schema")?;
    if !matches!(active.kind, SchemaKind::Group { .. }) {
        return Err("nested join anchor must select a primary source group".into());
    }

    let mut repeating_components = RepeatingComponents::default();
    for input in &inputs {
        match input.kind {
            InputKind::PrimarySingleton => {}
            InputKind::PrimaryRepeating if join_sources.len() == 2 => {}
            InputKind::PrimaryRepeating => {
                return Err(
                    "nested repeating join inputs must belong to static named source components"
                        .into(),
                );
            }
            InputKind::NamedSingleton | InputKind::NamedRepeating => {
                validate_named_input(
                    input,
                    anchor,
                    primary.schema,
                    sources,
                    &mut repeating_components,
                )?;
            }
        }
    }

    let mut primary_singletons = primary_singletons.into_iter();
    inputs
        .iter()
        .map(|input| match input.kind {
            InputKind::PrimarySingleton => primary_singletons
                .next()
                .ok_or_else(|| "nested join singleton resolution is incomplete".to_string()),
            _ => Ok(input.collection.to_vec()),
        })
        .collect()
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum InputKind {
    PrimarySingleton,
    PrimaryRepeating,
    NamedSingleton,
    NamedRepeating,
}

struct NestedInput<'a> {
    collection: &'a [String],
    kind: InputKind,
}

impl<'a> NestedInput<'a> {
    fn classify(source: &'a JoinSource, sources: &SourceExports<'_>) -> Self {
        let collection = source.collection();
        let named = sources.is_named_extra_path(collection);
        let kind = match (source.cardinality(), named) {
            (JoinSourceCardinality::Singleton, false) => InputKind::PrimarySingleton,
            (JoinSourceCardinality::Singleton, true) => InputKind::NamedSingleton,
            (JoinSourceCardinality::Repeating, false) => InputKind::PrimaryRepeating,
            (JoinSourceCardinality::Repeating, true) => InputKind::NamedRepeating,
        };
        Self { collection, kind }
    }
}

/// Named repeating inputs already admitted, by source index and component.
#[derive(Default)]
struct RepeatingComponents {
    indices: BTreeSet<usize>,
    uids: BTreeSet<u32>,
}

fn validate_named_input(
    input: &NestedInput<'_>,
    anchor: &[String],
    primary_schema: &SchemaNode,
    sources: &SourceExports<'_>,
    repeating_components: &mut RepeatingComponents,
) -> Result<(), String> {
    let repeating = input.kind == InputKind::NamedRepeating;
    let (index, component, local) = sources.owner_with_index(input.collection);
    if index == 0 || component.dynamic_path_node.is_some() {
        return Err(if repeating {
            "nested repeating join inputs must belong to static named source components".into()
        } else {
            "nested named join singleton must belong to a static named source component".into()
        });
    }
    if repeating {
        if !repeating_components.indices.insert(index)
            || !repeating_components.uids.insert(component.component_uid)
            || component.component_uid == sources.primary_component_uid()
        {
            return Err(
                "nested repeating join inputs must use distinct named source components".into(),
            );
        }
    } else if component.component_uid == sources.primary_component_uid() {
        return Err(
            "nested named join singleton must not share the primary source component".into(),
        );
    }
    let resolved = sources
        .join_collection(input.collection)
        .ok_or("nested named join input has no exact source schema and port")?;
    let shape_matches = if repeating {
        resolved.schema.repeating && matches!(resolved.schema.kind, SchemaKind::Group { .. })
    } else {
        !resolved.schema.repeating && resolved.schema.is_scalar()
    };
    if !shape_matches || component.ports.key_for_abs(local) != Some(resolved.port) {
        return Err(if repeating {
            "nested named join input must resolve to a repeating group source port".into()
        } else {
            "nested named join singleton must resolve to one scalar source port".into()
        });
    }
    let name = input
        .collection
        .first()
        .ok_or("nested named join collection is empty")?;
    // Every possible primary frame on the anchor is checked conservatively.
    // Runtime lookup chooses the innermost frame owning this first segment.
    for length in 0..=anchor.len() {
        let frame = schema_node_at(primary_schema, &anchor[..length])
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
    Ok(())
}
