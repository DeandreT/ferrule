use super::*;

impl FerruleApp {
    pub(super) fn try_insert_function_call(&mut self, function: FunctionId) -> Result<(), String> {
        let Some(argument_count) = self
            .project
            .user_functions
            .get(&function)
            .map(|definition| definition.parameters.len())
        else {
            return Ok(());
        };
        let active = self.mapping_workspace.active;
        let owned_items = if matches!(active, MappingDocument::Function(_)) {
            Default::default()
        } else {
            crate::graph_viewer::project_sequence_item_ids(&self.project)
        };
        let graph = match active {
            MappingDocument::Main => &self.project.graph,
            MappingDocument::Target(target) => {
                if target >= self.project.extra_targets.len() {
                    return Ok(());
                }
                &self.project.graph
            }
            MappingDocument::Function(owner) => {
                let Some(definition) = self.project.user_functions.get(&owner) else {
                    return Ok(());
                };
                &definition.body
            }
        };
        // Prepare the complete action before creating even a cached canvas:
        // exhaustion must not alter model nodes, wires, layout or edit history.
        let (args, call) = reserve_call_ids(graph, &owned_items, argument_count)?;
        match active {
            MappingDocument::Main => insert_call(
                &mut self.project.graph,
                &mut self.main_canvas.snarl,
                function,
                args,
                call,
            ),
            MappingDocument::Target(target) => {
                if !self.ensure_target_canvas(target) {
                    return Ok(());
                }
                if let Some(canvas) = self.mapping_workspace.target_canvases.get_mut(&target) {
                    insert_call(
                        &mut self.project.graph,
                        &mut canvas.snarl,
                        function,
                        args,
                        call,
                    );
                }
            }
            MappingDocument::Function(owner) => {
                if !self.ensure_function_canvas(owner) {
                    return Ok(());
                }
                let (functions, canvases) = (
                    &mut self.project.user_functions,
                    &mut self.mapping_workspace.function_canvases,
                );
                if let (Some(definition), Some(canvas)) =
                    (functions.get_mut(&owner), canvases.get_mut(&owner))
                {
                    insert_call(
                        &mut definition.body,
                        &mut canvas.snarl,
                        function,
                        args,
                        call,
                    );
                }
            }
        }
        Ok(())
    }
}

pub(super) fn reserve_call_ids(
    graph: &Graph,
    owned_items: &std::collections::BTreeSet<NodeId>,
    argument_count: usize,
) -> Result<(Vec<NodeId>, NodeId), String> {
    const EXHAUSTED: &str =
        "No mapping node identifiers remain for all function inputs and the call";
    let count = argument_count
        .checked_add(1)
        .ok_or_else(|| EXHAUSTED.to_string())?;
    let mut candidate = match graph.nodes.keys().next_back() {
        Some(id) => id.checked_add(1).ok_or_else(|| EXHAUSTED.to_string())?,
        None => 0,
    };
    let available = u64::from(NodeId::MAX) - u64::from(candidate) + 1;
    let count_u64 = u64::try_from(count).map_err(|_| EXHAUSTED.to_string())?;
    if count_u64 > available {
        return Err(EXHAUSTED.to_string());
    }
    let mut ids = Vec::with_capacity(count);
    while ids.len() < count {
        if !owned_items.contains(&candidate) {
            ids.push(candidate);
        }
        if ids.len() == count {
            break;
        }
        candidate = candidate
            .checked_add(1)
            .ok_or_else(|| EXHAUSTED.to_string())?;
    }
    let call = ids.pop().ok_or_else(|| EXHAUSTED.to_string())?;
    Ok((ids, call))
}

fn insert_call(
    graph: &mut Graph,
    snarl: &mut Snarl<CanvasNode>,
    function: FunctionId,
    args: Vec<NodeId>,
    call: NodeId,
) {
    let position = egui::pos2(80.0, graph.nodes.len() as f32 * 24.0);
    for &id in &args {
        graph.nodes.insert(id, Node::Unconnected);
    }
    graph
        .nodes
        .insert(call, Node::UserFunctionCall { function, args });
    snarl.insert_node(position, CanvasNode::Graph(call));
}
