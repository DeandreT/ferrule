use ir::Value;

use super::{
    ParsedOperand, ParsedPredicate, ParsedQuery, QueryCardinality, QueryOperator, QueryOrder,
    QueryProjection, ensure_unique_names,
};

#[derive(Clone, Debug, PartialEq)]
enum Token {
    Word(String),
    String(String),
    Number(String),
    Parameter(String),
    Star,
    Comma,
    Dot,
    LeftParen,
    RightParen,
    Equal,
    Greater,
    Semicolon,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ColumnRef {
    pub(super) table: Option<String>,
    pub(super) column: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum JoinedProjectionExpr {
    Column(ColumnRef),
    Multiply(ColumnRef, ColumnRef),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct JoinedProjection {
    pub(super) output: String,
    pub(super) expr: JoinedProjectionExpr,
}

pub(super) struct JoinedQuery {
    pub(super) primary_table: String,
    pub(super) joined_table: String,
    pub(super) join_left: ColumnRef,
    pub(super) join_right: ColumnRef,
    pub(super) projections: Vec<JoinedProjection>,
    pub(super) predicate_column: ColumnRef,
    pub(super) predicate_operand: ParsedOperand,
}

pub(super) struct Parser {
    tokens: Vec<Token>,
    position: usize,
}

impl Parser {
    pub(super) fn new(sql: &str) -> Result<Self, String> {
        Ok(Self {
            tokens: tokenize(sql)?,
            position: 0,
        })
    }

    pub(super) fn parse(mut self) -> Result<ParsedQuery, String> {
        self.keyword("SELECT")?;
        let projection = if self.take(&Token::Star) {
            QueryProjection::All
        } else {
            let mut columns = vec![self.identifier()?];
            while self.take(&Token::Comma) {
                columns.push(self.identifier()?);
            }
            ensure_unique_names("SQL projection", &columns)?;
            QueryProjection::Columns(columns)
        };
        self.keyword("FROM")?;
        let table = self.identifier()?;
        let mut predicates = Vec::new();
        if self.take_keyword("WHERE") {
            loop {
                let column = self.identifier()?;
                let operator = if self.take(&Token::Equal) {
                    QueryOperator::Equal
                } else if self.take_keyword("LIKE") {
                    QueryOperator::Like
                } else {
                    return Err("query predicates must use `=` or `LIKE`".to_string());
                };
                let operand = match self.next() {
                    Some(Token::Parameter(name)) => ParsedOperand::Parameter(name),
                    Some(Token::String(value)) => ParsedOperand::Literal(Value::String(value)),
                    Some(Token::Number(value)) => ParsedOperand::Literal(parse_number(&value)?),
                    _ => {
                        return Err(
                            "query predicate operands must be named parameters or literals"
                                .to_string(),
                        );
                    }
                };
                predicates.push(ParsedPredicate {
                    column,
                    operator,
                    operand,
                });
                if !self.take_keyword("AND") {
                    break;
                }
            }
        }
        let order = if self.take_keyword("ORDER") {
            self.keyword("BY")?;
            let column = self.identifier()?;
            let descending = if self.take_keyword("DESC") {
                true
            } else {
                self.take_keyword("ASC");
                false
            };
            Some(QueryOrder { column, descending })
        } else {
            None
        };
        let cardinality = if self.take_keyword("LIMIT") {
            match self.next() {
                Some(Token::Number(limit)) if limit == "1" => QueryCardinality::AtMostOne,
                _ => return Err("only the literal SQL clause `LIMIT 1` is supported".to_string()),
            }
        } else {
            QueryCardinality::Many
        };
        if self.take_keyword("OFFSET") {
            return Err("SQL OFFSET is not supported".to_string());
        }
        self.take(&Token::Semicolon);
        if self.position != self.tokens.len() {
            return Err(
                "only a single-table SELECT with conjunction predicates is supported".to_string(),
            );
        }
        Ok(ParsedQuery {
            table,
            projection,
            predicates,
            order,
            cardinality,
        })
    }

    /// Parses the bounded relational SELECT shape that can be lowered to the
    /// existing nested SQLite reader: one inner many-to-one foreign-key join,
    /// direct columns or a two-factor multiplication, and one `>` predicate.
    pub(super) fn parse_joined(mut self) -> Result<JoinedQuery, String> {
        self.keyword("SELECT")?;
        let mut projections = vec![self.joined_projection()?];
        while self.take(&Token::Comma) {
            projections.push(self.joined_projection()?);
        }
        ensure_unique_names(
            "SQL projection",
            &projections
                .iter()
                .map(|projection| projection.output.clone())
                .collect::<Vec<_>>(),
        )?;

        self.keyword("FROM")?;
        let primary_table = self.identifier()?;
        self.take_keyword("INNER");
        self.keyword("JOIN")?;
        let joined_table = self.identifier()?;
        self.keyword("ON")?;
        let join_left = self.column_ref()?;
        if !self.take(&Token::Equal) {
            return Err("joined query must use one equality join".to_string());
        }
        let join_right = self.column_ref()?;

        self.keyword("WHERE")?;
        let column = self.column_ref()?;
        if !self.take(&Token::Greater) {
            return Err("joined query predicate must use `>`".to_string());
        }
        let operand = match self.next() {
            Some(Token::Parameter(name)) => ParsedOperand::Parameter(name),
            Some(Token::Number(value)) => ParsedOperand::Literal(parse_number(&value)?),
            _ => {
                return Err(
                    "joined query predicate operand must be a named parameter or numeric literal"
                        .to_string(),
                );
            }
        };
        self.take(&Token::Semicolon);
        if self.position != self.tokens.len() {
            return Err(
                "joined query supports one inner join and one greater-than predicate".to_string(),
            );
        }
        Ok(JoinedQuery {
            primary_table,
            joined_table,
            join_left,
            join_right,
            projections,
            predicate_column: column,
            predicate_operand: operand,
        })
    }

    fn joined_projection(&mut self) -> Result<JoinedProjection, String> {
        let parenthesized = self.take(&Token::LeftParen);
        let first = self.column_ref()?;
        let expr = if self.take(&Token::Star) {
            let second = self.column_ref()?;
            JoinedProjectionExpr::Multiply(first, second)
        } else {
            JoinedProjectionExpr::Column(first.clone())
        };
        if parenthesized && !self.take(&Token::RightParen) {
            return Err("expected `)` after joined query projection".to_string());
        }
        if !parenthesized && matches!(expr, JoinedProjectionExpr::Multiply(_, _)) {
            return Err("computed joined projection must be parenthesized".to_string());
        }
        let output = if self.take_keyword("AS") {
            self.identifier()?
        } else {
            match &expr {
                JoinedProjectionExpr::Column(column) => column.column.clone(),
                JoinedProjectionExpr::Multiply(_, _) => {
                    return Err("computed joined projection requires an alias".to_string());
                }
            }
        };
        Ok(JoinedProjection { output, expr })
    }

    fn column_ref(&mut self) -> Result<ColumnRef, String> {
        let first = self.identifier()?;
        if self.take(&Token::Dot) {
            Ok(ColumnRef {
                table: Some(first),
                column: self.identifier()?,
            })
        } else {
            Ok(ColumnRef {
                table: None,
                column: first,
            })
        }
    }

    fn identifier(&mut self) -> Result<String, String> {
        match self.next() {
            Some(Token::Word(word)) if valid_identifier(&word) => Ok(word),
            _ => Err("expected a simple SQL identifier".to_string()),
        }
    }

    fn keyword(&mut self, expected: &str) -> Result<(), String> {
        self.take_keyword(expected)
            .then_some(())
            .ok_or_else(|| format!("expected SQL keyword `{expected}`"))
    }

    fn take_keyword(&mut self, expected: &str) -> bool {
        let matches = self.tokens.get(self.position).is_some_and(
            |token| matches!(token, Token::Word(word) if word.eq_ignore_ascii_case(expected)),
        );
        if matches {
            self.position += 1;
        }
        matches
    }

    fn take(&mut self, expected: &Token) -> bool {
        if self.tokens.get(self.position) == Some(expected) {
            self.position += 1;
            true
        } else {
            false
        }
    }

    fn next(&mut self) -> Option<Token> {
        let token = self.tokens.get(self.position).cloned();
        self.position += usize::from(token.is_some());
        token
    }
}

fn tokenize(sql: &str) -> Result<Vec<Token>, String> {
    let chars = sql.chars().collect::<Vec<_>>();
    let mut tokens = Vec::new();
    let mut index = 0;
    while index < chars.len() {
        match chars[index] {
            character if character.is_ascii_whitespace() => index += 1,
            ',' => {
                tokens.push(Token::Comma);
                index += 1;
            }
            '.' => {
                tokens.push(Token::Dot);
                index += 1;
            }
            '(' => {
                tokens.push(Token::LeftParen);
                index += 1;
            }
            ')' => {
                tokens.push(Token::RightParen);
                index += 1;
            }
            '*' => {
                tokens.push(Token::Star);
                index += 1;
            }
            '=' => {
                tokens.push(Token::Equal);
                index += 1;
            }
            '>' => {
                tokens.push(Token::Greater);
                index += 1;
            }
            ';' => {
                tokens.push(Token::Semicolon);
                index += 1;
            }
            '"' => {
                let (value, next) = quoted(&chars, index + 1, '"')?;
                tokens.push(Token::Word(value));
                index = next;
            }
            '\'' => {
                let (value, next) = quoted(&chars, index + 1, '\'')?;
                tokens.push(Token::String(value));
                index = next;
            }
            ':' => {
                let (value, next) = bare(&chars, index + 1);
                if !valid_identifier(&value) {
                    return Err("query contains an invalid named parameter".to_string());
                }
                tokens.push(Token::Parameter(value));
                index = next;
            }
            character if character.is_ascii_digit() || matches!(character, '+' | '-') => {
                let start = index;
                index += 1;
                while index < chars.len()
                    && (chars[index].is_ascii_digit()
                        || matches!(chars[index], '.' | 'e' | 'E' | '+' | '-'))
                {
                    index += 1;
                }
                tokens.push(Token::Number(chars[start..index].iter().collect()));
            }
            character if character == '_' || character.is_ascii_alphabetic() => {
                let (value, next) = bare(&chars, index);
                tokens.push(Token::Word(value));
                index = next;
            }
            character => {
                return Err(format!(
                    "unsupported SQL token `{character}`; joins, expressions, and comments are not accepted"
                ));
            }
        }
    }
    Ok(tokens)
}

fn quoted(chars: &[char], mut index: usize, quote: char) -> Result<(String, usize), String> {
    let mut value = String::new();
    while index < chars.len() {
        if chars[index] == quote {
            if chars.get(index + 1) == Some(&quote) {
                value.push(quote);
                index += 2;
            } else {
                return Ok((value, index + 1));
            }
        } else {
            value.push(chars[index]);
            index += 1;
        }
    }
    Err("unterminated quoted SQL value".to_string())
}

fn bare(chars: &[char], mut index: usize) -> (String, usize) {
    let start = index;
    while index < chars.len() && (chars[index] == '_' || chars[index].is_ascii_alphanumeric()) {
        index += 1;
    }
    (chars[start..index].iter().collect(), index)
}

pub(super) fn valid_identifier(identifier: &str) -> bool {
    let mut bytes = identifier.bytes();
    bytes
        .next()
        .is_some_and(|byte| byte == b'_' || byte.is_ascii_alphabetic())
        && bytes.all(|byte| byte == b'_' || byte.is_ascii_alphanumeric())
}

fn parse_number(number: &str) -> Result<Value, String> {
    number
        .parse::<i64>()
        .map(Value::Int)
        .or_else(|_| number.parse::<f64>().map(Value::Float))
        .map_err(|_| format!("invalid numeric SQL literal `{number}`"))
}
