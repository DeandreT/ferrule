/// Stable functional area used to group built-ins in authoring surfaces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BuiltinCategory {
    Boolean,
    String,
    Numeric,
    DateTime,
    Path,
    Json,
    FlexText,
    Generator,
    Conversion,
    Validation,
    Internal,
}

/// Scalar value domain accepted or returned by a built-in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ScalarDomain {
    Any,
    String,
    Boolean,
    Numeric,
    Integer,
    Date,
    Time,
    DateTime,
    Duration,
    Path,
    JsonText,
    FlexText,
    XmlNil,
}

/// One named scalar input in a built-in signature.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BuiltinParameter {
    pub name: &'static str,
    pub domain: ScalarDomain,
}

/// Invariant-preserving accepted argument counts for a built-in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BuiltinArity(BuiltinArityKind);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum BuiltinArityKind {
    Fixed(usize),
    Range {
        minimum: usize,
        maximum: usize,
    },
    /// `step` supports repeated argument groups such as JSON path/type/value
    /// triples. Ordinary variadics use a step of one.
    Variadic {
        minimum: usize,
        step: usize,
    },
}

impl BuiltinArity {
    pub const fn fixed(count: usize) -> Self {
        Self(BuiltinArityKind::Fixed(count))
    }

    pub const fn range(minimum: usize, maximum: usize) -> Self {
        assert!(minimum <= maximum, "arity range minimum exceeds maximum");
        Self(BuiltinArityKind::Range { minimum, maximum })
    }

    pub const fn variadic(minimum: usize, step: usize) -> Self {
        assert!(step > 0, "variadic arity step must be positive");
        Self(BuiltinArityKind::Variadic { minimum, step })
    }

    pub const fn minimum(self) -> usize {
        match self.0 {
            BuiltinArityKind::Fixed(count) => count,
            BuiltinArityKind::Range { minimum, .. }
            | BuiltinArityKind::Variadic { minimum, .. } => minimum,
        }
    }

    pub const fn maximum(self) -> Option<usize> {
        match self.0 {
            BuiltinArityKind::Fixed(count) => Some(count),
            BuiltinArityKind::Range { maximum, .. } => Some(maximum),
            BuiltinArityKind::Variadic { .. } => None,
        }
    }

    pub const fn step(self) -> Option<usize> {
        match self.0 {
            BuiltinArityKind::Variadic { step, .. } => Some(step),
            BuiltinArityKind::Fixed(_) | BuiltinArityKind::Range { .. } => None,
        }
    }

    pub const fn is_fixed(self) -> bool {
        matches!(self.0, BuiltinArityKind::Fixed(_))
    }

    pub const fn is_range(self) -> bool {
        matches!(self.0, BuiltinArityKind::Range { .. })
    }

    pub const fn is_variadic(self) -> bool {
        matches!(self.0, BuiltinArityKind::Variadic { .. })
    }

    pub const fn accepts(self, count: usize) -> bool {
        match self.0 {
            BuiltinArityKind::Fixed(expected) => count == expected,
            BuiltinArityKind::Range { minimum, maximum } => count >= minimum && count <= maximum,
            BuiltinArityKind::Variadic { minimum, step } => {
                count >= minimum && (count - minimum).is_multiple_of(step)
            }
        }
    }
}

/// Whether a built-in belongs in ordinary mapping authoring surfaces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BuiltinExposure {
    Authoring,
    Internal,
}

/// Authoritative metadata and dispatch identity for one scalar built-in.
#[derive(Debug, Clone, Copy)]
pub struct BuiltinDefinition {
    pub native_name: &'static str,
    pub display_name: &'static str,
    pub category: BuiltinCategory,
    pub parameters: &'static [BuiltinParameter],
    pub arity: BuiltinArity,
    pub return_domain: ScalarDomain,
    pub pure: bool,
    pub deterministic: bool,
    pub documentation: &'static str,
    pub exposure: BuiltinExposure,
    pub(crate) id: BuiltinId,
}

