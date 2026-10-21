use std::collections::{BTreeMap, BTreeSet};

use mapping::{NodeId, Scope, ScopeConstruction, ScopeIteration, ScopeSequence};

use crate::import::schema::SchemaComponent;

type Anchors = BTreeMap<Vec<String>, Vec<String>>;
pub(super) type BindingOrigins = BTreeMap<(Vec<String>, NodeId), BTreeSet<usize>>;

/// Import-only ownership. These markers identify declared target entries, not
/// source expressions or serialized Ferrule scope identities.
#[derive(Clone)]
#[cfg_attr(test, derive(Debug))]
pub(super) struct DeclaredBranch {
    pub(super) lineage: Vec<u32>,
    pub(super) anchors: Anchors,
    /// Exact imported binding indices, before source anchors are rewritten.
    pub(super) bindings: BindingOrigins,
}

#[derive(Clone, Default)]
#[cfg_attr(test, derive(Debug))]
pub(super) struct DeclaredBranches {
    pub(super) layouts: BTreeMap<Vec<String>, Vec<DeclaredBranch>>,
}

/// Retained original binding indices, counted per target field.
type RetainedOrigins = BTreeMap<(Vec<String>, usize), usize>;

/// Import-local occurrence record for distributed nested clones. For each
/// distributed child path it keeps, per declared parent segment, the declared
/// child branches placed beneath it in segment order. Scopes carry no
/// occurrence identity, so this record is what lets a later ancestor proof
/// attribute an interned binding to the exact clone that produced it.
#[derive(Default)]
#[cfg_attr(test, derive(Debug))]
struct Placements(BTreeMap<Vec<String>, Vec<Vec<usize>>>);

/// One declared occurrence enclosing a scope during a provenance walk.
#[derive(Clone, Copy)]
struct Occurrence<'a> {
    layout: &'a [String],
    index: usize,
    branch: &'a DeclaredBranch,
}

/// The candidate distributions for one concatenated wrapper, each with its
/// ownership proof, plus diagnostics in emission order. Planning reads only;
/// [`DeclaredBranches::apply_level`] performs every mutation afterwards.
#[cfg_attr(test, derive(Debug))]
struct LevelPlan {
    distributions: Vec<Distribution>,
    diagnostics: Vec<String>,
}

#[cfg_attr(test, derive(Debug))]
struct Distribution {
    /// Index of the concatenated child wrapper in the parent wrapper.
    child: usize,
    /// Declared child branch indices owned by each parent segment, admitted
    /// only after every replaced placeholder binding is proved retained once.
    groups: Vec<Vec<usize>>,
}

impl DeclaredBranches {
    pub(super) fn record(&mut self, path: &[String], branches: Vec<DeclaredBranch>) {
        if !branches.is_empty()
            && branches.iter().all(|branch| !branch.lineage.is_empty())
            && branches.iter().enumerate().all(|(index, branch)| {
                branches[..index]
                    .iter()
                    .all(|previous| previous.lineage != branch.lineage)
            })
        {
            self.layouts.insert(path.to_vec(), branches);
        }
    }

    pub(super) fn enclosing_anchors(
        &self,
        path: &[String],
        lineage: &[u32],
        fallback: &Anchors,
    ) -> Option<Anchors> {
        let Some((_, branches)) = self
            .layouts
            .iter()
            .filter(|(parent, _)| parent.len() < path.len() && path.starts_with(parent))
            .max_by_key(|(parent, _)| parent.len())
        else {
            return Some(fallback.clone());
        };
        let mut owners = branches
            .iter()
            .filter(|branch| lineage.starts_with(&branch.lineage));
        let owner = owners.next()?;
        owners.next().is_none().then(|| owner.anchors.clone())
    }

    pub(super) fn distribute(&self, root: &mut Scope, warnings: &mut Vec<String>) {
        self.distribute_at(root, &mut Vec::new(), warnings, &mut Placements::default());
    }

