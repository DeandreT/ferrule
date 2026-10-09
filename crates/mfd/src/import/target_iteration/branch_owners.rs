use std::collections::{BTreeMap, BTreeSet};

use mapping::{NodeId, Scope, ScopeConstruction, ScopeIteration, ScopeSequence};

use crate::import::schema::SchemaComponent;

type Anchors = BTreeMap<Vec<String>, Vec<String>>;
pub(super) type BindingOrigins = BTreeMap<(Vec<String>, NodeId), BTreeSet<usize>>;

/// Import-only ownership. These markers identify declared target entries, not
/// source expressions or serialized Ferrule scope identities.
#[derive(Debug)]
pub(super) struct DeclaredBranch {
    pub(super) lineage: Vec<u32>,
    pub(super) anchors: Anchors,
    /// Exact imported binding indices, before source anchors are rewritten.
    pub(super) bindings: BindingOrigins,
}

#[derive(Debug, Default)]
pub(super) struct DeclaredBranches {
    layouts: BTreeMap<Vec<String>, Vec<DeclaredBranch>>,
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
        self.distribute_at(root, &mut Vec::new(), warnings);
    }

    fn distribute_at(&self, scope: &mut Scope, path: &mut Vec<String>, warnings: &mut Vec<String>) {
        // Resolve deeper owners first, before their wrapper is partitioned.
        for child in &mut scope.children {
            path.push(child.target_field.clone());
            self.distribute_at(child, path, warnings);
            path.pop();
        }
        if let Some(segments) = scope.concatenated_mut() {
            for segment in segments.iter_mut() {
                self.distribute_at(segment, path, warnings);
            }
        }
        let Some(parents) = self.layouts.get(path) else {
            return;
        };
        if scope
            .concatenated()
            .is_none_or(|segments| segments.len() != parents.len())
        {
            return;
        }
        let ScopeIteration::Concatenate(parent_segments) = &scope.iteration else {
            return;
        };
        let mut distributions = Vec::new();
        scope.children.retain(|child| {
            let Some(segments) = child.concatenated() else {
                return true;
            };
            let mut child_path = path.clone();
            child_path.push(child.target_field.clone());
            let Some(children) = self.layouts.get(&child_path) else {
                return true;
            };
            let grouped = plain_wrapper(child)
                .then(|| partition(parents, children, segments))
                .flatten();
            if let Some(grouped) = grouped {
                if self.preserves_placeholders(
                    parents, parent_segments, children, segments, &grouped, &child_path,
                ) {
                    let grouped = grouped.into_iter().map(|indices| {
                        indices.into_iter().map(|index| segments.iter().nth(index).expect("partition index").clone()).collect()
                    }).collect::<Vec<Vec<Scope>>>();
                    distributions.push((child.target_field.clone(), child.iteration_output, grouped));
                    false
                } else {
                    warnings.push(format!(
                        "nested cloned target `{}` cannot prove complete placeholder binding preservation; its scope remains unsupported",
                        child_path.join("/")
                    ));
                    true
                }
            } else {
                warnings.push(format!(
                    "nested cloned target `{}` has no complete, unique declared ancestor ownership; its scope remains unsupported",
                    child_path.join("/")
                ));
                true
            }
        });
        let Some(parent_segments) = scope.concatenated_mut() else {
            return;
        };
        for (target_field, output, groups) in distributions {
            for (parent, group) in parent_segments.iter_mut().zip(groups) {
                let mut group = group.into_iter();
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
        }
    }

    fn preserves_placeholders(
        &self,
        parents: &[DeclaredBranch],
        parent_segments: &ScopeSequence,
        children: &[DeclaredBranch],
        child_segments: &ScopeSequence,
        groups: &[Vec<usize>],
        child_path: &[String],
    ) -> bool {
        let target_field = child_path.last().expect("child path");
        parents
            .iter()
            .zip(parent_segments.iter())
            .zip(groups)
            .all(|((parent, segment), group)| {
                let placeholders = segment
                    .children
                    .iter()
                    .filter(|child| child.target_field == *target_field)
                    .collect::<Vec<_>>();
                if placeholders.len() > 1 {
                    return false;
                }
                let Some(placeholder) = placeholders.first() else {
                    return true;
                };
                if !replaceable_placeholder(placeholder) {
                    return false;
                }
                let mut retained = BTreeMap::new();
                for index in group {
                    self.collect_retained(
                        &children[*index],
                        child_segments.iter().nth(*index).expect("partition index"),
                        child_path,
                        &mut child_path.to_vec(),
                        &mut retained,
                    );
                }
                placeholder_bindings_preserved(
                    placeholder,
                    &mut child_path.to_vec(),
                    &parent.bindings,
                    &retained,
                )
            })
    }

    fn collect_retained(
        &self,
        owner: &DeclaredBranch,
        scope: &Scope,
        owner_path: &[String],
        path: &mut Vec<String>,
        retained: &mut BTreeMap<(Vec<String>, usize), usize>,
    ) {
        for binding in &scope.bindings {
            let mut field = path.clone();
            field.push(binding.target_field.clone());
            // A deeper replacement may use a different anchored node. Its exact
            // original binding index still belongs to this declared branch.
            let origins = std::iter::once(owner)
                .chain(
                    self.layouts
                        .iter()
                        .filter(|(descendant, _)| {
                            descendant.len() > owner_path.len()
                                && descendant.starts_with(owner_path)
                        })
                        .flat_map(|(_, branches)| branches)
                        .filter(|branch| branch.lineage.starts_with(&owner.lineage)),
                )
                .filter_map(|branch| branch.bindings.get(&(field.clone(), binding.node)))
                .flatten()
                .copied()
                .collect::<BTreeSet<_>>();
            for origin in origins {
                *retained.entry((field.clone(), origin)).or_default() += 1;
            }
        }
        for child in &scope.children {
            path.push(child.target_field.clone());
            self.collect_retained(owner, child, owner_path, path, retained);
            path.pop();
        }
        if let Some(segments) = scope.concatenated() {
            for segment in segments.iter() {
                self.collect_retained(owner, segment, owner_path, path, retained);
            }
        }
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

fn replaceable_placeholder(scope: &Scope) -> bool {
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
        layouts.collect_retained(
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
        layouts.collect_retained(
            &children[0],
            &scope,
            &["Child".into()],
            &mut vec!["Child".into()],
            &mut descendant,
        );
        std::fs::write(
            evidence.join("outcomes.original.txt"),
            format!("partition={partitioned:#?}\nsibling={sibling:#?}\ndescendant={descendant:#?}"),
        )
        .expect("retain outcomes");
        assert_eq!(partitioned, Some(vec![vec![0], vec![1]]));
        assert!(sibling.is_empty());
        assert_eq!(descendant, BTreeMap::from([((field, 7), 1)]));
    }
}
