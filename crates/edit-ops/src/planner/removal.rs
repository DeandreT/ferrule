use super::*;

pub(super) fn referenced(project: &Project, id: u32) -> bool {
    if project
        .graph
        .nodes
        .iter()
        .any(|(&owner, node)| owner != id && node.dependencies().contains(&id))
    {
        return true;
    }
    let mut found = false;
    project
        .root
        .visit_tree_roots(&mut |root| found |= root.node == id);
    for target in &project.extra_targets {
        target
            .root
            .visit_tree_roots(&mut |root| found |= root.node == id);
    }
    for rule in &project.failure_rules {
        rule.visit_roots(&mut |root| found |= root.node == id);
    }
    found
        || project.extra_sources.iter().any(|source| {
            source
                .dynamic_path
                .as_ref()
                .is_some_and(|path| path.node == id)
        })
}

impl Planner {
    pub(super) fn cleanup(&mut self, id: u32) -> Result<(), EditError> {
        if !referenced(&self.project, id)
            && self.project.graph.nodes.get(&id).is_some_and(|node| {
                editor_ui::is_hidden_input(node)
                    && match node {
                        Node::SourceField { path, frame } => {
                            self.check_source(path, frame.as_deref()).is_ok()
                        }
                        _ => true,
                    }
            })
        {
            let mut after = self.project.clone();
            after.graph.nodes.remove(&id);
            self.graph_change(
                after,
                GraphAction::RemoveNodes {
                    requested: BTreeSet::from([id]),
                },
            )?;
        }
        Ok(())
    }

    pub(super) fn disconnect(
        &mut self,
        node: NodeId,
        input: usize,
        replacement: Option<NodeId>,
    ) -> Result<(), EditError> {
        let old = self
            .node(node.0)?
            .evaluated_inputs()
            .get(input)
            .copied()
            .ok_or(EditError::Input {
                node,
                input: crate::Index(input as u32),
            })?;
        let mut after = self.project.clone();
        if matches!(self.node(node.0)?, Node::Raise { .. }) {
            if replacement.is_some() {
                return invalid("optional message disconnection does not allocate a node");
            }
            // The optional Raise message is removed rather than replaced with Null.
            if let Some(Node::Raise { message }) = after.graph.nodes.get_mut(&node.0) {
                *message = None;
            }
        } else {
            let replacement = replacement.ok_or_else(|| {
                EditError::Invalid("a required input needs a replacement identity".into())
            })?;
            self.fresh(replacement)?;
            let target = after
                .graph
                .nodes
                .get_mut(&node.0)
                .ok_or(EditError::MissingNode(node))?;
            if !editor_ui::set_scalar_input(target, input, replacement.0) {
                return invalid("input is read-only");
            }
            after.graph.nodes.insert(replacement.0, Node::Unconnected);
        }
        self.graph_change(
            after,
            GraphAction::DisconnectWire {
                sink: WireSink::NodeInput {
                    node: node.0,
                    input,
                },
            },
        )?;
        self.cleanup(old)
    }

    pub(super) fn remove_nodes(
        &mut self,
        at: &ScopeRef,
        nodes: &[NodeId],
        replacements: &[NodeId],
    ) -> Result<(), EditError> {
        self.scope(at)?;
        let requested = nodes.iter().map(|id| id.0).collect::<BTreeSet<_>>();
        if requested.is_empty() || requested.len() != nodes.len() {
            return invalid("removal requires distinct nodes");
        }
        for &id in &requested {
            self.node(id)?;
        }
        let mut after = self.project.clone();
        let mut inputs = Vec::new();
        for id in &requested {
            inputs.extend(self.node(*id)?.evaluated_inputs());
            after.graph.nodes.remove(id);
        }
        let mut replacements = replacements.iter();
        let mut new_ids = BTreeSet::new();
        // BTree graph ordering and canvas input ordering fix allocation order.
        for (&owner, node) in &mut after.graph.nodes {
            let original_inputs = node.evaluated_inputs();
            for (index, input) in original_inputs.into_iter().enumerate() {
                if requested.contains(&input) {
                    if let Node::Raise { message } = node {
                        *message = None;
                    } else {
                        let replacement = replacements.next().ok_or_else(|| {
                            EditError::Invalid("not enough replacement IDs".into())
                        })?;
                        self.fresh(*replacement)?;
                        if !new_ids.insert(replacement.0) {
                            return invalid("replacement IDs must be distinct");
                        }
                        if !editor_ui::set_scalar_input(node, index, replacement.0) {
                            return invalid(&format!(
                                "node {owner} has a read-only dependency on removed node {input}"
                            ));
                        }
                        inputs.push(replacement.0);
                    }
                }
            }
        }
        if replacements.next().is_some() {
            return invalid("unused replacement IDs");
        }
        // Reconstruct required hidden placeholders after the mutable graph walk.
        for id in new_ids {
            after.graph.nodes.insert(id, Node::Unconnected);
        }
        remove_bindings(target_root_mut(&mut after, at)?, &requested);
        for id in &requested {
            if referenced(&after, *id) {
                return invalid(&format!("node {id} has a protected owner"));
            }
        }
        self.target_change(at, after, GraphAction::RemoveNodes { requested })?;
        for input in inputs {
            self.cleanup(input)?;
        }
        Ok(())
    }
}

fn target_root_mut<'a>(
    project: &'a mut Project,
    at: &ScopeRef,
) -> Result<&'a mut Scope, EditError> {
    scope_mut(
        project,
        &ScopeRef {
            target: at.target.clone(),
            children: Vec::new(),
        },
    )
}
fn remove_bindings(scope: &mut Scope, requested: &BTreeSet<u32>) {
    scope
        .bindings
        .retain(|binding| !requested.contains(&binding.node));
    for child in &mut scope.children {
        remove_bindings(child, requested);
    }
    if let Some(segments) = scope.concatenated_mut() {
        for segment in segments.iter_mut() {
            remove_bindings(segment, requested);
        }
    }
}
