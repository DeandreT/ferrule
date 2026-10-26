use super::{Separators, decode_release, repetition_separator, split_unescaped};
use crate::segments::Segment;
use crate::{EdiFormatError, MAX_RUNTIME_INPUT_BYTES};

pub(super) struct Tokenized {
    pub segments: Vec<Segment>,
    pub component_joins: Vec<char>,
    pub first_component: char,
}

/// Each ISA starts a new syntax scope. Body segments never inherit a preceding
/// interchange's delimiters, including when a lenient schema omits segments.
pub(super) fn tokenize(
    text: &str,
    configured: Option<Separators>,
) -> Result<Tokenized, EdiFormatError> {
    if text.len() > MAX_RUNTIME_INPUT_BYTES {
        return Err(EdiFormatError::NotX12("input exceeds the 64 MiB limit"));
    }
    let mut remaining = text.trim_start();
    let mut discovery_work = text.len().saturating_mul(2);
    let mut syntax = discover(remaining, configured, &mut discovery_work)?;
    let mut result = Tokenized {
        segments: Vec::new(),
        component_joins: Vec::new(),
        first_component: syntax.component,
    };
    while !remaining.is_empty() {
        if !result.segments.is_empty()
            && remaining.starts_with("ISA")
            && remaining
                .chars()
                .nth(3)
                .is_none_or(|value| !value.is_alphanumeric())
        {
            syntax = discover(remaining, configured, &mut discovery_work)?;
        }
        let (raw, rest) = take_segment(remaining, syntax.segment, syntax.release)?;
        remaining = rest.trim_start();
        let raw = raw.trim_start();
        if raw.is_empty() {
            continue;
        }
        let mut parts = split_unescaped(raw, syntax.element, syntax.release)?.into_iter();
        let id = parts.next().unwrap_or_default().to_string();
        let is_isa = id == "ISA";
        let elements = parts
            .map(|element| {
                // ISA11 and ISA16 declare delimiters rather than repetitions or
                // components. Preserve their complete scalar values.
                if is_isa {
                    return Ok(vec![vec![element.to_string()]]);
                }
                let repeats = match syntax.repetition {
                    Some(repetition) => split_unescaped(element, repetition, syntax.release)?,
                    None => vec![element],
                };
                repeats
                    .into_iter()
                    .map(|repeat| {
                        split_unescaped(repeat, syntax.component, syntax.release)?
                            .into_iter()
                            .map(|part| decode_release(part, syntax.release))
                            .collect::<Result<Vec<_>, _>>()
                    })
                    .collect::<Result<Vec<_>, _>>()
            })
            .collect::<Result<Vec<_>, EdiFormatError>>()?;
        result.segments.push(Segment { id, elements });
        result.component_joins.push(syntax.component);
    }
    Ok(result)
}

fn take_segment(
    text: &str,
    terminator: char,
    release: Option<char>,
) -> Result<(&str, &str), EdiFormatError> {
    let mut characters = text.char_indices();
    while let Some((index, character)) = characters.next() {
        if Some(character) == release {
            if characters.next().is_none() {
                return Err(EdiFormatError::NotX12(
                    "dangling release character at end of interchange",
                ));
            }
        } else if character == terminator {
            return Ok((&text[..index], &text[index + terminator.len_utf8()..]));
        }
    }
    Ok((text, ""))
}

fn discover(
    text: &str,
    configured: Option<Separators>,
    work: &mut usize,
) -> Result<Separators, EdiFormatError> {
    if !text.starts_with("ISA") {
        return Err(EdiFormatError::NotX12("interchange must start with ISA"));
    }
    let element = text
        .chars()
        .nth(3)
        .ok_or(EdiFormatError::NotX12("truncated ISA segment"))?;
    let mut positions = Vec::with_capacity(16);
    for (index, character) in text.char_indices() {
        *work = work
            .checked_sub(character.len_utf8())
            .ok_or(EdiFormatError::NotX12(
                "ISA syntax discovery work limit exceeded",
            ))?;
        if character == element {
            positions.push(index);
            if positions.len() == 16 {
                break;
            }
        }
    }
    if positions.len() < 16 {
        return Err(EdiFormatError::NotX12("ISA has fewer than 16 elements"));
    }
    let isa11 = &text[positions[10] + element.len_utf8()..positions[11]];
    let isa12 = &text[positions[11] + element.len_utf8()..positions[12]];
    let repetition = repetition_separator(isa11, isa12)?;
    let mut end = text[positions[15] + element.len_utf8()..].chars();
    let component = end
        .next()
        .ok_or(EdiFormatError::NotX12("truncated ISA segment"))?;
    let segment = end
        .next()
        .ok_or(EdiFormatError::NotX12("missing segment terminator"))?;
    if let Some(expected) = configured {
        expected.validate()?;
        for (kind, expected, found) in [
            ("element", expected.element, element),
            ("component", expected.component, component),
            ("segment", expected.segment, segment),
        ] {
            if expected != found {
                return Err(EdiFormatError::X12SeparatorMismatch {
                    kind,
                    expected,
                    found,
                });
            }
        }
        if let (Some(expected), Some(found)) = (expected.repetition, repetition)
            && expected != found
        {
            return Err(EdiFormatError::X12SeparatorMismatch {
                kind: "repetition",
                expected,
                found,
            });
        }
    }
    Ok(Separators {
        element,
        component,
        segment,
        repetition,
        release: configured.and_then(|value| value.release),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_discovery_consumes_one_shared_work_budget() {
        let header = "ISA*00*          *00*          *ZZ*SENDER         *ZZ*RECEIVER       *261025*1200*^*00501*000000001*0*T*:~";
        let mut work = header.len();
        assert!(discover(header, None, &mut work).is_ok());
        assert!(matches!(
            discover(header, None, &mut work),
            Err(EdiFormatError::NotX12(
                "ISA syntax discovery work limit exceeded"
            ))
        ));
    }
}
