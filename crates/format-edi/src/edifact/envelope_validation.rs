//! Opt-in complete-envelope validation, independent of a projected schema.
use crate::{EdiFormatError, segments::Segment};

struct Interchange {
    reference: String,
    messages: usize,
    groups: usize,
    grouped: Option<bool>,
}
struct Group {
    reference: String,
    messages: usize,
}
struct Message {
    reference: String,
    start: usize,
}

fn invalid(segment: usize, reason: &'static str) -> EdiFormatError {
    EdiFormatError::EdifactEnvelope { segment, reason }
}

fn value(segment: &Segment, index: usize, position: usize) -> Result<&str, EdiFormatError> {
    let repeats = segment
        .elements
        .get(position)
        .ok_or_else(|| invalid(index, "missing control value"))?;
    let components = repeats
        .first()
        .filter(|_| repeats.len() == 1)
        .ok_or_else(|| invalid(index, "control value must be scalar"))?;
    components
        .first()
        .filter(|text| components.len() == 1 && !text.is_empty())
        .map(String::as_str)
        .ok_or_else(|| invalid(index, "missing or composite control value"))
}

fn trailer(
    segment: &Segment,
    index: usize,
    count: usize,
    reference: &str,
) -> Result<(), EdiFormatError> {
    let raw = value(segment, index, 0)?;
    if !raw.bytes().all(|byte| byte.is_ascii_digit()) || raw.parse::<usize>().ok() != Some(count) {
        return Err(invalid(index, "control count mismatch"));
    }
    if value(segment, index, 1)? != reference {
        return Err(invalid(index, "control reference mismatch"));
    }
    Ok(())
}

/// Validate UNB/UNZ, optional UNG/UNE, and UNH/UNT ownership, counts and
/// references. Complete standalone UNH messages are also accepted. This is
/// opt-in: ordinary readers retain their existing schema-guided behavior.
pub fn validate_envelope(text: &str) -> Result<(), EdiFormatError> {
    let segments = super::tokenize(text)?;
    let standalone = segments.first().is_some_and(|segment| segment.id == "UNH");
    let mut interchange: Option<Interchange> = None;
    let mut group: Option<Group> = None;
    let mut message: Option<Message> = None;
    for (index, segment) in segments.iter().enumerate() {
        match segment.id.as_str() {
            "UNB" => {
                if standalone || interchange.is_some() || group.is_some() || message.is_some() {
                    return Err(invalid(
                        index,
                        "interchange begins before prior envelope is closed",
                    ));
                }
                interchange = Some(Interchange {
                    reference: value(segment, index, 4)?.into(),
                    messages: 0,
                    groups: 0,
                    grouped: None,
                });
            }
            "UNG" => {
                if group.is_some() || message.is_some() {
                    return Err(invalid(
                        index,
                        "group begins before prior envelope is closed",
                    ));
                }
                let owner = interchange
                    .as_mut()
                    .ok_or_else(|| invalid(index, "group has no interchange"))?;
                if owner.grouped == Some(false) {
                    return Err(invalid(index, "grouped and ungrouped messages cannot mix"));
                }
                owner.grouped = Some(true);
                group = Some(Group {
                    reference: value(segment, index, 4)?.into(),
                    messages: 0,
                });
            }
            "UNH" => {
                if message.is_some() {
                    return Err(invalid(index, "message begins before UNT"));
                }
                if !standalone {
                    let owner = interchange
                        .as_mut()
                        .ok_or_else(|| invalid(index, "message has no interchange"))?;
                    if group.is_none() && owner.grouped == Some(true) {
                        return Err(invalid(index, "message is outside its functional group"));
                    }
                    if group.is_none() {
                        owner.grouped = Some(false);
                    }
                }
                message = Some(Message {
                    reference: value(segment, index, 0)?.into(),
                    start: index,
                });
            }
            "UNT" => {
                let active = message
                    .take()
                    .ok_or_else(|| invalid(index, "UNT has no UNH"))?;
                trailer(segment, index, index - active.start + 1, &active.reference)?;
                if let Some(owner) = &mut group {
                    owner.messages += 1;
                }
                if let Some(owner) = &mut interchange {
                    owner.messages += 1;
                }
            }
            "UNE" => {
                if message.is_some() {
                    return Err(invalid(index, "UNE precedes UNT"));
                }
                let active = group
                    .take()
                    .ok_or_else(|| invalid(index, "UNE has no UNG"))?;
                trailer(segment, index, active.messages, &active.reference)?;
                interchange
                    .as_mut()
                    .ok_or_else(|| invalid(index, "group has no interchange"))?
                    .groups += 1;
            }
            "UNZ" => {
                if message.is_some() || group.is_some() {
                    return Err(invalid(index, "UNZ precedes its message or group trailer"));
                }
                let active = interchange
                    .take()
                    .ok_or_else(|| invalid(index, "UNZ has no UNB"))?;
                trailer(
                    segment,
                    index,
                    if active.grouped == Some(true) {
                        active.groups
                    } else {
                        active.messages
                    },
                    &active.reference,
                )?;
            }
            _ if message.is_none() => return Err(invalid(index, "segment is outside a message")),
            _ => {}
        }
    }
    if interchange.is_some() || group.is_some() || message.is_some() {
        return Err(invalid(segments.len(), "unterminated envelope"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn strict_envelopes_check_all_control_pairs_without_changing_reader_policy() {
        let direct = "UNB+UNOC:3+S+R+261025:1200+I'UNH+M'BGM+X'UNT+3+M'UNZ+1+I'";
        let grouped = "UNB+UNOC:3+S+R+261025:1200+I'UNG+G+S+R+D+G1'UNH+M'UNT+2+M'UNE+1+G1'UNZ+1+I'";
        for text in [direct, grouped, "UNH+M'UNT+2+M'", "UNA;,.! ~UNH,M~UNT,2,M~"] {
            validate_envelope(text).unwrap();
        }
        for text in [
            direct.replace("UNT+3", "UNT+2"),
            direct.replace("UNZ+1", "UNZ+2"),
            direct.replace("UNT+3+M", "UNT+3+X"),
            grouped.replace("UNE+1", "UNE+2"),
            grouped.replace("UNE+1+G1", "UNE+1+X"),
            direct.replace("UNZ+1+I", "UNZ+1+X"),
            "UNH+M'".into(),
            "UNH+M'UNH+N'UNT+2+N'".into(),
        ] {
            assert!(
                matches!(
                    validate_envelope(&text),
                    Err(EdiFormatError::EdifactEnvelope { .. })
                ),
                "{text}"
            );
        }
    }
}
