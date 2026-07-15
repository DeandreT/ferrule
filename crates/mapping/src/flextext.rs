use std::collections::HashSet;
use std::num::NonZeroU32;

use ir::{ScalarType, SchemaNode};
use serde::{Deserialize, Serialize};

pub const MAX_FLEXTEXT_LAYOUT_DEPTH: usize = 64;
pub const MAX_FLEXTEXT_LAYOUT_NODES: usize = 4_096;
pub const MAX_FLEXTEXT_LAYOUT_STRING_BYTES: usize = 1_048_576;

/// A validated recursive structured-text layout.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FlexTextLayout {
    root_name: String,
    command: FlexCommand,
    output_line_ending: FlexLineEnding,
    write_bom: bool,
}

impl FlexTextLayout {
    pub fn new(
        root_name: impl Into<String>,
        command: FlexCommand,
        output_line_ending: FlexLineEnding,
        write_bom: bool,
    ) -> Result<Self, FlexTextLayoutError> {
        let root_name = root_name.into();
        validate_name(&root_name, "root")?;
        let mut nodes = 0;
        validate_command(&command, 1, &mut nodes)?;
        let string_bytes = root_name
            .len()
            .checked_add(command_string_bytes(&command)?)
            .ok_or(FlexTextLayoutError::TooManyStringBytes)?;
        if string_bytes > MAX_FLEXTEXT_LAYOUT_STRING_BYTES {
            return Err(FlexTextLayoutError::TooManyStringBytes);
        }
        if command.schema_node().is_none() {
            return Err(FlexTextLayoutError::NoOutput);
        }
        Ok(Self {
            root_name,
            command,
            output_line_ending,
            write_bom,
        })
    }

    pub fn root_name(&self) -> &str {
        &self.root_name
    }

    pub fn command(&self) -> &FlexCommand {
        &self.command
    }

    pub const fn output_line_ending(&self) -> FlexLineEnding {
        self.output_line_ending
    }

    pub const fn write_bom(&self) -> bool {
        self.write_bom
    }

    pub fn schema(&self) -> SchemaNode {
        let child = self.command.schema_node().into_iter().collect::<Vec<_>>();
        SchemaNode::group(&self.root_name, child)
    }
}

impl<'de> Deserialize<'de> for FlexTextLayout {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Repr {
            root_name: String,
            command: FlexCommand,
            output_line_ending: FlexLineEnding,
            #[serde(default)]
            write_bom: bool,
        }

        let value = Repr::deserialize(deserializer)?;
        Self::new(
            value.root_name,
            value.command,
            value.output_line_ending,
            value.write_bom,
        )
        .map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FlexLineEnding {
    #[default]
    Lf,
    Crlf,
}

impl FlexLineEnding {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Lf => "\n",
            Self::Crlf => "\r\n",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FlexCommand {
    SplitOnce {
        name: String,
        splitter: OnceSplitter,
        first: Box<FlexCommand>,
        second: Box<FlexCommand>,
    },
    SplitMany {
        name: String,
        splitter: ManySplitter,
        child: Box<FlexCommand>,
    },
    Store {
        name: String,
        ty: ScalarType,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        trim: Option<StoreTrim>,
    },
    Ignore,
    FixedWidthRecords {
        name: String,
        fields: Vec<FixedWidthRecordField>,
    },
    DelimitedRecords {
        name: String,
        dialect: DelimitedDialect,
        fields: Vec<DelimitedRecordField>,
    },
    Switch {
        name: String,
        arms: Vec<SwitchArm>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        default: Option<Box<FlexCommand>>,
    },
}

impl FlexCommand {
    pub fn store(name: impl Into<String>, ty: ScalarType, trim: Option<StoreTrim>) -> Self {
        Self::Store {
            name: name.into(),
            ty,
            trim,
        }
    }

    pub fn output_name(&self) -> Option<&str> {
        match self {
            Self::SplitOnce { name, .. }
            | Self::SplitMany { name, .. }
            | Self::Store { name, .. }
            | Self::FixedWidthRecords { name, .. }
            | Self::DelimitedRecords { name, .. }
            | Self::Switch { name, .. } => Some(name),
            Self::Ignore => None,
        }
    }

