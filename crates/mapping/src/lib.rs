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
}

impl SequenceExpr {
    pub fn inputs(&self) -> [NodeId; 2] {
        match self {
            Self::Tokenize {
                input, delimiter, ..
            } => [*input, *delimiter],
            Self::TokenizeByLength { input, length, .. } => [*input, *length],
        }
    }

    pub fn item(&self) -> NodeId {
        match self {
            Self::Tokenize { item, .. } | Self::TokenizeByLength { item, .. } => *item,
        }
    }
}

/// Populates one target group.
///
/// If `source` is set, this scope iterates the source data found by
/// following that path from the parent scope's current item -- crossing a
/// repeating element branches, producing one target group per source item
/// (a path may cross several repeating levels at once, e.g. `["Order",
/// "Items", "Item"]`, which flattens nested repetition into a single
/// target-side repetition). A `sequence` instead evaluates a producer in the
/// parent context and iterates its generated scalar items. When both are
/// absent, the scope runs exactly once and produces a non-repeating group.
///
/// If `sort_by` is set, candidate items are stably sorted by that expression
/// before filtering/grouping. If `filter` is set, it's evaluated (in the same per-item context as
/// `bindings`) for each candidate item and must return a `bool`; items for
/// which it's `false` are dropped, producing fewer target items than source
/// items. `take` limits the number of produced items after filtering/grouping.
/// These controls apply to both source-backed and generated iteration.
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
    #[serde(default)]
    pub bindings: Vec<Binding>,
    #[serde(default)]
    pub children: Vec<Scope>,
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
