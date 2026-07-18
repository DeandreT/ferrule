use std::collections::{HashMap, HashSet};

use crate::{MAX_SCHEMA_BYTES, ProtobufError};

const MAX_SCHEMA_TOKENS: usize = 100_000;
const MAX_MESSAGE_NESTING: usize = 128;

/// Stable identifier for a resolved message within one [`Layout`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MessageId(usize);

impl MessageId {
    pub fn index(self) -> usize {
        self.0
    }
}

/// Stable identifier for a resolved enum within one [`Layout`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EnumId(usize);

impl EnumId {
    pub fn index(self) -> usize {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cardinality {
    Required,
    Optional,
    /// A proto3 singular field without an explicit presence label.
    Implicit,
    Repeated,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScalarType {
    Double,
    Float,
    Int32,
    Int64,
    Uint32,
    Uint64,
    Sint32,
    Sint64,
    Fixed32,
    Fixed64,
    Sfixed32,
    Sfixed64,
    Bool,
    String,
    Bytes,
}

impl ScalarType {
    fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "double" => Self::Double,
            "float" => Self::Float,
            "int32" => Self::Int32,
            "int64" => Self::Int64,
            "uint32" => Self::Uint32,
            "uint64" => Self::Uint64,
            "sint32" => Self::Sint32,
            "sint64" => Self::Sint64,
            "fixed32" => Self::Fixed32,
            "fixed64" => Self::Fixed64,
            "sfixed32" => Self::Sfixed32,
            "sfixed64" => Self::Sfixed64,
            "bool" => Self::Bool,
            "string" => Self::String,
            "bytes" => Self::Bytes,
            _ => return None,
        })
    }

