use std::collections::BTreeSet;

use ir::{SchemaKind, SchemaNode, Value};
use mapping::{IterationOutput, Node, SequenceExpr};

use super::graph::GraphBuilder;
use super::schema::{ComponentFormat, SchemaComponent};
use super::scope::{IterationNodes, ScopeBuilder, TargetLeaf};

/// A repeated protobuf message with connected descendants but no structural
/// sequence input represents one constructed occurrence. This is distinct
/// from an explicitly connected repeated message, whose feed drives ordinary
/// target iteration.
pub(super) fn infer_singleton_messages(
    target: &SchemaComponent,
    bindings: &[(TargetLeaf, u32)],
    explicit_iterations: &BTreeSet<Vec<String>>,
    builder: &mut GraphBuilder<'_>,
    scopes: &mut ScopeBuilder,
    skipped: &mut Vec<Vec<String>>,
) {
    if target.format != ComponentFormat::Protobuf || target.options.protobuf.is_none() {
        return;
    }
    let connected = bindings
        .iter()
        .map(|(target, _)| target.path())
        .collect::<BTreeSet<_>>();
    let mut singleton_messages = Vec::new();
    let mut repeated_scalars = Vec::new();
    collect_unfed_repetition(
        &target.schema,
        &mut Vec::new(),
        &connected,
        explicit_iterations,
        &mut singleton_messages,
        &mut repeated_scalars,
    );

    for path in singleton_messages {
        let upper = builder.alloc(Node::Const {
            value: Value::Int(1),
        });
        let item = builder.alloc(Node::SourceField {
            path: Vec::new(),
            frame: None,
        });
        scopes.add_sequence(
            &path,
            SequenceExpr::Generate {
                from: None,
                to: upper,
                item,
            },
            IterationNodes {
                filter: None,
                group_by: None,
                group_starting_with: None,
                group_into_blocks: None,
                sort_by: None,
                sort_descending: false,
                take: None,
            },
            IterationOutput::Repeated,
        );
    }
    for path in repeated_scalars {
        builder.warnings.push(format!(
            "repeated protobuf scalar target `{}` has no sequence input; binding skipped",
            path.join("/")
        ));
        skipped.push(path);
    }
}

fn collect_unfed_repetition(
    node: &SchemaNode,
    path: &mut Vec<String>,
    connected: &BTreeSet<Vec<String>>,
    explicit_iterations: &BTreeSet<Vec<String>>,
    messages: &mut Vec<Vec<String>>,
    scalars: &mut Vec<Vec<String>>,
) {
    let SchemaKind::Group { children, .. } = &node.kind else {
        return;
    };
    for child in children {
        path.push(child.name.clone());
        if child.repeating && !explicit_iterations.contains(path) {
            match &child.kind {
                SchemaKind::Group { .. }
                    if connected
                        .iter()
                        .any(|binding| binding.len() > path.len() && binding.starts_with(path)) =>
                {
                    messages.push(path.clone());
                }
                SchemaKind::Scalar { .. } if connected.contains(path) => {
                    scalars.push(path.clone());
                }
                SchemaKind::Group { .. } | SchemaKind::Scalar { .. } => {}
            }
        }
        collect_unfed_repetition(
            child,
            path,
            connected,
            explicit_iterations,
            messages,
            scalars,
        );
        path.pop();
    }
}