impl BuiltinDefinition {
    pub const fn accepts_arity(self, count: usize) -> bool {
        self.arity.accepts(count)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BuiltinId {
    Concat,
    Upper,
    Lower,
    NormalizeSpace,
    IsEmpty,
    Trim,
    Left,
    Right,
    LeftTrim,
    RightTrim,
    Length,
    StartsWith,
    EndsWith,
    Contains,
    Matches,
    Replace,
    SqlLike,
    PadStringLeft,
    PadStringRight,
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
    And,
    Or,
    Not,
    Substring,
    SubstringBefore,
    SubstringAfter,
    String,
    IsNumeric,
    ToNumber,
    Boolean,
    Positive,
    Floor,
    CreateGuid,
    FormatNumber,
    Exists,
    Round,
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
    DurationFromParts,
    DatetimeAdd,
    ParseDate,
    ParseDatetime,
    ParseTime,
    FormatDate,
    FormatDatetime,
    FormatTime,
    EdifactToDatetime,
    SubstituteMissing,
    SubstituteMissingWithXmlNil,
    GetFolder,
    RemoveFolder,
    GetFileext,
    ResolveFilepath,
    IsXmlNil,
    Isbn10ToIsbn13,
    SqliteMultiply,
    JsonSerializeObject,
    JsonParseField,
    FlextextParseField,
    DelayPassthrough,
    CoerceDatetime,
}

const fn parameter(name: &'static str, domain: ScalarDomain) -> BuiltinParameter {
    BuiltinParameter { name, domain }
}

macro_rules! builtin {
    ($id:ident, $native:literal, $display:literal, $category:ident, $parameters:expr,
     $arity:expr, $returns:ident, $pure:literal, $deterministic:literal,
     $exposure:ident, $documentation:literal) => {
        BuiltinDefinition {
            native_name: $native,
            display_name: $display,
            category: BuiltinCategory::$category,
            parameters: $parameters,
            arity: $arity,
            return_domain: ScalarDomain::$returns,
            pure: $pure,
            deterministic: $deterministic,
            documentation: $documentation,
            exposure: BuiltinExposure::$exposure,
            id: BuiltinId::$id,
        }
    };
}

use ScalarDomain::*;

pub(crate) const BUILTINS: &[BuiltinDefinition] = &[
    builtin!(
        Concat,
        "concat",
        "Concat",
        String,
        &[parameter("value", Any)],
        BuiltinArity::variadic(0, 1),
        String,
        true,
        true,
        Authoring,
        "Concatenates scalar lexical values."
    ),
    builtin!(
        Upper,
        "upper",
        "Uppercase",
        String,
        &[parameter("value", String)],
        BuiltinArity::fixed(1),
        String,
        true,
        true,
        Authoring,
        "Converts text to uppercase."
    ),
    builtin!(
        Lower,
        "lower",
        "Lowercase",
        String,
        &[parameter("value", String)],
        BuiltinArity::fixed(1),
        String,
        true,
        true,
        Authoring,
        "Converts text to lowercase."
    ),
    builtin!(
        NormalizeSpace,
        "normalize_space",
        "Normalize Space",
        String,
        &[parameter("value", String)],
        BuiltinArity::fixed(1),
        String,
        true,
        true,
        Authoring,
        "Collapses XML whitespace runs."
    ),
    builtin!(
        IsEmpty,
        "is_empty",
        "Is Empty",
        String,
        &[parameter("value", Any)],
        BuiltinArity::fixed(1),
        Boolean,
        true,
        true,
        Authoring,
        "Tests whether lexical text is empty."
    ),
    builtin!(
        Trim,
        "trim",
        "Trim",
        String,
        &[parameter("value", String)],
        BuiltinArity::fixed(1),
        String,
        true,
        true,
        Authoring,
        "Removes surrounding whitespace."
    ),
    builtin!(
        Left,
        "left",
        "Left",
        String,
        &[parameter("value", String), parameter("count", Numeric)],
        BuiltinArity::fixed(2),
        String,
        true,
        true,
        Authoring,
        "Returns leading Unicode characters."
    ),
    builtin!(
        Right,
        "right",
        "Right",
        String,
        &[parameter("value", String), parameter("count", Numeric)],
        BuiltinArity::fixed(2),
        String,
        true,
        true,
        Authoring,
        "Returns trailing Unicode characters."
    ),
    builtin!(
        LeftTrim,
        "left_trim",
        "Trim Left",
        String,
        &[parameter("value", String)],
        BuiltinArity::fixed(1),
        String,
        true,
        true,
        Authoring,
        "Removes leading XML whitespace."
    ),
    builtin!(
        RightTrim,
        "right_trim",
        "Trim Right",
        String,
        &[parameter("value", String)],
        BuiltinArity::fixed(1),
        String,
        true,
        true,
        Authoring,
        "Removes trailing XML whitespace."
    ),
    builtin!(
        Length,
        "length",
        "Length",
        String,
        &[parameter("value", Any)],
        BuiltinArity::fixed(1),
        Integer,
        true,
        true,
        Authoring,
        "Counts Unicode characters."
    ),
    builtin!(
        StartsWith,
        "starts_with",
        "Starts With",
        String,
        &[parameter("value", Any), parameter("prefix", Any)],
        BuiltinArity::fixed(2),
        Boolean,
        true,
        true,
        Authoring,
        "Tests a lexical prefix."
    ),
    builtin!(
        EndsWith,
        "ends_with",
        "Ends With",
        String,
        &[parameter("value", Any), parameter("suffix", Any)],
        BuiltinArity::fixed(2),
        Boolean,
        true,
        true,
        Authoring,
        "Tests a lexical suffix."
    ),
    builtin!(
        Contains,
        "contains",
        "Contains",
        String,
        &[parameter("value", Any), parameter("needle", Any)],
        BuiltinArity::fixed(2),
        Boolean,
        true,
        true,
        Authoring,
        "Tests for a lexical substring."
    ),
    builtin!(
        Matches,
        "matches",
        "Regex Matches",
        String,
        &[
            parameter("value", Any),
            parameter("pattern", Any),
            parameter("flags", Any)
        ],
        BuiltinArity::range(2, 3),
        Boolean,
        true,
        true,
        Authoring,
        "Tests a bounded regular expression."
    ),
    builtin!(
        Replace,
        "replace",
        "Regex Replace",
        String,
        &[
            parameter("value", Any),
            parameter("pattern", Any),
            parameter("replacement", Any),
            parameter("flags", Any)
        ],
        BuiltinArity::range(3, 4),
        String,
        true,
        true,
        Authoring,
        "Performs bounded regular-expression replacement."
    ),
    builtin!(
        SqlLike,
        "sql_like",
        "SQL Like",
        String,
        &[parameter("value", String), parameter("pattern", String)],
        BuiltinArity::fixed(2),
        Boolean,
        true,
        true,
        Authoring,
        "Matches SQL LIKE wildcards."
    ),
    builtin!(
        PadStringLeft,
        "pad_string_left",
        "Pad Left",
        String,
        &[
            parameter("value", Any),
            parameter("length", Numeric),
            parameter("padding", Any)
        ],
        BuiltinArity::fixed(3),
        String,
        true,
        true,
        Authoring,
        "Left-pads text to a bounded length."
    ),
    builtin!(
        PadStringRight,
        "pad_string_right",
        "Pad Right",
        String,
        &[
            parameter("value", Any),
            parameter("length", Numeric),
            parameter("padding", Any)
        ],
        BuiltinArity::fixed(3),
        String,
        true,
        true,
        Authoring,
        "Right-pads text to a bounded length."
    ),
    builtin!(
        Add,
        "add",
        "Add",
        Numeric,
        &[parameter("left", Numeric), parameter("right", Numeric)],
        BuiltinArity::variadic(2, 1),
        Numeric,
        true,
        true,
        Authoring,
        "Adds two numeric values."
    ),
    builtin!(
        Subtract,
        "subtract",
        "Subtract",
        Numeric,
        &[parameter("left", Numeric), parameter("right", Numeric)],
        BuiltinArity::variadic(2, 1),
        Numeric,
        true,
        true,
        Authoring,
        "Subtracts two numeric values."
    ),
    builtin!(
        Multiply,
        "multiply",
        "Multiply",
        Numeric,
        &[parameter("left", Numeric), parameter("right", Numeric)],
        BuiltinArity::variadic(2, 1),
        Numeric,
        true,
        true,
        Authoring,
        "Multiplies two numeric values."
    ),
    builtin!(
        Divide,
        "divide",
        "Divide",
        Numeric,
        &[parameter("left", Numeric), parameter("right", Numeric)],
        BuiltinArity::fixed(2),
        Numeric,
        true,
        true,
        Authoring,
        "Divides two numeric values."
    ),
    builtin!(
        Equal,
        "equal",
        "Equal",
        Boolean,
        &[parameter("left", Any), parameter("right", Any)],
        BuiltinArity::fixed(2),
        Boolean,
        true,
        true,
        Authoring,
        "Tests scalar equality."
    ),
    builtin!(
        NotEqual,
        "not_equal",
        "Not Equal",
        Boolean,
        &[parameter("left", Any), parameter("right", Any)],
        BuiltinArity::fixed(2),
        Boolean,
        true,
        true,
        Authoring,
        "Tests scalar inequality."
    ),
    builtin!(
        LessThan,
        "less_than",
        "Less Than",
        Boolean,
        &[parameter("left", Any), parameter("right", Any)],
        BuiltinArity::fixed(2),
        Boolean,
        true,
        true,
        Authoring,
        "Tests scalar ordering."
    ),
    builtin!(
        GreaterThan,
        "greater_than",
        "Greater Than",
        Boolean,
        &[parameter("left", Any), parameter("right", Any)],
        BuiltinArity::fixed(2),
        Boolean,
        true,
        true,
        Authoring,
        "Tests scalar ordering."
    ),
    builtin!(
        LessOrEqual,
        "less_or_equal",
        "Less or Equal",
        Boolean,
        &[parameter("left", Any), parameter("right", Any)],
        BuiltinArity::fixed(2),
        Boolean,
        true,
        true,
        Authoring,
        "Tests scalar ordering."
    ),
    builtin!(
        GreaterOrEqual,
        "greater_or_equal",
        "Greater or Equal",
        Boolean,
        &[parameter("left", Any), parameter("right", Any)],
        BuiltinArity::fixed(2),
        Boolean,
        true,
        true,
        Authoring,
        "Tests scalar ordering."
    ),
    builtin!(
        And,
        "and",
        "And",
        Boolean,
        &[parameter("left", Boolean), parameter("right", Boolean)],
        BuiltinArity::fixed(2),
        Boolean,
        true,
        true,
        Authoring,
        "Computes boolean conjunction."
    ),
    builtin!(
        Or,
        "or",
        "Or",
        Boolean,
        &[parameter("left", Boolean), parameter("right", Boolean)],
        BuiltinArity::fixed(2),
        Boolean,
        true,
        true,
        Authoring,
        "Computes boolean disjunction."
    ),
    builtin!(
        Not,
        "not",
        "Not",
        Boolean,
        &[parameter("value", Boolean)],
        BuiltinArity::fixed(1),
        Boolean,
        true,
        true,
        Authoring,
        "Negates a boolean."
    ),
    builtin!(
        Substring,
        "substring",
        "Substring",
        String,
        &[
            parameter("value", String),
            parameter("start", Numeric),
            parameter("length", Numeric)
        ],
        BuiltinArity::range(2, 3),
        String,
        true,
        true,
        Authoring,
        "Extracts an XPath-style substring."
    ),
    builtin!(
        SubstringBefore,
        "substring_before",
        "Substring Before",
        String,
        &[parameter("value", String), parameter("separator", String)],
        BuiltinArity::fixed(2),
        String,
        true,
        true,
        Authoring,
        "Returns text before a separator."
    ),
    builtin!(
        SubstringAfter,
        "substring_after",
        "Substring After",
        String,
        &[parameter("value", String), parameter("separator", String)],
        BuiltinArity::fixed(2),
        String,
        true,
        true,
        Authoring,
        "Returns text after a separator."
    ),
    builtin!(
        String,
        "string",
        "To String",
        Conversion,
        &[parameter("value", Any)],
        BuiltinArity::fixed(1),
        String,
        true,
        true,
        Authoring,
        "Converts a scalar to lexical text."
    ),
    builtin!(
        IsNumeric,
        "is_numeric",
        "Is Numeric",
        Validation,
        &[parameter("value", Any)],
        BuiltinArity::fixed(1),
        Boolean,
        true,
        true,
        Authoring,
        "Tests finite numeric lexical form."
    ),
    builtin!(
        ToNumber,
        "to_number",
        "To Number",
        Conversion,
        &[parameter("value", Any)],
        BuiltinArity::fixed(1),
        Numeric,
        true,
        true,
        Authoring,
        "Converts a scalar to a finite number."
    ),
    builtin!(
        Boolean,
        "boolean",
        "To Boolean",
        Conversion,
        &[parameter("value", Any)],
        BuiltinArity::fixed(1),
        Boolean,
        true,
        true,
        Authoring,
        "Computes effective boolean value."
    ),
    builtin!(
        Positive,
        "positive",
        "Positive",
        Numeric,
        &[parameter("value", Numeric)],
        BuiltinArity::fixed(1),
        Numeric,
        true,
        true,
        Authoring,
        "Applies numeric unary plus."
    ),
    builtin!(
        Floor,
        "floor",
        "Floor",
        Numeric,
        &[parameter("value", Numeric)],
        BuiltinArity::fixed(1),
        Numeric,
        true,
        true,
        Authoring,
        "Rounds a number toward negative infinity."
    ),
    builtin!(
        CreateGuid,
        "create_guid",
        "Create GUID",
        Generator,
        &[],
        BuiltinArity::fixed(0),
        String,
        false,
        false,
        Authoring,
        "Generates a random compact UUID."
    ),
    builtin!(
        FormatNumber,
        "format_number",
        "Format Number",
        Numeric,
        &[
            parameter("value", Numeric),
            parameter("picture", String),
            parameter("decimal_separator", String),
            parameter("grouping_separator", String)
        ],
        BuiltinArity::range(2, 4),
        String,
        true,
        true,
        Authoring,
        "Formats a number with a picture."
    ),
    builtin!(
        Exists,
        "exists",
        "Exists",
        Validation,
        &[parameter("value", Any)],
        BuiltinArity::fixed(1),
        Boolean,
        true,
        true,
        Authoring,
        "Tests whether a value is present."
    ),
    builtin!(
        Round,
        "round",
        "Round",
        Numeric,
        &[parameter("value", Numeric), parameter("precision", Numeric)],
        BuiltinArity::range(1, 2),
        Numeric,
        true,
        true,
        Authoring,
        "Rounds a number with optional precision."
    ),
    builtin!(
        DateFromDatetime,
        "date_from_datetime",
        "Date from DateTime",
        DateTime,
        &[parameter("value", DateTime)],
        BuiltinArity::fixed(1),
        Date,
        true,
        true,
        Authoring,
        "Extracts an ISO date."
    ),
    builtin!(
        YearFromDatetime,
        "year_from_datetime",
        "Year from DateTime",
        DateTime,
        &[parameter("value", DateTime)],
        BuiltinArity::fixed(1),
        Integer,
        true,
        true,
        Authoring,
        "Extracts the local year."
    ),
    builtin!(
        MonthFromDatetime,
        "month_from_datetime",
        "Month from DateTime",
        DateTime,
        &[parameter("value", DateTime)],
        BuiltinArity::fixed(1),
        Integer,
        true,
        true,
        Authoring,
        "Extracts the local month."
    ),
    builtin!(
        DayFromDatetime,
        "day_from_datetime",
        "Day from DateTime",
        DateTime,
        &[parameter("value", DateTime)],
        BuiltinArity::fixed(1),
        Integer,
        true,
        true,
        Authoring,
        "Extracts the local day."
    ),
    builtin!(
        Weekday,
        "weekday",
        "Weekday",
        DateTime,
        &[parameter("value", Date)],
        BuiltinArity::fixed(1),
        Integer,
        true,
        true,
        Authoring,
        "Returns the ISO weekday."
    ),
    builtin!(
        HoursFromDatetime,
        "hours_from_datetime",
        "Hours from DateTime",
        DateTime,
        &[parameter("value", DateTime)],
        BuiltinArity::fixed(1),
        Integer,
        true,
        true,
        Authoring,
        "Extracts the local hour."
    ),
    builtin!(
        MinutesFromDatetime,
        "minutes_from_datetime",
        "Minutes from DateTime",
        DateTime,
        &[parameter("value", DateTime)],
        BuiltinArity::fixed(1),
        Integer,
        true,
        true,
        Authoring,
        "Extracts the local minute."
    ),
    builtin!(
        TimeFromDatetime,
        "time_from_datetime",
        "Time from DateTime",
        DateTime,
        &[parameter("value", DateTime)],
        BuiltinArity::fixed(1),
        Time,
        true,
        true,
        Authoring,
        "Extracts an ISO time."
    ),
    builtin!(
        DatetimeFromDateAndTime,
        "datetime_from_date_and_time",
        "DateTime from Date and Time",
        DateTime,
        &[parameter("date", Date), parameter("time", Time)],
        BuiltinArity::range(1, 2),
        DateTime,
        true,
        true,
        Authoring,
        "Combines ISO date and time values."
    ),
    builtin!(
        DatetimeFromParts,
        "datetime_from_parts",
        "DateTime from Parts",
        DateTime,
        &[
            parameter("year", Integer),
            parameter("month", Integer),
            parameter("day", Integer),
            parameter("hour", Integer),
            parameter("minute", Integer),
            parameter("second", Integer),
            parameter("millisecond", Numeric),
            parameter("timezone_minutes", Integer)
        ],
        BuiltinArity::range(3, 8),
        DateTime,
        true,
        true,
        Authoring,
        "Constructs an ISO dateTime."
    ),
    builtin!(
        DurationFromParts,
        "duration_from_parts",
        "Duration from Parts",
        DateTime,
        &[
            parameter("years", Integer),
            parameter("months", Integer),
            parameter("days", Integer),
            parameter("hours", Integer),
            parameter("minutes", Integer),
            parameter("seconds", Integer),
            parameter("milliseconds", Numeric),
            parameter("negative", Boolean)
        ],
        BuiltinArity::range(3, 8),
        Duration,
        true,
        true,
        Authoring,
        "Constructs an ISO duration."
    ),
    builtin!(
        DatetimeAdd,
        "datetime_add",
        "Add Duration",
        DateTime,
        &[
            parameter("value", DateTime),
            parameter("duration", Duration)
        ],
        BuiltinArity::variadic(2, 1),
        DateTime,
        true,
        true,
        Authoring,
        "Adds one or more durations to a date or dateTime."
    ),
    builtin!(
        ParseDate,
        "parse_date",
        "Parse Date",
        DateTime,
        &[parameter("value", String), parameter("picture", String)],
        BuiltinArity::fixed(2),
        Date,
        true,
        true,
        Authoring,
        "Parses a date picture."
    ),
    builtin!(
        ParseDatetime,
        "parse_datetime",
        "Parse DateTime",
        DateTime,
        &[parameter("value", String), parameter("picture", String)],
        BuiltinArity::fixed(2),
        DateTime,
        true,
        true,
        Authoring,
        "Parses a dateTime picture."
    ),
    builtin!(
        ParseTime,
        "parse_time",
        "Parse Time",
        DateTime,
        &[parameter("value", String), parameter("picture", String)],
        BuiltinArity::fixed(2),
        Time,
        true,
        true,
        Authoring,
        "Parses a time picture."
    ),
    builtin!(
        FormatDate,
        "format_date",
        "Format Date",
        DateTime,
        &[
            parameter("value", Date),
            parameter("picture", String),
            parameter("language", String),
            parameter("calendar", String),
            parameter("place", String)
        ],
        BuiltinArity::range(2, 5),
        String,
        true,
        true,
        Authoring,
        "Formats an ISO date picture."
    ),
    builtin!(
        FormatDatetime,
        "format_datetime",
        "Format DateTime",
        DateTime,
        &[
            parameter("value", DateTime),
            parameter("picture", String),
            parameter("language", String),
            parameter("calendar", String),
            parameter("place", String)
        ],
        BuiltinArity::range(2, 5),
        String,
        true,
        true,
        Authoring,
        "Formats an ISO dateTime picture."
    ),
    builtin!(
        FormatTime,
        "format_time",
        "Format Time",
        DateTime,
        &[
            parameter("value", Time),
            parameter("picture", String),
            parameter("language", String),
            parameter("calendar", String),
            parameter("place", String)
        ],
        BuiltinArity::range(2, 5),
        String,
        true,
        true,
        Authoring,
        "Formats an ISO time picture."
    ),
    builtin!(
        EdifactToDatetime,
        "edifact_to_datetime",
        "EDIFACT to DateTime",
        DateTime,
        &[parameter("value", String), parameter("format_code", String)],
        BuiltinArity::fixed(2),
        DateTime,
        true,
        true,
        Authoring,
        "Converts an EDIFACT 2379 date/time."
    ),
    builtin!(
        SubstituteMissing,
        "substitute_missing",
        "Substitute Missing",
        Conversion,
        &[parameter("value", Any), parameter("replacement", Any)],
        BuiltinArity::fixed(2),
        Any,
        true,
        true,
        Authoring,
        "Replaces an absent or nil value."
    ),
    builtin!(
        SubstituteMissingWithXmlNil,
        "substitute_missing_with_xml_nil",
        "Missing to XML Nil",
        Conversion,
        &[parameter("value", Any)],
        BuiltinArity::fixed(1),
        XmlNil,
        true,
        true,
        Authoring,
        "Converts absence to XML nil."
    ),
    builtin!(
        GetFolder,
        "get_folder",
        "Get Folder",
        Path,
        &[parameter("path", Path)],
        BuiltinArity::fixed(1),
        Path,
        true,
        true,
        Authoring,
        "Returns a path's folder prefix."
    ),
    builtin!(
        RemoveFolder,
        "remove_folder",
        "Remove Folder",
        Path,
        &[parameter("path", Path)],
        BuiltinArity::fixed(1),
        Path,
        true,
        true,
        Authoring,
        "Returns a path's final component."
    ),
    builtin!(
        GetFileext,
        "get_fileext",
        "Get File Extension",
        Path,
        &[parameter("path", Path)],
        BuiltinArity::fixed(1),
        String,
        true,
        true,
        Authoring,
        "Returns a path's file extension."
    ),
    builtin!(
        ResolveFilepath,
        "resolve_filepath",
        "Resolve File Path",
        Path,
        &[parameter("base", Path), parameter("path", Path)],
        BuiltinArity::fixed(2),
        Path,
        true,
        true,
        Authoring,
        "Resolves a lexical relative path."
    ),
    builtin!(
        IsXmlNil,
        "is_xml_nil",
        "Is XML Nil",
        Validation,
        &[parameter("value", Any)],
        BuiltinArity::fixed(1),
        Boolean,
        true,
        true,
        Authoring,
        "Tests for explicit XML nil."
    ),
    builtin!(
        Isbn10ToIsbn13,
        "isbn10_to_isbn13",
        "ISBN-10 to ISBN-13",
        Validation,
        &[parameter("isbn", String)],
        BuiltinArity::fixed(1),
        String,
        true,
        true,
        Authoring,
        "Converts and validates an ISBN-10."
    ),
    builtin!(
        SqliteMultiply,
        "sqlite_multiply",
        "SQLite Multiply",
        Internal,
        &[parameter("left", Numeric), parameter("right", Numeric)],
        BuiltinArity::fixed(2),
        Numeric,
        true,
        true,
        Internal,
        "Multiplies nullable SQLite values."
    ),
    builtin!(
        JsonSerializeObject,
        "json_serialize_object",
        "Serialize JSON Object",
        Json,
        &[
            parameter("path", String),
            parameter("scalar_type", String),
            parameter("value", Any)
        ],
        BuiltinArity::variadic(3, 3),
        JsonText,
        true,
        true,
        Internal,
        "Serializes repeated path/type/value triples."
    ),
    builtin!(
        JsonParseField,
        "json_parse_field",
        "Parse JSON Field",
        Json,
        &[
            parameter("input", JsonText),
            parameter("schema", JsonText),
            parameter("path", JsonText)
        ],
        BuiltinArity::fixed(3),
        Any,
        true,
        true,
        Internal,
        "Projects a typed field from JSON text."
    ),
    builtin!(
        FlextextParseField,
        "flextext_parse_field",
        "Parse FlexText Field",
        FlexText,
        &[
            parameter("input", FlexText),
            parameter("layout", JsonText),
            parameter("path", JsonText)
        ],
        BuiltinArity::fixed(3),
        Any,
        true,
        true,
        Internal,
        "Projects a field from embedded FlexText."
    ),
    builtin!(
        DelayPassthrough,
        "delay_passthrough",
        "Delay Passthrough",
        Internal,
        &[parameter("value", Any), parameter("seconds", Numeric)],
        BuiltinArity::fixed(2),
        Any,
        true,
        true,
        Internal,
        "Validates a captured delay dependency."
    ),
    builtin!(
        CoerceDatetime,
        "coerce_datetime",
        "Coerce DateTime",
        DateTime,
        &[parameter("value", Any)],
        BuiltinArity::fixed(1),
        DateTime,
        true,
        true,
        Internal,
        "Normalizes a date or dateTime lexical value."
    ),
];

pub fn catalog() -> &'static [BuiltinDefinition] {
    BUILTINS
}

pub fn find(name: &str) -> Option<&'static BuiltinDefinition> {
    BUILTINS.iter().find(|builtin| builtin.native_name == name)
}

/// Compatibility iterator for authoring-only native names.
#[derive(Debug, Clone, Copy)]
pub struct BuiltinNames;

pub struct BuiltinNameIter {
    inner: std::slice::Iter<'static, BuiltinDefinition>,
}

impl Iterator for BuiltinNameIter {
    type Item = &'static &'static str;

    fn next(&mut self) -> Option<Self::Item> {
        self.inner
            .find(|builtin| builtin.exposure == BuiltinExposure::Authoring)
            .map(|builtin| &builtin.native_name)
    }
}

impl IntoIterator for BuiltinNames {
    type Item = &'static &'static str;
    type IntoIter = BuiltinNameIter;

    fn into_iter(self) -> Self::IntoIter {
        BuiltinNameIter {
            inner: BUILTINS.iter(),
        }
    }
}

#[cfg(test)]
mod tests;
