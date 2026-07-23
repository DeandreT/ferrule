use std::collections::{BTreeMap, BTreeSet};

use mapping::{FunctionId, Graph, Node, NodeId, Project, UserFunction};

use super::ValidationIssue;
use crate::user_function::MAX_USER_FUNCTION_DEPTH;

pub(super) fn validate_user_functions(project: &Project, issues: &mut Vec<ValidationIssue>) {
    validate_main_graph(project, issues);

    let mut names = BTreeMap::new();
    for (&id, function) in &project.user_functions {
        let location = function_location(id, function);
        let library = function.library.trim();
        let name = function.name.trim();
        if library.is_empty() {
            issues.push(ValidationIssue::new(
                &location,
                "function library cannot be empty",
            ));
        }
        if name.is_empty() {
            issues.push(ValidationIssue::new(
                &location,
                "function name cannot be empty",
            ));
        } else if let Some(first) = names.insert((library, name), id) {
            issues.push(ValidationIssue::new(
                &location,
                format!(
                    "function library and name duplicate function {}",
                    first.get()
                ),
            ));
        }
        if function.output_name.trim().is_empty() {
            issues.push(ValidationIssue::new(
                &location,
                "function output name cannot be empty",
            ));
        }
        validate_parameters(&location, function, issues);
        validate_body(project, id, function, issues);
    }
    validate_call_graph(project, issues);
}

fn validate_main_graph(project: &Project, issues: &mut Vec<ValidationIssue>) {
    for (&node_id, node) in &project.graph.nodes {
        let location = format!("graph node {node_id}");
        match node {
            Node::FunctionParameter { .. } => issues.push(ValidationIssue::new(
                location,
                "function parameters are valid only inside a user-defined function",
            )),
            Node::UserFunctionCall { function, args } => {
                validate_call(project, &location, *function, args.len(), issues);
            }
            _ => {}
        }
    }
}

fn validate_parameters(location: &str, function: &UserFunction, issues: &mut Vec<ValidationIssue>) {
    let mut ids = BTreeSet::new();
    let mut names = BTreeSet::new();
    for parameter in &function.parameters {
        if !ids.insert(parameter.id) {
            issues.push(ValidationIssue::new(
                location,
                format!("parameter id {} is duplicated", parameter.id.get()),
            ));
        }
        let name = parameter.name.trim();
        if name.is_empty() {
            issues.push(ValidationIssue::new(
                location,
                format!("parameter {} has an empty name", parameter.id.get()),
            ));
        } else if !names.insert(name) {
            issues.push(ValidationIssue::new(
                location,
                format!("parameter name `{name}` is duplicated"),
            ));
        }
    }
}

fn validate_body(
    project: &Project,
    function_id: FunctionId,
    function: &UserFunction,
    issues: &mut Vec<ValidationIssue>,
) {
    let location = function_location(function_id, function);
    if !function.body.nodes.contains_key(&function.output) {
        issues.push(ValidationIssue::new(
            &location,
            format!("output references missing body node {}", function.output),
        ));
    }
    let parameter_ids: BTreeSet<_> = function
        .parameters
        .iter()
        .map(|parameter| parameter.id)
        .collect();
    for (&node_id, node) in &function.body.nodes {
        let node_location = format!("{location} body node {node_id}");
        for dependency in node.dependencies() {
            if !function.body.nodes.contains_key(&dependency) {
                issues.push(ValidationIssue::new(
                    &node_location,
                    format!("references missing body node {dependency}"),
                ));
            }
        }
        match node {
            Node::Unconnected | Node::Const { .. } | Node::If { .. } | Node::ValueMap { .. } => {}
            Node::FunctionParameter { parameter } => {
                if !parameter_ids.contains(parameter) {
                    issues.push(ValidationIssue::new(
                        &node_location,
                        format!("references undeclared parameter {}", parameter.get()),
                    ));
                }
            }
            Node::Call { function, .. } => {
                if !functions::is_known(function) {
                    issues.push(ValidationIssue::new(
                        &node_location,
                        format!("unknown function `{function}`"),
                    ));
                }
            }
            Node::UserFunctionCall { function, args } => {
                validate_call(project, &node_location, *function, args.len(), issues);
            }
            _ => issues.push(ValidationIssue::new(
                &node_location,
                "node kind is not supported in an isolated scalar user-defined function",
            )),
        }
    }
    validate_body_cycles(&location, &function.body, issues);
}

