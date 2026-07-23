use std::collections::BTreeMap;

use ir::{ScalarType, Value};
use serde::{Deserialize, Serialize};

use crate::{FunctionId, FunctionParameterId, JoinId, JoinPlan, Scope};

pub type NodeId = u32;

const fn default_xml_indent() -> bool {
    true
}

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
    /// Reads the resolved location retained by the nearest active source
    /// document. This is boundary metadata, not a schema field.
    SourceDocumentPath,
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
    /// Editor-owned `Null` value for a required input that has no wire.
    /// Canvas renderers hide this node so an empty pin remains visually empty.
    Unconnected,
    /// A literal value.
    Const { value: Value },
    /// Reads one input from the isolated context of a user-defined function.
    FunctionParameter { parameter: FunctionParameterId },
    /// Reads a value supplied explicitly by the execution host.
    RuntimeValue { value: RuntimeValue },
    /// Calls a built-in function (see the `functions` crate) with the
    /// evaluated outputs of the given argument nodes.
    Call { function: String, args: Vec<NodeId> },
    /// Invokes a reusable project-local scalar mapping. Arguments correspond
    /// positionally to the function's ordered parameters.
    UserFunctionCall {
        function: FunctionId,
        args: Vec<NodeId>,
    },
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
        /// Scalar type MapForce applies to the input before matching. Native
        /// ferrule maps leave this unset and compare the input as-is.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        input_type: Option<ScalarType>,
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
    /// Reads one runtime-named scalar field from an open source object.
    /// `frame` pins colliding nested repetitions with the same semantics as
    /// [`Node::SourceField`], while `key` computes the property name.
    DynamicSourceField {
        object: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        frame: Option<Vec<String>>,
        key: NodeId,
    },
    /// Atomizes one XML mixed-content group while replacing selected direct
    /// child occurrences with graph-computed strings. XML readers retain the
    /// source node order as private instance metadata; `path` and `frame`
    /// use the same resolution rules as [`Node::SourceField`].
    XmlMixedContent {
        path: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        frame: Option<Vec<String>>,
        replacements: Vec<XmlMixedContentReplacement>,
    },
    /// Serializes one complete XML source element as a string. Unlike a
    /// scalar `SourceField`, this retains the structured instance so nested
    /// elements, attributes, and repetitions reach the XML writer intact.
    XmlSerialize {
        path: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        frame: Option<Vec<String>>,
        schema: ir::SchemaNode,
        #[serde(default)]
        declaration: bool,
        #[serde(default = "default_xml_indent")]
        indent: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        namespace: Option<String>,
    },
    /// Scans `collection` in source order, evaluating `predicate` and
    /// `value` once per item. Returns the value for the first item whose
    /// predicate is true, or `Null` when no item matches.
    CollectionFind {
        collection: Vec<String>,
        predicate: NodeId,
        value: NodeId,
    },
    /// Returns whether any item produced by `sequence` satisfies `predicate`.
    /// The sequence's item node is an owned empty-path `SourceField` that is
    /// available only while evaluating the predicate.
    SequenceExists {
        sequence: SequenceExpr,
        predicate: NodeId,
    },
    /// Selects one 1-based scalar from a generated sequence. The sequence's
    /// item node is owned by this expression, while `index` is evaluated in
    /// the enclosing context rather than once per generated item.
    SequenceItemAt {
        sequence: SequenceExpr,
        index: NodeId,
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
    /// Reduces the tuples produced by a naked inner join. The plan is
    /// evaluated in the aggregate's parent context. A source collection
    /// already active there contributes only its current item; other sources
    /// enumerate normally. `expression`, when set, is evaluated once per
    /// joined tuple with that join's fields and position active. `arg` remains
    /// a parent-context expression.
    JoinAggregate {
        function: AggregateOp,
        join: JoinId,
        plan: JoinPlan,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        expression: Option<NodeId>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        arg: Option<NodeId>,
    },
}

impl Node {
    /// Graph nodes whose outputs must be available to evaluate this node.
    /// The order follows expression evaluation where that order is meaningful.
    pub fn dependencies(&self) -> Vec<NodeId> {
        match self {
            Self::SourceField { .. }
            | Self::SourceDocumentPath
            | Self::Position { .. }
            | Self::JoinField { .. }
            | Self::JoinPosition { .. }
            | Self::Unconnected
            | Self::Const { .. }
            | Self::FunctionParameter { .. }
            | Self::RuntimeValue { .. }
            | Self::XmlSerialize { .. } => Vec::new(),
            Self::Call { args, .. } | Self::UserFunctionCall { args, .. } => args.clone(),
            Self::If {
                condition,
                then,
                else_,
            } => vec![*condition, *then, *else_],
            Self::ValueMap { input, .. } => vec![*input],
            Self::Lookup { matches, .. } => vec![*matches],
            Self::DynamicSourceField { key, .. } => vec![*key],
            Self::XmlMixedContent { replacements, .. } => replacements
                .iter()
                .map(|replacement| replacement.expression)
                .collect(),
            Self::CollectionFind {
                predicate, value, ..
            } => vec![*predicate, *value],
            Self::SequenceExists {
                sequence,
                predicate,
            } => sequence
                .inputs()
                .into_iter()
                .chain([sequence.item(), *predicate])
                .collect(),
            Self::SequenceItemAt { sequence, index } => sequence
                .inputs()
                .into_iter()
                .chain([sequence.item(), *index])
                .collect(),
            Self::Aggregate {
                expression, arg, ..
            }
            | Self::JoinAggregate {
                expression, arg, ..
            } => expression.iter().chain(arg).copied().collect(),
        }
    }
}

/// One direct-element replacement in [`Node::XmlMixedContent`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct XmlMixedContentReplacement {
    pub element: String,
    /// Repeating source collection represented by each matching occurrence.
    /// Empty means the expression remains in the mixed group's parent frame.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub collection: Vec<String>,
    pub expression: NodeId,
}

/// A reduction applied by [`Node::Aggregate`] or [`Node::JoinAggregate`].
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
/// Each generated value can become a scope's current scalar iteration frame
/// or belong to a scalar reducer node. In either case, `item` identifies the
/// uniquely owned empty-path [`Node::SourceField`] used by expressions in
/// that generated-item context.
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
    /// Splits a string around regular-expression matches. `flags` is absent
    /// when MapForce's optional third input is disconnected.
    TokenizeRegex {
        input: NodeId,
        pattern: NodeId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        flags: Option<NodeId>,
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
    /// Walks a recursive group depth-first and collects scalar leaves while
    /// carrying an accumulated prefix between parent and child groups.
    RecursiveCollect {
        collection: Vec<String>,
        children: Vec<String>,
        descent_value: Vec<String>,
        values: Vec<String>,
        value: Vec<String>,
        prefix: NodeId,
        separator: NodeId,
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
            Self::TokenizeRegex {
                input,
                pattern,
                flags,
                ..
            } => [Some(*input), Some(*pattern), *flags]
                .into_iter()
                .flatten()
                .collect(),
            Self::Generate { from, to, .. } => from.iter().copied().chain([*to]).collect(),
            Self::RecursiveCollect {
                prefix, separator, ..
            } => vec![*prefix, *separator],
        }
    }

    pub fn item(&self) -> NodeId {
        match self {
            Self::Tokenize { item, .. }
            | Self::TokenizeByLength { item, .. }
            | Self::TokenizeRegex { item, .. }
            | Self::Generate { item, .. }
            | Self::RecursiveCollect { item, .. } => *item,
        }
    }
}
