use std::collections::HashSet;

use crate::ProtobufError;

use super::model::{Cardinality, EnumValue, Layout, ScalarType};
use super::resolve::{RawDefault, RawEnum, RawField, RawMessage, RawSchema};

const MAX_SCHEMA_TOKENS: usize = 100_000;
const MAX_MESSAGE_NESTING: usize = 128;

pub(super) fn parse(source: &str) -> Result<Layout, ProtobufError> {
    Parser::new(source)?.parse()?.resolve()
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
            } else if "{}[]=;.,-<>".contains(ch) {
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
        let mut oneofs = Vec::new();
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
            } else if self.peek_identifier("oneof") {
                let (oneof, oneof_fields) = self.parse_oneof(&full_name)?;
                oneofs.push(oneof);
                fields.extend(oneof_fields);
            } else if self.peek_identifier("map") {
                let (entry, field) = self.parse_map_field(&full_name)?;
                self.messages.push(entry);
                fields.push(field);
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
            oneofs,
            map_entry: false,
        });
        Ok(())
    }

    fn parse_map_field(&mut self, scope: &str) -> Result<(RawMessage, RawField), ProtobufError> {
        self.expect_identifier("map")?;
        self.expect_symbol('<')?;
        let key_type = self.parse_qualified_name(false)?;
        let key_scalar = ScalarType::parse(&key_type).ok_or_else(|| {
            self.error_value(format!(
                "map key type `{key_type}` is not a scalar key type"
            ))
        })?;
        if !matches!(
            key_scalar,
            ScalarType::Int32
                | ScalarType::Int64
                | ScalarType::Uint32
                | ScalarType::Uint64
                | ScalarType::Sint32
                | ScalarType::Sint64
                | ScalarType::Fixed32
                | ScalarType::Fixed64
                | ScalarType::Sfixed32
                | ScalarType::Sfixed64
                | ScalarType::Bool
                | ScalarType::String
        ) {
            return self.error(format!(
                "map key type `{key_type}` must be an integer, bool, or string"
            ));
        }
        self.expect_symbol(',')?;
        let value_type = self.parse_qualified_name(true)?;
        self.expect_symbol('>')?;
        let name = self.expect_any_identifier()?;
        self.expect_symbol('=')?;
        let number = self.parse_unsigned_number::<u32>("field number")?;
        if self.consume_symbol('[') {
            loop {
                let option = self.parse_qualified_name(false)?;
                self.expect_symbol('=')?;
                let _ = self.parse_default()?;
                if option != "deprecated" {
                    return self.error(format!("map field cannot use option `{option}`"));
                }
                if self.consume_symbol(']') {
                    break;
                }
                self.expect_symbol(',')?;
            }
        }
        self.expect_symbol(';')?;

        let entry_name = map_entry_name(&name);
        let entry_full_name = qualify(scope, &entry_name);
        let entry = RawMessage {
            name: entry_name,
            full_name: entry_full_name.clone(),
            fields: vec![
                RawField {
                    name: "key".to_string(),
                    number: 1,
                    cardinality: Cardinality::Implicit,
                    type_name: key_type,
                    scope: entry_full_name.clone(),
                    packed: false,
                    default: None,
                    oneof: None,
                    map: false,
                },
                RawField {
                    name: "value".to_string(),
                    number: 2,
                    cardinality: Cardinality::Implicit,
                    type_name: value_type,
                    scope: entry_full_name.clone(),
                    packed: false,
                    default: None,
                    oneof: None,
                    map: false,
                },
            ],
            oneofs: Vec::new(),
            map_entry: true,
        };
        let field = RawField {
            name,
            number,
            cardinality: Cardinality::Repeated,
            type_name: format!(".{entry_full_name}"),
            scope: scope.to_string(),
            packed: false,
            default: None,
            oneof: None,
            map: true,
        };
        Ok((entry, field))
    }

    fn parse_oneof(&mut self, scope: &str) -> Result<(String, Vec<RawField>), ProtobufError> {
        self.expect_identifier("oneof")?;
        let name = self.expect_any_identifier()?;
        self.expect_symbol('{')?;
        let mut fields = Vec::new();
        while !self.consume_symbol('}') {
            if self.consume_symbol(';') {
                continue;
            }
            if self.peek_identifier("option") {
                self.skip_statement()?;
                continue;
            }
            if self.peek_identifier("required")
                || self.peek_identifier("optional")
                || self.peek_identifier("repeated")
            {
                return self.error(format!(
                    "oneof `{name}` fields cannot declare a cardinality label"
                ));
            }
            if !matches!(self.peek().kind, TokenKind::Identifier(_)) {
                return self.error(format!("expected a field in oneof `{name}`"));
            }
            let field = self.parse_field_tail(scope, Cardinality::Optional, Some(name.clone()))?;
            if field.default.is_some() {
                return self.error(format!("oneof `{name}` fields cannot declare defaults"));
            }
            if field.packed {
                return self.error(format!("oneof `{name}` fields cannot use packed encoding"));
            }
            fields.push(field);
        }
        if fields.is_empty() {
            return self.error(format!("oneof `{name}` must declare at least one field"));
        }
        Ok((name, fields))
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
            "optional" => Cardinality::Optional,
            "repeated" => Cardinality::Repeated,
            _ => return self.error("field must be required, optional, or repeated"),
        };
        if self.peek_identifier("required")
            || self.peek_identifier("optional")
            || self.peek_identifier("repeated")
        {
            return self.error("field cannot declare more than one cardinality label");
        }
        self.parse_field_tail(scope, cardinality, None)
    }

    fn parse_implicit_field(&mut self, scope: &str) -> Result<RawField, ProtobufError> {
        self.parse_field_tail(scope, Cardinality::Implicit, None)
    }

    fn parse_field_tail(
        &mut self,
        scope: &str,
        cardinality: Cardinality,
        oneof: Option<String>,
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
            oneof,
            map: false,
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

fn map_entry_name(field: &str) -> String {
    let mut name = String::new();
    let mut uppercase = true;
    for character in field.chars() {
        if character == '_' {
            uppercase = true;
        } else if uppercase {
            name.extend(character.to_uppercase());
            uppercase = false;
        } else {
            name.push(character);
        }
    }
    name.push_str("Entry");
    name
}
