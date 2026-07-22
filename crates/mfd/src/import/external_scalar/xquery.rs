use std::path::{Path, PathBuf};

use ir::Value;

use super::Expr;
use super::lexer::{Token, ident, lex, matching};

const MAX_EXPRESSION_NODES: usize = 256;

pub(super) fn source_path(mapping_path: &Path, library: &str) -> Option<PathBuf> {
    if !safe_module_name(library) {
        return None;
    }
    let parent = mapping_path.parent().unwrap_or_else(|| Path::new("."));
    ["xq", "xquery"]
        .into_iter()
        .map(|extension| parent.join(format!("{library}.{extension}")))
        .find(|path| path.is_file())
}

pub(super) fn parse(source: &str, function_name: &str, input_count: usize) -> Result<Expr, String> {
    let tokens = lex(source, true)?;
    let declarations = declarations(&tokens, function_name);
    let [(parameters, body)] = declarations.as_slice() else {
        return Err(format!(
            "XQuery module must contain exactly one function named `{function_name}`"
        ));
    };
    let parameters = parse_parameters(parameters)?;
    if parameters.len() != input_count {
        return Err(format!(
            "XQuery function declares {} parameters but the component has {input_count} inputs",
            parameters.len()
        ));
    }
    Parser {
        tokens: body,
        position: 0,
        parameters: &parameters,
        nodes: 0,
    }
    .parse()
}

fn declarations<'a>(tokens: &'a [Token], function_name: &str) -> Vec<(&'a [Token], &'a [Token])> {
    tokens
        .windows(3)
        .enumerate()
        .filter(|(_, window)| {
            ident(window.first()) == Some("declare")
                && ident(window.get(1)) == Some("function")
                && ident(window.get(2)) == Some(function_name)
        })
        .filter_map(|(index, _)| {
            let open = index + 3;
            let close = matching(tokens, open, '(', ')')?;
            let body_open = tokens[close + 1..]
                .iter()
                .position(|token| token == &Token::Symbol('{'))?
                + close
                + 1;
            let body_close = matching(tokens, body_open, '{', '}')?;
            Some((&tokens[open + 1..close], &tokens[body_open + 1..body_close]))
        })
        .collect()
}

fn parse_parameters(tokens: &[Token]) -> Result<Vec<String>, String> {
    if tokens.is_empty() {
        return Ok(Vec::new());
    }
    let mut parameters = Vec::new();
    for parameter in tokens.split(|token| token == &Token::Symbol(',')) {
        let dollars = parameter
            .iter()
            .enumerate()
            .filter(|(_, token)| token == &&Token::Symbol('$'))
            .collect::<Vec<_>>();
        let [(index, _)] = dollars.as_slice() else {
            return Err("each XQuery parameter must contain exactly one `$` name".to_string());
        };
        let name = ident(parameter.get(index + 1))
            .filter(|name| safe_identifier(name))
            .ok_or_else(|| "XQuery parameter name is invalid".to_string())?;
        if parameters.iter().any(|parameter| parameter == name) {
            return Err(format!("XQuery parameter `${name}` is duplicated"));
        }
        parameters.push(name.to_string());
    }
    Ok(parameters)
}

struct Parser<'a> {
    tokens: &'a [Token],
    position: usize,
    parameters: &'a [String],
    nodes: usize,
}

