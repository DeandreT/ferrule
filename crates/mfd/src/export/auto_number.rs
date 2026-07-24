use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use ir::Value;
use mapping::{Graph, Node, NodeId};

use super::schema::KeyAlloc;

#[derive(Clone, Copy)]
pub(super) struct AutoNumberPattern {
    pub(super) start: NodeId,
    pub(super) increment: NodeId,
    internal: [NodeId; 4],
}

#[derive(Default)]
pub(super) struct AutoNumbers {
    patterns: BTreeMap<NodeId, AutoNumberPattern>,
    internal: BTreeSet<NodeId>,
}

pub(super) struct AutoNumberInputs {
    pub(super) start: (NodeId, u32),
    pub(super) increment: (NodeId, u32),
}

impl AutoNumbers {
    pub(super) fn collect(graph: &Graph) -> Self {
        let mut patterns = BTreeMap::new();
        let mut internal = BTreeSet::new();
        for &root in graph.nodes.keys() {
            let Some(pattern) = pattern_at(graph, root) else {
                continue;
            };
            if pattern.internal.iter().any(|id| internal.contains(id)) {
                continue;
            }
            internal.extend(pattern.internal);
            patterns.insert(root, pattern);
        }
        Self { patterns, internal }
    }

    pub(super) fn pattern(&self, id: NodeId) -> Option<AutoNumberPattern> {
        self.patterns.get(&id).copied()
    }

    pub(super) fn owns_internal(&self, id: NodeId) -> bool {
        self.internal.contains(&id)
    }
}

pub(super) fn pattern_at(graph: &Graph, root: NodeId) -> Option<AutoNumberPattern> {
    let Node::Call {
        function,
        args: add_args,
    } = graph.nodes.get(&root)?
    else {
        return None;
    };
    if function != "add" {
        return None;
    }
    let [start, offset] = add_args.as_slice() else {
        return None;
    };
    let Node::Call {
        function,
        args: multiply_args,
    } = graph.nodes.get(offset)?
    else {
        return None;
    };
    if function != "multiply" {
        return None;
    }
    let [zero_based, increment] = multiply_args.as_slice() else {
        return None;
    };
    let Node::Call {
        function,
        args: subtract_args,
    } = graph.nodes.get(zero_based)?
    else {
        return None;
    };
    if function != "subtract" {
        return None;
    }
    let [position, one] = subtract_args.as_slice() else {
        return None;
    };
    if !matches!(graph.nodes.get(position), Some(Node::Position { collection }) if collection.is_empty())
        || !matches!(
            graph.nodes.get(one),
            Some(Node::Const {
                value: Value::Int(1)
            })
        )
    {
        return None;
    }

    let internal = [*offset, *zero_based, *position, *one];
    let distinct = internal.into_iter().collect::<BTreeSet<_>>();
    if distinct.len() != internal.len()
        || distinct.contains(start)
        || distinct.contains(increment)
        || consumers(graph, *offset) != [root]
        || consumers(graph, *zero_based) != [*offset]
        || consumers(graph, *position) != [*zero_based]
        || consumers(graph, *one) != [*zero_based]
    {
        return None;
    }

    Some(AutoNumberPattern {
        start: *start,
        increment: *increment,
        internal,
    })
}

pub(super) fn render_component(
    pattern: AutoNumberPattern,
    keys: &mut KeyAlloc,
    uid: &mut u32,
    components: &mut String,
) -> (u32, AutoNumberInputs) {
    let start_input = keys.next();
    let increment_input = keys.next();
    let output = keys.next();
    *uid += 1;
    let _ = write!(
        components,
        "\t\t\t\t<component name=\"auto-number\" library=\"core\" uid=\"{uid}\" kind=\"5\">\n\
         \t\t\t\t\t<sources><datapoint/><datapoint pos=\"1\" key=\"{start_input}\"/><datapoint pos=\"2\" key=\"{increment_input}\"/><datapoint/></sources>\n\
         \t\t\t\t\t<targets><datapoint pos=\"0\" key=\"{output}\"/></targets>\n\
         \t\t\t\t\t<view ltx=\"20\" lty=\"20\" rbx=\"120\" rby=\"60\"/>\n\
         \t\t\t\t</component>\n"
    );
    (
        output,
        AutoNumberInputs {
            start: (pattern.start, start_input),
            increment: (pattern.increment, increment_input),
        },
    )
}

fn consumers(graph: &Graph, id: NodeId) -> Vec<NodeId> {
    graph
        .nodes
        .iter()
        .filter_map(|(&consumer, node)| node_inputs(node).contains(&id).then_some(consumer))
        .collect()
}

fn node_inputs(node: &Node) -> Vec<NodeId> {
    match node {
        Node::Call { args, .. } | Node::UserFunctionCall { args, .. } => args.clone(),
        Node::If {
            condition,
            then,
            else_,
        } => vec![*condition, *then, *else_],
        Node::ValueMap { input, .. } => vec![*input],
        Node::Lookup { matches, .. } => vec![*matches],
        Node::DynamicSourceField { key, .. } => vec![*key],
        Node::XmlMixedContent { replacements, .. } => replacements
            .iter()
            .map(|replacement| replacement.expression)
            .collect(),
        Node::CollectionFind {
            predicate, value, ..
        } => vec![*predicate, *value],
        Node::SequenceExists {
            sequence,
            predicate,
        } => sequence
            .inputs()
            .into_iter()
            .chain([sequence.item(), *predicate])
            .collect(),
        Node::SequenceItemAt { sequence, index } => sequence
            .inputs()
            .into_iter()
            .chain([sequence.item(), *index])
            .collect(),
        Node::Aggregate {
            expression, arg, ..
        }
        | Node::JoinAggregate {
            expression, arg, ..
        } => expression.iter().chain(arg).copied().collect(),
        Node::SourceField { .. }
        | Node::SourceDocumentPath
        | Node::Position { .. }
        | Node::JoinField { .. }
        | Node::JoinPosition { .. }
        | Node::Unconnected
        | Node::Const { .. }
        | Node::FunctionParameter { .. }
        | Node::RuntimeValue { .. }
        | Node::RuntimeParameter { .. }
        | Node::XmlSerialize { .. } => Vec::new(),
    }
}
