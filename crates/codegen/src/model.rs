use ir::{ScalarType, SchemaNode, Value};
use mapping::{AggregateOp, FunctionId, FunctionParameterId, NodeId};

use crate::{InnerJoin, JoinId};

/// Host-supplied values available to generated mappings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RuntimeValue {
    MappingFilePath,
    MainMappingFilePath,
    CurrentDateTime,
}

impl From<mapping::RuntimeValue> for RuntimeValue {
    fn from(value: mapping::RuntimeValue) -> Self {
        match value {
            mapping::RuntimeValue::MappingFilePath => Self::MappingFilePath,
            mapping::RuntimeValue::MainMappingFilePath => Self::MainMappingFilePath,
            mapping::RuntimeValue::CurrentDateTime => Self::CurrentDateTime,
        }
    }
}

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
    EndsWith,
    Contains,
    Matches,
    Replace,
    Concat,
    Upper,
    Lower,
    NormalizeSpace,
    Trim,
    Left,
    Right,
    LeftTrim,
    RightTrim,
    Length,
    SubstringBefore,
    SubstringAfter,
    String,
    IsNumeric,
    ToNumber,
    FormatNumber,
    SubstituteMissing,
    SubstituteMissingWithXmlNil,
    IsXmlNil,
    GetFolder,
    RemoveFolder,
    GetFileext,
    ResolveFilepath,
    Substring,
    SqlLike,
    PadStringLeft,
    PadStringRight,
    Isbn10ToIsbn13,
    Round,
    DelayPassthrough,
    DateFromDatetime,
    YearFromDatetime,
    MonthFromDatetime,
    DayFromDatetime,
    Weekday,
    HoursFromDatetime,
    MinutesFromDatetime,
    TimeFromDatetime,
    DatetimeFromDateAndTime,
    DatetimeFromParts,
    CoerceDatetime,
    ParseDate,
    ParseDatetime,
    ParseTime,
    DatetimeAdd,
    EdifactToDatetime,
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
        Self::EndsWith,
        Self::Contains,
        Self::Matches,
        Self::Replace,
        Self::Concat,
        Self::Upper,
        Self::Lower,
        Self::NormalizeSpace,
        Self::Trim,
        Self::Left,
        Self::Right,
        Self::LeftTrim,
        Self::RightTrim,
        Self::Length,
        Self::SubstringBefore,
        Self::SubstringAfter,
        Self::String,
        Self::IsNumeric,
        Self::ToNumber,
        Self::FormatNumber,
        Self::SubstituteMissing,
        Self::SubstituteMissingWithXmlNil,
        Self::IsXmlNil,
        Self::GetFolder,
        Self::RemoveFolder,
        Self::GetFileext,
        Self::ResolveFilepath,
        Self::Substring,
        Self::SqlLike,
        Self::PadStringLeft,
        Self::PadStringRight,
        Self::Isbn10ToIsbn13,
        Self::Round,
        Self::DelayPassthrough,
        Self::DateFromDatetime,
        Self::YearFromDatetime,
        Self::MonthFromDatetime,
        Self::DayFromDatetime,
        Self::Weekday,
        Self::HoursFromDatetime,
        Self::MinutesFromDatetime,
        Self::TimeFromDatetime,
        Self::DatetimeFromDateAndTime,
        Self::DatetimeFromParts,
        Self::CoerceDatetime,
        Self::ParseDate,
        Self::ParseDatetime,
        Self::ParseTime,
        Self::DatetimeAdd,
        Self::EdifactToDatetime,
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
            Self::EndsWith => "ends_with",
            Self::Contains => "contains",
            Self::Matches => "matches",
            Self::Replace => "replace",
            Self::Concat => "concat",
            Self::Upper => "upper",
            Self::Lower => "lower",
            Self::NormalizeSpace => "normalize_space",
            Self::Trim => "trim",
            Self::Left => "left",
            Self::Right => "right",
            Self::LeftTrim => "left_trim",
            Self::RightTrim => "right_trim",
            Self::Length => "length",
            Self::SubstringBefore => "substring_before",
            Self::SubstringAfter => "substring_after",
            Self::String => "string",
            Self::IsNumeric => "is_numeric",
            Self::ToNumber => "to_number",
            Self::FormatNumber => "format_number",
            Self::SubstituteMissing => "substitute_missing",
            Self::SubstituteMissingWithXmlNil => "substitute_missing_with_xml_nil",
            Self::IsXmlNil => "is_xml_nil",
            Self::GetFolder => "get_folder",
            Self::RemoveFolder => "remove_folder",
            Self::GetFileext => "get_fileext",
            Self::ResolveFilepath => "resolve_filepath",
            Self::Substring => "substring",
            Self::SqlLike => "sql_like",
            Self::PadStringLeft => "pad_string_left",
            Self::PadStringRight => "pad_string_right",
            Self::Isbn10ToIsbn13 => "isbn10_to_isbn13",
            Self::Round => "round",
            Self::DelayPassthrough => "delay_passthrough",
            Self::DateFromDatetime => "date_from_datetime",
            Self::YearFromDatetime => "year_from_datetime",
            Self::MonthFromDatetime => "month_from_datetime",
            Self::DayFromDatetime => "day_from_datetime",
            Self::Weekday => "weekday",
            Self::HoursFromDatetime => "hours_from_datetime",
            Self::MinutesFromDatetime => "minutes_from_datetime",
            Self::TimeFromDatetime => "time_from_datetime",
            Self::DatetimeFromDateAndTime => "datetime_from_date_and_time",
            Self::DatetimeFromParts => "datetime_from_parts",
            Self::CoerceDatetime => "coerce_datetime",
            Self::ParseDate => "parse_date",
            Self::ParseDatetime => "parse_datetime",
            Self::ParseTime => "parse_time",
            Self::DatetimeAdd => "datetime_add",
            Self::EdifactToDatetime => "edifact_to_datetime",
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
            "ends_with" => Some(Self::EndsWith),
            "contains" => Some(Self::Contains),
            "matches" => Some(Self::Matches),
            "replace" => Some(Self::Replace),
            "concat" => Some(Self::Concat),
            "upper" => Some(Self::Upper),
            "lower" => Some(Self::Lower),
            "normalize_space" => Some(Self::NormalizeSpace),
            "trim" => Some(Self::Trim),
            "left" => Some(Self::Left),
            "right" => Some(Self::Right),
            "left_trim" => Some(Self::LeftTrim),
            "right_trim" => Some(Self::RightTrim),
            "length" => Some(Self::Length),
            "substring_before" => Some(Self::SubstringBefore),
            "substring_after" => Some(Self::SubstringAfter),
            "string" => Some(Self::String),
            "is_numeric" => Some(Self::IsNumeric),
            "to_number" => Some(Self::ToNumber),
            "format_number" => Some(Self::FormatNumber),
            "substitute_missing" => Some(Self::SubstituteMissing),
            "substitute_missing_with_xml_nil" => Some(Self::SubstituteMissingWithXmlNil),
            "is_xml_nil" => Some(Self::IsXmlNil),
            "get_folder" => Some(Self::GetFolder),
            "remove_folder" => Some(Self::RemoveFolder),
            "get_fileext" => Some(Self::GetFileext),
            "resolve_filepath" => Some(Self::ResolveFilepath),
            "substring" => Some(Self::Substring),
            "sql_like" => Some(Self::SqlLike),
            "pad_string_left" => Some(Self::PadStringLeft),
            "pad_string_right" => Some(Self::PadStringRight),
            "isbn10_to_isbn13" => Some(Self::Isbn10ToIsbn13),
            "round" => Some(Self::Round),
            "delay_passthrough" => Some(Self::DelayPassthrough),
            "date_from_datetime" => Some(Self::DateFromDatetime),
            "year_from_datetime" => Some(Self::YearFromDatetime),
            "month_from_datetime" => Some(Self::MonthFromDatetime),
            "day_from_datetime" => Some(Self::DayFromDatetime),
            "weekday" => Some(Self::Weekday),
            "hours_from_datetime" => Some(Self::HoursFromDatetime),
            "minutes_from_datetime" => Some(Self::MinutesFromDatetime),
            "time_from_datetime" => Some(Self::TimeFromDatetime),
            "datetime_from_date_and_time" => Some(Self::DatetimeFromDateAndTime),
            "datetime_from_parts" => Some(Self::DatetimeFromParts),
            "coerce_datetime" => Some(Self::CoerceDatetime),
            "parse_date" => Some(Self::ParseDate),
            "parse_datetime" => Some(Self::ParseDatetime),
            "parse_time" => Some(Self::ParseTime),
            "datetime_add" => Some(Self::DatetimeAdd),
            "edifact_to_datetime" => Some(Self::EdifactToDatetime),
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
    /// Additional typed inputs available through outward source fallback.
    pub extra_sources: Vec<NamedSourceProgram>,
    pub target: SchemaNode,
    /// Reachable expressions ordered by node ID.
    pub expressions: Vec<ExpressionNode>,
    /// Reachable user functions ordered with callees before callers.
    pub user_functions: Vec<UserFunctionProgram>,
    /// Ordered pre-target failures evaluated against the shared source frames.
    pub failure_rules: Vec<FailureRule>,
    pub root: TargetScope,
    /// Additional independently shaped outputs in declaration order.
    pub extra_targets: Vec<NamedTargetProgram>,
}

