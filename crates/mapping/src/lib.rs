//! The mapping graph IR: nodes and connections that describe how a source
//! instance is transformed into a target instance, plus the project file
//! (source schema + target schema + graph + scope tree) that gets
//! saved/loaded.

use std::collections::BTreeMap;

use ir::{SchemaNode, Value};
use serde::{Deserialize, Serialize};

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
    SourceField { path: Vec<String> },
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

/// Populates one target group.
///
/// If `source` is set, this scope iterates the source data found by
/// following that path from the parent scope's current item -- crossing a
/// repeating element branches, producing one target group per source item
/// (a path may cross several repeating levels at once, e.g. `["Order",
/// "Items", "Item"]`, which flattens nested repetition into a single
/// target-side repetition). If `source` is `None`, the scope runs exactly
/// once, producing a single (non-repeating) target group instance.
///
/// If `filter` is set, it's evaluated (in the same per-item context as
/// `bindings`) for each candidate item and must return a `bool`; items for
/// which it's `false` are dropped, producing fewer target items than source
/// items. Only meaningful when `source` is set.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Scope {
    /// Name of the field this scope populates in its parent scope; ignored
    /// for a project's root scope.
    #[serde(default)]
    pub target_field: String,
    #[serde(default)]
    pub source: Option<Vec<String>>,
    #[serde(default)]
    pub filter: Option<NodeId>,
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
    /// used to pick the component format on `.mfd` export. `run` ignores
    /// them; the CLI takes explicit paths.
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