    fn distribute_at(
        &self,
        scope: &mut Scope,
        path: &mut Vec<String>,
        warnings: &mut Vec<String>,
        placements: &mut Placements,
    ) {
        // Resolve deeper owners first, before their wrapper is partitioned.
        for child in &mut scope.children {
            path.push(child.target_field.clone());
            self.distribute_at(child, path, warnings, placements);
            path.pop();
        }
        if let Some(segments) = scope.concatenated_mut() {
            for segment in segments.iter_mut() {
                self.distribute_at(segment, path, warnings, placements);
            }
        }
        if let Some(plan) = self.plan_level(scope, path, placements) {
            apply_level(scope, path, plan, warnings, placements);
        }
    }

    /// Proves every distributable nested clone of one concatenated wrapper
    /// without mutating the scope, warnings, or occurrence record.
    fn plan_level(
        &self,
        scope: &Scope,
        path: &[String],
        placements: &Placements,
    ) -> Option<LevelPlan> {
        let parents = self.layouts.get(path)?;
        let ScopeIteration::Concatenate(parent_segments) = &scope.iteration else {
            return None;
        };
        if parent_segments.len() != parents.len() {
            return None;
        }
        let mut plan = LevelPlan {
            distributions: Vec::new(),
            diagnostics: Vec::new(),
        };
        for (index, child) in scope.children.iter().enumerate() {
            let Some(segments) = child.concatenated() else {
                continue;
            };
            let mut child_path = path.to_vec();
            child_path.push(child.target_field.clone());
            let Some(children) = self.layouts.get(&child_path) else {
                continue;
            };
            let Some(groups) = plain_wrapper(child)
                .then(|| partition(parents, children, segments))
                .flatten()
            else {
                plan.diagnostics.push(format!(
                    "nested cloned target `{}` has no complete, unique declared ancestor ownership; its scope remains unsupported",
                    child_path.join("/")
                ));
                continue;
            };
            match self.preserves_placeholders(
                parents,
                parent_segments,
                segments,
                &groups,
                &child_path,
                placements,
            ) {
                Some(_) => plan.distributions.push(Distribution {
                    child: index,
                    groups,
                }),
                None => plan.diagnostics.push(format!(
                    "nested cloned target `{}` cannot prove complete placeholder binding preservation; its scope remains unsupported",
                    child_path.join("/")
                )),
            }
        }
        Some(plan)
    }

    /// Returns, per parent segment, each retained original binding index and
    /// how many retained bindings it accounts for, when every replaced
    /// placeholder binding is retained exactly once.
    fn preserves_placeholders(
        &self,
        parents: &[DeclaredBranch],
        parent_segments: &ScopeSequence,
        child_segments: &ScopeSequence,
        groups: &[Vec<usize>],
        child_path: &[String],
        placements: &Placements,
    ) -> Option<Vec<RetainedOrigins>> {
        let target_field = child_path.last()?;
        let children = self.layouts.get(child_path)?;
        let mut witness = Vec::with_capacity(groups.len());
        for ((parent, segment), group) in parents.iter().zip(parent_segments.iter()).zip(groups) {
            let placeholders = segment
                .children
                .iter()
                .filter(|child| child.target_field == *target_field)
                .collect::<Vec<_>>();
            if placeholders.len() > 1 {
                return None;
            }
            let mut retained = RetainedOrigins::new();
            if let Some(placeholder) = placeholders.first() {
                if !replaceable_placeholder(placeholder) {
                    return None;
                }
                for index in group {
                    let occurrence = Occurrence {
                        layout: child_path,
                        index: *index,
                        branch: children.get(*index)?,
                    };
                    if !self.collect_retained_within(
                        occurrence,
                        child_segments.iter().nth(*index)?,
                        &mut child_path.to_vec(),
                        &mut retained,
                        placements,
                        &mut vec![occurrence],
                    ) {
                        return None;
                    }
                }
                if !placeholder_bindings_preserved(
                    placeholder,
                    &mut child_path.to_vec(),
                    &parent.bindings,
                    &retained,
                ) {
                    return None;
                }
            }
            witness.push(retained);
        }
        Some(witness)
    }

