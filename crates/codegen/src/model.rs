use ir::{ScalarType, SchemaNode, Value};
use mapping::{AggregateOp, NodeId};

/// Collection reductions implemented identically by every generated backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AggregateFunction {
    Count,
    Sum,
    Avg,
    Min,
    Max,
    Join,
    ItemAt,
}

impl AggregateFunction {
    pub const ALL: &'static [Self] = &[
        Self::Count,
        Self::Sum,
        Self::Avg,
        Self::Min,
        Self::Max,
        Self::Join,
        Self::ItemAt,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Count => "count",
            Self::Sum => "sum",
            Self::Avg => "avg",
            Self::Min => "min",
            Self::Max => "max",
            Self::Join => "join",
            Self::ItemAt => "item_at",
        }
    }
}

impl From<AggregateOp> for AggregateFunction {
    fn from(function: AggregateOp) -> Self {
        match function {
            AggregateOp::Count => Self::Count,
            AggregateOp::Sum => Self::Sum,
            AggregateOp::Avg => Self::Avg,
            AggregateOp::Min => Self::Min,
            AggregateOp::Max => Self::Max,
            AggregateOp::Join => Self::Join,
            AggregateOp::ItemAt => Self::ItemAt,
        }
    }
}

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
        frame: Option<Vec<String>>,
        path: Vec<String>,
    },
    Position {
        collection: Vec<String>,
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
    /// Reduces a source collection. The value expression executes once per
    /// item, while `arg` executes once afterward in the parent context.
    Aggregate {
        function: AggregateFunction,
        collection: Vec<String>,
        value: AggregateValue,
        arg: Option<NodeId>,
    },
}

/// Exactly one way to obtain each aggregate item's scalar value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AggregateValue {
    /// Reads a scalar relative to the terminal collection item. An empty path
    /// selects scalar collection items directly.
    Path(Vec<String>),
    /// Evaluates a graph expression in each collection item's context.
    Expression(NodeId),
}

impl AggregateValue {
    pub const fn expression(&self) -> Option<NodeId> {
        match self {
            Self::Path(_) => None,
            Self::Expression(node) => Some(*node),
        }
    }
}

/// Cardinality retained by one iterating target scope.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum IterationOutput {
    #[default]
    Repeated,
    First,
    MappedSequence,
}

impl From<mapping::IterationOutput> for IterationOutput {
    fn from(output: mapping::IterationOutput) -> Self {
        match output {
            mapping::IterationOutput::Repeated => Self::Repeated,
            mapping::IterationOutput::First => Self::First,
            mapping::IterationOutput::MappedSequence => Self::MappedSequence,
        }
    }
}

/// Relative evaluation order for the ordinary per-item filter and sort.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SortFilterOrder {
    #[default]
    SortThenFilter,
    FilterThenSort,
}

impl From<mapping::SortFilterOrder> for SortFilterOrder {
    fn from(order: mapping::SortFilterOrder) -> Self {
        match order {
            mapping::SortFilterOrder::SortThenFilter => Self::SortThenFilter,
            mapping::SortFilterOrder::FilterThenSort => Self::FilterThenSort,
        }
    }
}

/// One per-item scalar key and its direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SortKey {
    pub expression: NodeId,
    pub descending: bool,
}

impl From<mapping::SortKey> for SortKey {
    fn from(key: mapping::SortKey) -> Self {
        Self {
            expression: key.node,
            descending: key.descending,
        }
    }
}

/// A nonempty stable sort plan. Keeping the primary key separate makes an
/// orphaned secondary key unrepresentable in backend-neutral programs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SortPlan {
    primary: SortKey,
    then: Vec<SortKey>,
    filter_order: SortFilterOrder,
}

impl SortPlan {
    pub fn new(primary: SortKey, then: Vec<SortKey>, filter_order: SortFilterOrder) -> Self {
        Self {
            primary,
            then,
            filter_order,
        }
    }