fn validate_call(
    project: &Project,
    location: &str,
    function: FunctionId,
    argument_count: usize,
    issues: &mut Vec<ValidationIssue>,
) {
    let Some(definition) = project.user_functions.get(&function) else {
        issues.push(ValidationIssue::new(
            location,
            format!(
                "references missing user-defined function {}",
                function.get()
            ),
        ));
        return;
    };
    if argument_count != definition.parameters.len() {
        issues.push(ValidationIssue::new(
            location,
            format!(
                "user-defined function {} expects {} argument(s), got {argument_count}",
                function.get(),
                definition.parameters.len()
            ),
        ));
    }
}

fn validate_body_cycles(location: &str, graph: &Graph, issues: &mut Vec<ValidationIssue>) {
    fn visit(
        id: NodeId,
        graph: &Graph,
        active: &mut BTreeSet<NodeId>,
        done: &mut BTreeSet<NodeId>,
        reported: &mut BTreeSet<NodeId>,
        location: &str,
        issues: &mut Vec<ValidationIssue>,
    ) {
        active.insert(id);
        if let Some(node) = graph.nodes.get(&id) {
            for dependency in node.dependencies() {
                if active.contains(&dependency) {
                    if reported.insert(dependency) {
                        issues.push(ValidationIssue::new(
                            format!("{location} body node {id}"),
                            format!("cycle reaches body node {dependency}"),
                        ));
                    }
                } else if graph.nodes.contains_key(&dependency) && !done.contains(&dependency) {
                    visit(dependency, graph, active, done, reported, location, issues);
                }
            }
        }
        active.remove(&id);
        done.insert(id);
    }

    let mut active = BTreeSet::new();
    let mut done = BTreeSet::new();
    let mut reported = BTreeSet::new();
    for &id in graph.nodes.keys() {
        if !done.contains(&id) {
            visit(
                id,
                graph,
                &mut active,
                &mut done,
                &mut reported,
                location,
                issues,
            );
        }
    }
}

fn validate_call_graph(project: &Project, issues: &mut Vec<ValidationIssue>) {
    let mut calls = BTreeMap::<FunctionId, BTreeSet<FunctionId>>::new();
    let mut incoming = project
        .user_functions
        .keys()
        .map(|&id| (id, 0_usize))
        .collect::<BTreeMap<_, _>>();
    for (&caller, definition) in &project.user_functions {
        let callees = calls.entry(caller).or_default();
        for callee in definition
            .body
            .nodes
            .values()
            .filter_map(|node| match node {
                Node::UserFunctionCall { function, .. }
                    if project.user_functions.contains_key(function) =>
                {
                    Some(*function)
                }
                _ => None,
            })
        {
            if callees.insert(callee)
                && let Some(count) = incoming.get_mut(&callee)
            {
                *count += 1;
            }
        }
    }

    let mut ready = incoming
        .iter()
        .filter_map(|(&id, &count)| (count == 0).then_some(id))
        .collect::<BTreeSet<_>>();
    let mut depths = ready
        .iter()
        .map(|&id| (id, 1_usize))
        .collect::<BTreeMap<_, _>>();
    let mut processed = 0;
    let mut reported_depth = false;
    while let Some(function) = ready.pop_first() {
        processed += 1;
        let depth = depths.get(&function).copied().unwrap_or(1);
        for &callee in calls.get(&function).into_iter().flatten() {
            let next_depth = depth + 1;
            depths
                .entry(callee)
                .and_modify(|current| *current = (*current).max(next_depth))
                .or_insert(next_depth);
            if next_depth > MAX_USER_FUNCTION_DEPTH && !reported_depth {
                issues.push(ValidationIssue::new(
                    format!("user function {}", callee.get()),
                    format!(
                        "call nesting exceeds the limit of {MAX_USER_FUNCTION_DEPTH} functions"
                    ),
                ));
                reported_depth = true;
            }
            let Some(count) = incoming.get_mut(&callee) else {
                continue;
            };
            *count -= 1;
            if *count == 0 {
                ready.insert(callee);
            }
        }
    }
    if processed != project.user_functions.len() {
        issues.push(ValidationIssue::new(
            "user functions",
            "recursive user-defined function calls are not supported",
        ));
    }
}

fn function_location(id: FunctionId, function: &UserFunction) -> String {
    let library = function.library.trim();
    let name = function.name.trim();
    if library.is_empty() && name.is_empty() {
        format!("user function {}", id.get())
    } else if library.is_empty() {
        format!("user function `{name}` ({})", id.get())
    } else {
        format!("user function `{library}:{name}` ({})", id.get())
    }
}
