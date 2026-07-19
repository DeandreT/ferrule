use ir::{ScalarType, SchemaNode, Value};
use mapping::NodeId;

/// Scalar calls that every code-generation backend must implement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ScalarFunction {
    And,
    Or,
    Not,
    Exists,
    IsEmpty,
    StartsWith,
    Contains,
    Add,
    Subtract,
    Multiply,
    Divide,
    Equal,
    NotEqual,
    LessThan,
    GreaterThan,
    LessOrEqual,
    GreaterOrEqual,
}

impl ScalarFunction {
    pub const ALL: &'static [Self] = &[
        Self::And,
        Self::Or,
        Self::Not,
        Self::Exists,
        Self::IsEmpty,
        Self::StartsWith,
        Self::Contains,
        Self::Add,
        Self::Subtract,
        Self::Multiply,
        Self::Divide,
        Self::Equal,
        Self::NotEqual,
        Self::LessThan,
        Self::GreaterThan,
        Self::LessOrEqual,
        Self::GreaterOrEqual,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::And => "and",
            Self::Or => "or",
            Self::Not => "not",
            Self::Exists => "exists",
            Self::IsEmpty => "is_empty",
            Self::StartsWith => "starts_with",
            Self::Contains => "contains",
            Self::Add => "add",
            Self::Subtract => "subtract",
            Self::Multiply => "multiply",
            Self::Divide => "divide",
            Self::Equal => "equal",
            Self::NotEqual => "not_equal",
            Self::LessThan => "less_than",
            Self::GreaterThan => "greater_than",
            Self::LessOrEqual => "less_or_equal",
            Self::GreaterOrEqual => "greater_or_equal",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "and" => Some(Self::And),
            "or" => Some(Self::Or),
            "not" => Some(Self::Not),
            "exists" => Some(Self::Exists),
            "is_empty" => Some(Self::IsEmpty),
            "starts_with" => Some(Self::StartsWith),
            "contains" => Some(Self::Contains),
            "add" => Some(Self::Add),
            "subtract" => Some(Self::Subtract),
            "multiply" => Some(Self::Multiply),
            "divide" => Some(Self::Divide),
            "equal" => Some(Self::Equal),
            "not_equal" => Some(Self::NotEqual),
            "less_than" => Some(Self::LessThan),
            "greater_than" => Some(Self::GreaterThan),
            "less_or_equal" => Some(Self::LessOrEqual),
            "greater_or_equal" => Some(Self::GreaterOrEqual),
            _ => None,
        }
    }
}

/// Closed scalar-function whitelist accepted by shared lowering.
pub const SUPPORTED_SCALAR_CALLS: &[ScalarFunction] = ScalarFunction::ALL;

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
    SourceField {
        path: Vec<String>,
    },
    Const {
        value: Value,
    },
    Call {
        function: ScalarFunction,
        args: Vec<NodeId>,
    },
    /// Conditional evaluation. Backends must evaluate only the selected
    /// branch after the condition has produced a boolean value.
    If {
        condition: NodeId,
        then: NodeId,
        else_: NodeId,
    },
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
