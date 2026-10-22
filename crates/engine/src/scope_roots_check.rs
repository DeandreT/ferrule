//! Side-by-side checks of the shared scope-root visitor against the
//! hand-written scope walks it is meant to replace.
//!
//! Every engine test that validates or runs a project calls
//! [`assert_equivalent`], and the corpus tests below run it over each
//! serialized Project fixture and each conformance model. Callers keep their
//! hand-written walks until a later change switches them; each walk's
//! visitor form names the exact slots that walk reads.

use std::collections::{BTreeMap, BTreeSet};

// The included conformance modules resolve these names through `super::*`,
// as they do in the code-generation harness.
use ir::{ScalarType, SchemaNode, Value};
use mapping::{
    Binding, Graph, Node, NodeId, Project, Scope, ScopeIteration, ScopeRootSlot as Slot,
};

use crate::TargetSelection;

fn string(name: &str) -> SchemaNode {
    SchemaNode::scalar(name, ScalarType::String)
}

fn int(name: &str) -> SchemaNode {
    SchemaNode::scalar(name, ScalarType::Int)
}

fn bool_(name: &str) -> SchemaNode {
    SchemaNode::scalar(name, ScalarType::Bool)
}

/// The slots a scope's owned-capability probe reads.
fn owned_probe_slot(slot: Slot) -> bool {
    matches!(
        slot,
        Slot::Binding(_)
            | Slot::ScalarValue
            | Slot::Filter
            | Slot::Presence
            | Slot::GroupBy
            | Slot::GroupAdjacentBy
            | Slot::GroupStartingWith
            | Slot::GroupEndingWith
            | Slot::GroupIntoBlocks
            | Slot::PostGroupFilter
            | Slot::SortBy
            | Slot::SortThenBy(_)
            | Slot::Window(_)
    )
}

/// The slots whose join references are validated together for one scope.
fn join_validation_slot(slot: Slot) -> bool {
    matches!(
        slot,
        Slot::Filter
            | Slot::PostGroupFilter
            | Slot::GroupBy
            | Slot::GroupAdjacentBy
            | Slot::GroupStartingWith
            | Slot::GroupEndingWith
            | Slot::GroupIntoBlocks
            | Slot::SortBy
            | Slot::OutputPath
            | Slot::SortThenBy(_)
            | Slot::Binding(_)
            | Slot::DynamicBindingKey(_)
            | Slot::DynamicBindingValue(_)
            | Slot::DynamicChildKey(_)
    )
}

fn sorted_roots(scope: &Scope, admit: impl Fn(Slot) -> bool) -> Vec<NodeId> {
    let mut roots = Vec::new();
    scope.visit_roots(&mut |root| {
        if admit(root.slot) {
            roots.push(root.node);
        }
    });
    roots.sort_unstable();
    roots
}

fn sorted(mut roots: Vec<NodeId>) -> Vec<NodeId> {
    roots.sort_unstable();
    roots
}

/// The nodes pruning keeps, found from the visitor's roots.
fn reachable_through_visitor(project: &Project) -> BTreeSet<NodeId> {
    let mut pending = Vec::new();
    let mut push = |root: mapping::ScopeRoot| pending.push(root.node);
    project.root.visit_tree_roots(&mut push);
    for target in &project.extra_targets {
        target.root.visit_tree_roots(&mut push);
    }
    for rule in &project.failure_rules {
        rule.visit_roots(&mut push);
    }
    pending.extend(
        project
            .extra_sources
            .iter()
            .filter_map(|source| source.dynamic_path.as_ref())
            .map(|path| path.node),
    );
    let mut reachable = BTreeSet::new();
    while let Some(id) = pending.pop() {
        if let Some(node) = project.graph.nodes.get(&id)
            && reachable.insert(id)
        {
            pending.extend(node.dependencies());
        }
    }
    reachable
}

fn each_scope<'a>(scope: &'a Scope, visit: &mut impl FnMut(&'a Scope)) {
    visit(scope);
    for (_, nested) in scope.nested_scopes() {
        each_scope(nested, visit);
    }
}

/// Panics when any hand-written scope walk and its visitor form disagree.
pub(crate) fn assert_equivalent(project: &Project) {
    let mut pruned = project.clone();
    pruned.prune_unreachable_nodes();
    assert_eq!(
        pruned.graph.nodes.keys().copied().collect::<BTreeSet<_>>(),
        reachable_through_visitor(project),
        "reachability"
    );

    let selections = std::iter::once(TargetSelection::Primary).chain(
        project
            .extra_targets
            .iter()
            .map(|target| TargetSelection::Named(&target.name)),
    );
    for selection in selections {
        let legacy = crate::required_sources::analysis_snapshot(project, selection, false);
        let visitor = crate::required_sources::analysis_snapshot(project, selection, true);
        match (legacy, visitor) {
            (Ok(legacy), Ok(visitor)) => assert_eq!(legacy, visitor, "required sources"),
            (Err(legacy), Err(visitor)) => {
                assert_eq!(legacy.to_string(), visitor.to_string(), "target selection")
            }
            (legacy, visitor) => panic!("required sources: {legacy:?} versus {visitor:?}"),
        }
    }

    let roots = std::iter::once(&project.root).chain(project.extra_targets.iter().map(|t| &t.root));
    for root in roots {
        each_scope(root, &mut |scope| {
            assert_eq!(
                sorted(crate::eval_scope::owned_probe_roots(scope)),
                sorted_roots(scope, owned_probe_slot),
                "owned-capability probe roots"
            );
            assert_eq!(
                sorted(crate::validate::scope_join_roots(scope).collect()),
                sorted_roots(scope, join_validation_slot),
                "join validation roots"
            );
        });
    }
}

