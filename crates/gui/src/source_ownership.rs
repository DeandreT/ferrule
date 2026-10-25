//! Native authoring of explicitly owned source reads.
//!
//! A legacy source path names its endpoint only by leading path text: a named
//! input is addressed as `<alias>/<path>`, and lookup falls outward through
//! active items and the primary input before the named namespace. A primary
//! path that starts with a named alias, or an alias that the primary input or
//! another input can define, therefore resolves ambiguously. Pins of those
//! endpoints author owned reads that name their endpoint exactly. Once a
//! project holds any owned form, every addressable pin authors owned reads.
//! Projects without either condition keep authoring legacy paths unchanged.

use std::collections::BTreeSet;
use std::path::Path;

use ir::{SchemaKind, SchemaNode};
use mapping::{NamedSource, OwnedPosition, OwnedSourceField, OwnedSourceRef, Project, SourceOwner};

use crate::canvas::SourceLeaf;

/// How wiring one source pin reads its value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PinRead {
    /// The unchanged legacy `SourceField` frame and path of the pin.
    Legacy,
    /// An exact owner with its endpoint-local frame and absolute path.
    Owned(OwnedSourceField),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NamedAccess {
    /// No other endpoint can define this alias.
    Unambiguous,
    /// Another endpoint can define this alias; only an owned read is exact.
    Shadowed,
    /// Neither form can address this input; the reason names the conflict.
    Unavailable(&'static str),
    /// Documents load per driver item through an owned dynamic driver; the
    /// editor does not author reads of them.
    OwnedDynamic,
}

#[derive(Debug, Clone)]
struct NamedEndpoint {
    name: String,
    access: NamedAccess,
    /// Owned reads select static roots only; per-item documents stay legacy.
    static_root: bool,
}

/// One owner-qualified repeating collection offered by owned pickers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OwnedCollectionChoice {
    pub(crate) source: OwnedSourceRef,
    pub(crate) label: String,
}

/// Endpoint addressability for one project's sources.
#[derive(Debug, Clone, Default)]
pub(crate) struct SourceOwnership {
    named: Vec<NamedEndpoint>,
    /// Aliases a primary path may begin with, so primary text can name them.
    primary_aliases: BTreeSet<String>,
    uses_ownership: bool,
    collections: Vec<OwnedCollectionChoice>,
    /// Saved owned dynamic drivers, shown read-only beside their loading scopes.
    owned_dynamic: Vec<(String, mapping::DynamicSourcePath)>,
}

/// Whether the project already holds an owned graph node, scope or failure walk.
pub(crate) fn project_uses_ownership(project: &Project) -> bool {
    project
        .graph
        .nodes
        .values()
        .any(|node| node.owned_source_kind().is_some())
        || mapping::first_project_owned_source_scope(project).is_some()
        || project
            .failure_rules
            .iter()
            .any(|rule| rule.iteration.owned_source().is_some())
}

impl SourceOwnership {
    pub(crate) fn new(primary: &SchemaNode, sources: &[NamedSource]) -> Self {
        let named = sources
            .iter()
            .enumerate()
            .map(|(index, source)| {
                let static_root = source.dynamic_path.is_none();
                let owned_dynamic = source
                    .dynamic_path
                    .as_ref()
                    .is_some_and(|dynamic| dynamic.owned_driver().is_some());
                let access = match crate::canvas::source_alias_conflict(primary, sources, index) {
                    _ if owned_dynamic => NamedAccess::OwnedDynamic,
                    None => NamedAccess::Unambiguous,
                    Some(conflict) if !static_root || !alias_is_addressable(sources, index) => {
                        NamedAccess::Unavailable(conflict)
                    }
                    Some(_) => NamedAccess::Shadowed,
                };
                NamedEndpoint {
                    name: source.name.clone(),
                    access,
                    static_root,
                }
            })
            .collect::<Vec<_>>();
        let primary_aliases = match &primary.kind {
            SchemaKind::Group { children, .. } => children
                .iter()
                .filter(|child| sources.iter().any(|source| source.name == child.name))
                .map(|child| child.name.clone())
                .collect(),
            SchemaKind::Scalar { .. } | SchemaKind::ScalarUnion { .. } => BTreeSet::new(),
        };
        let mut collections = Vec::new();
        collections.push(OwnedCollectionChoice {
            source: OwnedSourceRef {
                owner: SourceOwner::Primary,
                path: Vec::new(),
            },
            label: collection_label(&SourceOwner::Primary, &[]),
        });
        add_repeating(primary, &SourceOwner::Primary, &mut collections);
        for (source, endpoint) in sources.iter().zip(&named) {
            if !endpoint.static_root || matches!(endpoint.access, NamedAccess::Unavailable(_)) {
                continue;
            }
            let owner = SourceOwner::Named {
                name: source.name.clone(),
            };
            if is_row_collection(source) {
                collections.push(OwnedCollectionChoice {
                    label: collection_label(&owner, &[]),
                    source: OwnedSourceRef {
                        owner: owner.clone(),
                        path: Vec::new(),
                    },
                });
            }
            add_repeating(&source.schema, &owner, &mut collections);
        }
        let owned_dynamic = sources
            .iter()
            .filter_map(|source| {
                let dynamic = source.dynamic_path.as_ref()?;
                dynamic
                    .owned_driver()
                    .is_some()
                    .then(|| (source.name.clone(), dynamic.clone()))
            })
            .collect();
        Self {
            named,
            primary_aliases,
            uses_ownership: false,
            collections,
            owned_dynamic,
        }
    }

