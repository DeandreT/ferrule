//! Bounded syntax normalization for explicitly selected JSON5 companions.
//!
//! This layer accepts one complete value and returns compact strict JSON. It
//! preserves duplicate properties and decimal lexicals; it does not project a
//! schema, execute a mapping, or change the existing native JSON5 reader.
//! Unquoted names use the deliberate ASCII identifier subset. Quoted names
//! and strings retain Unicode scalar values. Nonfinite numbers always refuse,
//! including values later overwritten or ignored by schema projection.

mod numbers;
mod strings;

#[cfg(test)]
mod tests;

use thiserror::Error;

pub const MAX_ORIGINAL_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_NORMALIZED_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_CONTAINER_DEPTH: usize = 127;
pub const MAX_SYNTAX_WORK: usize = 536_870_912;

/// Stable grammar categories; offsets refer to original UTF-8 bytes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Json5SyntaxKind {
    UnexpectedToken,
    TrailingRootValue,
    UnterminatedComment,
    UnterminatedString,
    LineTerminatorInString,
    InvalidEscape,
    UnsupportedIdentifier,
    InvalidNumber,
    IntegerOutOfRange,
    NonFiniteNumber,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Json5SyntaxResource {
    OriginalDocumentBytes,
    NormalizedDocumentBytes,
    ContainerDepth,
    SyntaxWork,
}

#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum Json5SyntaxError {
    #[error("JSON5 syntax {kind:?} at original UTF-8 byte {offset}")]
    Syntax {
        kind: Json5SyntaxKind,
        offset: usize,
    },
    #[error(
        "JSON5 {resource:?} limit at original UTF-8 byte {offset}: requested {requested}, max {max}"
    )]
    Limit {
        resource: Json5SyntaxResource,
        offset: usize,
        requested: usize,
        max: usize,
    },
}

/// Normalize one bounded JSON5 value without schema adaptation.
///
/// The original limit includes BOM/trivia, unlike the native string reader's
/// historical BOM-stripped count. The returned document never includes a BOM
/// or trailing LF. Strings are escaped for strict JSON; decimal tokens retain
/// their spelling except a leading plus or a missing decimal-side zero.
pub fn normalize(source: &str) -> Result<String, Json5SyntaxError> {
    normalize_limited(source, Limits::default()).map(|normalized| normalized.output)
}

#[derive(Clone, Copy)]
struct Limits {
    original: usize,
    normalized: usize,
    depth: usize,
    work: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            original: MAX_ORIGINAL_BYTES,
            normalized: MAX_NORMALIZED_BYTES,
            depth: MAX_CONTAINER_DEPTH,
            work: MAX_SYNTAX_WORK,
        }
    }
}

struct Normalized {
    output: String,
    #[cfg(test)]
    work: usize,
}

#[derive(Clone, Copy)]
enum State {
    RootValue,
    RootDone,
    ObjectKey { after_comma: bool },
    ObjectColon,
    ObjectValue,
    ObjectNext,
    ArrayValue { after_comma: bool },
    ArrayNext,
}

struct Parser<'a> {
    source: &'a str,
    index: usize,
    output: String,
    states: Vec<State>,
    work: usize,
    limits: Limits,
}

fn normalize_limited(source: &str, limits: Limits) -> Result<Normalized, Json5SyntaxError> {
    if source.len() > limits.original {
        return Err(Json5SyntaxError::Limit {
            resource: Json5SyntaxResource::OriginalDocumentBytes,
            offset: 0,
            requested: source.len(),
            max: limits.original,
        });
    }
    Parser {
        source,
        index: 0,
        output: String::new(),
        states: vec![State::RootValue],
        work: 0,
        limits,
    }
    .run()
}

