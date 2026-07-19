use ir::{ScalarType, SchemaNode, Value};
use mapping::NodeId;

/// Deterministic backend-neutral representation of one supported mapping.
#[derive(Debug, Clone, PartialEq)]
pub struct Program {
    pub source: SchemaNode,
    pub target: SchemaNode,
    /// Reachable expressions ordered by node ID.
    pub expressions: Vec<ExpressionNode>,
    pub root: TargetScope,
}

/// One graph expression retained with its project identity for diagnostics.
#[derive(Debug, Clone, PartialEq)]
pub struct ExpressionNode {
    pub id: NodeId,
    pub expression: Expression,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Expression {
    SourceField { path: Vec<String> },
    Const { value: Value },
}

/// One statically named constructed target group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetScope {
    /// Empty only for the primary target's root scope.
    pub target_field: String,
    /// Non-iterating scopes targeting a repeating group still produce one
    /// repeated item, matching the engine's target-boundary cardinality.
    pub repeating: bool,
    /// Declaration order is semantically significant and is preserved.
    pub bindings: Vec<Binding>,
    pub children: Vec<TargetScope>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Binding {
    pub target_field: String,
    pub expression: NodeId,
    /// Scalar coercion applied by the engine at this target boundary.
    pub target_type: ScalarType,
    /// Repeating scalars map Null to no items and other values to one item.
    pub repeating: bool,
}
