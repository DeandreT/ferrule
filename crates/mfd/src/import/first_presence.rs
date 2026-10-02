//! Reconstruct a selected child item whose parent wire preserves empty output.

use std::collections::BTreeSet;

use ir::SchemaKind;

use super::graph::GraphBuilder;
use super::schema::{ComponentFormat, SchemaComponent, schema_node_at};

pub(super) enum Recognition {
    First(u32),
    Rejected,
}

struct Candidate<'a> {
    component: &'a SchemaComponent,
    output: u32,
    path: Vec<String>,
}

#[derive(Default)]
struct Trace {
    contexts: BTreeSet<usize>,
    positions: BTreeSet<usize>,
    exact: bool,
}

fn trace(
    feed: u32,
    candidates: &[Candidate<'_>],
    builder: &GraphBuilder<'_>,
    active: &mut BTreeSet<u32>,
) -> Trace {
    if active.len() >= 256 || !active.insert(feed) {
        return Trace::default();
    }
    let result = if let Some((index, candidate, path)) = candidates
        .iter()
        .enumerate()
        .filter_map(|(index, candidate)| {
            candidate
                .component
                .ports
                .get(&feed)
                .filter(|path| {
                    candidate.component.output_keys.contains(&feed)
                        && path.starts_with(&candidate.path)
                })
                .map(|path| (index, candidate, path))
        })
        .next()
    {
        let scalar = schema_node_at(&candidate.component.schema, path)
            .is_some_and(|node| node.is_scalar() && !node.repeating);
        let crosses_repetition = (candidate.path.len() + 1..=path.len()).any(|len| {
            schema_node_at(&candidate.component.schema, &path[..len])
                .is_some_and(|node| node.repeating)
        });
        Trace {
            contexts: BTreeSet::from([index]),
            exact: scalar && !crosses_repetition && !builder.xml_condition_ports.contains(&feed),
            ..Trace::default()
        }
    } else if let Some(&index) = builder.fn_by_output.get(&feed) {
        let component = &builder.fn_components[index];
        if component.library == "core"
            && component.name == "position"
            && component.inputs.len() == 1
            && let Some(input) = builder.input_feed(index, 0)
            && let Some(context) = candidates
                .iter()
                .position(|candidate| candidate.output == input)
        {
            Trace {
                contexts: BTreeSet::from([context]),
                positions: BTreeSet::from([context]),
                exact: !builder.xml_condition_ports.contains(&input),
            }
        } else {
            // Find selected-item dependencies even in unsupported functions so
            // a partial or inconsistent context cannot pass without a warning.
            let mut result = Trace::default();
            for input in component.inputs.iter().flatten() {
                if let Some(upstream) = builder.edge_from.get(input) {
                    let trace = trace(*upstream, candidates, builder, active);
                    result.contexts.extend(trace.contexts);
                    result.positions.extend(trace.positions);
                }
            }
            result
        }
    } else {
        Trace::default()
    };
    active.remove(&feed);
    result
}

fn reject(builder: &mut GraphBuilder<'_>, path: &[String], reason: &str) -> Recognition {
    builder.warnings.push(format!(
        "first-item parent-driven XML group `{}` cannot reconstruct its selected item: {reason}",
        path.join("/")
    ));
    Recognition::Rejected
}

/// Only typed copied payloads with one exact positional item context establish
/// this interpretation. Ordinary parent-driven construction remains untouched.
pub(super) fn reconstruct(
    target: &SchemaComponent,
    target_path: &[String],
    target_port: u32,
    parent_feed: u32,
    copy_all_targets: &BTreeSet<u32>,
    builder: &mut GraphBuilder<'_>,
) -> Option<Recognition> {
    let target_node = schema_node_at(&target.schema, target_path)?;
    if target.format != ComponentFormat::Xml
        || target_path.is_empty()
        || target_node.repeating
        || !matches!(target_node.kind, SchemaKind::Group { .. })
    {
        return None;
    }
    // A copied-variable driver already carries its selected sequence and is
    // handled by the ordinary group classifier. This pattern starts at the
    // raw, repeated XML parent that preserves a constructed empty child.
    if !builder.sources.iter().any(|source| {
        source.format == ComponentFormat::Xml
            && source.output_keys.contains(&parent_feed)
            && source.ports.get(&parent_feed).is_some_and(|path| {
                schema_node_at(&source.schema, path).is_some_and(|node| {
                    node.repeating && matches!(node.kind, SchemaKind::Group { .. })
                })
            })
    }) {
        return None;
    }
    let mut candidates = Vec::new();
    for component in builder.intermediates {
        if component.format != ComponentFormat::Xml
            || !component.is_variable
            || component.is_pass_through
        {
            continue;
        }
        for output in &component.output_keys {
            let Some(path) = component.ports.get(output) else {
                continue;
            };
            if !schema_node_at(&component.schema, path)
                .is_some_and(|node| node.repeating && matches!(node.kind, SchemaKind::Group { .. }))
            {
                continue;
            }
            let Some(intermediate) = builder.intermediate_feed(*output) else {
                continue;
            };
            if intermediate.suffix.is_empty()
                && (intermediate.control == Some(parent_feed)
                    || builder
                        .resolve_iteration_feed(*output)
                        .has_terminal_default_first())
                && !candidates.iter().any(|candidate: &Candidate<'_>| {
                    std::ptr::eq(candidate.component, *component) && candidate.path == *path
                })
            {
                candidates.push(Candidate {
                    component,
                    output: *output,
                    path: path.clone(),
                });
            }
        }
    }
    if candidates.is_empty() {
        return None;
    }
    let mut contexts = BTreeSet::new();
    let mut positions = BTreeSet::new();
    let mut all_exact = true;
    let mut leaves = 0usize;
    for (input, path) in &target.ports {
        if path.len() <= target_path.len()
            || !path.starts_with(target_path)
            || !target
                .input_ancestors
                .get(input)
                .is_some_and(|ports| ports.contains(&target_port))
        {
            continue;
        }
        let Some(feed) = builder.edge_from.get(input) else {
            continue;
        };
        if !schema_node_at(&target.schema, path).is_some_and(|node| node.is_scalar()) {
            all_exact = false;
            continue;
        }
        let trace = trace(*feed, &candidates, builder, &mut BTreeSet::new());
        contexts.extend(trace.contexts);
        positions.extend(trace.positions);
        all_exact &= trace.exact;
        all_exact &= !copy_all_targets.contains(input);
        all_exact &= !builder.xml_condition_ports.contains(input);
        all_exact &= !(target_path.len() + 1..=path.len()).any(|len| {
            schema_node_at(&target.schema, &path[..len]).is_some_and(|node| node.repeating)
        });
        leaves += 1;
    }
    if contexts.is_empty() {
        return None;
    }
    if contexts.len() != 1 {
        return Some(reject(
            builder,
            target_path,
            "scalar leaves use different typed variables",
        ));
    }
    let context = *contexts.first().expect("one context");
    if !all_exact || leaves == 0 || positions != BTreeSet::from([context]) {
        return Some(reject(
            builder,
            target_path,
            "scalar leaves do not all use one direct typed payload/position context",
        ));
    }
    if copy_all_targets.contains(&target_port) {
        return Some(reject(
            builder,
            target_path,
            "its parent presence driver requests a whole-group copy",
        ));
    }
    let candidate = &candidates[context];
    let intermediate = builder.intermediate_feed(candidate.output)?;
    if intermediate.control != Some(parent_feed) {
        return Some(reject(
            builder,
            target_path,
            "its variable has a different compute-when parent",
        ));
    }
    let inputs = candidate
        .component
        .input_keys
        .iter()
        .filter(|input| {
            candidate.component.ports.get(input) == Some(&candidate.path)
                && builder.edge_from.get(input) == Some(&intermediate.feed)
                && copy_all_targets.contains(input)
        })
        .count();
    if inputs != 1 || !intermediate.projections.is_empty() {
        return Some(reject(
            builder,
            target_path,
            "its variable is not one structural copied payload",
        ));
    }
    if candidate.component.ports.iter().any(|(port, path)| {
        builder.xml_condition_ports.contains(port)
            && (path.starts_with(&candidate.path) || candidate.path.starts_with(path))
    }) || target.ports.iter().any(|(port, path)| {
        builder.xml_condition_ports.contains(port)
            && (*port == target_port
                || target
                    .input_ancestors
                    .get(port)
                    .is_some_and(|ports| ports.contains(&target_port))
                || target
                    .input_ancestors
                    .get(&target_port)
                    .is_some_and(|ports| ports.contains(port)))
            && (path.starts_with(target_path) || target_path.starts_with(path))
    }) {
        return Some(reject(
            builder,
            target_path,
            "its type conditions are not qualified for parent-driven presence",
        ));
    }
    let (Some(parent), Some(selected)) = (
        builder.sequence_source_path(parent_feed),
        builder.iteration_source_path(&builder.resolve_iteration_feed(candidate.output)),
    ) else {
        return Some(reject(
            builder,
            target_path,
            "source ownership cannot be resolved",
        ));
    };
    if parent.source != selected.source
        || !selected.path.starts_with(&parent.path)
        || selected.path.len() <= parent.path.len()
        || !builder.sources.get(parent.source).is_some_and(|source| {
            source.format == ComponentFormat::Xml
                && (parent.path.len() + 1..=selected.path.len()).any(|len| {
                    schema_node_at(&source.schema, &selected.path[..len])
                        .is_some_and(|node| node.repeating)
                })
        })
    {
        return Some(reject(
            builder,
            target_path,
            "the selected payload does not cross a fresh child repetition",
        ));
    }
    if builder.sources.get(parent.source).is_some_and(|source| {
        source.ports.iter().any(|(port, path)| {
            builder.xml_condition_ports.contains(port)
                && (path.starts_with(&selected.path) || selected.path.starts_with(path))
        })
    }) {
        return Some(reject(
            builder,
            target_path,
            "its source payload or raw parent has an unqualified entry condition",
        ));
    }
    let parent_owned = target.ports.iter().any(|(input, path)| {
        path.len() < target_path.len()
            && target_path.starts_with(path)
            && target
                .input_ancestors
                .get(&target_port)
                .is_some_and(|ports| ports.contains(input))
            && builder.edge_from.get(input) == Some(&parent_feed)
            && schema_node_at(&target.schema, path).is_some_and(|node| node.repeating)
    });
    if !parent_owned {
        return Some(reject(
            builder,
            target_path,
            "the presence driver is not the current repeating parent",
        ));
    }
    let resolved = builder.resolve_iteration_feed(candidate.output);
    if !resolved.has_terminal_default_first() {
        return Some(reject(
            builder,
            target_path,
            "the copied child sequence has no terminal default first-items control",
        ));
    }
    if !super::group_projection::mapped_group_sequence(
        target,
        target_path,
        builder,
        &resolved,
        false,
    ) {
        return Some(reject(
            builder,
            target_path,
            "its selected sequence has unsupported controls or projections",
        ));
    }
    Some(Recognition::First(candidate.output))
}
