#[derive(Clone, Debug, PartialEq)]
pub(super) enum Token {
    Ident(String),
    String(String),
    Number(String),
    Symbol(char),
}

const MAX_TOKENS: usize = 100_000;
const MAX_TOKEN_BYTES: usize = 4096;

pub(super) fn lex(source: &str, xquery_comments: bool) -> Result<Vec<Token>, String> {
    let mut lexer = Lexer {
        source,
        position: 0,
        xquery_comments,
        tokens: Vec::new(),
    };
    lexer.run()?;
    Ok(lexer.tokens)
}

struct Lexer<'a> {
    source: &'a str,
    position: usize,
    xquery_comments: bool,
    tokens: Vec<Token>,
}

impl Lexer<'_> {
    fn run(&mut self) -> Result<(), String> {
        while self.position < self.source.len() {
            self.skip_trivia()?;
            if self.position >= self.source.len() {
                break;
            }
            let character = self
                .current()
                .ok_or_else(|| "invalid UTF-8 boundary".to_string())?;
            let token = if character == '"' || character == '\'' {
                self.string(character)?
            } else if character.is_ascii_digit() {
                self.number()?
            } else if is_identifier_start(character) {
                self.identifier()?
            } else {
                self.position += character.len_utf8();
                Token::Symbol(character)
            };
            self.tokens.push(token);
            if self.tokens.len() > MAX_TOKENS {
                return Err(format!(
                    "source module exceeds the {MAX_TOKENS}-token limit"
                ));
            }
        }
        Ok(())
    }

    fn skip_trivia(&mut self) -> Result<(), String> {
        loop {
            while self.current().is_some_and(char::is_whitespace) {
                self.advance();
            }
            let remaining = &self.source[self.position..];
            if remaining.starts_with("//") {
                self.position += 2;
                while self.current().is_some_and(|character| character != '\n') {
                    self.advance();
                }
            } else if let Some(comment) = remaining.strip_prefix("/*") {
                let Some(end) = comment.find("*/") else {
                    return Err("source module has an unterminated block comment".to_string());
                };
                self.position += 2 + end + 2;
            } else if self.xquery_comments && remaining.starts_with("(:") {
                self.skip_xquery_comment()?;
            } else {
                return Ok(());
            }
        }
    }

    fn skip_xquery_comment(&mut self) -> Result<(), String> {
        let mut depth = 0usize;
        while self.position < self.source.len() {
            let remaining = &self.source[self.position..];
            if remaining.starts_with("(:") {
                depth = depth
                    .checked_add(1)
                    .filter(|depth| *depth <= 64)
                    .ok_or_else(|| "XQuery comments exceed nesting depth 64".to_string())?;
                self.position += 2;
            } else if remaining.starts_with(":)") {
                self.position += 2;
                depth -= 1;
                if depth == 0 {
                    return Ok(());
                }
            } else {
                self.advance();
            }
        }
        Err("source module has an unterminated XQuery comment".to_string())
    }

    fn string(&mut self, quote: char) -> Result<Token, String> {
        self.advance();
        let mut output = String::new();
        while let Some(character) = self.current() {
            self.advance();
            if character == quote {
                if self.current() == Some(quote) {
                    self.advance();
                    output.push(quote);
                    continue;
                }
                return Ok(Token::String(output));
            }
            if character == '\\' {
                let escaped = self
                    .current()
                    .ok_or_else(|| "source module has an unterminated string escape".to_string())?;
                self.advance();
                output.push(match escaped {
                    '\\' => '\\',
                    '"' => '"',
                    '\'' => '\'',
                    'n' => '\n',
                    'r' => '\r',
                    't' => '\t',
                    _ => return Err(format!("unsupported string escape `\\{escaped}`")),
                });
            } else {
                output.push(character);
            }
            if output.len() > MAX_TOKEN_BYTES {
                return Err(format!("string literal exceeds {MAX_TOKEN_BYTES} bytes"));
            }
        }
        Err("source module has an unterminated string literal".to_string())
    }

    fn identifier(&mut self) -> Result<Token, String> {
        let start = self.position;
        self.advance();
        while self.current().is_some_and(is_identifier_continue) {
            self.advance();
        }
        self.bounded_token(start, Token::Ident)
    }

    fn number(&mut self) -> Result<Token, String> {
        let start = self.position;
        while self
            .current()
            .is_some_and(|character| character.is_ascii_digit())
        {
            self.advance();
        }
        if self.current() == Some('.') {
            self.advance();
            while self
                .current()
                .is_some_and(|character| character.is_ascii_digit())
            {
                self.advance();
            }
        }
        if self
            .current()
            .is_some_and(|character| matches!(character, 'e' | 'E'))
        {
            self.advance();
            if self
                .current()
                .is_some_and(|character| matches!(character, '+' | '-'))
            {
                self.advance();
            }
            let exponent = self.position;
            while self
                .current()
                .is_some_and(|character| character.is_ascii_digit())
            {
                self.advance();
            }
            if exponent == self.position {
                return Err("numeric exponent has no digits".to_string());
            }
        }
        self.bounded_token(start, Token::Number)
    }

    fn bounded_token(
        &self,
        start: usize,
        constructor: impl FnOnce(String) -> Token,
    ) -> Result<Token, String> {
        let value = &self.source[start..self.position];
        if value.len() > MAX_TOKEN_BYTES {
            return Err(format!("source token exceeds {MAX_TOKEN_BYTES} bytes"));
        }
        Ok(constructor(value.to_string()))
    }

    fn current(&self) -> Option<char> {
        self.source[self.position..].chars().next()
    }

    fn advance(&mut self) {
        if let Some(character) = self.current() {
            self.position += character.len_utf8();
        }
    }
}

fn is_identifier_start(character: char) -> bool {
    character.is_alphabetic() || character == '_'
}

fn is_identifier_continue(character: char) -> bool {
    character.is_alphanumeric() || matches!(character, '_' | '-' | ':')
}

pub(super) fn matching(tokens: &[Token], open: usize, left: char, right: char) -> Option<usize> {
    if tokens.get(open) != Some(&Token::Symbol(left)) {
        return None;
    }
    let mut depth = 0usize;
    for (index, token) in tokens.iter().enumerate().skip(open) {
        match token {
            Token::Symbol(character) if *character == left => depth += 1,
            Token::Symbol(character) if *character == right => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(index);
                }
            }
            _ => {}
        }
    }
    None
}

pub(super) fn ident(token: Option<&Token>) -> Option<&str> {
    match token {
        Some(Token::Ident(value)) => Some(value),
        _ => None,
    }
}