/// One isolated scalar user function retained for deterministic helper emission.
#[derive(Debug, Clone, PartialEq)]
pub struct UserFunctionProgram {
    pub id: FunctionId,
    pub library: String,
    pub name: String,
    pub parameters: Vec<UserFunctionParameter>,
    pub output_type: ScalarType,
    pub expressions: Vec<ExpressionNode>,
    pub output: NodeId,
}

/// One ordered, typed input to a generated user function.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserFunctionParameter {
    pub id: FunctionParameterId,
    pub ty: ScalarType,
}

/// One ordered pre-target failure evaluated until its first selected item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FailureRule {
    pub iteration: FailureIteration,
    pub selection: FailureSelection,
    /// Absence remains distinct from an expression that evaluates to empty text.
    pub message: Option<NodeId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FailureIteration {
    Source(SourceIteration),
    Generated(GeneratedSequence),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureSelection {
    All,
    WhenTrue(NodeId),
    WhenFalse(NodeId),
}

impl FailureSelection {
    pub const fn predicate(self) -> Option<NodeId> {
        match self {
            Self::All => None,
            Self::WhenTrue(predicate) | Self::WhenFalse(predicate) => Some(predicate),
        }
    }
}

/// One named in-memory input consumed alongside the primary source.
#[derive(Debug, Clone, PartialEq)]
pub struct NamedSourceProgram {
    pub name: String,
    pub source: SchemaNode,
}

/// One named output lowered against the program's shared source and graph.
#[derive(Debug, Clone, PartialEq)]
pub struct NamedTargetProgram {
    pub name: String,
    pub target: SchemaNode,
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
    /// Serializes one complete structured XML source element with its exact
    /// static schema and document-level formatting policy.
    XmlSerialize {
        frame: Option<Vec<String>>,
        path: Vec<String>,
        schema: SchemaNode,
        declaration: bool,
        indent: bool,
        namespace: Option<String>,
    },
    /// Atomizes one retained ordered XML content stream, replacing selected
    /// direct element occurrences in their exact source-item contexts.
    XmlMixedContent {
        frame: Option<Vec<String>>,
        path: Vec<String>,
        replacements: Vec<XmlMixedContentReplacement>,
    },
    /// Reads the resolved path retained by the nearest source document.
    SourceDocumentPath,
    Position {
        collection: Vec<String>,
    },
    /// Reads one exact scalar from a source tuple owned by `join`.
    JoinField {
        join: JoinId,
        collection: Vec<String>,
        path: Vec<String>,
    },
    /// Returns the compacted one-based output position of an active join.
    JoinPosition {
        join: JoinId,
    },
    Const {
        value: Value,
    },
    FunctionParameter {
        parameter: FunctionParameterId,
    },
    RuntimeValue {
        value: RuntimeValue,
    },
    Call {
        function: ScalarFunction,
        args: Vec<NodeId>,
    },
    UserFunctionCall {
        function: FunctionId,
        args: Vec<NodeId>,
    },
    /// Conditional evaluation. Backends must evaluate only the selected
    /// branch after the condition has produced a boolean value.
    If {
        condition: NodeId,
        then: NodeId,
        else_: NodeId,
    },
    /// Applies optional scalar coercion, then selects the first exactly
    /// matching row. A failed coercion retains the original input value.
    ValueMap {
        input: NodeId,
        input_type: Option<ScalarType>,
        table: Vec<(Value, Value)>,
        default: Option<Value>,
    },
    /// Scans one exact repeating collection in source order. The first item
    /// whose scalar key equals `matches` contributes its scalar value; a miss
    /// or missing value produces Null.
    Lookup {
        collection: Vec<String>,
        key: Vec<String>,
        matches: NodeId,
        value: Vec<String>,
    },
    /// Flattens one source path in source order and returns the value from
    /// the first item whose predicate is true. Predicate and value expressions
    /// execute in the selected item's repeated-frame context.
    CollectionFind {
        collection: Vec<String>,
        predicate: NodeId,
        value: NodeId,
    },
    /// Reduces a source collection. The value expression executes once per
    /// item, while `arg` executes once afterward in the parent context.
    Aggregate {
        function: AggregateFunction,
        collection: Vec<String>,
        value: AggregateValue,
        arg: Option<NodeId>,
    },
    /// Reduces the tuples produced by a locally owned static inner join.
    /// The value expression executes once per tuple with the join active;
    /// `arg` executes once afterward in the unchanged parent context.
    JoinAggregate {
        function: AggregateFunction,
        join: InnerJoin,
        expression: Option<NodeId>,
        arg: Option<NodeId>,
    },
    /// Generates a private scalar sequence and returns whether its predicate
    /// is true for any item. The predicate runs in a one-based item context
    /// and short-circuits after the first match.
    SequenceExists {
        sequence: GeneratedSequence,
        predicate: NodeId,
    },
    /// Generates a private scalar sequence and selects one one-based item.
    /// The index executes after sequence materialization in the parent
    /// context.
    SequenceItemAt {
        sequence: GeneratedSequence,
        index: NodeId,
    },
}

/// One direct-element replacement in an ordered XML mixed-content stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XmlMixedContentReplacement {
    pub element: String,
    /// Repeating source collection represented by matching occurrences.
    /// Empty leaves the expression in the mixed group's parent context.
    pub collection: Vec<String>,
    pub expression: NodeId,
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

/// One scalar sequence evaluated in its owner's parent context. `item` owns
/// the unframed empty-path source-field expression that becomes visible only
/// while a scope candidate or existential predicate is evaluated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GeneratedSequence {
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
    TokenizeRegex {
        input: NodeId,
        pattern: NodeId,
        flags: Option<NodeId>,
        item: NodeId,
    },
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
    Range {
        from: Option<NodeId>,
        to: NodeId,
        item: NodeId,
    },
}