    /// Counts retained original binding indices without an occurrence
    /// record: every identical `(field, node)` origin under `owner` competes.
    #[cfg(test)]
    fn collect_retained(
        &self,
        owner: &DeclaredBranch,
        scope: &Scope,
        owner_path: &[String],
        path: &mut Vec<String>,
        retained: &mut RetainedOrigins,
    ) -> bool {
        let occurrence = Occurrence {
            layout: owner_path,
            index: 0,
            branch: owner,
        };
        self.collect_retained_within(
            occurrence,
            scope,
            path,
            retained,
            &Placements::default(),
            &mut Vec::new(),
        )
    }

    fn collect_retained_within<'a>(
        &'a self,
        owner: Occurrence<'a>,
        scope: &Scope,
        path: &mut Vec<String>,
        retained: &mut RetainedOrigins,
        placements: &Placements,
        occurrences: &mut Vec<Occurrence<'a>>,
    ) -> bool {
        // Bindings are installed per authored input, so one occurrence may
        // retain one binding for each of several entries fed by one node.
        let mut shared = Vec::<(&mapping::Binding, usize)>::new();
        for binding in &scope.bindings {
            match shared.iter_mut().find(|(seen, _)| {
                seen.target_field == binding.target_field && seen.node == binding.node
            }) {
                Some((_, count)) => *count += 1,
                None => shared.push((binding, 1)),
            }
        }
        for (binding, count) in shared {
            let mut field = path.clone();
            field.push(binding.target_field.clone());
            // A deeper replacement may use a different anchored node. Its exact
            // original binding index still belongs to this declared branch.
            let mut origins = std::iter::once(owner.branch)
                .chain(
                    self.layouts
                        .iter()
                        .filter(|(descendant, _)| {
                            descendant.len() > owner.layout.len()
                                && descendant.starts_with(owner.layout)
                        })
                        .flat_map(|(_, branches)| branches)
                        .filter(|branch| branch.lineage.starts_with(&owner.branch.lineage)),
                )
                .filter_map(|branch| branch.bindings.get(&(field.clone(), binding.node)))
                .flatten()
                .copied()
                .collect::<BTreeSet<_>>();
            // Sibling clones fed by one interned expression share a
            // `(field, node)` key. The innermost recorded occurrence that
            // holds this binding decides which of those origins it is; it
            // never adds an origin.
            if origins.len() > 1
                && let Some(exact) = occurrences.iter().rev().find_map(|occurrence| {
                    occurrence
                        .branch
                        .bindings
                        .get(&(field.clone(), binding.node))
                })
                && exact.len() == count
                && exact.is_subset(&origins)
            {
                origins.clone_from(exact);
            }
            match origins.len() {
                0 => {}
                1 => {
                    if let Some(origin) = origins.first() {
                        *retained.entry((field.clone(), *origin)).or_default() += count;
                    }
                }
                // Several original edges survived separately only when each
                // has its own retained binding, even with one cached node.
                origins_len if origins_len == count => {
                    for origin in origins {
                        *retained.entry((field.clone(), origin)).or_default() += 1;
                    }
                }
                _ => return false,
            }
        }
        for child in &scope.children {
            path.push(child.target_field.clone());
            let preserved = match self.placed_occurrences(child, path, placements, occurrences) {
                Some(placed) => placed.into_iter().all(|(occurrence, segment)| {
                    occurrences.push(occurrence);
                    let preserved = self.collect_retained_within(
                        owner,
                        segment,
                        path,
                        retained,
                        placements,
                        occurrences,
                    );
                    occurrences.pop();
                    preserved
                }),
                None => self.collect_retained_within(
                    owner,
                    child,
                    path,
                    retained,
                    placements,
                    occurrences,
                ),
            };
            path.pop();
            if !preserved {
                return false;
            }
        }
        if let Some(segments) = scope.concatenated() {
            for segment in segments.iter() {
                if !self.collect_retained_within(
                    owner,
                    segment,
                    path,
                    retained,
                    placements,
                    occurrences,
                ) {
                    return false;
                }
            }
        }
        true
    }

    /// Pairs the scopes of a previously distributed child with the declared
    /// child occurrences recorded for the enclosing occurrence, when that
    /// correspondence is exact.
    fn placed_occurrences<'a, 's>(
        &'a self,
        child: &'s Scope,
        path: &[String],
        placements: &Placements,
        occurrences: &[Occurrence<'a>],
    ) -> Option<Vec<(Occurrence<'a>, &'s Scope)>> {
        let (layout, branches) = self.layouts.get_key_value(path)?;
        let enclosing = occurrences.last()?;
        if path.split_last().map(|(_, parent)| parent) != Some(enclosing.layout) {
            return None;
        }
        let placed = placements.0.get(path)?.get(enclosing.index)?;
        let scopes = match child.concatenated() {
            Some(segments) => segments.iter().collect::<Vec<_>>(),
            None => vec![child],
        };
        if scopes.len() != placed.len() {
            return None;
        }
        placed
            .iter()
            .zip(scopes)
            .map(|(index, scope)| {
                Some((
                    Occurrence {
                        layout,
                        index: *index,
                        branch: branches.get(*index)?,
                    },
                    scope,
                ))
            })
            .collect()
    }
}