#[path = "../../mapping/src/tests/project_corpus.rs"]
mod project_corpus;

#[test]
fn serialized_projects_agree_with_every_hand_written_walk() {
    let corpus = project_corpus::projects(|text| mapping::project_file::decode_str(text).ok());
    assert!(
        corpus.len() >= 50,
        "expected the repository's serialized Project fixtures, found {}",
        corpus.len()
    );
    for (path, project) in &corpus {
        let checked =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| assert_equivalent(project)));
        assert!(checked.is_ok(), "{} diverged", path.display());
    }
}

// The conformance corpora author their Projects in Rust. These modules are
// included unchanged, as the code-generation harness includes them; helpers
// used only by that harness stay unused here.
#[rustfmt::skip]
#[allow(dead_code)]
#[path = "../../../conformance/basic-owned-adjacency-tree/projects.rs"]
mod adjacency_tree;
#[rustfmt::skip]
#[allow(dead_code)]
#[path = "../../../conformance/basic-owned-collection-aggregates/projects.rs"]
mod collection_aggregates;
#[rustfmt::skip]
#[allow(dead_code)]
#[path = "../../../conformance/basic-owned-dynamic-field/projects.rs"]
mod dynamic_field;
#[rustfmt::skip]
#[allow(dead_code)]
#[path = "../../../conformance/basic-owned-failure-sources/projects.rs"]
mod failure_sources;
#[rustfmt::skip]
#[allow(dead_code)]
#[path = "../../../conformance/basic-owned-group-adjacent-by/projects.rs"]
mod group_adjacent_by;
#[rustfmt::skip]
#[allow(dead_code)]
#[path = "../../../conformance/basic-owned-group-by/projects.rs"]
mod group_by;
#[rustfmt::skip]
#[allow(dead_code)]
#[path = "../../../conformance/basic-owned-group-ending-with/projects.rs"]
mod group_ending_with;
#[rustfmt::skip]
#[allow(dead_code)]
#[path = "../../../conformance/basic-owned-group-into-blocks/projects.rs"]
mod group_into_blocks;
#[rustfmt::skip]
#[allow(dead_code, clippy::possible_missing_else)]
#[path = "../../../conformance/basic-owned-group-starting-with/projects.rs"]
mod group_starting_with;
#[rustfmt::skip]
#[allow(dead_code)]
#[path = "../../../conformance/basic-owned-joins/projects.rs"]
mod joins;
#[rustfmt::skip]
#[allow(dead_code)]
#[path = "../../../conformance/basic-owned-lookup-find/projects.rs"]
mod lookup_find;
#[rustfmt::skip]
#[allow(dead_code)]
#[path = "../../../conformance/basic-owned-path-hierarchy/projects.rs"]
mod path_hierarchy;
#[rustfmt::skip]
#[allow(dead_code)]
#[path = "../../../conformance/basic-owned-recursive-collect/projects.rs"]
mod recursive_collect;
#[rustfmt::skip]
#[allow(dead_code)]
#[path = "../../../conformance/basic-owned-recursive-filter/projects.rs"]
mod recursive_filter;
#[rustfmt::skip]
#[allow(dead_code)]
#[path = "../../../conformance/basic-owned-scalar-construction/projects.rs"]
mod scalar_construction;
#[rustfmt::skip]
#[allow(dead_code)]
#[path = "../../../conformance/basic-owned-sequence-aggregate/projects.rs"]
mod sequence_aggregate;
#[rustfmt::skip]
#[allow(dead_code)]
#[path = "../../../conformance/basic-owned-sequence-exists/projects.rs"]
mod sequence_exists;
#[rustfmt::skip]
#[allow(dead_code)]
#[path = "../../../conformance/basic-owned-sequence-item-at/projects.rs"]
mod sequence_item_at;
#[rustfmt::skip]
#[allow(dead_code)]
#[path = "../../../conformance/basic-owned-xml-mixed-construction/projects.rs"]
mod xml_mixed_construction;
#[rustfmt::skip]
#[allow(dead_code)]
#[path = "../../../conformance/basic-owned-xml-mixed-content/projects.rs"]
mod xml_mixed_content;
#[rustfmt::skip]
#[allow(dead_code)]
#[path = "../../../conformance/basic-owned-xml-serialize/projects.rs"]
mod xml_serialize;