impl GeneratedSequence {
    pub const fn item(&self) -> NodeId {
        match self {
            Self::Tokenize { item, .. }
            | Self::TokenizeByLength { item, .. }
            | Self::TokenizeRegex { item, .. }
            | Self::RecursiveCollect { item, .. }
            | Self::Range { item, .. } => *item,
        }
    }

    pub fn inputs(&self) -> impl Iterator<Item = NodeId> + '_ {
        let inputs = match self {
            Self::Tokenize {
                input, delimiter, ..
            } => [Some(*input), Some(*delimiter), None],
            Self::TokenizeByLength { input, length, .. } => [Some(*input), Some(*length), None],
            Self::TokenizeRegex {
                input,
                pattern,
                flags,
                ..
            } => [Some(*input), Some(*pattern), *flags],
            Self::RecursiveCollect {
                prefix, separator, ..
            } => [Some(*prefix), Some(*separator), None],
            Self::Range { from, to, .. } => [*from, Some(*to), None],
        };
        inputs.into_iter().flatten()
    }

    pub fn roots(&self) -> impl Iterator<Item = NodeId> + '_ {
        self.inputs().chain([self.item()])
    }
}

/// A non-empty ordered composition of independently evaluated target scopes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopeSequence {
    first: Box<TargetScope>,
    rest: Vec<TargetScope>,
}

