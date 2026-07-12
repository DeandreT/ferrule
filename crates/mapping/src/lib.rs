//! The mapping graph IR: nodes and connections that describe how a source
//! instance is transformed into a target instance, plus the project file
//! (source schema + target schema + graph + scope tree) that gets
//! saved/loaded.

use std::collections::BTreeMap;

use ir::{SchemaNode, Value};
use serde::{Deserialize, Serialize};

mod iteration;
mod scope_serde;

pub use iteration::{
    JoinConditions, JoinId, JoinKey, JoinPlan, JoinPlanError, JoinSource, ScopeIteration,
};

fn is_false(value: &bool) -> bool {
    !*value
}

pub type NodeId = u32;

/// A value supplied by the execution host rather than source instance data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeValue {
    /// Path of the mapping that owns the expression being evaluated.
    MappingFilePath,
    /// Path of the top-level mapping for the current run.
    MainMappingFilePath,
    /// One stable local timestamp captured for the current run.
    CurrentDateTime,
}

/// A single node in the mapping graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Node {
    /// Reads a scalar field at `path`, resolved against the innermost
    /// currently-iterating source item, falling back to enclosing items
    /// (nearest enclosing wins) if not found there. That fallback is what
    /// lets a leaf from an outer group (e.g. an Order's ID) be "broadcast"
    /// into a nested target group (e.g. every Item row) with no extra
    /// plumbing -- see `engine::resolve_scalar`.
    SourceField {
        path: Vec<String>,
        /// Absolute repeated collection whose current item owns `path`.
        /// `None` preserves the usual innermost-first outward fallback.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        frame: Option<Vec<String>>,
    },
    /// Returns the 1-based position of the current item in `collection`'s
    /// iteration. An empty collection selects the innermost iteration frame.
    Position {
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        collection: Vec<String>,
    },
    /// Reads a scalar from one source frame owned by an inner-join scope.
    /// Unlike `SourceField`, this node cannot fall back to an unrelated
    /// context when the owning join is not active.
    JoinField {
        join: JoinId,
        collection: Vec<String>,
        path: Vec<String>,
    },
    /// Returns the flattened 1-based output position of an inner join.
    JoinPosition { join: JoinId },
    /// A literal value.
    Const { value: Value },
    /// Reads a value supplied explicitly by the execution host.
    RuntimeValue { value: RuntimeValue },
    /// Calls a built-in function (see the `functions` crate) with the
    /// evaluated outputs of the given argument nodes.
    Call { function: String, args: Vec<NodeId> },
    /// Evaluates `condition`; if it's `true` evaluates and returns `then`,
    /// otherwise `else_`. Unlike `Call`, only the taken branch is evaluated
    /// -- important once branches can error or have side effects.
    If {
        condition: NodeId,
        then: NodeId,
        #[serde(rename = "else")]
        else_: NodeId,
    },
    /// Looks `input` up in `table` (first matching entry wins) and returns
    /// its paired value, falling back to `default` if there's no match.
    ValueMap {
        input: NodeId,
        table: Vec<(Value, Value)>,
        default: Option<Value>,
    },
    /// A cross-source join: evaluates `matches`, then scans the repeating
    /// data at `collection` (resolved with the same outward fallback as
    /// `SourceField`, so it can name an extra source) for the first item
    /// whose `key` field equals it, returning that item's `value` field.
    /// A miss returns `Null` -- pair with `If`/`equal` when a miss should
    /// mean something else.
    Lookup {
        collection: Vec<String>,
        key: Vec<String>,
        matches: NodeId,
        value: Vec<String>,
    },
    /// Returns whether any item produced by `sequence` satisfies `predicate`.
    /// The sequence's item node is an owned empty-path `SourceField` that is
    /// available only while evaluating the predicate.
    SequenceExists {
        sequence: SequenceExpr,
        predicate: NodeId,
    },
    /// Reduces a repeating collection to one scalar. `collection` is
    /// resolved with the same outward fallback as `Lookup`; `value` picks
    /// the scalar inside each item (empty = the item itself, for collections
    /// of scalars). When `expression` is set it is evaluated once per item
    /// instead, which represents sequence-producing mappings such as
    /// `sum(Item/Price * Item/Quantity)`. `arg` supplies the extra scalar
    /// parameter some operations take: `join`'s separator (default "") and
    /// `item_at`'s 1-based position.
    Aggregate {
        function: AggregateOp,
        collection: Vec<String>,
        #[serde(default)]
        value: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        expression: Option<NodeId>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        arg: Option<NodeId>,
    },
}

