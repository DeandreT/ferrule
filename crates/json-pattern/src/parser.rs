use std::sync::Arc;

use crate::{CompileError, CompileErrorKind, MAX_AST_NODES, MAX_PARSE_DEPTH};

#[derive(Clone, Debug)]
pub(crate) enum Expression {
    Empty,
    Literal(char),
    Class(Arc<CharacterClass>),
    Dot,
    Start,
    End,
    Group(Box<Self>),
    Concatenation(Vec<Self>),
    Alternation(Vec<Self>),
    Repeat {
        expression: Box<Self>,
        minimum: u32,
        maximum: Option<u32>,
    },
}

#[derive(Clone, Debug)]
pub(crate) struct CharacterClass {
    pub(crate) negated: bool,
    pub(crate) ranges: Vec<(char, char)>,
}

impl CharacterClass {
    pub(crate) fn contains(&self, value: char) -> bool {
        let candidate = self.ranges.partition_point(|(_, end)| *end < value);
        let included = self
            .ranges
            .get(candidate)
            .is_some_and(|(start, end)| *start <= value && value <= *end);
        included != self.negated
    }
}

pub(crate) fn parse(source: &str) -> Result<Expression, CompileError> {
    let mut parser = Parser {
        source,
        position: 0,
        depth: 0,
        nodes: 0,
    };
    let expression = parser.parse_alternation()?;
    if parser.peek().is_some() {
        return Err(parser.error(CompileErrorKind::UnexpectedToken));
    }
    Ok(expression)
}

struct Parser<'a> {
    source: &'a str,
    position: usize,
    depth: usize,
    nodes: usize,
}