    /// The saved dynamic path of an owner whose documents load through an
    /// owned driver.
    pub(crate) fn owned_dynamic_driver(
        &self,
        owner: &SourceOwner,
    ) -> Option<&mapping::DynamicSourcePath> {
        let SourceOwner::Named { name } = owner else {
            return None;
        };
        self.owned_dynamic
            .iter()
            .find(|(candidate, _)| candidate == name)
            .map(|(_, dynamic)| dynamic)
    }

    pub(crate) fn with_project_usage(mut self, uses_ownership: bool) -> Self {
        self.uses_ownership = uses_ownership;
        self
    }

    /// Whether collection and position choices author owner-qualified forms:
    /// the project already uses ownership, or some endpoint path is ambiguous.
    pub(crate) fn active(&self) -> bool {
        self.uses_ownership
            || !self.primary_aliases.is_empty()
            || self
                .named
                .iter()
                .any(|endpoint| endpoint.access == NamedAccess::Shadowed)
    }

    pub(crate) fn collections(&self) -> &[OwnedCollectionChoice] {
        &self.collections
    }

    /// The owner of a source block: zero is primary, others are named inputs.
    pub(crate) fn block_owner(&self, block: usize) -> Option<SourceOwner> {
        match block.checked_sub(1) {
            None => Some(SourceOwner::Primary),
            Some(index) => self.named.get(index).map(|endpoint| SourceOwner::Named {
                name: endpoint.name.clone(),
            }),
        }
    }

    /// The reason a named block cannot be wired in either form.
    pub(crate) fn connection_error(&self, block: usize) -> Option<String> {
        let endpoint = self.named.get(block.checked_sub(1)?)?;
        if endpoint.access == NamedAccess::OwnedDynamic {
            return Some(format!(
                "Named input `{}` loads one document per driver item through an owned dynamic driver; its reads are not authorable here. Existing bindings are unchanged.",
                endpoint.name
            ));
        }
        let NamedAccess::Unavailable(conflict) = endpoint.access else {
            return None;
        };
        Some(format!(
            "Named input `{}` is unavailable: its name/path conflicts with {conflict}. Rename the input to use an unambiguous name. Existing bindings are unchanged.",
            endpoint.name
        ))
    }

    /// Whether wires from a named block author owned reads.
    pub(crate) fn block_reads_owned(&self, block: usize) -> bool {
        match block.checked_sub(1) {
            None => self.uses_ownership,
            Some(index) => self.named.get(index).is_some_and(|endpoint| {
                endpoint.static_root
                    && match endpoint.access {
                        NamedAccess::Shadowed => true,
                        NamedAccess::Unambiguous => self.uses_ownership,
                        NamedAccess::Unavailable(_) | NamedAccess::OwnedDynamic => false,
                    }
            }),
        }
    }