impl<'a> Parser<'a> {
    fn run(mut self) -> Result<Normalized, Json5SyntaxError> {
        loop {
            // One grammar-dispatch/transition unit, even at EOF.
            self.charge(1)?;
            self.trivia()?;
            let state = self
                .states
                .last()
                .copied()
                .ok_or_else(|| self.syntax(Json5SyntaxKind::UnexpectedToken, self.index))?;
            match state {
                State::RootDone => {
                    if self.peek()?.is_some() {
                        return Err(self.syntax(Json5SyntaxKind::TrailingRootValue, self.index));
                    }
                    return Ok(Normalized {
                        output: self.output,
                        #[cfg(test)]
                        work: self.work,
                    });
                }
                State::RootValue | State::ObjectValue => self.value()?,
                State::ArrayValue { after_comma } => {
                    if self.peek()? == Some(']') {
                        self.close(']')?;
                    } else {
                        if after_comma {
                            self.append(",")?;
                        }
                        self.value()?;
                    }
                }
                State::ObjectKey { after_comma } => match self.peek()? {
                    Some('}') => self.close('}')?,
                    Some('\'' | '"') => {
                        if after_comma {
                            self.append(",")?;
                        }
                        self.string()?;
                        self.set_state(State::ObjectColon)?;
                    }
                    Some(c) if c.is_ascii_alphabetic() || matches!(c, '_' | '$' | '\\') => {
                        if after_comma {
                            self.append(",")?;
                        }
                        self.identifier()?;
                        self.set_state(State::ObjectColon)?;
                    }
                    Some(c) if !c.is_ascii() && !whitespace(c) => {
                        return Err(self.syntax(Json5SyntaxKind::UnsupportedIdentifier, self.index));
                    }
                    _ => return Err(self.syntax(Json5SyntaxKind::UnexpectedToken, self.index)),
                },
                State::ObjectColon => {
                    if self.peek()? != Some(':') {
                        return Err(self.syntax(Json5SyntaxKind::UnexpectedToken, self.index));
                    }
                    self.take()?;
                    self.append(":")?;
                    self.set_state(State::ObjectValue)?;
                }
                State::ObjectNext => self.next(',', '}', State::ObjectKey { after_comma: true })?,
                State::ArrayNext => self.next(',', ']', State::ArrayValue { after_comma: true })?,
            }
        }
    }

    fn next(&mut self, comma: char, close: char, following: State) -> Result<(), Json5SyntaxError> {
        match self.peek()? {
            Some(c) if c == comma => {
                self.take()?;
                // Defer emission so the single final comma disappears.
                self.set_state(following)
            }
            Some(c) if c == close => self.close(close),
            _ => Err(self.syntax(Json5SyntaxKind::UnexpectedToken, self.index)),
        }
    }

    fn value(&mut self) -> Result<(), Json5SyntaxError> {
        let c = self
            .peek()?
            .ok_or_else(|| self.syntax(Json5SyntaxKind::UnexpectedToken, self.index))?;
        match c {
            '{' | '[' => {
                let depth = self.states.len();
                if depth > self.limits.depth {
                    return Err(self.limit(
                        Json5SyntaxResource::ContainerDepth,
                        depth,
                        self.limits.depth,
                    ));
                }
                self.value_done()?;
                self.take()?;
                self.append(if c == '{' { "{" } else { "[" })?;
                self.charge(1)?;
                self.states.push(if c == '{' {
                    State::ObjectKey { after_comma: false }
                } else {
                    State::ArrayValue { after_comma: false }
                });
                Ok(())
            }
            '\'' | '"' => {
                self.string()?;
                self.value_done()
            }
            '+' | '-' | '.' | '0'..='9' => {
                self.number()?;
                self.value_done()
            }
            't' | 'f' | 'n' | 'I' | 'N' => {
                let start = self.index;
                let end = self.token_end()?;
                let source = self.source;
                let token = &source[start..end];
                // Reserve three full token-comparison passes for the literal
                // alternatives, regardless of the matching branch.
                self.charge(token.len().saturating_mul(3))?;
                match token {
                    "true" | "false" | "null" => self.append(token)?,
                    "Infinity" | "NaN" => {
                        return Err(self.syntax(Json5SyntaxKind::NonFiniteNumber, start));
                    }
                    _ => return Err(self.syntax(Json5SyntaxKind::UnexpectedToken, start)),
                }
                self.value_done()
            }
            _ => Err(self.syntax(Json5SyntaxKind::UnexpectedToken, self.index)),
        }
    }

    fn value_done(&mut self) -> Result<(), Json5SyntaxError> {
        let state = match self.states.last() {
            Some(State::RootValue) => State::RootDone,
            Some(State::ObjectValue) => State::ObjectNext,
            Some(State::ArrayValue { .. }) => State::ArrayNext,
            _ => return Err(self.syntax(Json5SyntaxKind::UnexpectedToken, self.index)),
        };
        self.set_state(state)
    }