    fn schema_node(&self) -> Option<SchemaNode> {
        match self {
            Self::SplitOnce {
                name,
                first,
                second,
                ..
            } => Some(SchemaNode::group(
                name,
                [first.schema_node(), second.schema_node()]
                    .into_iter()
                    .flatten()
                    .collect(),
            )),
            Self::SplitMany { name, child, .. } => {
                let mut node = SchemaNode::group(name, child.schema_node().into_iter().collect());
                node.repeating = true;
                Some(node)
            }
            Self::Store { name, ty, .. } => Some(SchemaNode::scalar(name, *ty)),
            Self::Ignore => None,
            Self::FixedWidthRecords { name, fields } => Some(repeating_record_schema(
                name,
                fields.iter().map(|field| (field.name(), field.ty())),
            )),
            Self::DelimitedRecords { name, fields, .. } => Some(repeating_record_schema(
                name,
                fields.iter().map(|field| (field.name(), field.ty())),
            )),
            Self::Switch {
                name,
                arms,
                default,
            } => {
                let children = arms
                    .iter()
                    .filter_map(|arm| arm.command.schema_node())
                    .chain(default.iter().filter_map(|command| command.schema_node()))
                    .collect();
                Some(SchemaNode::group(name, children))
            }
        }
    }
}

fn repeating_record_schema<'a>(
    name: &str,
    fields: impl Iterator<Item = (&'a str, ScalarType)>,
) -> SchemaNode {
    let mut node = SchemaNode::group(
        name,
        fields
            .map(|(name, ty)| SchemaNode::scalar(name, ty))
            .collect(),
    );
    node.repeating = true;
    node
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum OnceSplitter {
    FixedLines(NonZeroU32),
    FixedColumns(NonZeroU32),
    Delimiter(String),
    LineStartingWith(String),
    LineContaining(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum ManySplitter {
    FixedLines(NonZeroU32),
    LinesStartingWith(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrimSide {
    Left,
    Right,
    Both,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StoreTrim {
    side: TrimSide,
    characters: String,
}

impl StoreTrim {
    pub fn new(side: TrimSide, characters: impl Into<String>) -> Result<Self, FlexTextLayoutError> {
        let characters = characters.into();
        validate_nonempty_string(&characters, "trim characters")?;
        let mut unique = HashSet::new();
        if characters
            .chars()
            .any(|character| !unique.insert(character))
        {
            return Err(FlexTextLayoutError::DuplicateTrimCharacter);
        }
        Ok(Self { side, characters })
    }

    pub const fn side(&self) -> TrimSide {
        self.side
    }

    pub fn characters(&self) -> &str {
        &self.characters
    }
}

impl<'de> Deserialize<'de> for StoreTrim {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Repr {
            side: TrimSide,
            characters: String,
        }
        let value = Repr::deserialize(deserializer)?;
        Self::new(value.side, value.characters).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FixedWidthRecordField {
    name: String,
    ty: ScalarType,
    width: NonZeroU32,
}

impl FixedWidthRecordField {
    pub fn new(
        name: impl Into<String>,
        ty: ScalarType,
        width: NonZeroU32,
    ) -> Result<Self, FlexTextLayoutError> {
        let name = name.into();
        validate_name(&name, "fixed-width field")?;
        Ok(Self { name, ty, width })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub const fn ty(&self) -> ScalarType {
        self.ty
    }

    pub const fn width(&self) -> NonZeroU32 {
        self.width
    }
}

impl<'de> Deserialize<'de> for FixedWidthRecordField {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Repr {
            name: String,
            ty: ScalarType,
            width: NonZeroU32,
        }
        let value = Repr::deserialize(deserializer)?;
        Self::new(value.name, value.ty, value.width).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DelimitedRecordField {
    name: String,
    ty: ScalarType,
}

impl DelimitedRecordField {
    pub fn new(name: impl Into<String>, ty: ScalarType) -> Result<Self, FlexTextLayoutError> {
        let name = name.into();
        validate_name(&name, "delimited field")?;
        Ok(Self { name, ty })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub const fn ty(&self) -> ScalarType {
        self.ty
    }
}

impl<'de> Deserialize<'de> for DelimitedRecordField {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Repr {
            name: String,
            ty: ScalarType,
        }
        let value = Repr::deserialize(deserializer)?;
        Self::new(value.name, value.ty).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DelimitedDialect {
    field_separator: String,
    record_separator: String,
    quote: char,
    escape: char,
}

impl DelimitedDialect {
    pub fn new(
        field_separator: char,
        record_separator: impl Into<String>,
        quote: char,
        escape: char,
    ) -> Result<Self, FlexTextLayoutError> {
        Self::new_with_field_separator(field_separator.to_string(), record_separator, quote, escape)
    }

    pub fn new_with_field_separator(
        field_separator: impl Into<String>,
        record_separator: impl Into<String>,
        quote: char,
        escape: char,
    ) -> Result<Self, FlexTextLayoutError> {
        let field_separator = field_separator.into();
        let record_separator = record_separator.into();
        if field_separator
            .chars()
            .any(|value| matches!(value, '\r' | '\n' | '\0'))
            || matches!(quote, '\r' | '\n' | '\0')
            || matches!(escape, '\r' | '\n' | '\0')
            || field_separator.contains(quote)
        {
            return Err(FlexTextLayoutError::InvalidDelimitedDialect);
        }
        validate_nonempty_string(&field_separator, "field separator")?;
        validate_nonempty_string(&record_separator, "record separator")?;
        Ok(Self {
            field_separator,
            record_separator,
            quote,
            escape,
        })
    }

    pub fn field_separator(&self) -> &str {
        &self.field_separator
    }

    pub fn record_separator(&self) -> &str {
        &self.record_separator
    }

    pub const fn quote(&self) -> char {
        self.quote
    }

    pub const fn escape(&self) -> char {
        self.escape
    }
}

impl<'de> Deserialize<'de> for DelimitedDialect {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Repr {
            field_separator: String,
            record_separator: String,
            quote: char,
            escape: char,
        }
        let value = Repr::deserialize(deserializer)?;
        Self::new_with_field_separator(
            value.field_separator,
            value.record_separator,
            value.quote,
            value.escape,
        )
        .map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SwitchArm {
    prefix: String,
    command: Box<FlexCommand>,
}

impl SwitchArm {
    pub fn new(
        prefix: impl Into<String>,
        command: FlexCommand,
    ) -> Result<Self, FlexTextLayoutError> {
        let prefix = prefix.into();
        validate_nonempty_string(&prefix, "switch prefix")?;
        Ok(Self {
            prefix,
            command: Box::new(command),
        })
    }

    pub fn prefix(&self) -> &str {
        &self.prefix
    }

    pub fn command(&self) -> &FlexCommand {
        &self.command
    }
}

impl<'de> Deserialize<'de> for SwitchArm {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Repr {
            prefix: String,
            command: FlexCommand,
        }
        let value = Repr::deserialize(deserializer)?;
        Self::new(value.prefix, value.command).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FlexTextLayoutError {
    EmptyName(&'static str),
    EmptyString(&'static str),
    StringTooLong(&'static str),
    TooManyStringBytes,
    DuplicateName(String),
    DuplicateSwitchPrefix(String),
    DuplicateTrimCharacter,
    EmptyRecord,
    EmptySwitch,
    NoOutput,
    InvalidDelimitedDialect,
    LayoutTooDeep,
    TooManyNodes,
    RecordWidthOverflow,
}

impl std::fmt::Display for FlexTextLayoutError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyName(kind) => write!(formatter, "{kind} name must not be empty"),
            Self::EmptyString(kind) => write!(formatter, "{kind} must not be empty"),
            Self::StringTooLong(kind) => write!(
                formatter,
                "{kind} exceeds the {MAX_FLEXTEXT_LAYOUT_STRING_BYTES}-byte limit"
            ),
            Self::TooManyStringBytes => write!(
                formatter,
                "layout strings exceed the {MAX_FLEXTEXT_LAYOUT_STRING_BYTES}-byte total limit"
            ),
            Self::DuplicateName(name) => write!(formatter, "duplicate sibling name `{name}`"),
            Self::DuplicateSwitchPrefix(prefix) => {
                write!(formatter, "duplicate switch prefix `{prefix}`")
            }
            Self::DuplicateTrimCharacter => {
                formatter.write_str("trim characters must form a set without duplicates")
            }
            Self::EmptyRecord => formatter.write_str("record must contain at least one field"),
            Self::EmptySwitch => {
                formatter.write_str("switch must contain an arm or a default command")
            }
            Self::NoOutput => formatter.write_str("command tree has no viable output"),
            Self::InvalidDelimitedDialect => formatter.write_str(
                "field separator, quote, and escape must be non-NUL non-line characters and the field separator must differ from the quote",
            ),
            Self::LayoutTooDeep => write!(
                formatter,
                "command nesting exceeds the limit of {MAX_FLEXTEXT_LAYOUT_DEPTH}"
            ),
            Self::TooManyNodes => write!(
                formatter,
                "command tree exceeds the limit of {MAX_FLEXTEXT_LAYOUT_NODES} nodes"
            ),
            Self::RecordWidthOverflow => {
                formatter.write_str("fixed-width record exceeds this platform's limits")
            }
        }
    }
}

impl std::error::Error for FlexTextLayoutError {}

fn validate_command(
    command: &FlexCommand,
    depth: usize,
    nodes: &mut usize,
) -> Result<(), FlexTextLayoutError> {
    if depth > MAX_FLEXTEXT_LAYOUT_DEPTH {
        return Err(FlexTextLayoutError::LayoutTooDeep);
    }
    *nodes = nodes
        .checked_add(1)
        .ok_or(FlexTextLayoutError::TooManyNodes)?;
    if *nodes > MAX_FLEXTEXT_LAYOUT_NODES {
        return Err(FlexTextLayoutError::TooManyNodes);
    }
    if let Some(name) = command.output_name() {
        validate_name(name, "command")?;
    }
    match command {
        FlexCommand::SplitOnce {
            splitter,
            first,
            second,
            ..
        } => {
            validate_once_splitter(splitter)?;
            validate_command(first, depth + 1, nodes)?;
            validate_command(second, depth + 1, nodes)?;
            ensure_unique_outputs([first.as_ref(), second.as_ref()])?;
            if first.schema_node().is_none() && second.schema_node().is_none() {
                return Err(FlexTextLayoutError::NoOutput);
            }
        }
        FlexCommand::SplitMany {
            splitter, child, ..
        } => {
            validate_many_splitter(splitter)?;
            validate_command(child, depth + 1, nodes)?;
            if child.schema_node().is_none() {
                return Err(FlexTextLayoutError::NoOutput);
            }
        }
        FlexCommand::Store { trim, .. } => {
            if let Some(trim) = trim {
                validate_nonempty_string(trim.characters(), "trim characters")?;
            }
        }
        FlexCommand::Ignore => {}
        FlexCommand::FixedWidthRecords { fields, .. } => {
            validate_fields(fields.iter().map(FixedWidthRecordField::name))?;
            fields.iter().try_fold(0_usize, |total, field| {
                let width = usize::try_from(field.width().get())
                    .map_err(|_| FlexTextLayoutError::RecordWidthOverflow)?;
                total
                    .checked_add(width)
                    .ok_or(FlexTextLayoutError::RecordWidthOverflow)
            })?;
        }
        FlexCommand::DelimitedRecords {
            dialect, fields, ..
        } => {
            validate_fields(fields.iter().map(DelimitedRecordField::name))?;
            validate_nonempty_string(dialect.field_separator(), "field separator")?;
            validate_nonempty_string(dialect.record_separator(), "record separator")?;
        }
        FlexCommand::Switch { arms, default, .. } => {
            if arms.is_empty() && default.is_none() {
                return Err(FlexTextLayoutError::EmptySwitch);
            }
            let mut prefixes = HashSet::new();
            for arm in arms {
                validate_nonempty_string(arm.prefix(), "switch prefix")?;
                if !prefixes.insert(arm.prefix()) {
                    return Err(FlexTextLayoutError::DuplicateSwitchPrefix(
                        arm.prefix().to_string(),
                    ));
                }
                validate_command(arm.command(), depth + 1, nodes)?;
            }
            if let Some(default) = default {
                validate_command(default, depth + 1, nodes)?;
            }
            ensure_unique_outputs(
                arms.iter()
                    .map(SwitchArm::command)
                    .chain(default.iter().map(AsRef::as_ref)),
            )?;
            if arms.iter().all(|arm| arm.command().schema_node().is_none())
                && default
                    .as_deref()
                    .is_none_or(|command| command.schema_node().is_none())
            {
                return Err(FlexTextLayoutError::NoOutput);
            }
        }
    }
    Ok(())
}

fn command_string_bytes(command: &FlexCommand) -> Result<usize, FlexTextLayoutError> {
    let add = |total: usize, length: usize| {
        total
            .checked_add(length)
            .ok_or(FlexTextLayoutError::TooManyStringBytes)
    };
    let name_bytes = command.output_name().map_or(0, str::len);
    match command {
        FlexCommand::SplitOnce {
            splitter,
            first,
            second,
            ..
        } => {
            let splitter = match splitter {
                OnceSplitter::Delimiter(value)
                | OnceSplitter::LineStartingWith(value)
                | OnceSplitter::LineContaining(value) => value.len(),
                OnceSplitter::FixedLines(_) | OnceSplitter::FixedColumns(_) => 0,
            };
            let total = add(name_bytes, splitter)?;
            let total = add(total, command_string_bytes(first)?)?;
            add(total, command_string_bytes(second)?)
        }
        FlexCommand::SplitMany {
            splitter, child, ..
        } => {
            let splitter = match splitter {
                ManySplitter::LinesStartingWith(value) => value.len(),
                ManySplitter::FixedLines(_) => 0,
            };
            add(add(name_bytes, splitter)?, command_string_bytes(child)?)
        }
        FlexCommand::Store { trim, .. } => add(
            name_bytes,
            trim.as_ref().map_or(0, |trim| trim.characters().len()),
        ),
        FlexCommand::Ignore => Ok(0),
        FlexCommand::FixedWidthRecords { fields, .. } => fields
            .iter()
            .try_fold(name_bytes, |total, field| add(total, field.name().len())),
        FlexCommand::DelimitedRecords {
            dialect, fields, ..
        } => {
            let total = add(name_bytes, dialect.field_separator().len())?;
            fields.iter().try_fold(
                add(total, dialect.record_separator().len())?,
                |total, field| add(total, field.name().len()),
            )
        }
        FlexCommand::Switch { arms, default, .. } => {
            let mut total = name_bytes;
            for arm in arms {
                total = add(total, arm.prefix().len())?;
                total = add(total, command_string_bytes(arm.command())?)?;
            }
            if let Some(default) = default {
                total = add(total, command_string_bytes(default)?)?;
            }
            Ok(total)
        }
    }
}

fn validate_once_splitter(splitter: &OnceSplitter) -> Result<(), FlexTextLayoutError> {
    match splitter {
        OnceSplitter::Delimiter(value) => validate_nonempty_string(value, "delimiter"),
        OnceSplitter::LineStartingWith(value) => {
            validate_nonempty_string(value, "line-start marker")
        }
        OnceSplitter::LineContaining(value) => {
            validate_nonempty_string(value, "line-content marker")
        }
        OnceSplitter::FixedLines(_) | OnceSplitter::FixedColumns(_) => Ok(()),
    }
}

fn validate_many_splitter(splitter: &ManySplitter) -> Result<(), FlexTextLayoutError> {
    match splitter {
        ManySplitter::LinesStartingWith(value) => {
            validate_nonempty_string(value, "line-start marker")
        }
        ManySplitter::FixedLines(_) => Ok(()),
    }
}

fn validate_fields<'a>(names: impl Iterator<Item = &'a str>) -> Result<(), FlexTextLayoutError> {
    let mut names_seen = HashSet::new();
    let mut count = 0;
    for name in names {
        count += 1;
        validate_name(name, "record field")?;
        if !names_seen.insert(name) {
            return Err(FlexTextLayoutError::DuplicateName(name.to_string()));
        }
    }
    if count == 0 {
        return Err(FlexTextLayoutError::EmptyRecord);
    }
    Ok(())
}

fn ensure_unique_outputs<'a>(
    commands: impl IntoIterator<Item = &'a FlexCommand>,
) -> Result<(), FlexTextLayoutError> {
    let mut names = HashSet::new();
    for command in commands {
        if let Some(name) = command.output_name()
            && !names.insert(name)
        {
            return Err(FlexTextLayoutError::DuplicateName(name.to_string()));
        }
    }
    Ok(())
}

fn validate_name(name: &str, kind: &'static str) -> Result<(), FlexTextLayoutError> {
    if name.is_empty() {
        return Err(FlexTextLayoutError::EmptyName(kind));
    }
    validate_string_size(name, kind)
}

fn validate_nonempty_string(value: &str, kind: &'static str) -> Result<(), FlexTextLayoutError> {
    if value.is_empty() {
        return Err(FlexTextLayoutError::EmptyString(kind));
    }
    validate_string_size(value, kind)
}

fn validate_string_size(value: &str, kind: &'static str) -> Result<(), FlexTextLayoutError> {
    if value.len() > MAX_FLEXTEXT_LAYOUT_STRING_BYTES {
        Err(FlexTextLayoutError::StringTooLong(kind))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use ir::SchemaKind;

    use super::*;

    fn nonzero(value: u32) -> NonZeroU32 {
        match NonZeroU32::new(value) {
            Some(value) => value,
            None => panic!("test width must be nonzero"),
        }
    }

    #[test]
    fn schema_projects_nested_and_repeating_commands() {
        let layout = FlexTextLayout::new(
            "document",
            FlexCommand::SplitOnce {
                name: "body".into(),
                splitter: OnceSplitter::FixedColumns(nonzero(2)),
                first: Box::new(FlexCommand::store("tag", ScalarType::String, None)),
                second: Box::new(FlexCommand::FixedWidthRecords {
                    name: "rows".into(),
                    fields: vec![
                        FixedWidthRecordField::new("name", ScalarType::String, nonzero(4)).unwrap(),
                        FixedWidthRecordField::new("count", ScalarType::Int, nonzero(2)).unwrap(),
                    ],
                }),
            },
            FlexLineEnding::Crlf,
            true,
        )
        .unwrap();

        let schema = layout.schema();
        let SchemaKind::Group { children, .. } = schema.kind else {
            panic!("root should be a group");
        };
        assert_eq!(children[0].name, "body");
        let SchemaKind::Group { children, .. } = &children[0].kind else {
            panic!("split should be a group");
        };
        assert_eq!(children[0].name, "tag");
        assert!(children[1].repeating);
    }

    #[test]
    fn serde_revalidates_duplicate_names_and_depth() {
        let invalid = r#"{
            "root_name":"root",
            "command":{"kind":"split_once","name":"parts",
                "splitter":{"kind":"delimiter","value":"|"},
                "first":{"kind":"store","name":"same","ty":"string"},
                "second":{"kind":"store","name":"same","ty":"string"}},
            "output_line_ending":"lf","write_bom":false
        }"#;
        assert!(serde_json::from_str::<FlexTextLayout>(invalid).is_err());

        let mut command = FlexCommand::store("leaf", ScalarType::String, None);
        for index in 0..MAX_FLEXTEXT_LAYOUT_DEPTH {
            command = FlexCommand::SplitMany {
                name: format!("level{index}"),
                splitter: ManySplitter::FixedLines(nonzero(1)),
                child: Box::new(command),
            };
        }
        assert!(matches!(
            FlexTextLayout::new("root", command, FlexLineEnding::Lf, false),
            Err(FlexTextLayoutError::LayoutTooDeep)
        ));
    }

    #[test]
    fn line_containing_splitter_has_a_distinct_serialized_kind() {
        let splitter = OnceSplitter::LineContaining("record".into());
        let encoded = serde_json::to_string(&splitter).unwrap();
        assert_eq!(encoded, r#"{"kind":"line_containing","value":"record"}"#);
        assert_eq!(
            serde_json::from_str::<OnceSplitter>(&encoded).unwrap(),
            splitter
        );
    }
}