    pub(crate) fn is_packable(self) -> bool {
        !matches!(self, Self::String | Self::Bytes)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldType {
    Scalar(ScalarType),
    Message(MessageId),
    Enum(EnumId),
}

#[derive(Debug, Clone, PartialEq)]
pub enum DefaultValue {
    Float(f64),
    Signed(i64),
    Unsigned(u64),
    Bool(bool),
    String(String),
    Bytes(Vec<u8>),
    Enum(i32),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Field {
    name: String,
    number: u32,
    cardinality: Cardinality,
    ty: FieldType,
    packed: bool,
    default: Option<DefaultValue>,
}

impl Field {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn number(&self) -> u32 {
        self.number
    }

    pub fn cardinality(&self) -> Cardinality {
        self.cardinality
    }

    pub fn ty(&self) -> FieldType {
        self.ty
    }

    pub fn packed(&self) -> bool {
        self.packed
    }

    pub fn default(&self) -> Option<&DefaultValue> {
        self.default.as_ref()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Message {
    name: String,
    full_name: String,
    fields: Vec<Field>,
}

impl Message {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn full_name(&self) -> &str {
        &self.full_name
    }

    pub fn fields(&self) -> &[Field] {
        &self.fields
    }

    pub fn field(&self, name: &str) -> Option<&Field> {
        self.fields.iter().find(|field| field.name == name)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnumValue {
    name: String,
    number: i32,
}

impl EnumValue {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn number(&self) -> i32 {
        self.number
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Enum {
    name: String,
    full_name: String,
    values: Vec<EnumValue>,
}

impl Enum {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn full_name(&self) -> &str {
        &self.full_name
    }

    pub fn values(&self) -> &[EnumValue] {
        &self.values
    }

    pub fn value_by_name(&self, name: &str) -> Option<&EnumValue> {
        self.values.iter().find(|value| value.name == name)
    }

    pub fn value_by_number(&self, number: i32) -> Option<&EnumValue> {
        self.values.iter().find(|value| value.number == number)
    }
}

/// Fully resolved and validated proto2/proto3-lite schema.
#[derive(Debug, Clone, PartialEq)]
pub struct Layout {
    package: Option<String>,
    messages: Vec<Message>,
    enums: Vec<Enum>,
}

impl Layout {
    pub fn parse(source: &str) -> Result<Self, ProtobufError> {
        if source.len() > MAX_SCHEMA_BYTES {
            return Err(ProtobufError::schema(format!(
                "schema exceeds the {MAX_SCHEMA_BYTES}-byte limit"
            )));
        }
        Parser::new(source)?.parse()?.resolve()
    }

    pub fn package(&self) -> Option<&str> {
        self.package.as_deref()
    }

    pub fn messages(&self) -> &[Message] {
        &self.messages
    }

    pub fn enums(&self) -> &[Enum] {
        &self.enums
    }

    pub fn message(&self, id: MessageId) -> Option<&Message> {
        self.messages.get(id.0)
    }

    pub fn enumeration(&self, id: EnumId) -> Option<&Enum> {
        self.enums.get(id.0)
    }

    pub fn resolve_message(&self, name: &str) -> Result<MessageId, ProtobufError> {
        let canonical = name.strip_prefix('.').unwrap_or(name);
        if let Some((index, _)) = self
            .messages
            .iter()
            .enumerate()
            .find(|(_, message)| message.full_name == canonical)
        {
            return Ok(MessageId(index));
        }
        let mut matches = self
            .messages
            .iter()
            .enumerate()
            .filter(|(_, message)| message.name == canonical);
        let Some((index, _)) = matches.next() else {
            return Err(ProtobufError::UnknownRoot(name.to_string()));
        };
        if matches.next().is_some() {
            return Err(ProtobufError::AmbiguousRoot(name.to_string()));
        }
        Ok(MessageId(index))
    }
}

#[derive(Debug)]
struct RawSchema {
    package: Option<String>,
    messages: Vec<RawMessage>,
    enums: Vec<RawEnum>,
}

#[derive(Debug)]
struct RawMessage {
    name: String,
    full_name: String,
    fields: Vec<RawField>,
}

#[derive(Debug)]
struct RawField {
    name: String,
    number: u32,
    cardinality: Cardinality,
    type_name: String,
    scope: String,
    packed: bool,
    default: Option<RawDefault>,
}

#[derive(Debug)]
enum RawDefault {
    Identifier(String),
    String(String),
    Number(String),
}

#[derive(Debug)]
struct RawEnum {
    name: String,
    full_name: String,
    values: Vec<EnumValue>,
}

impl RawSchema {
    fn resolve(self) -> Result<Layout, ProtobufError> {
        let mut names = HashMap::new();
        for (index, message) in self.messages.iter().enumerate() {
            if names
                .insert(
                    message.full_name.as_str(),
                    DeclId::Message(MessageId(index)),
                )
                .is_some()
            {
                return Err(ProtobufError::schema(format!(
                    "duplicate declaration `{}`",
                    message.full_name
                )));
            }
        }
        for (index, enumeration) in self.enums.iter().enumerate() {
            if names
                .insert(enumeration.full_name.as_str(), DeclId::Enum(EnumId(index)))
                .is_some()
            {
                return Err(ProtobufError::schema(format!(
                    "duplicate declaration `{}`",
                    enumeration.full_name
                )));
            }
        }

        let messages = self
            .messages
            .iter()
            .map(|message| resolve_message(message, self.package.as_deref(), &names, &self.enums))
            .collect::<Result<Vec<_>, _>>()?;
        let enums = self
            .enums
            .into_iter()
            .map(|enumeration| Enum {
                name: enumeration.name,
                full_name: enumeration.full_name,
                values: enumeration.values,
            })
            .collect();
        Ok(Layout {
            package: self.package,
            messages,
            enums,
        })
    }
}

#[derive(Clone, Copy)]
enum DeclId {
    Message(MessageId),
    Enum(EnumId),
}

fn resolve_message(
    raw: &RawMessage,
    package: Option<&str>,
    names: &HashMap<&str, DeclId>,
    enums: &[RawEnum],
) -> Result<Message, ProtobufError> {
    let mut field_names = HashSet::new();
    let mut field_numbers = HashSet::new();
    let mut fields = Vec::with_capacity(raw.fields.len());
    for field in &raw.fields {
        if !field_names.insert(field.name.as_str()) {
            return Err(ProtobufError::schema(format!(
                "message `{}` has duplicate field `{}`",
                raw.full_name, field.name
            )));
        }
        if !field_numbers.insert(field.number) {
            return Err(ProtobufError::schema(format!(
                "message `{}` has duplicate field number {}",
                raw.full_name, field.number
            )));
        }
        validate_field_number(raw, field)?;
        let ty = match ScalarType::parse(&field.type_name) {
            Some(scalar) => FieldType::Scalar(scalar),
            None => resolve_named_type(&field.type_name, &field.scope, package, names)?,
        };
        if field.packed
            && (field.cardinality != Cardinality::Repeated
                || !matches!(ty, FieldType::Scalar(scalar) if scalar.is_packable())
                    && !matches!(ty, FieldType::Enum(_)))
        {
            return Err(ProtobufError::schema(format!(
                "field `{}.{}` uses packed encoding but is not a repeated numeric, bool, or enum field",
                raw.full_name, field.name
            )));
        }
        if field.default.is_some() && field.cardinality != Cardinality::Optional {
            return Err(ProtobufError::schema(format!(
                "non-optional field `{}.{}` cannot declare a default",
                raw.full_name, field.name
            )));
        }
        let default = field
            .default
            .as_ref()
            .map(|value| resolve_default(value, ty, enums))
            .transpose()?;
        let default = if field.cardinality == Cardinality::Implicit {
            proto3_default(ty, enums)?
        } else {
            default
        };
        fields.push(Field {
            name: field.name.clone(),
            number: field.number,
            cardinality: field.cardinality,
            ty,
            packed: field.packed,
            default,
        });
    }
    Ok(Message {
        name: raw.name.clone(),
        full_name: raw.full_name.clone(),
        fields,
    })
}

fn proto3_default(ty: FieldType, enums: &[RawEnum]) -> Result<Option<DefaultValue>, ProtobufError> {
    let value = match ty {
        FieldType::Message(_) => return Ok(None),
        FieldType::Enum(id) => {
            let enumeration = enums.get(id.0).ok_or_else(|| {
                ProtobufError::schema(format!("unknown resolved enum id {}", id.index()))
            })?;
            if enumeration.values.first().map(EnumValue::number) != Some(0) {
                return Err(ProtobufError::schema(format!(
                    "proto3 enum `{}` must declare zero as its first value",
                    enumeration.full_name
                )));
            }
            DefaultValue::Enum(0)
        }
        FieldType::Scalar(ScalarType::Double | ScalarType::Float) => DefaultValue::Float(0.0),
        FieldType::Scalar(
            ScalarType::Int32
            | ScalarType::Int64
            | ScalarType::Sint32
            | ScalarType::Sint64
            | ScalarType::Sfixed32
            | ScalarType::Sfixed64,
        ) => DefaultValue::Signed(0),
        FieldType::Scalar(
            ScalarType::Uint32 | ScalarType::Uint64 | ScalarType::Fixed32 | ScalarType::Fixed64,
        ) => DefaultValue::Unsigned(0),
        FieldType::Scalar(ScalarType::Bool) => DefaultValue::Bool(false),
        FieldType::Scalar(ScalarType::String) => DefaultValue::String(String::new()),
        FieldType::Scalar(ScalarType::Bytes) => DefaultValue::Bytes(Vec::new()),
    };
    Ok(Some(value))
}

fn validate_field_number(message: &RawMessage, field: &RawField) -> Result<(), ProtobufError> {
    const MAX_FIELD_NUMBER: u32 = (1 << 29) - 1;
    if field.number == 0
        || field.number > MAX_FIELD_NUMBER
        || (19_000..=19_999).contains(&field.number)
    {
        return Err(ProtobufError::schema(format!(
            "field `{}.{}` has invalid or reserved number {}",
            message.full_name, field.name, field.number
        )));
    }
    Ok(())
}

fn resolve_named_type(
    type_name: &str,
    scope: &str,
    package: Option<&str>,
    names: &HashMap<&str, DeclId>,
) -> Result<FieldType, ProtobufError> {
    if let Some(absolute) = type_name.strip_prefix('.') {
        return names
            .get(absolute)
            .copied()
            .map(field_type)
            .ok_or_else(|| ProtobufError::schema(format!("unknown field type `{type_name}`")));
    }

    let package_parts = package.map_or(0, |value| value.split('.').count());
    let parts: Vec<_> = scope.split('.').collect();
    for length in (package_parts..=parts.len()).rev() {
        let prefix = parts[..length].join(".");
        let candidate = if prefix.is_empty() {
            type_name.to_string()
        } else {
            format!("{prefix}.{type_name}")
        };
        if let Some(id) = names.get(candidate.as_str()).copied() {
            return Ok(field_type(id));
        }
    }
    Err(ProtobufError::schema(format!(
        "field in `{scope}` references unknown type `{type_name}`"
    )))
}

fn field_type(id: DeclId) -> FieldType {
    match id {
        DeclId::Message(id) => FieldType::Message(id),
        DeclId::Enum(id) => FieldType::Enum(id),
    }
}

fn resolve_default(
    raw: &RawDefault,
    ty: FieldType,
    enums: &[RawEnum],
) -> Result<DefaultValue, ProtobufError> {
    let invalid = || ProtobufError::schema("field default is incompatible with its type");
    let value = match ty {
        FieldType::Message(_) => return Err(invalid()),
        FieldType::Enum(id) => {
            let RawDefault::Identifier(name) = raw else {
                return Err(invalid());
            };
            let enumeration = enums.get(id.0).ok_or_else(invalid)?;
            let value = enumeration
                .values
                .iter()
                .find(|value| value.name == *name)
                .ok_or_else(|| {
                    ProtobufError::schema(format!(
                        "enum `{}` has no default value named `{name}`",
                        enumeration.full_name
                    ))
                })?;
            DefaultValue::Enum(value.number)
        }
        FieldType::Scalar(ScalarType::String) => match raw {
            RawDefault::String(value) => DefaultValue::String(value.clone()),
            _ => return Err(invalid()),
        },
        FieldType::Scalar(ScalarType::Bytes) => match raw {
            RawDefault::String(value) => DefaultValue::Bytes(value.as_bytes().to_vec()),
            _ => return Err(invalid()),
        },
        FieldType::Scalar(ScalarType::Bool) => match raw {
            RawDefault::Identifier(value) if value == "true" => DefaultValue::Bool(true),
            RawDefault::Identifier(value) if value == "false" => DefaultValue::Bool(false),
            _ => return Err(invalid()),
        },
        FieldType::Scalar(ScalarType::Double | ScalarType::Float) => {
            let lexical = raw_number(raw).ok_or_else(invalid)?;
            let value = lexical.parse::<f64>().map_err(|_| invalid())?;
            if !value.is_finite() {
                return Err(invalid());
            }
            if ty == FieldType::Scalar(ScalarType::Float) && !(value as f32).is_finite() {
                return Err(invalid());
            }
            DefaultValue::Float(value)
        }
        FieldType::Scalar(ScalarType::Int32 | ScalarType::Sint32 | ScalarType::Sfixed32) => {
            let value = raw_number(raw)
                .ok_or_else(invalid)?
                .parse::<i64>()
                .map_err(|_| invalid())?;
            i32::try_from(value).map_err(|_| invalid())?;
            DefaultValue::Signed(value)
        }
        FieldType::Scalar(ScalarType::Int64 | ScalarType::Sint64 | ScalarType::Sfixed64) => {
            DefaultValue::Signed(
                raw_number(raw)
                    .ok_or_else(invalid)?
                    .parse()
                    .map_err(|_| invalid())?,
            )
        }
        FieldType::Scalar(ScalarType::Uint32 | ScalarType::Fixed32) => {
            let value = raw_number(raw)
                .ok_or_else(invalid)?
                .parse::<u64>()
                .map_err(|_| invalid())?;
            u32::try_from(value).map_err(|_| invalid())?;
            DefaultValue::Unsigned(value)
        }
        FieldType::Scalar(ScalarType::Uint64 | ScalarType::Fixed64) => DefaultValue::Unsigned(
            raw_number(raw)
                .ok_or_else(invalid)?
                .parse()
                .map_err(|_| invalid())?,
        ),
    };
    Ok(value)
}

fn raw_number(raw: &RawDefault) -> Option<&str> {
    match raw {
        RawDefault::Number(value) => Some(value),
        RawDefault::Identifier(_) | RawDefault::String(_) => None,
    }
}

#[derive(Debug, Clone, PartialEq)]
enum TokenKind {
    Identifier(String),
    Number(String),
    String(String),
    Symbol(char),
    Eof,
}

#[derive(Debug, Clone)]
struct Token {
    kind: TokenKind,
    line: usize,
    column: usize,
}

struct Lexer<'a> {
    chars: std::iter::Peekable<std::str::Chars<'a>>,
    line: usize,
    column: usize,
}

impl<'a> Lexer<'a> {
    fn new(source: &'a str) -> Self {
        Self {
            chars: source.chars().peekable(),
            line: 1,
            column: 1,
        }
    }

    fn tokenize(mut self) -> Result<Vec<Token>, ProtobufError> {
        let mut tokens = Vec::new();
        loop {
            self.skip_trivia()?;
            let line = self.line;
            let column = self.column;
            let Some(ch) = self.peek() else {
                tokens.push(Token {
                    kind: TokenKind::Eof,
                    line,
                    column,
                });
                return Ok(tokens);
            };
            let kind = if ch.is_ascii_alphabetic() || ch == '_' {
                TokenKind::Identifier(self.take_while(|c| c.is_ascii_alphanumeric() || c == '_'))
            } else if ch.is_ascii_digit() {
                TokenKind::Number(self.number())
            } else if ch == '"' || ch == '\'' {
                TokenKind::String(self.string(ch)?)
            } else if "{}[]=;.,-".contains(ch) {
                self.next();
                TokenKind::Symbol(ch)
            } else {
                return Err(ProtobufError::parse(
                    line,
                    column,
                    format!("unsupported character `{ch}`"),
                ));
            };
            tokens.push(Token { kind, line, column });
            if tokens.len() > MAX_SCHEMA_TOKENS {
                return Err(ProtobufError::schema(format!(
                    "schema exceeds the {MAX_SCHEMA_TOKENS}-token limit"
                )));
            }
        }
    }

    fn skip_trivia(&mut self) -> Result<(), ProtobufError> {
        loop {
            while self.peek().is_some_and(char::is_whitespace) {
                self.next();
            }
            let mut probe = self.chars.clone();
            if probe.next() != Some('/') {
                return Ok(());
            }
            match probe.next() {
                Some('/') => {
                    self.next();
                    self.next();
                    while self.peek().is_some_and(|ch| ch != '\n') {
                        self.next();
                    }
                }
                Some('*') => {
                    let line = self.line;
                    let column = self.column;
                    self.next();
                    self.next();
                    let mut previous = '\0';
                    loop {
                        let Some(ch) = self.next() else {
                            return Err(ProtobufError::parse(
                                line,
                                column,
                                "unterminated block comment",
                            ));
                        };
                        if previous == '*' && ch == '/' {
                            break;
                        }
                        previous = ch;
                    }
                }
                _ => return Ok(()),
            }
        }
    }

    fn number(&mut self) -> String {
        let mut value = self.take_while(|ch| ch.is_ascii_digit());
        if self.peek() == Some('.') {
            value.push(self.next().unwrap_or('.'));
            value.push_str(&self.take_while(|ch| ch.is_ascii_digit()));
        }
        if matches!(self.peek(), Some('e' | 'E')) {
            value.push(self.next().unwrap_or('e'));
            if matches!(self.peek(), Some('+' | '-')) {
                value.push(self.next().unwrap_or('+'));
            }
            value.push_str(&self.take_while(|ch| ch.is_ascii_digit()));
        }
        value
    }

    fn string(&mut self, quote: char) -> Result<String, ProtobufError> {
        let line = self.line;
        let column = self.column;
        self.next();
        let mut value = String::new();
        loop {
            let Some(ch) = self.next() else {
                return Err(ProtobufError::parse(
                    line,
                    column,
                    "unterminated string literal",
                ));
            };
            if ch == quote {
                return Ok(value);
            }
            if ch != '\\' {
                value.push(ch);
                continue;
            }
            let escape_line = self.line;
            let escape_column = self.column;
            let escaped = match self.next() {
                Some('n') => '\n',
                Some('r') => '\r',
                Some('t') => '\t',
                Some('\\') => '\\',
                Some('\'') => '\'',
                Some('"') => '"',
                Some(other) => {
                    return Err(ProtobufError::parse(
                        escape_line,
                        escape_column,
                        format!("unsupported string escape `\\{other}`"),
                    ));
                }
                None => {
                    return Err(ProtobufError::parse(
                        line,
                        column,
                        "unterminated string escape",
                    ));
                }
            };
            value.push(escaped);
        }
    }

    fn take_while(&mut self, predicate: impl Fn(char) -> bool) -> String {
        let mut value = String::new();
        while self.peek().is_some_and(&predicate) {
            if let Some(ch) = self.next() {
                value.push(ch);
            }
        }
        value
    }

    fn peek(&mut self) -> Option<char> {
        self.chars.peek().copied()
    }

    fn next(&mut self) -> Option<char> {
        let ch = self.chars.next()?;
        if ch == '\n' {
            self.line += 1;
            self.column = 1;
        } else {
            self.column += 1;
        }
        Some(ch)
    }
}

struct Parser {
    tokens: Vec<Token>,
    index: usize,
    package: Option<String>,
    messages: Vec<RawMessage>,
    enums: Vec<RawEnum>,
    syntax: Syntax,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Syntax {
    Proto2,
    Proto3,
}

impl Parser {
    fn new(source: &str) -> Result<Self, ProtobufError> {
        Ok(Self {
            tokens: Lexer::new(source).tokenize()?,
            index: 0,
            package: None,
            messages: Vec::new(),
            enums: Vec::new(),
            syntax: Syntax::Proto2,
        })
    }

    fn parse(mut self) -> Result<RawSchema, ProtobufError> {
        if self.peek_identifier("syntax") {
            self.parse_syntax()?;
        }
        while !matches!(self.peek().kind, TokenKind::Eof) {
            if self.consume_symbol(';') {
                continue;
            }
            if self.peek_identifier("package") {
                self.parse_package()?;
            } else if self.peek_identifier("message") {
                let prefix = self.package.clone().unwrap_or_default();
                self.parse_message(&prefix, 1)?;
            } else if self.peek_identifier("enum") {
                let prefix = self.package.clone().unwrap_or_default();
                self.parse_enum(&prefix)?;
            } else if self.peek_identifier("option") {
                self.skip_statement()?;
            } else {
                return self.error("expected `package`, `message`, or `enum`");
            }
        }
        if self.messages.is_empty() {
            return Err(ProtobufError::schema(
                "schema must declare at least one message",
            ));
        }
        Ok(RawSchema {
            package: self.package,
            messages: self.messages,
            enums: self.enums,
        })
    }

    fn parse_syntax(&mut self) -> Result<(), ProtobufError> {
        self.expect_identifier("syntax")?;
        self.expect_symbol('=')?;
        let syntax = self.expect_string()?;
        self.expect_symbol(';')?;
        self.syntax = match syntax.as_str() {
            "proto2" => Syntax::Proto2,
            "proto3" => Syntax::Proto3,
            _ => return self.error("syntax must be `proto2` or `proto3`"),
        };
        Ok(())
    }

    fn parse_package(&mut self) -> Result<(), ProtobufError> {
        if self.package.is_some() {
            return self.error("schema declares more than one package");
        }
        self.expect_identifier("package")?;
        self.package = Some(self.parse_qualified_name(false)?);
        self.expect_symbol(';')
    }

    fn parse_message(&mut self, prefix: &str, depth: usize) -> Result<(), ProtobufError> {
        if depth > MAX_MESSAGE_NESTING {
            return self.error(format!(
                "message nesting exceeds the limit of {MAX_MESSAGE_NESTING}"
            ));
        }
        self.expect_identifier("message")?;
        let name = self.expect_any_identifier()?;
        let full_name = qualify(prefix, &name);
        self.expect_symbol('{')?;
        let mut fields = Vec::new();
        while !self.consume_symbol('}') {
            if self.consume_symbol(';') {
                continue;
            }
            if self.peek_identifier("message") {
                self.parse_message(&full_name, depth + 1)?;
            } else if self.peek_identifier("enum") {
                self.parse_enum(&full_name)?;
            } else if self.peek_identifier("option") {
                self.skip_statement()?;
            } else if self.peek_identifier("required")
                || self.peek_identifier("optional")
                || self.peek_identifier("repeated")
            {
                fields.push(self.parse_field(&full_name)?);
            } else if self.syntax == Syntax::Proto3
                && matches!(self.peek().kind, TokenKind::Identifier(_))
            {
                fields.push(self.parse_implicit_field(&full_name)?);
            } else {
                return self.error(
                    "expected a labeled field, nested message, nested enum, or closing `}`",
                );
            }
        }
        self.messages.push(RawMessage {
            name,
            full_name,
            fields,
        });
        Ok(())
    }

    fn parse_enum(&mut self, prefix: &str) -> Result<(), ProtobufError> {
        self.expect_identifier("enum")?;
        let name = self.expect_any_identifier()?;
        let full_name = qualify(prefix, &name);
        self.expect_symbol('{')?;
        let mut values = Vec::new();
        let mut names = HashSet::new();
        let mut numbers = HashSet::new();
        while !self.consume_symbol('}') {
            if self.consume_symbol(';') {
                continue;
            }
            if self.peek_identifier("option") {
                self.skip_statement()?;
                continue;
            }
            let value_name = self.expect_any_identifier()?;
            self.expect_symbol('=')?;
            let number = self.parse_signed_number::<i32>("enum value")?;
            self.expect_symbol(';')?;
            if !names.insert(value_name.clone()) {
                return self.error(format!(
                    "enum `{full_name}` has duplicate value `{value_name}`"
                ));
            }
            if !numbers.insert(number) {
                return self.error(format!(
                    "enum `{full_name}` has duplicate number `{number}`"
                ));
            }
            values.push(EnumValue {
                name: value_name,
                number,
            });
        }
        if values.is_empty() {
            return self.error(format!("enum `{full_name}` must declare a value"));
        }
        if self.syntax == Syntax::Proto3 && values.first().map(EnumValue::number) != Some(0) {
            return self.error(format!(
                "proto3 enum `{full_name}` must declare zero as its first value"
            ));
        }
        self.enums.push(RawEnum {
            name,
            full_name,
            values,
        });
        Ok(())
    }

    fn parse_field(&mut self, scope: &str) -> Result<RawField, ProtobufError> {
        let cardinality = match self.expect_any_identifier()?.as_str() {
            "required" if self.syntax == Syntax::Proto2 => Cardinality::Required,
            "required" => return self.error("proto3 fields cannot be `required`"),
            "optional" if self.syntax == Syntax::Proto2 => Cardinality::Optional,
            "optional" => return self.error("explicit proto3 `optional` fields are not supported"),
            "repeated" => Cardinality::Repeated,
            _ => return self.error("field must be required, optional, or repeated"),
        };
        self.parse_field_tail(scope, cardinality)
    }

    fn parse_implicit_field(&mut self, scope: &str) -> Result<RawField, ProtobufError> {
        self.parse_field_tail(scope, Cardinality::Implicit)
    }

    fn parse_field_tail(
        &mut self,
        scope: &str,
        cardinality: Cardinality,
    ) -> Result<RawField, ProtobufError> {
        let type_name = self.parse_qualified_name(true)?;
        let name = self.expect_any_identifier()?;
        self.expect_symbol('=')?;
        let number = self.parse_unsigned_number::<u32>("field number")?;
        let mut packed = false;
        let mut default = None;
        if self.consume_symbol('[') {
            loop {
                let option = self.parse_qualified_name(false)?;
                self.expect_symbol('=')?;
                let value = self.parse_default()?;
                match option.as_str() {
                    "packed" => match value {
                        RawDefault::Identifier(value) if value == "true" => packed = true,
                        RawDefault::Identifier(value) if value == "false" => packed = false,
                        _ => return self.error("`packed` must be true or false"),
                    },
                    "default" => {
                        if self.syntax == Syntax::Proto3 {
                            return self.error("proto3 fields cannot declare explicit defaults");
                        }
                        if default.replace(value).is_some() {
                            return self.error("field declares more than one default");
                        }
                    }
                    "deprecated" => {}
                    _ => return self.error(format!("unsupported field option `{option}`")),
                }
                if self.consume_symbol(']') {
                    break;
                }
                self.expect_symbol(',')?;
            }
        }
        self.expect_symbol(';')?;
        Ok(RawField {
            name,
            number,
            cardinality,
            type_name,
            scope: scope.to_string(),
            packed,
            default,
        })
    }

    fn parse_default(&mut self) -> Result<RawDefault, ProtobufError> {
        if let TokenKind::String(value) = &self.peek().kind {
            let value = value.clone();
            self.advance();
            return Ok(RawDefault::String(value));
        }
        let negative = self.consume_symbol('-');
        if let TokenKind::Number(value) = &self.peek().kind {
            let mut value = value.clone();
            self.advance();
            if negative {
                value.insert(0, '-');
            }
            return Ok(RawDefault::Number(value));
        }
        if negative {
            return self.error("expected a number after `-`");
        }
        Ok(RawDefault::Identifier(self.expect_any_identifier()?))
    }

    fn parse_qualified_name(&mut self, allow_absolute: bool) -> Result<String, ProtobufError> {
        let absolute = allow_absolute && self.consume_symbol('.');
        let mut value = String::new();
        if absolute {
            value.push('.');
        }
        value.push_str(&self.expect_any_identifier()?);
        while self.consume_symbol('.') {
            value.push('.');
            value.push_str(&self.expect_any_identifier()?);
        }
        Ok(value)
    }

    fn parse_signed_number<T>(&mut self, description: &str) -> Result<T, ProtobufError>
    where
        T: std::str::FromStr,
    {
        let negative = self.consume_symbol('-');
        let mut value = self.expect_number()?;
        if negative {
            value.insert(0, '-');
        }
        value
            .parse()
            .map_err(|_| self.error_value(format!("invalid {description} `{value}`")))
    }

    fn parse_unsigned_number<T>(&mut self, description: &str) -> Result<T, ProtobufError>
    where
        T: std::str::FromStr,
    {
        let value = self.expect_number()?;
        value
            .parse()
            .map_err(|_| self.error_value(format!("invalid {description} `{value}`")))
    }

    fn skip_statement(&mut self) -> Result<(), ProtobufError> {
        while !self.consume_symbol(';') {
            if matches!(self.peek().kind, TokenKind::Eof | TokenKind::Symbol('}')) {
                return self.error("unterminated option statement");
            }
            self.advance();
        }
        Ok(())
    }

    fn peek_identifier(&self, expected: &str) -> bool {
        matches!(&self.peek().kind, TokenKind::Identifier(value) if value == expected)
    }

    fn expect_identifier(&mut self, expected: &str) -> Result<(), ProtobufError> {
        if self.peek_identifier(expected) {
            self.advance();
            Ok(())
        } else {
            self.error(format!("expected `{expected}`"))
        }
    }

    fn expect_any_identifier(&mut self) -> Result<String, ProtobufError> {
        match &self.peek().kind {
            TokenKind::Identifier(value) => {
                let value = value.clone();
                self.advance();
                Ok(value)
            }
            _ => self.error("expected an identifier"),
        }
    }

    fn expect_number(&mut self) -> Result<String, ProtobufError> {
        match &self.peek().kind {
            TokenKind::Number(value) => {
                let value = value.clone();
                self.advance();
                Ok(value)
            }
            _ => self.error("expected a number"),
        }
    }

    fn expect_string(&mut self) -> Result<String, ProtobufError> {
        match &self.peek().kind {
            TokenKind::String(value) => {
                let value = value.clone();
                self.advance();
                Ok(value)
            }
            _ => self.error("expected a string literal"),
        }
    }

    fn consume_symbol(&mut self, expected: char) -> bool {
        if matches!(self.peek().kind, TokenKind::Symbol(actual) if actual == expected) {
            self.advance();
            true
        } else {
            false
        }
    }

    fn expect_symbol(&mut self, expected: char) -> Result<(), ProtobufError> {
        if self.consume_symbol(expected) {
            Ok(())
        } else {
            self.error(format!("expected `{expected}`"))
        }
    }

    fn peek(&self) -> &Token {
        &self.tokens[self.index]
    }

    fn advance(&mut self) {
        if self.index + 1 < self.tokens.len() {
            self.index += 1;
        }
    }

    fn error<T>(&self, message: impl Into<String>) -> Result<T, ProtobufError> {
        Err(self.error_value(message))
    }

    fn error_value(&self, message: impl Into<String>) -> ProtobufError {
        let token = self.peek();
        ProtobufError::parse(token.line, token.column, message)
    }
}

fn qualify(prefix: &str, name: &str) -> String {
    if prefix.is_empty() {
        name.to_string()
    } else {
        format!("{prefix}.{name}")
    }
}