    /// The read one pin of `block` authors. Endpoint-local paths drop the
    /// legacy alias prefix that named catalogs add to their pins.
    pub(crate) fn pin_read(&self, block: usize, leaf: &SourceLeaf) -> Result<PinRead, String> {
        if let Some(error) = self.connection_error(block) {
            return Err(error);
        }
        let owned = match block.checked_sub(1) {
            None => self.uses_ownership || self.primary_leaf_collides(leaf),
            Some(_) => self.block_reads_owned(block),
        };
        if !owned {
            return Ok(PinRead::Legacy);
        }
        let owner = self
            .block_owner(block)
            .ok_or_else(|| format!("source block {block} does not exist"))?;
        let (frame, suffix) = match &owner {
            SourceOwner::Primary => (leaf.frame.clone(), leaf.path.clone()),
            SourceOwner::Named { name } => local_leaf(name, leaf)
                .ok_or_else(|| format!("source pin `{}` is not owned by `{name}`", leaf.label))?,
        };
        // The pin's innermost repetition is the frame an enclosing owned
        // iteration activates; without one the read starts at the root.
        let mut path = frame.clone().unwrap_or_default();
        path.extend(suffix);
        Ok(PinRead::Owned(OwnedSourceField {
            source: OwnedSourceRef { owner, path },
            frame,
        }))
    }

    fn primary_leaf_collides(&self, leaf: &SourceLeaf) -> bool {
        leaf.frame
            .as_deref()
            .unwrap_or(&leaf.path)
            .first()
            .or_else(|| leaf.path.first())
            .is_some_and(|first| self.primary_aliases.contains(first))
    }

    /// A new owned position for the palette, or None when positions stay legacy.
    pub(crate) fn palette_position(&self) -> Option<OwnedPosition> {
        self.uses_ownership.then_some(OwnedPosition {
            owner: SourceOwner::Primary,
            collection: None,
        })
    }
}

/// Duplicate or blank aliases cannot select one owner either.
fn alias_is_addressable(sources: &[NamedSource], index: usize) -> bool {
    let Some(source) = sources.get(index) else {
        return false;
    };
    !source.name.trim().is_empty()
        && sources
            .iter()
            .enumerate()
            .all(|(other, candidate)| other == index || candidate.name.trim() != source.name.trim())
}

fn local_leaf(name: &str, leaf: &SourceLeaf) -> Option<(Option<Vec<String>>, Vec<String>)> {
    match &leaf.frame {
        Some(frame) => {
            let (first, rest) = frame.split_first()?;
            (first == name).then(|| (Some(rest.to_vec()), leaf.path.clone()))
        }
        None => {
            let (first, rest) = leaf.path.split_first()?;
            (first == name).then(|| (None, rest.to_vec()))
        }
    }
}

fn add_repeating(
    schema: &SchemaNode,
    owner: &SourceOwner,
    collections: &mut Vec<OwnedCollectionChoice>,
) {
    let mut pending = vec![(schema, Vec::new())];
    let mut found = Vec::new();
    while let Some((node, path)) = pending.pop() {
        let SchemaKind::Group { children, .. } = &node.kind else {
            continue;
        };
        for child in children.iter().rev() {
            let mut child_path = path.clone();
            child_path.push(child.name.clone());
            if child.repeating {
                found.push(child_path.clone());
            }
            pending.push((child, child_path));
        }
    }
    found.sort();
    collections.extend(found.into_iter().map(|path| OwnedCollectionChoice {
        label: collection_label(owner, &path),
        source: OwnedSourceRef {
            owner: owner.clone(),
            path,
        },
    }));
}

pub(crate) fn owner_label(owner: &SourceOwner) -> String {
    match owner {
        SourceOwner::Primary => "primary".to_owned(),
        SourceOwner::Named { name } => name.clone(),
    }
}

pub(crate) fn collection_label(owner: &SourceOwner, path: &[String]) -> String {
    let path = if path.is_empty() {
        "<rows>".to_owned()
    } else {
        path.join(" / ")
    };
    format!("{}: {path}", owner_label(owner))
}

fn is_row_collection(source: &NamedSource) -> bool {
    if source.schema.repeating {
        return true;
    }
    let extension = Path::new(&source.path)
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default();
    matches!(
        extension.to_ascii_lowercase().as_str(),
        "csv" | "db" | "sqlite" | "sqlite3"
    )
}

#[cfg(test)]
#[path = "source_ownership_tests.rs"]
mod tests;
