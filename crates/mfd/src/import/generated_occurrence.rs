use std::collections::{BTreeMap, BTreeSet};

use ir::{SchemaKind, XML_TEXT_FIELD};
use mapping::IterationOutput;

use super::function::{
    aggregate_op, is_distinct_values, is_filter, is_first_items, is_group_into_blocks,
    is_group_starting_with, is_input, is_sequence_producer, is_sort, produces_scalar,
};
use super::graph::GraphBuilder;
use super::group_projection::TargetIteration;
use super::schema::{ComponentFormat, SchemaComponent, schema_node_at};

/// Infers generated XML occurrence scopes whose sequence is visible only
/// through computed descendant feeds. MapForce permits these mappings to emit
/// multiple physical elements even when the target XSD declares one element.
pub(super) fn infer(
    target: &SchemaComponent,
    builder: &mut GraphBuilder<'_>,
    iterations: &mut Vec<TargetIteration>,
) {
    if target.format != ComponentFormat::Xml {
        return;
    }

    let mut candidates: BTreeMap<Vec<String>, BTreeSet<usize>> = BTreeMap::new();
    for (input, target_path) in &target.ports {
        let Some(feed) = builder.edge_from.get(input).copied() else {
            continue;
        };
        let producers = sequence_dependencies(builder, feed);
        if producers.is_empty() {
            continue;
        }
        let Some(group_path) = nearest_occurrence_group(target, target_path) else {
            continue;
        };
        candidates.entry(group_path).or_default().extend(producers);
    }

    let candidate_paths = candidates.keys().cloned().collect::<Vec<_>>();
    for (target_path, producers) in candidates {
        if producers.len() != 1 {
            builder.warnings.push(format!(
                "computed target group `{}` depends on multiple generated sequences; occurrence inference skipped",
                target_path.join("/")
            ));
            continue;
        }
        if iterations.iter().any(|iteration| {
            iteration.target_path == target_path || iteration.target_path.starts_with(&target_path)
        }) {
            builder.warnings.push(format!(
                "computed target group `{}` already contains an iteration; generated occurrence inference skipped",
                target_path.join("/")
            ));
            continue;
        }
        if candidate_paths.iter().any(|other| {
            other != &target_path
                && (other.starts_with(&target_path) || target_path.starts_with(other))
        }) {
            builder.warnings.push(format!(
                "computed target group `{}` overlaps another generated occurrence scope; inference skipped",
                target_path.join("/")
            ));
            continue;
        }
        let Some(producer) = producers.iter().next().copied() else {
            continue;
        };
        let Some(component) = builder.fn_components.get(producer) else {
            continue;
        };
        if !sequence_inputs_are_scalar(builder, component) {
            builder.warnings.push(format!(
                "generated sequence feeding `{}` has sequence-valued or controlled inputs; occurrence inference skipped",
                target_path.join("/")
            ));
            continue;
        }
        let Some(feed) = component.outputs.first().copied() else {
            continue;
        };
        iterations.push(TargetIteration {
            target_path,
            feed,
            output: IterationOutput::MappedSequence,
            projects_whole_group: false,
            join: None,
        });
    }
}

fn nearest_occurrence_group(
    target: &SchemaComponent,
    target_path: &[String],
) -> Option<Vec<String>> {
    if target_path.is_empty() {
        return None;
    }
    if schema_node_at(&target.schema, target_path).is_some_and(|node| {
        !node.repeating
            && matches!(&node.kind, SchemaKind::Group { children, .. } if children.iter().any(|child| child.name == XML_TEXT_FIELD))
    }) {
        return Some(target_path.to_vec());
    }
    (1..target_path.len()).rev().find_map(|length| {
        let path = &target_path[..length];
        schema_node_at(&target.schema, path)
            .is_some_and(|node| !node.repeating && matches!(node.kind, SchemaKind::Group { .. }))
            .then(|| path.to_vec())
    })
}

fn sequence_dependencies(builder: &GraphBuilder<'_>, feed: u32) -> BTreeSet<usize> {
    fn visit(
        builder: &GraphBuilder<'_>,
        feed: u32,
        visited: &mut BTreeSet<u32>,
        producers: &mut BTreeSet<usize>,
    ) {
        if !visited.insert(feed) {
            return;
        }
        let Some(index) = builder.fn_by_output.get(&feed).copied() else {
            return;
        };
        let Some(component) = builder.fn_components.get(index) else {
            return;
        };
        if is_sequence_producer(component) {
            producers.insert(index);
            return;
        }
        if !is_plain_scalar_component(component) {
            return;
        }
        for input in component.inputs.iter().flatten() {
            if let Some(upstream) = builder.edge_from.get(input).copied() {
                visit(builder, upstream, visited, producers);
            }
        }
    }

    let mut producers = BTreeSet::new();
    visit(builder, feed, &mut BTreeSet::new(), &mut producers);
    producers
}

fn is_plain_scalar_component(component: &super::function::FnComponent) -> bool {
    produces_scalar(component)
        && component.name != "exists"
        && aggregate_op(&component.name).is_none()
        && !is_filter(component)
        && !is_sort(component)
        && !is_first_items(component)
        && !is_group_into_blocks(component)
        && !is_group_starting_with(component)
        && !is_distinct_values(component)
}

fn sequence_inputs_are_scalar(
    builder: &GraphBuilder<'_>,
    component: &super::function::FnComponent,
) -> bool {
    component.inputs.iter().flatten().all(|input| {
        builder
            .edge_from
            .get(input)
            .copied()
            .is_none_or(|feed| scalar_feed_is_plain(builder, feed, &mut BTreeSet::new()))
    })
}

fn scalar_feed_is_plain(
    builder: &GraphBuilder<'_>,
    feed: u32,
    visited: &mut BTreeSet<u32>,
) -> bool {
    if !visited.insert(feed) {
        return false;
    }
    let result = if let Some(index) = builder.fn_by_output.get(&feed).copied() {
        let Some(component) = builder.fn_components.get(index) else {
            visited.remove(&feed);
            return false;
        };
        if is_input(component) || is_plain_scalar_component(component) {
            component.inputs.iter().flatten().all(|input| {
                builder
                    .edge_from
                    .get(input)
                    .copied()
                    .is_none_or(|upstream| scalar_feed_is_plain(builder, upstream, visited))
            })
        } else {
            false
        }
    } else {
        true
    };
    visited.remove(&feed);
    result
}