impl Parser<'_> {
    fn parse(mut self) -> Result<Expr, String> {
        let expression = self.additive()?;
        if self.position != self.tokens.len() {
            return Err("XQuery function body contains unsupported trailing syntax".to_string());
        }
        Ok(expression)
    }

    fn additive(&mut self) -> Result<Expr, String> {
        let mut expression = self.multiplicative()?;
        loop {
            let function = if self.consume_symbol('+') {
                "add"
            } else if self.consume_symbol('-') {
                "subtract"
            } else {
                break;
            };
            let right = self.multiplicative()?;
            expression = self.call(function, expression, right)?;
        }
        Ok(expression)
    }

    fn multiplicative(&mut self) -> Result<Expr, String> {
        let mut expression = self.primary()?;
        loop {
            let function = if self.consume_symbol('*') {
                "multiply"
            } else if self.consume_ident("div") {
                "divide"
            } else {
                break;
            };
            let right = self.primary()?;
            expression = self.call(function, expression, right)?;
        }
        Ok(expression)
    }

    fn primary(&mut self) -> Result<Expr, String> {
        if self.consume_symbol('(') {
            let expression = self.additive()?;
            if !self.consume_symbol(')') {
                return Err("XQuery arithmetic expression has an unclosed group".to_string());
            }
            return Ok(expression);
        }
        if self.consume_symbol('-') {
            let value = self.primary()?;
            return self.call("subtract", Expr::Const(Value::Int(0)), value);
        }
        if self.consume_symbol('$') {
            let name = ident(self.tokens.get(self.position))
                .ok_or_else(|| "XQuery `$` reference has no parameter name".to_string())?;
            self.position += 1;
            let index = self
                .parameters
                .iter()
                .position(|parameter| parameter == name)
                .ok_or_else(|| format!("XQuery expression references unknown `${name}`"))?;
            self.note_node()?;
            return Ok(Expr::Input(index));
        }
        let Some(Token::Number(number)) = self.tokens.get(self.position) else {
            return Err("XQuery arithmetic expression expected a number or parameter".to_string());
        };
        self.position += 1;
        let value = if number.contains(['.', 'e', 'E']) {
            let value = number
                .parse::<f64>()
                .ok()
                .filter(|value| value.is_finite())
                .ok_or_else(|| format!("XQuery numeric literal `{number}` is invalid"))?;
            Value::Float(value)
        } else {
            Value::Int(
                number
                    .parse::<i64>()
                    .map_err(|_| format!("XQuery integer literal `{number}` is out of range"))?,
            )
        };
        self.note_node()?;
        Ok(Expr::Const(value))
    }

    fn call(&mut self, function: &str, left: Expr, right: Expr) -> Result<Expr, String> {
        self.note_node()?;
        Ok(Expr::Call {
            function: function.to_string(),
            args: vec![left, right],
        })
    }

    fn note_node(&mut self) -> Result<(), String> {
        self.nodes += 1;
        if self.nodes > MAX_EXPRESSION_NODES {
            return Err(format!(
                "XQuery expression exceeds {MAX_EXPRESSION_NODES} nodes"
            ));
        }
        Ok(())
    }

    fn consume_symbol(&mut self, expected: char) -> bool {
        if self.tokens.get(self.position) == Some(&Token::Symbol(expected)) {
            self.position += 1;
            true
        } else {
            false
        }
    }

    fn consume_ident(&mut self, expected: &str) -> bool {
        if ident(self.tokens.get(self.position)) == Some(expected) {
            self.position += 1;
            true
        } else {
            false
        }
    }
}

fn safe_module_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 255
        && value != "."
        && value != ".."
        && value
            .chars()
            .all(|character| character.is_alphanumeric() || matches!(character, '_' | '-'))
}

fn safe_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 1024
        && value
            .chars()
            .all(|character| character.is_alphanumeric() || matches!(character, '_' | '-' | ':'))
}

#[cfg(test)]
mod tests {
    use ir::Value;

    use super::{Expr, parse};

    #[test]
    fn parses_typed_parameter_arithmetic() {
        let source = r#"
            xquery version "1.0";
            module namespace fee="urn:test";
            declare function fee:calculate($amount as xs:decimal) as xs:decimal {
                ($amount * 0.15) + 2
            };
        "#;
        let parsed = parse(source, "fee:calculate", 1);
        assert!(matches!(
            parsed,
            Ok(Expr::Call { function, args })
                if function == "add"
                    && args.len() == 2
                    && matches!(args.get(1), Some(Expr::Const(Value::Int(2))))
        ));
        assert!(
            parse(
                source.replace("$amount *", "$missing *").as_str(),
                "fee:calculate",
                1
            )
            .is_err()
        );
    }
}