impl Parser<'_> {
    fn parse_alternation(&mut self) -> Result<Expression, CompileError> {
        let offset = self.position;
        let mut branches = vec![self.parse_concatenation()?];
        while self.peek() == Some('|') {
            self.next();
            branches.push(self.parse_concatenation()?);
        }
        if branches.len() == 1 {
            branches
                .pop()
                .ok_or_else(|| self.error(CompileErrorKind::UnexpectedEnd))
        } else {
            self.node(Expression::Alternation(branches), offset)
        }
    }

    fn parse_concatenation(&mut self) -> Result<Expression, CompileError> {
        let offset = self.position;
        let mut expressions = Vec::new();
        while !matches!(self.peek(), None | Some('|') | Some(')')) {
            expressions.push(self.parse_repetition()?);
        }
        match expressions.len() {
            0 => self.node(Expression::Empty, offset),
            1 => expressions
                .pop()
                .ok_or_else(|| self.error(CompileErrorKind::UnexpectedEnd)),
            _ => self.node(Expression::Concatenation(expressions), offset),
        }
    }

    fn parse_repetition(&mut self) -> Result<Expression, CompileError> {
        let offset = self.position;
        let (mut expression, assertion) = self.parse_atom()?;
        let quantifier = match self.peek() {
            Some('*') => {
                self.next();
                Some((0, None))
            }
            Some('+') => {
                self.next();
                Some((1, None))
            }
            Some('?') => {
                self.next();
                Some((0, Some(1)))
            }
            Some('{') => Some(self.parse_braced_quantifier()?),
            _ => None,
        };

        if let Some((minimum, maximum)) = quantifier {
            if assertion {
                return Err(CompileError::new(
                    CompileErrorKind::QuantifiedAssertion,
                    offset,
                ));
            }
            if self.peek() == Some('?') {
                self.next();
            }
            expression = self.node(
                Expression::Repeat {
                    expression: Box::new(expression),
                    minimum,
                    maximum,
                },
                offset,
            )?;
            if matches!(self.peek(), Some('*' | '+' | '?' | '{')) {
                return Err(self.error(CompileErrorKind::InvalidQuantifier));
            }
        }
        Ok(expression)
    }

    fn parse_atom(&mut self) -> Result<(Expression, bool), CompileError> {
        let offset = self.position;
        let value = self
            .next()
            .ok_or_else(|| self.error(CompileErrorKind::UnexpectedEnd))?;
        match value {
            '.' => Ok((self.node(Expression::Dot, offset)?, false)),
            '^' => Ok((self.node(Expression::Start, offset)?, true)),
            '$' => Ok((self.node(Expression::End, offset)?, true)),
            '[' => Ok((
                self.parse_character_class(offset)
                    .and_then(|class| self.node(Expression::Class(Arc::new(class)), offset))?,
                false,
            )),
            '(' => {
                if self.peek() == Some('?') {
                    self.next();
                    if self.next() != Some(':') {
                        return Err(CompileError::new(
                            CompileErrorKind::UnsupportedConstruct,
                            offset,
                        ));
                    }
                }
                self.depth = self.depth.saturating_add(1);
                if self.depth > MAX_PARSE_DEPTH {
                    return Err(CompileError::new(
                        CompileErrorKind::ParseDepthExceeded,
                        offset,
                    ));
                }
                let nested = self.parse_alternation()?;
                if self.next() != Some(')') {
                    return Err(CompileError::new(
                        CompileErrorKind::UnexpectedEnd,
                        self.position,
                    ));
                }
                self.depth -= 1;
                Ok((
                    self.node(Expression::Group(Box::new(nested)), offset)?,
                    false,
                ))
            }
            '\\' => Ok((
                self.parse_escape(offset)
                    .and_then(|literal| self.node(Expression::Literal(literal), offset))?,
                false,
            )),
            '*' | '+' | '?' | '{' | '}' | ')' | ']' => {
                Err(CompileError::new(CompileErrorKind::UnexpectedToken, offset))
            }
            literal => Ok((self.node(Expression::Literal(literal), offset)?, false)),
        }
    }

    fn parse_braced_quantifier(&mut self) -> Result<(u32, Option<u32>), CompileError> {
        let offset = self.position;
        self.next();
        let minimum = self.parse_decimal(offset)?;
        match self.next() {
            Some('}') => Ok((minimum, Some(minimum))),
            Some(',') => {
                if self.peek() == Some('}') {
                    self.next();
                    return Ok((minimum, None));
                }
                let maximum = self.parse_decimal(offset)?;
                if self.next() != Some('}') || maximum < minimum {
                    return Err(CompileError::new(
                        CompileErrorKind::InvalidQuantifier,
                        offset,
                    ));
                }
                Ok((minimum, Some(maximum)))
            }
            _ => Err(CompileError::new(
                CompileErrorKind::InvalidQuantifier,
                offset,
            )),
        }
    }

    fn parse_decimal(&mut self, offset: usize) -> Result<u32, CompileError> {
        let mut value = 0_u32;
        let mut found = false;
        while let Some(digit) = self.peek().and_then(|value| value.to_digit(10)) {
            found = true;
            self.next();
            value = value
                .checked_mul(10)
                .and_then(|value| value.checked_add(digit))
                .ok_or_else(|| CompileError::new(CompileErrorKind::InvalidQuantifier, offset))?;
        }
        if found {
            Ok(value)
        } else {
            Err(CompileError::new(
                CompileErrorKind::InvalidQuantifier,
                offset,
            ))
        }
    }

    fn parse_character_class(&mut self, offset: usize) -> Result<CharacterClass, CompileError> {
        let negated = if self.peek() == Some('^') {
            self.next();
            true
        } else {
            false
        };
        let mut members = Vec::new();
        loop {
            if self.peek().is_none() {
                return Err(CompileError::new(
                    CompileErrorKind::InvalidCharacterClass,
                    offset,
                ));
            }
            if self.peek() == Some(']') {
                self.next();
                break;
            }
            if ["&&", "--", "~~", "||"]
                .iter()
                .any(|operator| self.remaining().starts_with(operator))
            {
                return Err(self.error(CompileErrorKind::UnsupportedConstruct));
            }
            let member_offset = self.position;
            let (value, escaped) = match self.next() {
                Some('\\') => (self.parse_escape(member_offset)?, true),
                Some('[') => {
                    return Err(CompileError::new(
                        CompileErrorKind::UnsupportedConstruct,
                        member_offset,
                    ));
                }
                Some(value) => (value, false),
                None => {
                    return Err(CompileError::new(
                        CompileErrorKind::InvalidCharacterClass,
                        offset,
                    ));
                }
            };
            members.push((value, member_offset, escaped));
        }

        let mut ranges = Vec::new();
        let mut index = 0;
        while index < members.len() {
            let (start, start_offset, start_escaped) = members[index];
            if start == '-' && !start_escaped && index != 0 && index + 1 != members.len() {
                return Err(CompileError::new(
                    CompileErrorKind::InvalidRange,
                    start_offset,
                ));
            }
            if (start != '-' || start_escaped)
                && index + 2 < members.len()
                && members[index + 1].0 == '-'
                && !members[index + 1].2
                && (members[index + 2].0 != '-' || members[index + 2].2)
            {
                let end = members[index + 2].0;
                if start > end {
                    return Err(CompileError::new(
                        CompileErrorKind::InvalidRange,
                        start_offset,
                    ));
                }
                ranges.push((start, end));
                index += 3;
            } else {
                ranges.push((start, start));
                index += 1;
            }
        }
        ranges.sort_unstable();
        let mut merged: Vec<(char, char)> = Vec::with_capacity(ranges.len());
        for (start, end) in ranges {
            if let Some((_, previous_end)) = merged.last_mut()
                && u32::from(start) <= u32::from(*previous_end).saturating_add(1)
            {
                *previous_end = (*previous_end).max(end);
            } else {
                merged.push((start, end));
            }
        }
        Ok(CharacterClass {
            negated,
            ranges: merged,
        })
    }

    fn parse_escape(&mut self, offset: usize) -> Result<char, CompileError> {
        let escaped = self
            .next()
            .ok_or_else(|| CompileError::new(CompileErrorKind::InvalidEscape, offset))?;
        match escaped {
            '^' | '$' | '\\' | '.' | '*' | '+' | '?' | '(' | ')' | '[' | ']' | '{' | '}' | '|'
            | '/' => Ok(escaped),
            'n' => Ok('\n'),
            'r' => Ok('\r'),
            't' => Ok('\t'),
            'f' => Ok('\u{000c}'),
            'v' => Ok('\u{000b}'),
            '0' if !self.peek().is_some_and(|value| value.is_ascii_digit()) => Ok('\0'),
            'x' => self.parse_fixed_hex(2, offset).and_then(|value| {
                char::from_u32(value)
                    .ok_or_else(|| CompileError::new(CompileErrorKind::InvalidEscape, offset))
            }),
            'u' if self.peek() == Some('{') => self.parse_braced_unicode(offset),
            'u' => self.parse_utf16_escape(offset),
            _ => Err(CompileError::new(CompileErrorKind::InvalidEscape, offset)),
        }
    }

    fn parse_fixed_hex(&mut self, digits: usize, offset: usize) -> Result<u32, CompileError> {
        let mut value = 0_u32;
        for _ in 0..digits {
            let digit = self
                .next()
                .and_then(|value| value.to_digit(16))
                .ok_or_else(|| CompileError::new(CompileErrorKind::InvalidEscape, offset))?;
            value = value * 16 + digit;
        }
        Ok(value)
    }

    fn parse_braced_unicode(&mut self, offset: usize) -> Result<char, CompileError> {
        self.next();
        let mut value = 0_u32;
        let mut digits = 0_usize;
        loop {
            match self.next() {
                Some('}') if digits > 0 => break,
                Some(digit) if digit.is_ascii_hexdigit() && digits < 6 => {
                    let Some(hex) = digit.to_digit(16) else {
                        return Err(CompileError::new(
                            CompileErrorKind::InvalidUnicodeEscape,
                            offset,
                        ));
                    };
                    digits += 1;
                    value = value
                        .checked_mul(16)
                        .and_then(|value| value.checked_add(hex))
                        .ok_or_else(|| {
                            CompileError::new(CompileErrorKind::InvalidUnicodeEscape, offset)
                        })?;
                }
                _ => {
                    return Err(CompileError::new(
                        CompileErrorKind::InvalidUnicodeEscape,
                        offset,
                    ));
                }
            }
        }
        if (0xd800..=0xdfff).contains(&value) {
            return Err(CompileError::new(
                CompileErrorKind::InvalidUnicodeEscape,
                offset,
            ));
        }
        char::from_u32(value)
            .ok_or_else(|| CompileError::new(CompileErrorKind::InvalidUnicodeEscape, offset))
    }

    fn parse_utf16_escape(&mut self, offset: usize) -> Result<char, CompileError> {
        let first = self.parse_fixed_hex(4, offset)?;
        if (0xd800..=0xdbff).contains(&first) {
            if self.next() != Some('\\') || self.next() != Some('u') {
                return Err(CompileError::new(
                    CompileErrorKind::InvalidUnicodeEscape,
                    offset,
                ));
            }
            let second = self.parse_fixed_hex(4, offset)?;
            if !(0xdc00..=0xdfff).contains(&second) {
                return Err(CompileError::new(
                    CompileErrorKind::InvalidUnicodeEscape,
                    offset,
                ));
            }
            let scalar = 0x10000 + ((first - 0xd800) << 10) + (second - 0xdc00);
            char::from_u32(scalar)
                .ok_or_else(|| CompileError::new(CompileErrorKind::InvalidUnicodeEscape, offset))
        } else if (0xdc00..=0xdfff).contains(&first) {
            Err(CompileError::new(
                CompileErrorKind::InvalidUnicodeEscape,
                offset,
            ))
        } else {
            char::from_u32(first)
                .ok_or_else(|| CompileError::new(CompileErrorKind::InvalidUnicodeEscape, offset))
        }
    }

    fn node(&mut self, expression: Expression, offset: usize) -> Result<Expression, CompileError> {
        self.nodes = self.nodes.saturating_add(1);
        if self.nodes > MAX_AST_NODES {
            Err(CompileError::new(
                CompileErrorKind::AstNodeLimitExceeded,
                offset,
            ))
        } else {
            Ok(expression)
        }
    }

    fn peek(&self) -> Option<char> {
        self.remaining().chars().next()
    }

    fn next(&mut self) -> Option<char> {
        let value = self.peek()?;
        self.position += value.len_utf8();
        Some(value)
    }

    fn remaining(&self) -> &str {
        self.source.get(self.position..).unwrap_or_default()
    }

    fn error(&self, kind: CompileErrorKind) -> CompileError {
        CompileError::new(kind, self.position)
    }
}
