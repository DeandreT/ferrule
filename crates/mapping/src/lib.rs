//! The mapping graph IR: nodes and connections that describe how a source
//! instance is transformed into a target instance, plus the project file
//! (source schema + target schema + graph + scope tree) that gets
//! saved/loaded.

use std::collections::BTreeMap;

use ir::{SchemaNode, Value};
use serde::{Deserialize, Serialize};

fn is_false(value: &bool) -> bool {
    !*value
}

pub type NodeId = u32;

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
    /// A literal value.
    Const { value: Value },
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
#[derive(Debug, Clone, Serialize, Deserialize)]
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
}

/// Populates one target group.
///
/// If `source` is set, this scope iterates the source data found by following
/// that path from the parent scope's current item -- crossing a repeating
/// element branches, producing one target group per source item (a path may
/// cross several repeating levels at once). A `sequence` instead evaluates a
/// producer in the parent context and iterates its generated scalar items.
/// When both are absent, the scope runs exactly once and produces a
/// non-repeating group.
///
/// Iterating scopes use [`Scope::iteration_output`] to retain every produced
/// group or return only the first surviving group. First-item output returns
/// an empty group when no item survives. Sorting, filtering, grouping, and
/// `take` are applied before output cardinality is selected.
///
/// If `filter` is set, it is evaluated in the same per-item context as
/// `bindings`; items for which it returns `false` are dropped. `sort_by`
/// stably orders candidates before filtering/grouping, and `take` limits the
/// number of produced items after filtering/grouping. These controls apply to
/// both source-backed and generated iteration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Scope {
    /// Name of the field this scope populates in its parent scope; ignored
    /// for a project's root scope.
    #[serde(default)]
    pub target_field: String,
    #[serde(default)]
    pub source: Option<Vec<String>>,
    /// A generated scalar sequence to iterate instead of a source path.
    /// Absent in older project files; mutually exclusive with `source`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sequence: Option<SequenceExpr>,
    #[serde(default)]
    pub filter: Option<NodeId>,
    /// Groups the iterated items by this key expression (evaluated once
    /// per item): the scope then produces one target group per distinct
    /// key, in first-seen order, and the iteration frame becomes the
    /// group's members -- so bindings read the first member, aggregates
    /// reduce the members, and nested scopes iterate them. Only meaningful
    /// for a source-backed or generated iteration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group_by: Option<NodeId>,
    /// Partitions iterated items into contiguous groups of this many members.
    /// The expression is evaluated once in the parent context and must produce
    /// a positive item count. Mutually exclusive with [`Scope::group_by`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group_into_blocks: Option<NodeId>,
    /// Sort key evaluated once per candidate item. Incomparable values keep
    /// their source order.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sort_by: Option<NodeId>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub sort_descending: bool,
    /// Expression evaluated once in the parent context to determine the
    /// maximum number of output items.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub take: Option<NodeId>,
    /// Cardinality of an iterating scope's target value. Older projects omit
    /// this field and retain the original repeated behavior.
    #[serde(default, skip_serializing_if = "is_repeated_output")]
    pub iteration_output: IterationOutput,
    #[serde(default)]
    pub bindings: Vec<Binding>,
    /// Computed scalar fields of an open target group.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dynamic_bindings: Vec<DynamicBinding>,
    #[serde(default)]
    pub children: Vec<Scope>,
    /// Computed fields whose values are complete child scopes (objects or
    /// arrays). Kept separate from `children` so static and computed names
    /// cannot form an invalid mixed target descriptor.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dynamic_children: Vec<DynamicChild>,
    /// An iterating scope normally produces an array. For an open object,
    /// each iteration may instead produce one property fragment; this flag
    /// merges those fragments into one object and rejects duplicate names.
    #[serde(default, skip_serializing_if = "is_false")]
    pub merge_dynamic_fields: bool,
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
    }

    #[test]
    fn dynamic_target_metadata_roundtrips() {
        let scope = Scope {
            dynamic_bindings: vec![DynamicBinding { key: 1, value: 2 }],
            dynamic_children: vec![DynamicChild {
                key: 3,
                scope: Scope {
                    source: Some(vec!["items".into()]),
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
            source: Some(vec!["items".into()]),
            iteration_output: IterationOutput::First,
            ..Scope::default()
        };
        let encoded = serde_json::to_string(&scope).unwrap();
        assert!(encoded.contains(r#""iteration_output":"first""#));
        let decoded: Scope = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded.iteration_output, IterationOutput::First);
    }
}
