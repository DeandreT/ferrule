use super::*;
use crate::{Grouping, SourceCollection};

impl Planner {
    pub(super) fn control(&mut self, op: &EditOperation) -> Result<(), EditError> {
        match op {
            EditOperation::AddScope { parent, field } => {
                check_constructed(self.scope(parent)?)?;
                let target = self.target_schema(parent)?.child(field).ok_or_else(|| {
                    EditError::Invalid(format!("target group {field} does not exist"))
                })?;
                if !matches!(target.kind, ir::SchemaKind::Group { .. }) {
                    return invalid("child scope must target a group");
                }
                let mut after = self.project.clone();
                let scope = scope_mut(&mut after, parent)?;
                if scope
                    .children
                    .iter()
                    .any(|child| child.target_field == *field)
                {
                    return invalid("child scope already exists");
                }
                scope.children.push(Scope {
                    target_field: field.clone(),
                    ..Scope::default()
                });
                self.target_change(
                    parent,
                    after,
                    GraphAction::AddScope {
                        parent: location(parent),
                        field: field.clone(),
                    },
                )?;
            }
            EditOperation::RemoveScope { scope: at } => {
                let Some((last, parents)) = at.children.split_last() else {
                    return invalid("cannot remove the root scope");
                };
                let parent = ScopeRef {
                    target: at.target.clone(),
                    children: parents.to_vec(),
                };
                let mut owned = BTreeSet::new();
                self.scope(at)?.visit_tree_roots(&mut |root| {
                    if root.role == mapping::ScopeRootRole::GeneratedItem {
                        owned.insert(root.node);
                    }
                });
                let mut after = self.project.clone();
                scope_mut(&mut after, &parent)?
                    .children
                    .remove(last.0 as usize);
                for item in owned {
                    if referenced(&after, item) {
                        return invalid("removed scope item is still referenced");
                    }
                    after.graph.nodes.remove(&item);
                }
                self.target_change(
                    at,
                    after,
                    GraphAction::RemoveScope {
                        scope: location(at),
                    },
                )?;
            }
            EditOperation::SetIteration { scope: at, source } => {
                self.editable_scope(at)?;
                let current = self.scope(at)?;
                if !source_editable(current) {
                    return invalid("iteration is read-only");
                }
                let mut after = self.project.clone();
                scope_mut(&mut after, at)?.iteration = match source {
                    Some(SourceCollection::Legacy { path }) => {
                        mapping::ScopeIteration::Source(path.clone())
                    }
                    Some(source) => mapping::ScopeIteration::OwnedSource {
                        source: self.owned_collection(source)?,
                        output_path: None,
                    },
                    None => mapping::ScopeIteration::None,
                };
                self.setting(at, after, ScopeSetting::Iteration)?;
            }
            EditOperation::SetFilter { scope: at, node } => {
                self.editable_scope(at)?;
                if let Some(node) = node {
                    self.node(node.0)?;
                }
                let mut after = self.project.clone();
                scope_mut(&mut after, at)?.filter = node.map(|n| n.0);
                self.setting(at, after, ScopeSetting::Filter)?;
            }
            EditOperation::SetGrouping {
                scope: at,
                grouping,
            } => {
                self.editable_scope(at)?;
                let id = match grouping {
                    Grouping::None {} => None,
                    Grouping::By { node }
                    | Grouping::AdjacentBy { node }
                    | Grouping::StartingWith { node }
                    | Grouping::EndingWith { node }
                    | Grouping::IntoBlocks { node } => Some(node.0),
                };
                if let Some(id) = id {
                    self.node(id)?;
                }
                let mut after = self.project.clone();
                let scope = scope_mut(&mut after, at)?;
                scope.group_by = None;
                scope.group_adjacent_by = None;
                scope.group_starting_with = None;
                scope.group_ending_with = None;
                scope.group_into_blocks = None;
                match grouping {
                    Grouping::None {} => {
                        scope.post_group_filter = None;
                    }
                    Grouping::By { .. } => scope.group_by = id,
                    Grouping::AdjacentBy { .. } => scope.group_adjacent_by = id,
                    Grouping::StartingWith { .. } => scope.group_starting_with = id,
                    Grouping::EndingWith { .. } => scope.group_ending_with = id,
                    Grouping::IntoBlocks { .. } => scope.group_into_blocks = id,
                }
                self.setting(at, after, ScopeSetting::Grouping)?;
            }
            EditOperation::SetSort { scope: at, keys } => {
                self.editable_scope(at)?;
                for key in keys {
                    self.node(key.node.0)?;
                }
                let mut after = self.project.clone();
                let scope = scope_mut(&mut after, at)?;
                scope.sort_by = keys.first().map(|key| key.node.0);
                if let Some(key) = keys.first() {
                    scope.sort_descending = key.descending;
                }
                scope.sort_then_by = keys
                    .iter()
                    .skip(1)
                    .map(|key| mapping::SortKey {
                        node: key.node.0,
                        descending: key.descending,
                    })
                    .collect();
                self.setting(at, after, ScopeSetting::Sort)?;
            }
            EditOperation::AddFailureRule { rule } => {
                let rule = self.failure_rule(rule, None)?;
                let index = self.project.failure_rules.len();
                let mut after = self.project.clone();
                after.failure_rules.push(rule);
                self.project_change(after, GraphAction::AddFailureRule { index })?;
            }
            EditOperation::EditFailureRule { index, rule } => {
                let previous = self
                    .project
                    .failure_rules
                    .get(index.0 as usize)
                    .ok_or(EditError::MissingRule(*index))?
                    .clone();
                let rule = self.failure_rule(rule, Some(&previous))?;
                let mut after = self.project.clone();
                let slot = after
                    .failure_rules
                    .get_mut(index.0 as usize)
                    .ok_or(EditError::MissingRule(*index))?;
                *slot = rule;
                self.project_change(
                    after,
                    GraphAction::EditFailureRule {
                        index: index.0 as usize,
                    },
                )?;
            }
            EditOperation::RemoveFailureRule { index } => {
                if self.project.failure_rules.get(index.0 as usize).is_none() {
                    return Err(EditError::MissingRule(*index));
                }
                let mut after = self.project.clone();
                let removed = after.failure_rules.remove(index.0 as usize);
                if let mapping::FailureIteration::Sequence { sequence } = removed.iteration {
                    for item in sequence.owned_items() {
                        if referenced(&after, item) {
                            return invalid("removed failure item is still referenced");
                        }
                        after.graph.nodes.remove(&item);
                    }
                }
                self.project_change(
                    after,
                    GraphAction::RemoveFailureRule {
                        index: index.0 as usize,
                    },
                )?;
            }
            EditOperation::MoveFailureRule { from, to } => {
                let mut current = from.0 as usize;
                let to = to.0 as usize;
                if current >= self.project.failure_rules.len() {
                    return Err(EditError::MissingRule(*from));
                }
                if to >= self.project.failure_rules.len() {
                    return Err(EditError::MissingRule(crate::Index(to as u32)));
                }
                while current != to {
                    let next = if current < to {
                        current + 1
                    } else {
                        current - 1
                    };
                    let mut after = self.project.clone();
                    after.failure_rules.swap(current, next);
                    self.project_change(
                        after,
                        GraphAction::MoveFailureRule {
                            from: current,
                            to: next,
                        },
                    )?;
                    current = next;
                }
            }
            _ => return invalid("operation is not a scope or failure edit"),
        }
        Ok(())
    }
    fn editable_scope(&self, at: &ScopeRef) -> Result<(), EditError> {
        let scope = self.scope(at)?;
        check_constructed(scope)?;
        if !source_editable(scope) {
            return invalid("scope iteration is read-only");
        }
        if let mapping::ScopeIteration::OwnedSource { source, .. } = &scope.iteration
            && let mapping::SourceOwner::Named { name } = &source.owner
            && self
                .project
                .extra_sources
                .iter()
                .any(|source| source.name == *name && source.dynamic_path.is_some())
        {
            return invalid("dynamically loaded source scope is read-only");
        }
        Ok(())
    }
    pub(super) fn owned_collection(
        &self,
        source: &SourceCollection,
    ) -> Result<mapping::OwnedSourceRef, EditError> {
        let (owner, path) = match source {
            SourceCollection::Primary { path } => (mapping::SourceOwner::Primary, path),
            SourceCollection::Named { name, path } => {
                let mut matching = self
                    .project
                    .extra_sources
                    .iter()
                    .filter(|source| source.name == *name);
                let selected = matching
                    .next()
                    .ok_or_else(|| EditError::Invalid(format!("source {name} does not exist")))?;
                if matching.next().is_some() || selected.dynamic_path.is_some() {
                    return invalid("source ownership is ambiguous or dynamic");
                }
                (mapping::SourceOwner::Named { name: name.clone() }, path)
            }
            SourceCollection::Legacy { .. } => {
                return invalid("legacy collection has no explicit owner");
            }
        };
        Ok(mapping::OwnedSourceRef {
            owner,
            path: path.clone(),
        })
    }
    fn setting(
        &mut self,
        at: &ScopeRef,
        after: Project,
        setting: ScopeSetting,
    ) -> Result<(), EditError> {
        self.target_change(
            at,
            after,
            GraphAction::EditScope {
                scope: location(at),
                setting,
            },
        )
    }
}

fn source_editable(scope: &Scope) -> bool {
    matches!(
        scope.iteration,
        mapping::ScopeIteration::None
            | mapping::ScopeIteration::Source(_)
            | mapping::ScopeIteration::OwnedSource {
                output_path: None,
                ..
            }
    )
}

fn referenced(project: &Project, id: u32) -> bool {
    super::removal::referenced(project, id)
}
