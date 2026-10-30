use std::collections::BTreeSet;

use ir::{SchemaKind, SchemaNode};
use mapping::{CollectionJoinSource, JoinSourceCardinality};

use super::super::source::{SourceExport, SourceExports};
use super::schema_node_at;

/// Resolves the source paths of a join nested inside one ordinary source
/// iteration whose item path is `anchor`.
///
/// The iteration runs over the primary source or a static named source.
/// Singletons come from the active item (anchor-relative) or from a static
/// named source. Repeating inputs come from static named sources, or below
/// the active item of a primary iteration (anchor-relative); a two-input
/// plan may instead pair one singleton with one primary repeating collection
/// outside the iteration. A plan needs at least one repeating input, and
/// needs no singleton.
///
/// With a computed key (`computed`), the one collection beside it must be
/// that primary repeating collection outside the iteration, as observed.
pub(super) fn resolved_nested_source_paths(
    join_sources: &[&CollectionJoinSource],
    computed: bool,
    anchor: &[String],
    sources: &SourceExports<'_>,
) -> Result<Vec<Vec<String>>, String> {
    if join_sources
        .iter()
        .all(|source| source.cardinality() == JoinSourceCardinality::Singleton)
    {
        return Err("nested join requires at least one repeating source".to_string());
    }
    // Reimport strips the enclosing anchor from every absolute input path, so a
    // source-relative path starting with the anchor would not survive it.
    for source in join_sources {
        if sources.is_named_extra_path(source.collection())
            && source.collection().starts_with(anchor)
        {
            return Err(
                "nested named join collection would change when its enclosing anchor is removed"
                    .to_string(),
            );
        }
    }

    let frames = AnchorFrames::resolve(anchor, sources)?;
    let inputs = join_sources
        .iter()
        .map(|source| NestedInput::classify(source, &frames, sources))
        .collect::<Result<Vec<_>, String>>()?;
    let singletons = inputs.iter().filter(|input| input.kind.singleton()).count();
    let mut anchored_repeating = 0;
    let mut repeating_components = RepeatingComponents::default();
    for input in &inputs {
        match input.kind {
            InputKind::AnchoredSingleton => frames.validate_anchored(input, false, sources)?,
            InputKind::AnchoredRepeating => {
                if computed {
                    return Err(format!(
                        "nested repeating join input `{}` below the iteration beside a computed key was not observed",
                        input.collection.join("/")
                    ));
                }
                if frames.index != 0 {
                    return Err(format!(
                        "nested repeating join input `{}` below a named source iteration is not exported",
                        input.collection.join("/")
                    ));
                }
                anchored_repeating += 1;
                if anchored_repeating > 1 {
                    return Err(
                        "nested join takes at most one repeating input below the iteration".into(),
                    );
                }
                frames.validate_anchored(input, true, sources)?;
            }
            InputKind::OuterRepeating => {
                let (partner, admitted) = if computed {
                    ("one computed key", inputs.len() == 1)
                } else {
                    ("one singleton", inputs.len() == 2 && singletons == 1)
                };
                if !admitted {
                    return Err(format!(
                        "nested primary repeating join input `{}` outside the iteration is exported only beside {partner} in a two-input plan",
                        input.collection.join("/")
                    ));
                }
                if input.collection.starts_with(anchor) {
                    return Err(
                        "nested primary join collection would change when its enclosing anchor is removed"
                            .to_string(),
                    );
                }
            }
            InputKind::NamedSingleton | InputKind::NamedRepeating => {
                validate_named_input(input, &frames, sources, &mut repeating_components)?;
            }
        }
    }

    Ok(inputs
        .iter()
        .map(|input| match input.kind {
            InputKind::AnchoredSingleton | InputKind::AnchoredRepeating => {
                let mut path = anchor.to_vec();
                path.extend(input.collection.iter().cloned());
                path
            }
            InputKind::OuterRepeating | InputKind::NamedSingleton | InputKind::NamedRepeating => {
                input.collection.to_vec()
            }
        })
        .collect())
}

/// The component owning the enclosing iteration, and the schema frames the
/// runtime searches for an unprefixed input path, innermost first. The first
/// frame is the active item; the primary root is always the last.
struct AnchorFrames<'s, 'a> {
    anchor: &'s [String],
    index: usize,
    component: &'s SourceExport<'a>,
    local: &'s [String],
    frames: Vec<&'a SchemaNode>,
}