    fn set_state(&mut self, state: State) -> Result<(), Json5SyntaxError> {
        self.charge(1)?;
        if let Some(current) = self.states.last_mut() {
            *current = state;
            Ok(())
        } else {
            Err(self.syntax(Json5SyntaxKind::UnexpectedToken, self.index))
        }
    }

    fn close(&mut self, c: char) -> Result<(), Json5SyntaxError> {
        self.take()?;
        self.append(if c == '}' { "}" } else { "]" })?;
        self.charge(1)?;
        let _ = self.states.pop();
        Ok(())
    }

    fn trivia(&mut self) -> Result<(), Json5SyntaxError> {
        loop {
            match self.peek()? {
                Some(c) if whitespace(c) => {
                    self.take()?;
                }
                Some('/') => {
                    let start = self.index;
                    self.take()?;
                    match self.peek()? {
                        Some('/') => {
                            self.take()?;
                            while let Some(c) = self.peek()? {
                                if line_terminator(c) {
                                    break;
                                }
                                self.take()?;
                            }
                        }
                        Some('*') => {
                            self.take()?;
                            let mut star = false;
                            loop {
                                let Some(c) = self.peek()? else {
                                    return Err(
                                        self.syntax(Json5SyntaxKind::UnterminatedComment, start)
                                    );
                                };
                                self.take()?;
                                if star && c == '/' {
                                    break;
                                }
                                star = c == '*';
                            }
                        }
                        _ => return Err(self.syntax(Json5SyntaxKind::UnexpectedToken, start)),
                    }
                }
                _ => return Ok(()),
            }
        }
    }

    fn token_end(&mut self) -> Result<usize, Json5SyntaxError> {
        while let Some(c) = self.peek()? {
            if whitespace(c) || matches!(c, ',' | ']' | '}' | '/') {
                break;
            }
            self.take()?;
        }
        Ok(self.index)
    }

    fn peek(&mut self) -> Result<Option<char>, Json5SyntaxError> {
        // Every lookahead, including repeat/EOF checks, is charged.
        let c = self.source[self.index..].chars().next();
        self.charge(c.map_or(1, char::len_utf8))?;
        Ok(c)
    }

    fn take(&mut self) -> Result<char, Json5SyntaxError> {
        let c = self
            .peek()?
            .ok_or_else(|| self.syntax(Json5SyntaxKind::UnexpectedToken, self.index))?;
        self.charge(c.len_utf8())?;
        self.index += c.len_utf8();
        Ok(c)
    }

    fn append(&mut self, text: &str) -> Result<(), Json5SyntaxError> {
        let Some(requested) = self.output.len().checked_add(text.len()) else {
            return Err(self.limit(
                Json5SyntaxResource::NormalizedDocumentBytes,
                usize::MAX,
                self.limits.normalized,
            ));
        };
        if requested > self.limits.normalized {
            return Err(self.limit(
                Json5SyntaxResource::NormalizedDocumentBytes,
                requested,
                self.limits.normalized,
            ));
        }
        self.charge(text.len())?;
        self.output.push_str(text);
        Ok(())
    }

    fn charge(&mut self, amount: usize) -> Result<(), Json5SyntaxError> {
        let Some(requested) = self.work.checked_add(amount) else {
            return Err(self.limit(
                Json5SyntaxResource::SyntaxWork,
                usize::MAX,
                self.limits.work,
            ));
        };
        if requested > self.limits.work {
            return Err(self.limit(Json5SyntaxResource::SyntaxWork, requested, self.limits.work));
        }
        self.work = requested;
        Ok(())
    }

    fn syntax(&self, kind: Json5SyntaxKind, offset: usize) -> Json5SyntaxError {
        Json5SyntaxError::Syntax { kind, offset }
    }

    fn limit(
        &self,
        resource: Json5SyntaxResource,
        requested: usize,
        max: usize,
    ) -> Json5SyntaxError {
        Json5SyntaxError::Limit {
            resource,
            offset: self.index,
            requested,
            max,
        }
    }
}

fn line_terminator(c: char) -> bool {
    matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}')
}

fn whitespace(c: char) -> bool {
    matches!(c, '\u{0009}'..='\u{000d}' | ' ' | '\u{00a0}' | '\u{1680}' |
        '\u{2000}'..='\u{200a}' | '\u{2028}' | '\u{2029}' | '\u{202f}' |
        '\u{205f}' | '\u{3000}' | '\u{feff}')
}
