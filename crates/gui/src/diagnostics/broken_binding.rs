use engine::{
    ValidationEndpoint, ValidationIssue, ValidationOwner, ValidationScopeLocation,
    ValidationScopeStep,
};
use mapping::{Binding, NodeId, Project, Scope};

/// Identity taken from the validator's scope owner and the binding it reported.
/// Neither the display path nor the diagnostic text is parsed as a node ID.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct BrokenBinding {
    pub(crate) owner: ValidationScopeLocation,
    pub(crate) index: usize,
    pub(crate) field: String,
    pub(crate) missing: NodeId,
}

impl BrokenBinding {
    pub(crate) fn from_issue(project: &Project, issue: &ValidationIssue) -> Option<Self> {
        if !issue.message.starts_with("binding for `") {
            return None;
        }
        let ValidationOwner::Scope(owner) = issue.owner.as_ref()? else {
            return None;
        };
        let (_, root) = target(project, &owner.target)?;
        let scope = owner.path.iter().try_fold(root, child)?;
        let mut matches = scope.bindings.iter().enumerate().filter(|(_, binding)| {
            !project.graph.nodes.contains_key(&binding.node)
                && issue.message
                    == format!(
                        "binding for `{}` references missing node {}",
                        binding.target_field, binding.node
                    )
        });
        let (index, binding) = matches.next()?;
        // Repeating scalar bindings may legitimately share a field. If two
        // match the same issue exactly, let the ordinary scope editor handle it.
        if matches.next().is_some() {
            return None;
        }
        Some(Self {
            owner: owner.clone(),
            index,
            field: binding.target_field.clone(),
            missing: binding.node,
        })
    }

    pub(crate) fn target_label(&self, project: &Project) -> Option<String> {
        let (schema, mut scope) = target(project, &self.owner.target)?;
        let mut label = match &self.owner.target {
            ValidationEndpoint::NamedTarget { index, name } => {
                format!("Target {} ({name}): {}", index + 1, schema.name)
            }
            _ => schema.name.clone(),
        };
        for step in &self.owner.path {
            scope = child(scope, step)?;
            match step {
                ValidationScopeStep::Child(_) => {
                    label.push_str(&format!(".{}", scope.target_field))
                }
                ValidationScopeStep::DynamicChild(index) => {
                    label.push_str(&format!(".[dynamic child {}]", index + 1))
                }
                ValidationScopeStep::Segment(index) => {
                    label.push_str(&format!(" [segment {}]", index + 1))
                }
            }
        }
        let field = if self.field == ir::XML_TYPE_FIELD {
            "XML type"
        } else {
            &self.field
        };
        Some(format!("{label}.{field}"))
    }

    pub(crate) fn binding_mut<'a>(&self, project: &'a mut Project) -> Option<&'a mut Binding> {
        let mut scope = match &self.owner.target {
            ValidationEndpoint::Target => &mut project.root,
            ValidationEndpoint::NamedTarget { index, name } => {
                &mut project
                    .extra_targets
                    .get_mut(*index)
                    .filter(|t| t.name == *name)?
                    .root
            }
            _ => return None,
        };
        for step in &self.owner.path {
            scope = match step {
                ValidationScopeStep::Child(i) => scope.children.get_mut(*i)?,
                ValidationScopeStep::DynamicChild(i) => {
                    &mut scope.dynamic_children.get_mut(*i)?.scope
                }
                ValidationScopeStep::Segment(i) => {
                    scope.iteration.concatenated_mut()?.iter_mut().nth(*i)?
                }
            };
        }
        scope
            .bindings
            .get_mut(self.index)
            .filter(|binding| binding.target_field == self.field && binding.node == self.missing)
    }
}

pub(crate) fn target<'a>(
    project: &'a Project,
    endpoint: &ValidationEndpoint,
) -> Option<(&'a ir::SchemaNode, &'a Scope)> {
    match endpoint {
        ValidationEndpoint::Target => Some((&project.target, &project.root)),
        ValidationEndpoint::NamedTarget { index, name } => {
            let t = project
                .extra_targets
                .get(*index)
                .filter(|t| t.name == *name)?;
            Some((&t.schema, &t.root))
        }
        _ => None,
    }
}

pub(crate) fn child<'a>(scope: &'a Scope, step: &ValidationScopeStep) -> Option<&'a Scope> {
    match step {
        ValidationScopeStep::Child(i) => scope.children.get(*i),
        ValidationScopeStep::DynamicChild(i) => {
            scope.dynamic_children.get(*i).map(|child| &child.scope)
        }
        ValidationScopeStep::Segment(i) => scope.iteration.concatenated()?.iter().nth(*i),
    }
}
