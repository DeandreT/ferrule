use std::path::{Path, PathBuf};

use super::lexer::{Token, ident, lex, matching};

pub(super) fn source_path(mapping_path: &Path, library: &str) -> Option<PathBuf> {
    let segments = library.split('.').collect::<Vec<_>>();
    let (class, package) = segments.split_last()?;
    if !safe_identifier(class) || package.iter().any(|segment| !safe_identifier(segment)) {
        return None;
    }
    let mut path = mapping_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .to_path_buf();
    for segment in package {
        path.push(segment);
    }
    path.push(format!("{class}.java"));
    Some(path)
}

pub(super) fn parse(source: &str, method: &str) -> Result<String, String> {
    let tokens = lex(source, false)?;
    let candidates = method_bodies(&tokens, method);
    let [(parameters, body)] = candidates.as_slice() else {
        return Err(format!(
            "Java source must contain exactly one method named `{method}`"
        ));
    };
    let parameter = one_parameter(parameters)?;
    let statements = split_statements(body)?;
    let [declaration, returned] = statements.as_slice() else {
        return Err(
            "Java formatter must contain one DecimalFormat declaration and one return".to_string(),
        );
    };
    let (formatter, picture) = parse_declaration(declaration)?;
    parse_return(returned, parameter, formatter)?;
    validate_picture(picture)
}

fn method_bodies<'a>(tokens: &'a [Token], method: &str) -> Vec<(&'a [Token], &'a [Token])> {
    tokens
        .iter()
        .enumerate()
        .filter(|(_, token)| ident(Some(token)) == Some(method))
        .filter_map(|(index, _)| {
            let open = index + 1;
            let close = matching(tokens, open, '(', ')')?;
            let body_open = close + 1;
            let body_close = matching(tokens, body_open, '{', '}')?;
            Some((&tokens[open + 1..close], &tokens[body_open + 1..body_close]))
        })
        .collect()
}

fn one_parameter(parameters: &[Token]) -> Result<&str, String> {
    if parameters.is_empty() || parameters.contains(&Token::Symbol(',')) {
        return Err("Java formatter must declare exactly one parameter".to_string());
    }
    let name = parameters
        .iter()
        .rev()
        .find_map(|token| ident(Some(token)))
        .ok_or_else(|| "Java formatter parameter has no name".to_string())?;
    if !parameters.iter().any(|token| {
        matches!(
            ident(Some(token)),
            Some("BigDecimal" | "double" | "float" | "int" | "long")
        )
    }) {
        return Err("Java formatter parameter is not a supported numeric type".to_string());
    }
    Ok(name)
}

fn split_statements(body: &[Token]) -> Result<Vec<&[Token]>, String> {
    let mut statements = Vec::new();
    let mut start = 0usize;
    let mut depth = 0usize;
    for (index, token) in body.iter().enumerate() {
        match token {
            Token::Symbol('(') => depth += 1,
            Token::Symbol(')') => {
                depth = depth
                    .checked_sub(1)
                    .ok_or_else(|| "Java formatter has unbalanced parentheses".to_string())?;
            }
            Token::Symbol(';') if depth == 0 => {
                if index == start {
                    return Err("Java formatter contains an empty statement".to_string());
                }
                statements.push(&body[start..index]);
                start = index + 1;
            }
            _ => {}
        }
    }
    if depth != 0 || start != body.len() {
        return Err("Java formatter has an incomplete statement".to_string());
    }
    Ok(statements)
}

fn parse_declaration(statement: &[Token]) -> Result<(&str, &str), String> {
    let equals = statement
        .iter()
        .position(|token| token == &Token::Symbol('='))
        .ok_or_else(|| "Java DecimalFormat declaration has no assignment".to_string())?;
    if statement[equals + 1..]
        .iter()
        .any(|token| token == &Token::Symbol('='))
    {
        return Err("Java DecimalFormat declaration has multiple assignments".to_string());
    }
    if equals < 2 {
        return Err("Java DecimalFormat declaration has no type and variable".to_string());
    }
    let formatter = statement[..equals]
        .iter()
        .rev()
        .find_map(|token| ident(Some(token)))
        .ok_or_else(|| "Java DecimalFormat declaration has no variable".to_string())?;
    if !qualified_name_ends_with(&statement[..equals - 1], "NumberFormat") {
        return Err("Java formatter variable is not declared as NumberFormat".to_string());
    }
    let rhs = &statement[equals + 1..];
    let [
        Token::Ident(new),
        middle @ ..,
        Token::Symbol('('),
        Token::String(picture),
        Token::Symbol(')'),
    ] = rhs
    else {
        return Err("Java formatter must construct DecimalFormat from one picture".to_string());
    };
    if new != "new" || !qualified_name_ends_with(middle, "DecimalFormat") {
        return Err("Java formatter must construct DecimalFormat".to_string());
    }
    Ok((formatter, picture))
}

fn parse_return(statement: &[Token], parameter: &str, formatter: &str) -> Result<(), String> {
    let [
        Token::Ident(return_keyword),
        Token::Ident(return_formatter),
        Token::Symbol('.'),
        Token::Ident(format),
        Token::Symbol('('),
        Token::Ident(value),
        Token::Symbol('.'),
        Token::Ident(double_value),
        Token::Symbol('('),
        Token::Symbol(')'),
        Token::Symbol(')'),
    ] = statement
    else {
        return Err("Java formatter return expression is unsupported".to_string());
    };
    if return_keyword != "return"
        || return_formatter != formatter
        || format != "format"
        || value != parameter
        || double_value != "doubleValue"
    {
        return Err("Java formatter return does not use its numeric parameter".to_string());
    }
    Ok(())
}

fn qualified_name_ends_with(tokens: &[Token], expected: &str) -> bool {
    if ident(tokens.last()) != Some(expected) {
        return false;
    }
    tokens.iter().enumerate().all(|(index, token)| {
        if index % 2 == 0 {
            matches!(token, Token::Ident(_))
        } else {
            token == &Token::Symbol('.')
        }
    })
}

fn validate_picture(picture: &str) -> Result<String, String> {
    if picture.is_empty() || picture.len() > 1024 {
        return Err("numeric picture must contain 1..=1024 bytes".to_string());
    }
    Ok(picture.to_string())
}

fn safe_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 255
        && value
            .chars()
            .all(|character| character.is_alphanumeric() || character == '_')
}

#[cfg(test)]
mod tests {
    use super::parse;

    #[test]
    fn parses_decimal_format_wrapper() {
        let source = r##"
            public class PriceText {
                public static String Render(java.math.BigDecimal amount) {
                    java.text.NumberFormat output = new java.text.DecimalFormat("#,##0.00");
                    return output.format(amount.doubleValue());
                }
            }
        "##;
        assert_eq!(parse(source, "Render"), Ok("#,##0.00".to_string()));
        assert!(parse(source.replace("doubleValue", "intValue").as_str(), "Render").is_err());
    }
}