impl ScopeSequence {
    pub fn new(first: TargetScope, rest: Vec<TargetScope>) -> Self {
        Self {
            first: Box::new(first),
            rest,
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = &TargetScope> {
        std::iter::once(self.first.as_ref()).chain(&self.rest)
    }

    pub fn len(&self) -> usize {
        self.rest.len() + 1
    }

    pub const fn is_empty(&self) -> bool {
        false
    }
}

/// Exactly one candidate source for an iterating scope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IterationSource {
    Source(SourceIteration),
    Generated(GeneratedSequence),
    InnerJoin(InnerJoin),
    Concatenate(ScopeSequence),
}

/// One mutually exclusive grouping operation in an iteration pipeline.
///
/// Key and boundary expressions run once per surviving candidate. Block size
/// runs once in the parent scope context before candidates are grouped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupingPlan {
    By { key: NodeId },
    AdjacentBy { key: NodeId },
    StartingWith { predicate: NodeId },
    EndingWith { predicate: NodeId },
    IntoBlocks { size: NodeId },
}

impl GroupingPlan {
    pub const fn expression(self) -> NodeId {
        match self {
            Self::By { key } | Self::AdjacentBy { key } => key,
            Self::StartingWith { predicate } | Self::EndingWith { predicate } => predicate,
            Self::IntoBlocks { size } => size,
        }
    }

