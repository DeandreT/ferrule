//! A bounded global-input record array adapter. The generated sequence owns
//! one one-based item index; every field uses that same index and complete
//! schema, so independent fields cannot shorten or reorder the record stream.

use std::collections::{BTreeMap, BTreeSet};

use ir::{SchemaKind, SchemaNode, Value};
use mapping::{IterationOutput, Node, SequenceExpr};

use super::super::graph::GraphBuilder;
use super::super::schema::{SchemaComponent, schema_node_at};
use super::super::scope::{BindingOccurrence, IterationNodes, ScopeBuilder, TargetLeaf};

fn record_object(node: &SchemaNode) -> bool {
    matches!(node.kind, SchemaKind::Group { .. })
        && !node.json_array_wrapper
        && !node.json_any
        && !node.container_nullable
        && node.json_object_item_nullable != Some(true)
        && node.dynamic_fields().is_none()
        && node.alternatives().is_empty()
}

pub(super) fn record_path(schema: &SchemaNode, path: &[String]) -> Option<Vec<String>> {
    if schema.repeating || !record_object(schema) {
        return None;
    }
    let mut array = None;
    for length in 1..=path.len() {
        let node = schema_node_at(schema, &path[..length])?;
        if node.repeating {
            if array.is_some() || !record_object(node) {
                return None;
            }
            array = Some(path[..length].to_vec());
        } else if length < path.len() && !record_object(node) {
            return None;
        }
    }
    let array = array?;
    let leaf = schema_node_at(schema, path)?;
    (path == array || (leaf.is_scalar() && !leaf.json_any)).then_some(array)
}

struct Connection {
    input: u32,
    feed: u32,
    recipe: usize,
    array: Vec<String>,
    source: Vec<String>,
    target: Vec<String>,
}

pub(in crate::import) fn install(
    target: &SchemaComponent,
    builder: &mut GraphBuilder<'_>,
    scopes: &mut ScopeBuilder,
) -> BTreeSet<u32> {
    let mut handled = BTreeSet::new();
    let mut groups: BTreeMap<Vec<String>, Vec<Connection>> = BTreeMap::new();
    for (&input, path) in &target.ports {
        let Some(feed) = builder.edge_from.get(&input).copied() else {
            continue;
        };
        for (recipe, parser) in builder.json_parsers.iter().enumerate() {
            let Some(source) = parser.outputs.get(&feed) else {
                continue;
            };
            let Some(array) = record_path(&parser.schema, source) else {
                continue;
            };
            handled.insert(input);
            let repeated: Vec<_> = (1..=path.len())
                .filter(|length| {
                    schema_node_at(&target.schema, &path[..*length])
                        .is_some_and(|node| node.repeating)
                })
                .collect();
            let [length] = repeated.as_slice() else {
                builder.warnings.push(format!(
                    "JSON string parser array output `{feed}` into `{}` requires exactly one repeating record target; skipped", path.join("/")
                ));
                continue;
            };
            groups
                .entry(path[..*length].to_vec())
                .or_default()
                .push(Connection {
                    input,
                    feed,
                    recipe,
                    array,
                    source: source.clone(),
                    target: path.clone(),
                });
        }
    }
    for (row, connections) in groups {
        let first = &connections[0];
        let parser = &builder.json_parsers[first.recipe];
        let reason = if !super::super::constructed_parser_scalar::input_is_required_string(
            parser, builder,
        ) || parser.unresolved_schema_reference.is_some()
        {
            Some(
                "requires a uniquely declared required global String input and a resolved JSON schema",
            )
        } else if connections.iter().any(|connection| {
            connection.recipe != first.recipe
                || connection.array != first.array
                || !parser.runtime_scalar_outputs.contains(&connection.feed)
                || super::super::constructed_parser_scalar::output_is_shadowed(
                    connection.feed,
                    builder,
                )
        }) {
            Some("requires one unambiguous parser array per target record")
        } else if target.schema.repeating
            || target.has_target_node_controls
            || !(0..row.len()).all(|length| {
                schema_node_at(&target.schema, &row[..length]).is_some_and(super::ordinary_object)
            })
            || !schema_node_at(&target.schema, &row).is_some_and(record_object)
            || target
                .input_branch_paths
                .iter()
                .any(|(_, path)| path.starts_with(&row))
            || connections
                .iter()
                .any(|connection| target.ambiguous_branch_inputs.contains(&connection.input))
            || connections.iter().any(|connection| {
                if connection.source == connection.array {
                    connection.target != row
                } else {
                    !schema_node_at(&target.schema, &connection.target)
                        .is_some_and(|node| node.is_scalar())
                }
            })
        {
            Some(
                "requires an ordinary repeating group with scalar field targets, without cloned occurrences or target node controls",
            )
        } else if target.ports.iter().any(|(input, path)| {
            builder.edge_from.contains_key(input)
                && (path.starts_with(&row) || row.starts_with(path))
                && !connections
                    .iter()
                    .any(|connection| connection.input == *input)
        }) || scopes.scope(&row).is_some_and(|scope| scope.iterates())
            || !scopes.anchors.is_empty()
        {
            Some(
                "cannot combine array records with other connected record feeds or enclosing iteration",
            )
        } else {
            None
        };
        if let Some(reason) = reason {
            builder.warnings.push(format!(
                "JSON string parser array into `{}` {reason}; skipped",
                row.join("/")
            ));
            continue;
        }
        let input_key = parser.input;
        let schema = serde_json::to_string(&parser.schema).expect("schema serializes");
        let Some(input) = builder.parser_input_node(input_key) else {
            builder.warnings.push(format!(
                "JSON string parser array into `{}` has an unresolved input; skipped",
                row.join("/")
            ));
            continue;
        };
        let schema = builder.alloc(Node::Const {
            value: Value::String(schema),
        });
        let array = builder.alloc(Node::Const {
            value: Value::String(serde_json::to_string(&first.array).expect("path serializes")),
        });
        let count = builder.alloc(Node::Call {
            function: "json_parse_array_length".into(),
            args: vec![input, schema, array],
        });
        // A unique owner node prevents sibling arrays from sharing item context.
        let item = builder.alloc(Node::SourceField {
            path: Vec::new(),
            frame: None,
        });
        scopes.add_sequence(
            &row,
            SequenceExpr::Generate {
                from: None,
                to: count,
                item,
            },
            IterationNodes::default(),
            IterationOutput::Repeated,
        );
        for connection in connections {
            if connection.source == connection.array {
                continue;
            }
            let path = builder.alloc(Node::Const {
                value: Value::String(
                    serde_json::to_string(&connection.source[connection.array.len()..])
                        .expect("path serializes"),
                ),
            });
            let value = builder.alloc(Node::Call {
                function: "json_parse_array_field".into(),
                args: vec![input, schema, array, path, item],
            });
            scopes.add_binding(
                TargetLeaf::from_path(&connection.target).expect("scalar target path"),
                value,
                BindingOccurrence::authored(target, connection.input),
            );
        }
    }
    handled
}
