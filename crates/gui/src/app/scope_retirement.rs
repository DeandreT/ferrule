use super::*;

fn owned_items(scope: &Scope, items: &mut std::collections::BTreeSet<NodeId>) {
    if let Some(sequence) = scope.sequence() {
        items.insert(sequence.item());
    }
    if let Some(segments) = scope.concatenated() {
        for segment in segments.iter() {
            owned_items(segment, items);
        }
    }
    for child in &scope.children {
        owned_items(child, items);
    }
    for child in &scope.dynamic_children {
        owned_items(&child.scope, items);
    }
}

fn retirement_items(project: &Project, removed: &Scope) -> Result<Vec<NodeId>, String> {
    let mut items = std::collections::BTreeSet::new();
    owned_items(removed, &mut items);
    for &item in &items {
        if project
            .graph
            .nodes
            .get(&item)
            .is_some_and(|node| !matches!(node, mapping::Node::SourceField { .. }))
        {
            return Err(format!(
                "Generated item #{item} has an invalid node kind; repair its ownership before removing the scope"
            ));
        }
        let references = crate::graph_viewer::references_outside_scope(project, removed, item);
        if !references.is_empty() {
            return Err(format!(
                "Generated item #{item} is still used by {}; disconnect those references before removing its scope",
                references.join(", ")
            ));
        }
    }
    Ok(items.into_iter().collect())
}

impl FerruleApp {
    pub(super) fn remove_selected_target_scope(&mut self) {
        let target_index = match self.mapping_workspace.active {
            MappingDocument::Main => None,
            MappingDocument::Target(index) => Some(index),
            MappingDocument::Function(_) => return,
        };
        let root = match target_index {
            Some(index) => self
                .project
                .extra_targets
                .get(index)
                .map(|target| &target.root),
            None => Some(&self.project.root),
        };
        let Some(removed) =
            root.and_then(|root| crate::auto_connect::scope_at(root, &self.selected_scope))
        else {
            self.scope_removal_failed("The selected scope no longer exists".into());
            return;
        };
        if join_authoring::subtree_has_join(removed) {
            self.scope_removal_failed(join_authoring::JOIN_REMOVAL_REASON.into());
            return;
        }
        let items = match retirement_items(&self.project, removed) {
            Ok(items) => items,
            Err(error) => {
                self.scope_removal_failed(error);
                return;
            }
        };
        let root = match target_index {
            Some(index) => &mut self.project.extra_targets[index].root,
            None => &mut self.project.root,
        };
        match remove_child_scope(root, &self.selected_scope) {
            Ok(selection) => {
                for item in items {
                    self.project.graph.nodes.remove(&item);
                }
                self.selected_scope = selection;
                self.rebuild_mapping_canvases_after_retirement();
                self.status = "scope tree updated".into();
                self.diagnostics.clear();
            }
            Err(error) => self.scope_removal_failed(error.to_string()),
        }
    }

    fn scope_removal_failed(&mut self, message: String) {
        self.status = "scope edit failed".into();
        self.diagnostics.error("Scope edit failed", message);
    }

    pub(super) fn rebuild_mapping_canvases_after_retirement(&mut self) {
        // Retired item nodes belong to the shared mapping graph. Every open
        // mapping canvas must forget them while retaining surviving positions.
        let layout = CanvasLayout::capture(
            &self.project,
            &self.main_canvas.snarl,
            &self.mapping_workspace,
        );
        self.main_canvas.snarl = build_snarl_with_layout(&self.project, Some(&layout));
        for (&index, canvas) in &mut self.mapping_workspace.target_canvases {
            let nodes = CanvasLayout::capture_nodes(&canvas.snarl);
            let mut snarl = canvas_build::build_named_target_snarl(&self.project, index);
            CanvasLayout::apply_nodes(&nodes, &mut snarl);
            *canvas = CanvasDocumentState::with_snarl(snarl);
        }
    }
}