/// The reduction an [`Node::Aggregate`] applies over its collection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AggregateOp {
    Count,
    Sum,
    Avg,
    Min,
    Max,
    Join,
    ItemAt,
}

/// The mapping graph for one project: every node that can be wired into a
/// target field, keyed by id so multiple target fields can share subgraphs.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Graph {
    pub nodes: BTreeMap<NodeId, Node>,
}

/// Connects a graph node's output to a named scalar field on the current
/// scope's target group.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Binding {
    pub target_field: String,
    pub node: NodeId,
}

/// Inserts one scalar under a property name evaluated at run time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DynamicBinding {
    pub key: NodeId,
    pub value: NodeId,
}

/// Inserts a child scope's complete value under a property name evaluated
/// in the enclosing scope context.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DynamicChild {
    pub key: NodeId,
    pub scope: Scope,
}

/// A scalar sequence generated in the enclosing scope context.
///
/// Sequence expressions live on a scope instead of in [`Node`] because graph
/// nodes produce one scalar value. Each generated value becomes the scope's
/// current scalar iteration frame, and `item` identifies the empty-path
/// [`Node::SourceField`] used by downstream graph expressions to read it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SequenceExpr {
    Tokenize {
        input: NodeId,
        delimiter: NodeId,
        item: NodeId,
    },
    TokenizeByLength {
        input: NodeId,
        length: NodeId,
        item: NodeId,
    },
    /// Generates the inclusive integer range `from..=to`. When `from` is
    /// omitted, MapForce's default lower boundary of 1 applies.
    Generate {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        from: Option<NodeId>,
        to: NodeId,
        item: NodeId,
    },
}

impl SequenceExpr {
    pub fn inputs(&self) -> Vec<NodeId> {
        match self {
            Self::Tokenize {
                input, delimiter, ..
            } => vec![*input, *delimiter],
            Self::TokenizeByLength { input, length, .. } => vec![*input, *length],
            Self::Generate { from, to, .. } => from.iter().copied().chain([*to]).collect(),
        }
    }

