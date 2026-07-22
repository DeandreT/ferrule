use std::path::{Path, PathBuf};

use super::lexer::{Token, ident, lex, matching};

pub(super) fn source_path(
    mapping_path: &Path,
    library: &str,
    component_name: &str,
) -> Option<PathBuf> {
    let assembly = library.split(',').next()?.trim();
    let (owner, _) = component_name.rsplit_once('.')?;
    let class = owner.rsplit('.').next()?;
    if !safe_segment(assembly) || !safe_segment(class) {
        return None;
    }
    Some(
        mapping_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(assembly)
            .join(format!("{class}.cs")),
    )
}

pub(super) fn parse(source: &str, method: &str) -> Result<String, String> {
    let tokens = lex(source, false)?;
    let candidates = method_bodies(&tokens, method);
    let [(parameters, body)] = candidates.as_slice() else {
        return Err(format!(
            "C# source must contain exactly one method named `{method}`"
        ));
    };
    let parameter = one_parameter(parameters)?;
    parse_return(body, parameter)
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
        return Err("C# formatter must declare exactly one parameter".to_string());
    }
    let name = parameters
        .iter()
        .rev()
        .find_map(|token| ident(Some(token)))
        .ok_or_else(|| "C# formatter parameter has no name".to_string())?;
    let numeric = parameters.iter().any(|token| {
        matches!(
            ident(Some(token)),
            Some("decimal" | "double" | "float" | "int" | "long")
        )
    });
    if !numeric {
        return Err("C# formatter parameter is not a supported numeric type".to_string());
    }
    Ok(name)
}

fn parse_return(body: &[Token], parameter: &str) -> Result<String, String> {
    let [
        Token::Ident(return_keyword),
        Token::Ident(value),
        Token::Symbol('.'),
        Token::Ident(to_string),
        Token::Symbol('('),
        Token::String(picture),
        Token::Symbol(','),
        culture @ ..,
        Token::Symbol(')'),
        Token::Symbol(';'),
    ] = body
    else {
        return Err(
            "C# formatter body must directly return numeric.ToString(picture, invariant culture)"
                .to_string(),
        );
    };
    if return_keyword != "return" || value != parameter || to_string != "ToString" {
        return Err("C# formatter return does not use its numeric parameter".to_string());
    }
    if !qualified_name(culture)
        || culture
            .iter()
            .any(|token| !matches!(token, Token::Ident(_) | Token::Symbol('.')))
    {
        return Err("C# formatter must use CultureInfo.InvariantCulture".to_string());
    }
    let culture_names = culture
        .iter()
        .filter_map(|token| ident(Some(token)))
        .collect::<Vec<_>>();
    if culture_names.last().copied() != Some("InvariantCulture")
        || !culture_names.contains(&"CultureInfo")
    {
        return Err("C# formatter must use CultureInfo.InvariantCulture".to_string());
    }
    validate_picture(picture)
}

fn qualified_name(tokens: &[Token]) -> bool {
    !tokens.is_empty()
        && tokens.len() % 2 == 1
        && tokens.iter().enumerate().all(|(index, token)| {
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

fn safe_segment(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 255
        && value != "."
        && value != ".."
        && value
            .chars()
            .all(|character| character.is_alphanumeric() || matches!(character, '_' | '-'))
}

#[cfg(test)]
mod tests {
    use super::parse;

    #[test]
    fn parses_invariant_numeric_formatter() {
        let source = r#"
            public class Money {
                public static string Render(decimal amount) {
                    return amount.ToString("0000.0", CultureInfo.InvariantCulture);
                }
            }
        "#;
        assert_eq!(parse(source, "Render"), Ok("0000.0".to_string()));
        assert!(
            parse(
                source
                    .replace("InvariantCulture", "CurrentCulture")
                    .as_str(),
                "Render"
            )
            .is_err()
        );
    }
}