    pub fn keys(&self) -> impl Iterator<Item = SortKey> + '_ {
        std::iter::once(self.primary).chain(self.then.iter().copied())
    }

    pub const fn filter_order(&self) -> SortFilterOrder {
        self.filter_order
    }
}

/// One ordered sequence window whose bounds execute in the parent context.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SequenceWindow {
    SkipFirst { count: NodeId },
    First { count: NodeId },
    From { position: NodeId },
    FromTo { first: NodeId, last: NodeId },
    Last { count: NodeId },
}

impl SequenceWindow {
    pub fn nodes(self) -> impl Iterator<Item = NodeId> {
        let nodes = match self {
            Self::SkipFirst { count } | Self::First { count } | Self::Last { count } => {
                [Some(count), None]
            }
            Self::From { position } => [Some(position), None],
            Self::FromTo { first, last } => [Some(first), Some(last)],
        };
        nodes.into_iter().flatten()
    }
}

impl From<mapping::SequenceWindow> for SequenceWindow {
    fn from(window: mapping::SequenceWindow) -> Self {
        match window {
            mapping::SequenceWindow::SkipFirst { count } => Self::SkipFirst { count },
            mapping::SequenceWindow::First { count } => Self::First { count },
            mapping::SequenceWindow::From { position } => Self::From { position },
            mapping::SequenceWindow::FromTo { first, last } => Self::FromTo { first, last },
            mapping::SequenceWindow::Last { count } => Self::Last { count },
        }
    }
}

/// One statically named constructed target group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetScope {
    /// Empty only for the primary target's root scope.
    pub target_field: String,
    /// Non-iterating scopes targeting a repeating group still produce one
    /// repeated item, matching the engine's target-boundary cardinality.
    pub repeating: bool,
    /// Source-backed iteration evaluated relative to the parent scope's
    /// current item. Absence means the scope runs exactly once.
    pub iteration: Option<IterationPlan>,
    /// Declaration order is semantically significant and is preserved.
    pub bindings: Vec<Binding>,
    pub children: Vec<TargetScope>,
}

/// One source-backed candidate pipeline. Controls live inside the iteration,
/// so a filter, sort, or window cannot exist on a non-iterating scope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IterationPlan {
    source: SourceIteration,
    filter: Option<NodeId>,
    sort: Option<SortPlan>,
    windows: Vec<SequenceWindow>,
    output: IterationOutput,
}

impl IterationPlan {
    pub fn source(path: Vec<String>) -> Self {
        Self::new(
            SourceIteration::new(path),
            None,
            None,
            Vec::new(),
            IterationOutput::Repeated,
        )
    }

    pub fn new(
        source: SourceIteration,
        filter: Option<NodeId>,
        sort: Option<SortPlan>,
        windows: Vec<SequenceWindow>,
        output: IterationOutput,
    ) -> Self {
        Self {
            source,
            filter,
            sort,
            windows,
            output,
        }
    }

    pub const fn source_iteration(&self) -> &SourceIteration {
        &self.source
    }

    pub const fn filter(&self) -> Option<NodeId> {
        self.filter
    }

    pub const fn sort(&self) -> Option<&SortPlan> {
        self.sort.as_ref()
    }

    pub fn windows(&self) -> &[SequenceWindow] {
        &self.windows
    }

    pub const fn output(&self) -> IterationOutput {
        self.output
    }

    pub fn roots(&self) -> impl Iterator<Item = NodeId> + '_ {
        self.filter
            .into_iter()
            .chain(
                self.sort
                    .iter()
                    .flat_map(SortPlan::keys)
                    .map(|key| key.expression),
            )
            .chain(self.windows.iter().copied().flat_map(SequenceWindow::nodes))
    }
}

/// One source path that drives a target scope's repeated output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceIteration {
    path: Vec<String>,
}

impl SourceIteration {
    pub fn new(path: Vec<String>) -> Self {
        Self { path }
    }

    pub fn path(&self) -> &[String] {
        &self.path
    }
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