/// Moves every proved nested clone into its owning parent segments and records
/// the resulting occurrence placement for ancestor proofs.
fn apply_level(
    scope: &mut Scope,
    path: &[String],
    plan: LevelPlan,
    warnings: &mut Vec<String>,
    placements: &mut Placements,
) {
    warnings.extend(plan.diagnostics);
    let mut removed = BTreeMap::new();
    for distribution in plan.distributions.iter().rev() {
        removed.insert(
            distribution.child,
            scope.children.remove(distribution.child),
        );
    }
    let Some(parent_segments) = scope.concatenated_mut() else {
        return;
    };
    for distribution in plan.distributions {
        let Some(mut child) = removed.remove(&distribution.child) else {
            continue;
        };
        let target_field = std::mem::take(&mut child.target_field);
        let output = child.iteration_output;
        // The proved partition assigns every child segment exactly once.
        let mut segments = child
            .concatenated_mut()
            .map(|segments| {
                segments
                    .iter_mut()
                    .map(|segment| Some(std::mem::take(segment)))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        for (parent, group) in parent_segments.iter_mut().zip(&distribution.groups) {
            let mut group = group
                .iter()
                .filter_map(|index| segments.get_mut(*index).and_then(Option::take));
            let Some(mut first) = group.next() else {
                continue;
            };
            let rest = group.collect::<Vec<_>>();
            let child = if rest.is_empty() {
                first.target_field.clone_from(&target_field);
                first
            } else {
                Scope {
                    target_field: target_field.clone(),
                    iteration: ScopeIteration::Concatenate(ScopeSequence::new(first, rest)),
                    iteration_output: output,
                    ..Scope::default()
                }
            };
            if let Some(existing) = parent
                .children
                .iter_mut()
                .find(|child| child.target_field == target_field)
            {
                *existing = child;
            } else {
                parent.children.push(child);
            }
        }
        let mut child_path = path.to_vec();
        child_path.push(target_field);
        placements.0.insert(child_path, distribution.groups);
    }
}

fn placeholder_bindings_preserved(
    scope: &Scope,
    path: &mut Vec<String>,
    origins: &BindingOrigins,
    retained: &BTreeMap<(Vec<String>, usize), usize>,
) -> bool {
    for binding in &scope.bindings {
        let mut field = path.clone();
        field.push(binding.target_field.clone());
        let Some(indices) = origins.get(&(field.clone(), binding.node)) else {
            return false;
        };
        if indices.is_empty()
            || indices
                .iter()
                .any(|index| retained.get(&(field.clone(), *index)) != Some(&1))
        {
            return false;
        }
    }
    for child in &scope.children {
        path.push(child.target_field.clone());
        let preserved = placeholder_bindings_preserved(child, path, origins, retained);
        path.pop();
        if !preserved {
            return false;
        }
    }
    true
}

fn plain_wrapper(scope: &Scope) -> bool {
    scope.construction == ScopeConstruction::Constructed
        && scope.filter.is_none()
        && scope.post_group_filter.is_none()
        && !scope.has_grouping()
        && !scope.has_sort()
        && scope.windows.is_empty()
        && scope.bindings.is_empty()
        && scope.dynamic_bindings.is_empty()
        && scope.children.is_empty()
        && scope.dynamic_children.is_empty()
        && !scope.merge_dynamic_fields
        && scope.iteration_output != mapping::IterationOutput::First
        && scope.concatenated().is_some_and(|segments| {
            segments.iter().all(|segment| {
                segment.target_field.is_empty()
                    && segment.iteration_output == scope.iteration_output
            })
        })
}

pub(super) fn replaceable_placeholder(scope: &Scope) -> bool {
    !scope.iterates()
        && scope.iteration_output == mapping::IterationOutput::Repeated
        && scope.construction == ScopeConstruction::Constructed
        && scope.filter.is_none()
        && scope.post_group_filter.is_none()
        && !scope.has_grouping()
        && !scope.has_sort()
        && scope.windows.is_empty()
        && scope.dynamic_bindings.is_empty()
        && scope.dynamic_children.is_empty()
        && !scope.merge_dynamic_fields
        && scope.children.iter().all(replaceable_placeholder)
}

fn partition(
    parents: &[DeclaredBranch],
    children: &[DeclaredBranch],
    segments: &ScopeSequence,
) -> Option<Vec<Vec<usize>>> {
    if children.len() != segments.len() {
        return None;
    }
    let mut groups = vec![Vec::new(); parents.len()];
    for (index, child) in children.iter().enumerate() {
        let mut owners = parents
            .iter()
            .enumerate()
            .filter(|(_, parent)| child.lineage.starts_with(&parent.lineage));
        let (owner, _) = owners.next()?;
        if owners.next().is_some() {
            return None;
        }
        groups[owner].push(index);
    }
    groups
        .iter()
        .all(|group| !group.is_empty())
        .then_some(groups)
}

pub(super) fn lineage(target: &SchemaComponent, marker: u32) -> Option<Vec<u32>> {
    let mut selected = None;
    for ancestors in target.input_ancestors.values() {
        let markers = ancestors
            .iter()
            .filter(|ancestor| !target.input_keys.contains(ancestor))
            .copied()
            .collect::<Vec<_>>();
        if let Some(index) = markers.iter().position(|ancestor| *ancestor == marker) {
            let candidate = markers[..=index].to_vec();
            if selected
                .as_ref()
                .is_some_and(|selected| selected != &candidate)
            {
                return None;
            }
            selected = Some(candidate);
        }
    }
    selected
}

#[cfg(test)]
mod occurrence_tests;

#[cfg(test)]
mod tests {
    use mapping::Binding;

    use super::*;

    fn evidence(label: &str) -> std::path::PathBuf {
        let parent = std::env::var_os("FERRULE_NESTED_CLONES_EVIDENCE_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let path = parent.join(format!(
            "ferrule_nested_clones239_guard_{label}_{}_{stamp}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).expect("retained guard evidence");
        eprintln!("NESTED_CLONES239_ORIGINALS={}", path.display());
        path
    }

    fn bound(field: &str, node: NodeId) -> Binding {
        Binding {
            target_field: field.into(),
            node,
        }
    }

    #[test]
    fn unclaimed_placeholder_binding_refuses_atomic_replacement() {
        // The ordinary Extra binding has no declared descendant owner. The
        // owned Value alone is insufficient evidence to delete the old child.
        let first = Scope {
            children: vec![Scope {
                target_field: "Child".into(),
                bindings: vec![bound("Value", 10), bound("Extra", 11)],
                ..Scope::default()
            }],
            ..Scope::default()
        };
        let second = Scope::default();
        let child = Scope {
            target_field: "Child".into(),
            iteration: ScopeIteration::Concatenate(ScopeSequence::new(
                Scope {
                    bindings: vec![bound("Value", 20)],
                    ..Scope::default()
                },
                vec![Scope {
                    bindings: vec![bound("Value", 21)],
                    ..Scope::default()
                }],
            )),
            ..Scope::default()
        };
        let mut root = Scope {
            iteration: ScopeIteration::Concatenate(ScopeSequence::new(first, vec![second])),
            children: vec![child],
            ..Scope::default()
        };
        let path = vec!["Child".into(), "Value".into()];
        let mut owners = DeclaredBranches::default();
        owners.record(
            &[],
            vec![
                DeclaredBranch {
                    lineage: vec![1],
                    anchors: Anchors::new(),
                    bindings: BindingOrigins::from([((path.clone(), 10), BTreeSet::from([0]))]),
                },
                DeclaredBranch {
                    lineage: vec![2],
                    anchors: Anchors::new(),
                    bindings: BindingOrigins::new(),
                },
            ],
        );
        owners.record(
            &["Child".into()],
            vec![
                DeclaredBranch {
                    lineage: vec![1, 3],
                    anchors: Anchors::new(),
                    bindings: BindingOrigins::from([((path.clone(), 20), BTreeSet::from([0]))]),
                },
                DeclaredBranch {
                    lineage: vec![2, 4],
                    anchors: Anchors::new(),
                    bindings: BindingOrigins::from([((path, 21), BTreeSet::from([1]))]),
                },
            ],
        );
        let original = serde_json::to_value(&root).expect("authored scope");
        let expected = ["nested cloned target `Child` cannot prove complete placeholder binding preservation; its scope remains unsupported".to_string()];
        let evidence = evidence("unclaimed");
        std::fs::write(
            evidence.join("input.original.json"),
            serde_json::to_vec_pretty(&original).expect("scope JSON"),
        )
        .expect("retain input");
        std::fs::write(
            evidence.join("ownership.original.txt"),
            format!("{owners:#?}"),
        )
        .expect("retain owners");
        std::fs::write(
            evidence.join("expected-before-run.txt"),
            format!("unchanged complete scope\n{expected:#?}"),
        )
        .expect("retain expected");
        let mut warnings = Vec::new();
        owners.distribute(&mut root, &mut warnings);
        std::fs::write(
            evidence.join("outcome.original.json"),
            serde_json::to_vec_pretty(&serde_json::json!({"scope": root, "warnings": warnings}))
                .expect("outcome JSON"),
        )
        .expect("retain outcome");
        assert_eq!(
            serde_json::to_value(&root).expect("retained scope"),
            original
        );
        assert_eq!(warnings, expected);

        // Even fully owned bindings cannot justify discarding an ordinary
        // placeholder's independent predicate.
        let mut controlled = root.clone();
        let placeholder = &mut controlled
            .concatenated_mut()
            .expect("parents")
            .iter_mut()
            .next()
            .expect("first parent")
            .children[0];
        placeholder
            .bindings
            .retain(|binding| binding.target_field != "Extra");
        placeholder.filter = Some(99);
        let before = serde_json::to_value(&controlled).expect("controlled scope");
        std::fs::write(
            evidence.join("controlled-input-before-run.json"),
            serde_json::to_vec_pretty(&before).expect("input JSON"),
        )
        .expect("retain input");
        let mut warnings = Vec::new();
        owners.distribute(&mut controlled, &mut warnings);
        std::fs::write(
            evidence.join("controlled-outcome.original.json"),
            serde_json::to_vec_pretty(
                &serde_json::json!({"scope": controlled, "warnings": warnings}),
            )
            .expect("outcome JSON"),
        )
        .expect("retain outcome");
        assert_eq!(
            serde_json::to_value(&controlled).expect("retained scope"),
            before
        );
        assert_eq!(warnings, expected);
    }

    #[test]
    fn missing_or_broadcast_provenance_cannot_replace_a_placeholder() {
        let path = vec!["Child".into(), "Value".into()];
        let old = Scope {
            bindings: vec![bound("Value", 10)],
            ..Scope::default()
        };
        let origins = BindingOrigins::from([((path.clone(), 10), BTreeSet::from([7]))]);
        let evidence = evidence("missing-or-broadcast");
        std::fs::write(evidence.join("input-and-expected-before-run.txt"), format!("scope={old:#?}\norigins={origins:#?}\nretained counts [0,2,1], expected [false,false,true]\n")).expect("retain inputs");
        let actual = [0, 2, 1].map(|count| {
            let retained = BTreeMap::from([((path.clone(), 7), count)]);
            placeholder_bindings_preserved(&old, &mut vec!["Child".into()], &origins, &retained)
        });
        std::fs::write(
            evidence.join("outcome.original.txt"),
            format!("{actual:#?}\n"),
        )
        .expect("retain outcome");
        assert_eq!(actual, [false, false, true]);
    }

    #[test]
    fn one_retained_binding_cannot_stand_for_two_original_bindings() {
        let field = vec!["Child".into(), "Value".into()];
        let segment = |node| Scope {
            bindings: vec![bound("Value", node)],
            ..Scope::default()
        };
        let parent = |node| Scope {
            children: vec![Scope {
                target_field: "Child".into(),
                ..segment(node)
            }],
            ..Scope::default()
        };
        let branch = |lineage, node, indices| DeclaredBranch {
            lineage,
            anchors: Anchors::new(),
            bindings: BindingOrigins::from([((field.clone(), node), indices)]),
        };
        for ambiguous in [true, false] {
            let evidence = evidence(if ambiguous {
                "aliased-origin"
            } else {
                "separate-origins"
            });
            let mut owners = DeclaredBranches::default();
            owners.record(
                &[],
                vec![
                    branch(vec![1], 10, BTreeSet::from([0, 1])),
                    branch(vec![2], 11, BTreeSet::from([2])),
                ],
            );
            let (child_segments, child_owners) = if ambiguous {
                (
                    ScopeSequence::new(segment(20), vec![segment(21)]),
                    vec![
                        branch(vec![1, 3], 20, BTreeSet::from([0, 1])),
                        branch(vec![2, 5], 21, BTreeSet::from([2])),
                    ],
                )
            } else {
                (
                    ScopeSequence::new(segment(20), vec![segment(22), segment(21)]),
                    vec![
                        branch(vec![1, 3], 20, BTreeSet::from([0])),
                        branch(vec![1, 4], 22, BTreeSet::from([1])),
                        branch(vec![2, 5], 21, BTreeSet::from([2])),
                    ],
                )
            };
            owners.record(&["Child".into()], child_owners);
            let mut root = Scope {
                iteration: ScopeIteration::Concatenate(ScopeSequence::new(
                    parent(10),
                    vec![parent(11)],
                )),
                children: vec![Scope {
                    target_field: "Child".into(),
                    iteration: ScopeIteration::Concatenate(child_segments),
                    ..Scope::default()
                }],
                ..Scope::default()
            };
            // These complete expected scopes are authored before distribution:
            // an ambiguous single binding keeps the original scope unchanged;
            // two explicitly owned bindings retain both children in order.
            let expected = if ambiguous {
                root.clone()
            } else {
                Scope {
                    iteration: ScopeIteration::Concatenate(ScopeSequence::new(
                        Scope {
                            children: vec![Scope {
                                target_field: "Child".into(),
                                iteration: ScopeIteration::Concatenate(ScopeSequence::new(
                                    segment(20),
                                    vec![segment(22)],
                                )),
                                ..Scope::default()
                            }],
                            ..Scope::default()
                        },
                        vec![Scope {
                            children: vec![Scope {
                                target_field: "Child".into(),
                                ..segment(21)
                            }],
                            ..Scope::default()
                        }],
                    )),
                    ..Scope::default()
                }
            };
            let expected_warnings = if ambiguous {
                vec!["nested cloned target `Child` cannot prove complete placeholder binding preservation; its scope remains unsupported".to_string()]
            } else {
                Vec::new()
            };
            std::fs::write(
                evidence.join("input-before-run.json"),
                serde_json::to_vec_pretty(&root).expect("input JSON"),
            )
            .expect("retain scope");
            std::fs::write(
                evidence.join("ownership-before-run.txt"),
                format!("{owners:#?}"),
            )
            .expect("retain ownership");
            std::fs::write(
                evidence.join("expected-before-run.json"),
                serde_json::to_vec_pretty(
                    &serde_json::json!({"scope": expected, "warnings": expected_warnings}),
                )
                .expect("expected JSON"),
            )
            .expect("retain expected");
            let mut warnings = Vec::new();
            owners.distribute(&mut root, &mut warnings);
            std::fs::write(
                evidence.join("outcome.original.json"),
                serde_json::to_vec_pretty(
                    &serde_json::json!({"scope": root, "warnings": warnings}),
                )
                .expect("outcome JSON"),
            )
            .expect("retain outcome");
            assert_eq!(
                serde_json::to_value(&root).expect("actual scope"),
                serde_json::to_value(&expected).expect("expected scope"),
            );
            assert_eq!(warnings, expected_warnings);
        }
    }

    #[test]
    fn equal_lineage_is_declared_ownership_but_sibling_provenance_is_excluded() {
        let branch = |lineage| DeclaredBranch {
            lineage,
            anchors: Anchors::new(),
            bindings: BindingOrigins::new(),
        };
        let parent = vec![branch(vec![1]), branch(vec![2])];
        let children = vec![branch(vec![1]), branch(vec![2])];
        let segments = ScopeSequence::new(Scope::default(), vec![Scope::default()]);
        let evidence = evidence("equal-lineage");
        std::fs::write(
            evidence.join("partition-input-and-expected-before-run.txt"),
            format!("parents={parent:#?}\nchildren={children:#?}\nexpected=[[0],[1]]"),
        )
        .expect("retain partition");
        let partitioned = partition(&parent, &children, &segments);
        let field = vec!["Child".into(), "Nested".into(), "Value".into()];
        let scope = Scope {
            children: vec![Scope {
                target_field: "Nested".into(),
                bindings: vec![bound("Value", 30)],
                ..Scope::default()
            }],
            ..Scope::default()
        };
        let mut layouts = DeclaredBranches::default();
        layouts.record(
            &["Sibling".into(), "Nested".into()],
            vec![DeclaredBranch {
                bindings: BindingOrigins::from([((field.clone(), 30), BTreeSet::from([7]))]),
                ..branch(vec![1])
            }],
        );
        std::fs::write(evidence.join("retained-input-and-expected-before-run.txt"), format!("scope={scope:#?}\nowner={:#?}\nlayouts={layouts:#?}\nexpected sibling count=0, proper descendant count=1", children[0])).expect("retain origins");
        let mut sibling = BTreeMap::new();
        let sibling_unambiguous = layouts.collect_retained(
            &children[0],
            &scope,
            &["Child".into()],
            &mut vec!["Child".into()],
            &mut sibling,
        );
        layouts.record(
            &["Child".into(), "Nested".into()],
            vec![DeclaredBranch {
                bindings: BindingOrigins::from([((field.clone(), 30), BTreeSet::from([7]))]),
                ..branch(vec![1])
            }],
        );
        let mut descendant = BTreeMap::new();
        let descendant_unambiguous = layouts.collect_retained(
            &children[0],
            &scope,
            &["Child".into()],
            &mut vec!["Child".into()],
            &mut descendant,
        );
        std::fs::write(
            evidence.join("outcomes.original.txt"),
            format!("partition={partitioned:#?}\nsibling={sibling:#?}, unambiguous={sibling_unambiguous}\ndescendant={descendant:#?}, unambiguous={descendant_unambiguous}"),
        )
        .expect("retain outcomes");
        assert_eq!(partitioned, Some(vec![vec![0], vec![1]]));
        assert!(sibling_unambiguous);
        assert!(sibling.is_empty());
        assert!(descendant_unambiguous);
        assert_eq!(descendant, BTreeMap::from([((field, 7), 1)]));
    }
}
