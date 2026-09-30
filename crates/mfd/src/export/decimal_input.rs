//! Reconstruct a connected native decimal input from a canonical lexical constant.
//!
//! A native kind-6 input with `datatype="decimal"` converts its connected
//! string value. Import represents that boundary as `to_number(constant)`.
//! The inverse is useful only when that conversion has one proven numeric
//! consumer; other uses keep the explicit Ferrule function.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use ir::{ScalarType, SchemaKind, Value};
use mapping::{Node, NodeId, Project};

use super::schema::KeyAlloc;

#[derive(Default)]
pub(super) struct DecimalInputs {
    calls: BTreeMap<NodeId, NodeId>,
}

impl DecimalInputs {
    pub(super) fn plan(project: &Project) -> Self {
        // A SourceField used as the other operand is resolved against the
        // single primary schema. Named sources need their own provenance.
        if !project.extra_sources.is_empty() {
            return Self::default();
        }
        let mut calls = BTreeMap::new();
        for (&id, node) in &project.graph.nodes {
            let Node::Call { function, args } = node else {
                continue;
            };
            let [argument] = args.as_slice() else {
                continue;
            };
            if function != "to_number"
                || !matches!(project.graph.nodes.get(argument), Some(Node::Const { value: Value::String(value) }) if canonical_finite_decimal_literal(value))
            {
                continue;
            }
            let consumers = project
                .graph
                .nodes
                .iter()
                .flat_map(|(&consumer, node)| {
                    node.dependencies()
                        .into_iter()
                        .filter(move |dependency| *dependency == id)
                        .map(move |_| consumer)
                })
                .collect::<Vec<_>>();
            let [consumer] = consumers.as_slice() else {
                continue;
            };
            let Some(Node::Call {
                function: consumer_function,
                args: consumer_args,
            }) = project.graph.nodes.get(consumer)
            else {
                continue;
            };
            let [other, selected] = consumer_args.as_slice() else {
                continue;
            };
            if *selected != id
                || !matches!(
                    consumer_function.as_str(),
                    "add" | "less_than" | "greater_than"
                )
                || !finite_numeric_other_operand(project, *other)
            {
                continue;
            }

            // A direct target binding or control can use this node without
            // appearing as a graph consumer. Replacing the one recognized
            // edge must make the conversion unreachable everywhere.
            let mut without_conversion = project.clone();
            let Some(Node::Call { args, .. }) = without_conversion.graph.nodes.get_mut(consumer)
            else {
                continue;
            };
            args[1] = *argument;
            without_conversion.prune_unreachable_nodes();
            if without_conversion.graph.nodes.contains_key(&id) {
                continue;
            }
            calls.insert(id, *argument);
        }
        Self { calls }
    }

    pub(super) fn input(&self, call: NodeId) -> Option<NodeId> {
        self.calls.get(&call).copied()
    }
}

pub(super) fn render_component(
    call: NodeId,
    keys: &mut KeyAlloc,
    uid: &mut u32,
    components: &mut String,
) -> (u32, u32) {
    let input = keys.next();
    let output = keys.next();
    *uid += 1;
    let name = format!("decimal-input-{call}");
    let _ = write!(
        components,
        "\t\t\t\t<component name=\"{name}\" library=\"core\" uid=\"{uid}\" kind=\"6\">\n\
         \t\t\t\t\t<sources><datapoint pos=\"0\" key=\"{input}\"/></sources>\n\
         \t\t\t\t\t<targets><datapoint pos=\"0\" key=\"{output}\"/></targets>\n\
         \t\t\t\t\t<data><input datatype=\"decimal\"/><parameter usageKind=\"input\" name=\"{name}\"/></data>\n\
         \t\t\t\t</component>\n"
    );
    (input, output)
}

fn canonical_finite_decimal_literal(value: &str) -> bool {
    let (whole, fraction) = value.split_once('.').unwrap_or((value, ""));
    !whole.is_empty()
        && whole.bytes().all(|byte| byte.is_ascii_digit())
        && (whole == "0" || !whole.starts_with('0'))
        && (!value.contains('.') || !fraction.is_empty())
        && fraction.bytes().all(|byte| byte.is_ascii_digit())
        && value.parse::<f64>().is_ok_and(f64::is_finite)
}

fn finite_numeric_other_operand(project: &Project, node: NodeId) -> bool {
    match project.graph.nodes.get(&node) {
        Some(Node::Const {
            value: Value::Int(_),
        }) => true,
        Some(Node::Const {
            value: Value::Float(value),
        }) => value.is_finite(),
        Some(Node::SourceField { path, frame }) => {
            let mut schema = &project.source;
            for segment in frame.iter().flatten().chain(path) {
                let Some(child) = schema.child(segment) else {
                    return false;
                };
                schema = child;
            }
            !schema.repeating
                && matches!(
                    schema.kind,
                    SchemaKind::Scalar {
                        ty: ScalarType::Int | ScalarType::Float
                    }
                )
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::canonical_finite_decimal_literal;

    #[test]
    fn canonical_finite_numeric_literals_only() {
        for value in ["1.5", "20", "0", "0.25"] {
            assert!(canonical_finite_decimal_literal(value), "{value}");
        }
        for value in [
            "",
            " ",
            " 5 ",
            "2e3",
            "05",
            ".5",
            "1.",
            "-0",
            "NaN",
            "inf",
            "1e309",
            "not-a-number",
        ] {
            assert!(!canonical_finite_decimal_literal(value), "{value}");
        }
    }
}