impl<'s, 'a> AnchorFrames<'s, 'a> {
    fn resolve(anchor: &'s [String], sources: &'s SourceExports<'a>) -> Result<Self, String> {
        let (index, component, local) = sources.owner_with_index(anchor);
        let (_, primary, _) = sources.owner_with_index(&[]);
        if index != 0 {
            let name = anchor.first().map(String::as_str).unwrap_or_default();
            if primary.schema.child(name).is_some() || primary.schema.dynamic_fields().is_some() {
                return Err(format!(
                    "nested join anchor `{name}` names both a primary source child and a named source"
                ));
            }
            if component.dynamic_path_node.is_some()
                || component.component_uid == sources.primary_component_uid()
            {
                return Err(
                    "nested join anchor must belong to the primary source or a static named source component"
                        .into(),
                );
            }
        }
        let mut frames = (0..=local.len())
            .rev()
            .map(|length| {
                schema_node_at(component.schema, &local[..length])
                    .ok_or("nested join anchor has an unresolved source frame")
            })
            .collect::<Result<Vec<_>, _>>()?;
        if !matches!(frames[0].kind, SchemaKind::Group { .. }) {
            return Err("nested join anchor must select a source group".into());
        }
        if index != 0 {
            frames.push(primary.schema);
        }
        Ok(Self {
            anchor,
            index,
            component,
            local,
            frames,
        })
    }

    /// Frames that declare `name`, or might through open content.
    fn declaring(&self, name: &str) -> Result<Vec<usize>, ()> {
        if self
            .frames
            .iter()
            .any(|frame| frame.dynamic_fields().is_some())
        {
            return Err(());
        }
        Ok(self
            .frames
            .iter()
            .enumerate()
            .filter(|(_, frame)| frame.child(name).is_some())
            .map(|(position, _)| position)
            .collect())
    }

    /// Which source owns the frame at `position`.
    fn owner_label(&self, position: usize) -> &'static str {
        if self.index == 0 || position + 1 == self.frames.len() {
            "primary"
        } else {
            "named"
        }
    }

    /// Checks an anchor-relative input against one exact port of the
    /// iteration's component.
    fn validate_anchored(
        &self,
        input: &NestedInput<'_>,
        repeating: bool,
        sources: &SourceExports<'_>,
    ) -> Result<(), String> {
        if input.collection.is_empty() {
            return Err("nested join singleton must be a field of the active source item".into());
        }
        let mut path = self.anchor.to_vec();
        path.extend(input.collection.iter().cloned());
        let mut local = self.local.to_vec();
        local.extend(input.collection.iter().cloned());
        let resolved = sources.join_collection(&path);
        let valid = resolved.is_some_and(|field| {
            let shape = if repeating {
                field.schema.repeating && matches!(field.schema.kind, SchemaKind::Group { .. })
            } else {
                !field.schema.repeating && field.schema.is_scalar()
            };
            shape
                && sources.owner_with_index(&path).0 == self.index
                && self.component.ports.key_for_abs(&local) == Some(field.port)
        });
        if valid {
            Ok(())
        } else if repeating {
            Err("nested join input below the iteration must resolve to one repeating group port of the active item".into())
        } else {
            Err("nested join singleton must resolve to one scalar port of the active item".into())
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum InputKind {
    AnchoredSingleton,
    AnchoredRepeating,
    OuterRepeating,
    NamedSingleton,
    NamedRepeating,
}

impl InputKind {
    const fn singleton(self) -> bool {
        matches!(self, Self::AnchoredSingleton | Self::NamedSingleton)
    }
}

struct NestedInput<'a> {
    collection: &'a [String],
    kind: InputKind,
}

impl<'a> NestedInput<'a> {
    /// Classifies an input the way the runtime resolves it: named paths by
    /// source name, singletons on the active item, and other repeating paths
    /// on the innermost frame declaring their first segment.
    fn classify(
        source: &'a CollectionJoinSource,
        frames: &AnchorFrames<'_, '_>,
        sources: &SourceExports<'_>,
    ) -> Result<Self, String> {
        let collection = source.collection();
        let singleton = source.cardinality() == JoinSourceCardinality::Singleton;
        let kind = if sources.is_named_extra_path(collection) {
            if singleton {
                InputKind::NamedSingleton
            } else {
                InputKind::NamedRepeating
            }
        } else if singleton {
            InputKind::AnchoredSingleton
        } else {
            let display = collection.join("/");
            let first = collection
                .first()
                .ok_or("nested repeating join collection is empty")?;
            let declaring = frames.declaring(first).map_err(|()| {
                format!("nested repeating join input `{display}` may be shadowed by an open source frame")
            })?;
            match declaring.as_slice() {
                [0] => InputKind::AnchoredRepeating,
                [position] if *position + 1 == frames.frames.len() => InputKind::OuterRepeating,
                [] => {
                    return Err(format!(
                        "nested repeating join input `{display}` is neither below the iteration nor in the primary source"
                    ));
                }
                _ => {
                    return Err(format!(
                        "nested repeating join input `{display}` is declared by more than one enclosing source frame"
                    ));
                }
            }
        };
        Ok(Self { collection, kind })
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
    frames: &AnchorFrames<'_, '_>,
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
    // Every possible source frame enclosing the join is checked conservatively.
    // Runtime lookup chooses the innermost frame owning this first segment.
    match frames.declaring(name) {
        Err(()) => Err(format!(
            "nested named join source `{name}` may be shadowed by an open source frame"
        )),
        Ok(declaring) => match declaring.first() {
            None => Ok(()),
            Some(&position) => Err(format!(
                "nested named join source `{name}` is shadowed by a {} source frame",
                frames.owner_label(position)
            )),
        },
    }
}