/// The group-by corpus derives its profiles from its cases; these are the
/// profiles its `project` function constructs.
const GROUP_BY_PROFILES: &[&str] = &[
    "root-repeated-scalar",
    "root-singleton",
    "scalar-values",
    "legacy-named-repeated-root",
    "legacy-named-repeated-root-pin",
    "basic-primary",
    "tagged-keys",
    "basic-named",
    "basic-unicode",
    "legacy-members",
    "post-primary",
    "post-named",
    "sorted-post",
    "sorted-post-last",
    "filter-before-sort",
    "sort-before-filter",
    "sorted-position-key",
    "raw-gap-filter",
    "legacy-key-only",
    "legacy-post-only",
    "legacy-island",
    "batch-per-parent",
    "batch-flat",
    "phase",
    "phase-prefilter",
    "phase-sorted",
    "phase-first-zero",
    "phase-bad-bound",
    "phase-bound-raise",
    "phase-target",
    "phase-post-string",
];

fn conformance_projects() -> Vec<(String, Project)> {
    fn each(
        corpus: &str,
        profiles: &[&str],
        project: fn(&str) -> Project,
    ) -> impl Iterator<Item = (String, Project)> {
        profiles
            .iter()
            .map(move |profile| (format!("{corpus}/{profile}"), project(profile)))
    }
    let mut projects: Vec<(String, Project)> = Vec::new();
    projects.extend(each(
        "adjacency-tree",
        adjacency_tree::MODELS,
        adjacency_tree::project,
    ));
    projects.extend(each(
        "collection-aggregates",
        collection_aggregates::PROFILES,
        collection_aggregates::project,
    ));
    projects.extend(each(
        "dynamic-field",
        dynamic_field::PROFILES,
        dynamic_field::project,
    ));
    projects.extend(each(
        "failure-sources",
        failure_sources::PROFILES,
        failure_sources::project,
    ));
    projects.extend(each(
        "group-adjacent-by",
        group_adjacent_by::PROFILES,
        group_adjacent_by::project,
    ));
    projects.extend(each("group-by", GROUP_BY_PROFILES, group_by::project));
    projects.extend(each(
        "group-ending-with",
        group_ending_with::PROFILES,
        group_ending_with::project,
    ));
    projects.extend(each(
        "group-into-blocks",
        group_into_blocks::PROFILES,
        group_into_blocks::project,
    ));
    projects.extend(each(
        "group-starting-with",
        group_starting_with::PROFILES,
        group_starting_with::project,
    ));
    projects.extend(each("joins", joins::PROFILES, joins::project));
    projects.extend(each(
        "lookup-find",
        lookup_find::PROFILES,
        lookup_find::project,
    ));
    projects.extend(each(
        "path-hierarchy",
        path_hierarchy::MODELS,
        path_hierarchy::project,
    ));
    projects.extend(each(
        "recursive-collect",
        recursive_collect::PROFILES,
        recursive_collect::project,
    ));
    projects.extend(each(
        "recursive-filter",
        recursive_filter::PROFILES,
        recursive_filter::project,
    ));
    projects.extend(each(
        "scalar-construction",
        scalar_construction::PROFILES,
        scalar_construction::project,
    ));
    projects.extend(each(
        "sequence-aggregate",
        sequence_aggregate::PROFILES,
        sequence_aggregate::project,
    ));
    projects.extend(each(
        "sequence-exists",
        sequence_exists::PROFILES,
        sequence_exists::project,
    ));
    projects.extend(each(
        "sequence-item-at",
        sequence_item_at::PROFILES,
        sequence_item_at::project,
    ));
    projects.extend(each(
        "xml-mixed-construction",
        xml_mixed_construction::MODELS,
        xml_mixed_construction::project,
    ));
    projects.extend(each(
        "xml-mixed-content",
        xml_mixed_content::MODELS,
        xml_mixed_content::project,
    ));
    projects.extend(each(
        "xml-serialize",
        xml_serialize::MODELS,
        xml_serialize::project,
    ));
    for (corpus, legacy) in [
        ("adjacency-tree", adjacency_tree::legacy as fn() -> Project),
        ("path-hierarchy", path_hierarchy::legacy),
        ("xml-mixed-construction", xml_mixed_construction::legacy),
        ("xml-mixed-content", xml_mixed_content::legacy),
        ("xml-serialize", xml_serialize::legacy),
    ] {
        projects.push((format!("{corpus}/legacy"), legacy()));
    }
    projects
}

#[test]
fn conformance_models_agree_with_every_hand_written_walk() {
    let projects = conformance_projects();
    assert!(projects.len() >= 200, "found {} models", projects.len());
    for (name, project) in &projects {
        let checked =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| assert_equivalent(project)));
        assert!(checked.is_ok(), "{name} diverged");
    }
}