    /// An expression evaluated against each candidate item, if any.
    pub const fn item_expression(self) -> Option<NodeId> {
        match self {
            Self::By { key } | Self::AdjacentBy { key } => Some(key),
            Self::StartingWith { predicate } | Self::EndingWith { predicate } => Some(predicate),
            Self::IntoBlocks { .. } => None,
        }
    }

    /// An expression evaluated once against the parent context, if any.
    pub const fn parent_expression(self) -> Option<NodeId> {
        match self {
            Self::IntoBlocks { size } => Some(size),
            Self::By { .. }
            | Self::AdjacentBy { .. }
            | Self::StartingWith { .. }
            | Self::EndingWith { .. } => None,
        }
    }
}

impl From<SourceIteration> for IterationSource {
    fn from(source: SourceIteration) -> Self {
        Self::Source(source)
    }
}

impl From<GeneratedSequence> for IterationSource {
    fn from(sequence: GeneratedSequence) -> Self {
        Self::Generated(sequence)
    }
}

impl From<InnerJoin> for IterationSource {
    fn from(join: InnerJoin) -> Self {
        Self::InnerJoin(join)
    }
}

impl From<ScopeSequence> for IterationSource {
    fn from(sequence: ScopeSequence) -> Self {
        Self::Concatenate(sequence)
    }
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

/// The one target value constructed for each scope candidate.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum TargetConstruction {
    #[default]
    Group,
    CopyCurrentSource,
    Scalar {
        expression: NodeId,
    },
    XmlMixedContent {
        elements: Vec<XmlMixedContentElement>,
    },
    RecursiveFilter {
        children: String,
        items: String,
        predicate: NodeId,
    },
}

/// One direct-child rename retained in a mixed-content target stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XmlMixedContentElement {
    pub source: String,
    pub target: String,
}

/// One statically named constructed target value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetScope {
    /// Empty only for the primary target's root scope.
    pub target_field: String,
    /// Non-iterating scopes targeting a repeating group still produce one
    /// repeated item, matching the engine's target-boundary cardinality.
    pub repeating: bool,
    /// Source-backed, generated, statically joined, or ordered concatenated
    /// iteration evaluated relative to the parent scope's current item.
    /// Absence means the scope runs exactly once.
    pub iteration: Option<IterationPlan>,
    pub construction: TargetConstruction,
    /// Declaration order is semantically significant and is preserved.
    pub bindings: Vec<Binding>,
    pub children: Vec<TargetScope>,
}