    pub fn item(&self) -> NodeId {
        match self {
            Self::Tokenize { item, .. }
            | Self::TokenizeByLength { item, .. }
            | Self::Generate { item, .. } => *item,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IterationOutput {
    /// Preserve every produced item as a repeating target value.
    #[default]
    Repeated,
    /// Produce the first surviving item as a non-repeating target group.
    First,
    /// Preserve mapping-produced XML occurrences independently of the
    /// target schema's declared repetition.
    MappedSequence,
}

/// Populates one target group.
///
/// [`ScopeIteration::Source`] follows a path from the parent scope's current
/// item, branching whenever it crosses repetition. A generated sequence or
/// inner join supplies items instead. [`ScopeIteration::None`] runs exactly
/// once and produces a non-repeating group.
///
/// Iterating scopes use [`Scope::iteration_output`] to retain every produced
/// group or return only the first surviving group. First-item output returns
/// an empty group when no item survives. Mapped-sequence output retains zero
/// or more XML element occurrences without changing schema cardinality.
/// Sorting, filtering, grouping, and `take` are applied before output
/// cardinality is selected.
///
/// If `filter` is set, it is evaluated in the same per-item context as
/// `bindings`; items for which it returns `false` are dropped. `sort_by`
/// stably orders candidates before filtering/grouping, and `take` limits the
/// number of produced items after filtering/grouping. These controls apply to
/// both source-backed and generated iteration.
#[derive(Debug, Clone, Default)]
pub struct Scope {
    /// Name of the field this scope populates in its parent scope; ignored
    /// for a project's root scope.
    pub target_field: String,
    /// Exactly one iteration form, or `None` for a non-iterating scope.
    pub iteration: ScopeIteration,
    pub filter: Option<NodeId>,
    /// Groups the iterated items by this key expression (evaluated once
    /// per item): the scope then produces one target group per distinct
    /// key, in first-seen order, and the iteration frame becomes the
    /// group's members -- so bindings read the first member, aggregates
    /// reduce the members, and nested scopes iterate them. Only meaningful
    /// for a source-backed or generated iteration.
    pub group_by: Option<NodeId>,
    /// Partitions items into contiguous groups whenever this per-item
    /// predicate is true. Items before its first true result form an initial
    /// group. Mutually exclusive with the other grouping modes.
    pub group_starting_with: Option<NodeId>,
    /// Partitions iterated items into contiguous groups of this many members.
    /// The expression is evaluated once in the parent context and must produce
    /// a positive item count. Mutually exclusive with the other grouping
    /// modes.
    pub group_into_blocks: Option<NodeId>,
    /// Sort key evaluated once per candidate item. Incomparable values keep
    /// their source order.
    pub sort_by: Option<NodeId>,
    pub sort_descending: bool,
    /// Expression evaluated once in the parent context to determine the
    /// maximum number of output items.
    pub take: Option<NodeId>,
    /// Cardinality of an iterating scope's target value. Older projects omit
    /// this field and retain the original repeated behavior.
    pub iteration_output: IterationOutput,
    pub bindings: Vec<Binding>,
    /// Computed scalar fields of an open target group.
    pub dynamic_bindings: Vec<DynamicBinding>,
    pub children: Vec<Scope>,
    /// Computed fields whose values are complete child scopes (objects or
    /// arrays). Kept separate from `children` so static and computed names
    /// cannot form an invalid mixed target descriptor.
    pub dynamic_children: Vec<DynamicChild>,
    /// An iterating scope normally produces an array. For an open object,
    /// each iteration may instead produce one property fragment; this flag
    /// merges those fragments into one object and rejects duplicate names.
    pub merge_dynamic_fields: bool,
}

impl Scope {
    pub fn source(&self) -> Option<&[String]> {
        self.iteration.source()
    }

    pub fn source_mut(&mut self) -> Option<&mut Vec<String>> {
        match &mut self.iteration {
            ScopeIteration::Source(path) => Some(path),
            ScopeIteration::None
            | ScopeIteration::Sequence(_)
            | ScopeIteration::InnerJoin { .. } => None,
        }
    }

    pub fn set_source(&mut self, source: Option<Vec<String>>) {
        match source {
            Some(path) => self.iteration = ScopeIteration::Source(path),
            None if matches!(self.iteration, ScopeIteration::Source(_)) => {
                self.iteration = ScopeIteration::None;
            }
            None => {}
        }
    }

    pub fn sequence(&self) -> Option<&SequenceExpr> {
        self.iteration.sequence()
    }

    pub fn sequence_mut(&mut self) -> Option<&mut SequenceExpr> {
        match &mut self.iteration {
            ScopeIteration::Sequence(sequence) => Some(sequence),
            ScopeIteration::None | ScopeIteration::Source(_) | ScopeIteration::InnerJoin { .. } => {
                None
            }
        }
    }

    pub fn set_sequence(&mut self, sequence: Option<SequenceExpr>) {
        match sequence {
            Some(sequence) => self.iteration = ScopeIteration::Sequence(sequence),
            None if matches!(self.iteration, ScopeIteration::Sequence(_)) => {
                self.iteration = ScopeIteration::None;
            }
            None => {}
        }
    }

    pub fn join(&self) -> Option<(JoinId, &JoinPlan)> {
        self.iteration.join()
    }

    pub fn iterates(&self) -> bool {
        self.iteration.iterates()
    }
}

fn is_repeated_output(output: &IterationOutput) -> bool {
    *output == IterationOutput::Repeated
}

/// A complete mapping project: the source/target shapes, the graph, and the
/// scope tree that maps one into the other.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub source: SchemaNode,
    pub target: SchemaNode,
    /// Default source/target instance files, resolved relative to the
    /// project file's directory -- carried over from imported designs and
    /// used to pick the component format on `.mfd` export. The CLI uses them
    /// as project-relative defaults; explicit input/output flags override.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_path: Option<String>,
    #[serde(default)]
    pub source_options: FormatOptions,
    #[serde(default)]
    pub target_options: FormatOptions,
    /// Secondary inputs (reference/lookup data) loaded alongside the
    /// primary source. Each becomes addressable by its `name` from any
    /// scope or field path via outward fallback.
    #[serde(default)]
    pub extra_sources: Vec<NamedSource>,
    pub graph: Graph,
    pub root: Scope,
}

/// A named secondary input. `path` is the instance file to load (format
/// picked by extension, exactly like the CLI's `--input`), resolved
/// relative to the project file's directory when not absolute.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NamedSource {
    pub name: String,
    pub path: String,
    pub schema: SchemaNode,
    #[serde(default)]
    pub options: FormatOptions,
}

/// Per-side format knobs. This is deliberately one flat bag of optional
/// settings rather than per-format sub-structs: each format adapter reads
/// only the fields that concern it, `mapping` stays free of format-crate
/// dependencies, and old project files load unchanged (everything
/// defaults).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FormatOptions {
    /// EDI: skip segments the schema doesn't mention instead of erroring
    /// on them. Skipping is bounded by the schema's own expectations, so
    /// declared segments are never swallowed.
    #[serde(default)]
    pub lenient_segments: bool,
    /// CSV: the field delimiter (default `,`).
    #[serde(default)]
    pub delimiter: Option<char>,
    /// CSV: whether the file's first row is a header (default true).
    #[serde(default)]
    pub has_header_row: Option<bool>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn join_plan() -> JoinPlan {
        let orders = JoinSource::new(vec!["orders".into()]);
        let products = JoinSource::new(vec!["products".into()]);
        let product_key = JoinKey::new(
            vec!["orders".into()],
            vec!["sku".into()],
            vec!["sku".into()],
        );
        JoinPlan::new(orders, products, JoinConditions::new(product_key)).unwrap()
    }

    #[test]
    fn old_scopes_default_dynamic_target_metadata_off() {
        let scope: Scope = serde_json::from_str(
            r#"{"target_field":"","source":null,"bindings":[],"children":[]}"#,
        )
        .unwrap();
        assert!(scope.dynamic_bindings.is_empty());
        assert!(scope.dynamic_children.is_empty());
        assert!(!scope.merge_dynamic_fields);
        assert_eq!(scope.iteration_output, IterationOutput::Repeated);
        assert!(scope.group_starting_with.is_none());
        assert!(!scope.iterates());
    }

    #[test]
    fn legacy_source_and_sequence_fields_select_typed_iteration() {
        let source: Scope = serde_json::from_str(
            r#"{"target_field":"","source":["items"],"bindings":[],"children":[]}"#,
        )
        .unwrap();
        assert_eq!(source.source(), Some(["items".to_string()].as_slice()));
        assert!(source.sequence().is_none());

        let sequence: Scope = serde_json::from_str(
            r#"{"target_field":"","source":null,"sequence":{"kind":"generate","to":2,"item":3},"bindings":[],"children":[]}"#,
        )
        .unwrap();
        assert!(matches!(
            sequence.sequence(),
            Some(SequenceExpr::Generate {
                from: None,
                to: 2,
                item: 3
            })
        ));

        let encoded = serde_json::to_string(&source).unwrap();
        assert!(encoded.contains(r#""source":["items"]"#));
        assert!(!encoded.contains(r#""iteration""#));
    }

    #[test]
    fn scope_deserialization_rejects_multiple_iteration_forms() {
        let source_and_sequence = serde_json::from_str::<Scope>(
            r#"{"source":["items"],"sequence":{"kind":"generate","to":2,"item":3}}"#,
        );
        assert!(
            source_and_sequence
                .unwrap_err()
                .to_string()
                .contains("mutually exclusive")
        );

        let join = serde_json::to_value(Scope {
            iteration: ScopeIteration::InnerJoin {
                id: JoinId::new(9),
                plan: join_plan(),
            },
            ..Scope::default()
        })
        .unwrap();
        let mut conflicting = join.as_object().cloned().unwrap();
        conflicting.insert("source".into(), serde_json::json!(["items"]));
        assert!(
            serde_json::from_value::<Scope>(serde_json::Value::Object(conflicting))
                .unwrap_err()
                .to_string()
                .contains("mutually exclusive")
        );
    }

    #[test]
    fn join_plan_enforces_ordered_distinct_sources() {
        let plan = join_plan()
            .then(
                JoinSource::new(vec!["inventory".into()]),
                JoinConditions::new(JoinKey::new(
                    vec!["products".into()],
                    vec!["id".into()],
                    vec!["product_id".into()],
                ))
                .and(JoinKey::new(
                    vec!["orders".into()],
                    vec!["region".into()],
                    vec!["region".into()],
                )),
            )
            .unwrap();
        let sources: Vec<_> = plan
            .sources()
            .map(|source| source.collection().join("/"))
            .collect();
        assert_eq!(sources, ["orders", "products", "inventory"]);
        assert_eq!(plan.stages().count(), 2);

        let duplicate = join_plan().then(
            JoinSource::new(vec!["orders".into()]),
            JoinConditions::new(JoinKey::new(
                vec!["products".into()],
                vec!["sku".into()],
                vec!["sku".into()],
            )),
        );
        assert!(matches!(
            duplicate,
            Err(JoinPlanError::DuplicateCollection(_))
        ));

        let unknown = JoinPlan::new(
            JoinSource::new(vec!["orders".into()]),
            JoinSource::new(vec!["products".into()]),
            JoinConditions::new(JoinKey::new(
                vec!["missing".into()],
                vec!["sku".into()],
                vec!["sku".into()],
            )),
        );
        assert!(matches!(
            unknown,
            Err(JoinPlanError::UnknownLeftCollection(_))
        ));
    }

    #[test]
    fn join_plan_deserialization_reapplies_constructor_invariants() {
        let join_scope = |second_collection: &str, left_collection: &str| {
            serde_json::json!({
                "join": {
                    "id": 1,
                    "plan": {
                        "first": { "collection": ["orders"] },
                        "second": {
                            "source": { "collection": [second_collection] },
                            "conditions": {
                                "first": {
                                    "left_collection": [left_collection],
                                    "left_path": ["sku"],
                                    "right_path": ["sku"]
                                }
                            }
                        }
                    }
                }
            })
        };

        let duplicate = serde_json::from_value::<Scope>(join_scope("orders", "orders"));
        assert!(
            duplicate
                .unwrap_err()
                .to_string()
                .contains("used more than once")
        );

        let unknown = serde_json::from_value::<Scope>(join_scope("products", "missing"));
        assert!(
            unknown
                .unwrap_err()
                .to_string()
                .contains("before it is joined")
        );
    }

    #[test]
    fn join_scope_and_owned_nodes_roundtrip() {
        let scope = Scope {
            iteration: ScopeIteration::InnerJoin {
                id: JoinId::new(44),
                plan: join_plan(),
            },
            ..Scope::default()
        };
        let encoded = serde_json::to_string(&scope).unwrap();
        assert!(encoded.contains(r#""join":{"id":44"#));
        let decoded: Scope = serde_json::from_str(&encoded).unwrap();
        let Some((id, plan)) = decoded.join() else {
            panic!("expected inner join");
        };
        assert_eq!(id.get(), 44);
        assert_eq!(plan.sources().count(), 2);

        for node in [
            Node::JoinField {
                join: id,
                collection: vec!["products".into()],
                path: vec!["name".into()],
            },
            Node::JoinPosition { join: id },
        ] {
            let encoded = serde_json::to_string(&node).unwrap();
            let decoded: Node = serde_json::from_str(&encoded).unwrap();
            assert!(matches!(
                decoded,
                Node::JoinField { join, .. } | Node::JoinPosition { join }
                    if join == JoinId::new(44)
            ));
        }
    }

    #[test]
    fn scope_iteration_helpers_replace_and_clear_only_their_form() {
        let mut scope = Scope::default();
        scope.set_source(Some(vec!["rows".into()]));
        scope.source_mut().unwrap().push("items".into());
        assert_eq!(
            scope.source(),
            Some(["rows".into(), "items".into()].as_slice())
        );

        let sequence = SequenceExpr::Generate {
            from: None,
            to: 7,
            item: 8,
        };
        scope.set_sequence(Some(sequence));
        scope.set_source(None);
        assert!(scope.sequence().is_some());
        scope.set_sequence(None);
        assert!(!scope.iterates());
    }

    #[test]
    fn group_starting_predicate_roundtrips() {
        let scope = Scope {
            iteration: ScopeIteration::Source(vec!["items".into()]),
            group_starting_with: Some(7),
            ..Scope::default()
        };
        let encoded = serde_json::to_string(&scope).unwrap();
        assert!(encoded.contains(r#""group_starting_with":7"#));
        let decoded: Scope = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded.group_starting_with, Some(7));
    }

    #[test]
    fn dynamic_target_metadata_roundtrips() {
        let scope = Scope {
            dynamic_bindings: vec![DynamicBinding { key: 1, value: 2 }],
            dynamic_children: vec![DynamicChild {
                key: 3,
                scope: Scope {
                    iteration: ScopeIteration::Source(vec!["items".into()]),
                    ..Scope::default()
                },
            }],
            merge_dynamic_fields: true,
            ..Scope::default()
        };
        let encoded = serde_json::to_string(&scope).unwrap();
        let decoded: Scope = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded.dynamic_bindings.len(), 1);
        assert_eq!(decoded.dynamic_children.len(), 1);
        assert!(decoded.merge_dynamic_fields);
    }

    #[test]
    fn first_item_iteration_output_roundtrips() {
        let scope = Scope {
            iteration: ScopeIteration::Source(vec!["items".into()]),
            iteration_output: IterationOutput::First,
            ..Scope::default()
        };
        let encoded = serde_json::to_string(&scope).unwrap();
        assert!(encoded.contains(r#""iteration_output":"first""#));
        let decoded: Scope = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded.iteration_output, IterationOutput::First);
    }

    #[test]
    fn mapped_sequence_iteration_output_roundtrips() {
        let scope = Scope {
            iteration: ScopeIteration::Source(vec!["items".into()]),
            iteration_output: IterationOutput::MappedSequence,
            ..Scope::default()
        };
        let encoded = serde_json::to_string(&scope).unwrap();
        assert!(encoded.contains(r#""iteration_output":"mapped_sequence""#));
        let decoded: Scope = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded.iteration_output, IterationOutput::MappedSequence);
    }

    #[test]
    fn sequence_exists_roundtrips() {
        let node = Node::SequenceExists {
            sequence: SequenceExpr::TokenizeByLength {
                input: 1,
                length: 2,
                item: 3,
            },
            predicate: 4,
        };
        let encoded = serde_json::to_string(&node).unwrap();
        let decoded: Node = serde_json::from_str(&encoded).unwrap();
        let Node::SequenceExists {
            sequence,
            predicate,
        } = decoded
        else {
            panic!("expected sequence-exists node");
        };
        assert!(matches!(
            sequence,
            SequenceExpr::TokenizeByLength {
                input: 1,
                length: 2,
                item: 3
            }
        ));
        assert_eq!(predicate, 4);
    }
}