/// One candidate pipeline. Controls live inside the iteration, so a filter,
/// sort, or window cannot exist on a non-iterating scope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IterationPlan {
    input: IterationSource,
    filter: Option<NodeId>,
    sort: Option<SortPlan>,
    grouping: Option<GroupingPlan>,
    post_group_filter: Option<NodeId>,
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

    pub fn generated(sequence: GeneratedSequence) -> Self {
        Self::new(sequence, None, None, Vec::new(), IterationOutput::Repeated)
    }

    pub fn concatenate(
        first: TargetScope,
        rest: Vec<TargetScope>,
        output: IterationOutput,
    ) -> Self {
        Self::new(
            ScopeSequence::new(first, rest),
            None,
            None,
            Vec::new(),
            output,
        )
    }

    pub fn join(join: InnerJoin) -> Self {
        Self::new(join, None, None, Vec::new(), IterationOutput::Repeated)
    }

    pub fn new(
        input: impl Into<IterationSource>,
        filter: Option<NodeId>,
        sort: Option<SortPlan>,
        windows: Vec<SequenceWindow>,
        output: IterationOutput,
    ) -> Self {
        Self::new_grouped(input, filter, sort, None, windows, output)
    }

    pub fn new_grouped(
        input: impl Into<IterationSource>,
        filter: Option<NodeId>,
        sort: Option<SortPlan>,
        grouping: Option<GroupingPlan>,
        windows: Vec<SequenceWindow>,
        output: IterationOutput,
    ) -> Self {
        Self {
            input: input.into(),
            filter,
            sort,
            grouping,
            post_group_filter: None,
            windows,
            output,
        }
    }

    pub fn with_grouping(mut self, grouping: GroupingPlan) -> Self {
        self.grouping = Some(grouping);
        self.post_group_filter = None;
        self
    }

    /// Groups candidates and retains only groups for which `predicate`
    /// evaluates to true for at least one surviving member.
    pub fn with_filtered_grouping(mut self, grouping: GroupingPlan, predicate: NodeId) -> Self {
        self.grouping = Some(grouping);
        self.post_group_filter = Some(predicate);
        self
    }

    pub const fn input(&self) -> &IterationSource {
        &self.input
    }

    pub const fn source_iteration(&self) -> Option<&SourceIteration> {
        match &self.input {
            IterationSource::Source(source) => Some(source),
            IterationSource::Generated(_)
            | IterationSource::InnerJoin(_)
            | IterationSource::Concatenate(_) => None,
        }
    }

    pub const fn generated_sequence(&self) -> Option<&GeneratedSequence> {
        match &self.input {
            IterationSource::Source(_) => None,
            IterationSource::Generated(sequence) => Some(sequence),
            IterationSource::InnerJoin(_) | IterationSource::Concatenate(_) => None,
        }
    }

    pub const fn inner_join(&self) -> Option<&InnerJoin> {
        match &self.input {
            IterationSource::InnerJoin(join) => Some(join),
            IterationSource::Source(_)
            | IterationSource::Generated(_)
            | IterationSource::Concatenate(_) => None,
        }
    }

    pub const fn concatenated(&self) -> Option<&ScopeSequence> {
        match &self.input {
            IterationSource::Concatenate(sequence) => Some(sequence),
            IterationSource::Source(_)
            | IterationSource::Generated(_)
            | IterationSource::InnerJoin(_) => None,
        }
    }

    pub const fn filter(&self) -> Option<NodeId> {
        self.filter
    }

    pub const fn sort(&self) -> Option<&SortPlan> {
        self.sort.as_ref()
    }

    pub const fn grouping(&self) -> Option<GroupingPlan> {
        self.grouping
    }

    pub const fn post_group_filter(&self) -> Option<NodeId> {
        self.post_group_filter
    }

    pub fn windows(&self) -> &[SequenceWindow] {
        &self.windows
    }

    pub const fn output(&self) -> IterationOutput {
        self.output
    }

    pub fn roots(&self) -> impl Iterator<Item = NodeId> + '_ {
        self.generated_sequence()
            .into_iter()
            .flat_map(GeneratedSequence::roots)
            .chain(self.filter)
            .chain(
                self.sort
                    .iter()
                    .flat_map(SortPlan::keys)
                    .map(|key| key.expression),
            )
            .chain(self.grouping.map(GroupingPlan::expression))
            .chain(self.post_group_filter)
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
