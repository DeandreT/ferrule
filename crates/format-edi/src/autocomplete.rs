use crate::EdiFormatError;
use crate::segments::Segment;

const MAX_COMPLETED_SEGMENTS: usize = 1_000_000;

#[derive(Debug)]
struct Message {
    start: usize,
    reference: String,
}

#[derive(Debug)]
struct Group {
    reference: String,
    messages: usize,
}

#[derive(Debug)]
struct Interchange {
    reference: String,
    messages: usize,
    groups: usize,
}

pub(crate) fn edifact(
    segments: Vec<Segment>,
    current_datetime: &str,
    syntax_level: Option<&str>,
    syntax_version: Option<&str>,
    controlling_agency: Option<&str>,
    message_type: Option<&str>,
) -> Result<Vec<Segment>, EdiFormatError> {
    bounded_input(&segments, "EDIFACT")?;
    let timestamp = Timestamp::parse(current_datetime, "EDIFACT")?;
    let mut output = Vec::with_capacity(segments.len().saturating_add(8));
    let mut interchange = None;
    let mut group = None;
    let mut message = None;
    let mut interchange_ordinal = 0_usize;
    let mut group_ordinal = 0_usize;
    let mut message_ordinal = 0_usize;

    for mut segment in segments {
        match segment.id.as_str() {
            "UNB" => {
                finish_edifact_message(
                    &mut output,
                    &mut message,
                    &mut group,
                    &mut interchange,
                    None,
                )?;
                finish_edifact_group(&mut output, &mut group, &mut interchange, None)?;
                finish_edifact_interchange(&mut output, &mut interchange, None)?;
                interchange_ordinal = checked_increment(interchange_ordinal, "EDIFACT")?;
                let generated = interchange_ordinal.to_string();
                complete_edifact_header(
                    &mut segment,
                    &timestamp,
                    syntax_level,
                    syntax_version,
                    controlling_agency,
                );
                let reference = ensure_value(&mut segment, 4, &generated);
                interchange = Some(Interchange {
                    reference,
                    messages: 0,
                    groups: 0,
                });
                push_bounded(&mut output, segment, "EDIFACT")?;
            }
            "UNG" => {
                finish_edifact_message(
                    &mut output,
                    &mut message,
                    &mut group,
                    &mut interchange,
                    None,
                )?;
                finish_edifact_group(&mut output, &mut group, &mut interchange, None)?;
                require_interchange(&interchange, "EDIFACT functional group precedes UNB")?;
                group_ordinal = checked_increment(group_ordinal, "EDIFACT")?;
                let generated = group_ordinal.to_string();
                let reference = ensure_value(&mut segment, 4, &generated);
                group = Some(Group {
                    reference,
                    messages: 0,
                });
                push_bounded(&mut output, segment, "EDIFACT")?;
            }
            "UNH" => {
                finish_edifact_message(
                    &mut output,
                    &mut message,
                    &mut group,
                    &mut interchange,
                    None,
                )?;
                message_ordinal = checked_increment(message_ordinal, "EDIFACT")?;
                complete_edifact_message_header(&mut segment, message_type, controlling_agency);
                let generated = message_ordinal.to_string();
                let reference = ensure_value(&mut segment, 0, &generated);
                let start = output.len();
                push_bounded(&mut output, segment, "EDIFACT")?;
                message = Some(Message { start, reference });
            }
            "UNT" => finish_edifact_message(
                &mut output,
                &mut message,
                &mut group,
                &mut interchange,
                Some(segment),
            )?,
            "UNE" => {
                finish_edifact_message(
                    &mut output,
                    &mut message,
                    &mut group,
                    &mut interchange,
                    None,
                )?;
                finish_edifact_group(&mut output, &mut group, &mut interchange, Some(segment))?;
            }
            "UNZ" => {
                finish_edifact_message(
                    &mut output,
                    &mut message,
                    &mut group,
                    &mut interchange,
                    None,
                )?;
                finish_edifact_group(&mut output, &mut group, &mut interchange, None)?;
                finish_edifact_interchange(&mut output, &mut interchange, Some(segment))?;
            }
            _ => push_bounded(&mut output, segment, "EDIFACT")?,
        }
    }
    finish_edifact_message(
        &mut output,
        &mut message,
        &mut group,
        &mut interchange,
        None,
    )?;
    finish_edifact_group(&mut output, &mut group, &mut interchange, None)?;
    finish_edifact_interchange(&mut output, &mut interchange, None)?;
    Ok(output)
}

fn finish_edifact_message(
    output: &mut Vec<Segment>,
    message: &mut Option<Message>,
    group: &mut Option<Group>,
    interchange: &mut Option<Interchange>,
    trailer: Option<Segment>,
) -> Result<(), EdiFormatError> {
    let Some(message) = message.take() else {
        return reject_orphan_trailer(trailer, "EDIFACT", "UNT has no preceding UNH");
    };
    let count = output
        .len()
        .checked_sub(message.start)
        .and_then(|count| count.checked_add(1))
        .ok_or(autocomplete_error(
            "EDIFACT",
            "message segment count overflow",
        ))?;
    let mut trailer = trailer.unwrap_or_else(|| control_segment("UNT", 2));
    ensure_value(&mut trailer, 0, &count.to_string());
    ensure_value(&mut trailer, 1, &message.reference);
    push_bounded(output, trailer, "EDIFACT")?;
    if let Some(group) = group {
        group.messages = checked_increment(group.messages, "EDIFACT")?;
    }
    if let Some(interchange) = interchange {
        interchange.messages = checked_increment(interchange.messages, "EDIFACT")?;
    }
    Ok(())
}

fn finish_edifact_group(
    output: &mut Vec<Segment>,
    group: &mut Option<Group>,
    interchange: &mut Option<Interchange>,
    trailer: Option<Segment>,
) -> Result<(), EdiFormatError> {
    let Some(group) = group.take() else {
        return reject_orphan_trailer(trailer, "EDIFACT", "UNE has no preceding UNG");
    };
    let mut trailer = trailer.unwrap_or_else(|| control_segment("UNE", 2));
    ensure_value(&mut trailer, 0, &group.messages.to_string());
    ensure_value(&mut trailer, 1, &group.reference);
    push_bounded(output, trailer, "EDIFACT")?;
    let interchange = interchange.as_mut().ok_or(autocomplete_error(
        "EDIFACT",
        "functional group is not owned by an interchange",
    ))?;
    interchange.groups = checked_increment(interchange.groups, "EDIFACT")?;
    Ok(())
}

fn finish_edifact_interchange(
    output: &mut Vec<Segment>,
    interchange: &mut Option<Interchange>,
    trailer: Option<Segment>,
) -> Result<(), EdiFormatError> {
    let Some(interchange) = interchange.take() else {
        return reject_orphan_trailer(trailer, "EDIFACT", "UNZ has no preceding UNB");
    };
    let control_count = if interchange.groups == 0 {
        interchange.messages
    } else {
        interchange.groups
    };
    let mut trailer = trailer.unwrap_or_else(|| control_segment("UNZ", 2));
    ensure_value(&mut trailer, 0, &control_count.to_string());
    ensure_value(&mut trailer, 1, &interchange.reference);
    push_bounded(output, trailer, "EDIFACT")
}

pub(crate) fn x12(
    segments: Vec<Segment>,
    current_datetime: &str,
    request_acknowledgement: bool,
    transaction_set: Option<&str>,
) -> Result<Vec<Segment>, EdiFormatError> {
    bounded_input(&segments, "X12")?;
    let timestamp = Timestamp::parse(current_datetime, "X12")?;
    let mut output = Vec::with_capacity(segments.len().saturating_add(12));
    let mut interchange = None;
    let mut group = None;
    let mut transaction = None;
    let mut interchange_ordinal = 0_usize;
    let mut group_ordinal = 0_usize;
    let mut transaction_ordinal = 0_usize;

    for mut segment in segments {
        match segment.id.as_str() {
            "ISA" => {
                finish_x12_transaction(&mut output, &mut transaction, &mut group, None)?;
                finish_x12_group(&mut output, &mut group, &mut interchange, None)?;
                finish_x12_interchange(&mut output, &mut interchange, None)?;
                interchange_ordinal = checked_increment(interchange_ordinal, "X12")?;
                complete_x12_header(&mut segment, &timestamp, request_acknowledgement)?;
                let reference = ensure_value(&mut segment, 12, "000000000");
                interchange = Some(Interchange {
                    reference,
                    messages: 0,
                    groups: 0,
                });
                push_bounded(&mut output, segment, "X12")?;
            }
            "GS" => {
                finish_x12_transaction(&mut output, &mut transaction, &mut group, None)?;
                finish_x12_group(&mut output, &mut group, &mut interchange, None)?;
                require_interchange(&interchange, "X12 functional group precedes ISA")?;
                group_ordinal = checked_increment(group_ordinal, "X12")?;
                ensure_value(&mut segment, 3, &timestamp.date_yyyy_mm_dd);
                ensure_value(&mut segment, 4, &timestamp.time_hh_mm_ss);
                normalize_x12_date(&mut segment, 3, false)?;
                normalize_x12_time(&mut segment, 4, false)?;
                let generated = group_ordinal.to_string();
                let reference = ensure_value(&mut segment, 5, &generated);
                group = Some(Group {
                    reference,
                    messages: 0,
                });
                push_bounded(&mut output, segment, "X12")?;
            }
            "ST" => {
                finish_x12_transaction(&mut output, &mut transaction, &mut group, None)?;
                if group.is_none() {
                    return Err(autocomplete_error("X12", "transaction set precedes GS"));
                }
                transaction_ordinal = checked_increment(transaction_ordinal, "X12")?;
                ensure_value(&mut segment, 0, transaction_set.unwrap_or_default());
                let generated = format!("{transaction_ordinal:04}");
                let reference = ensure_value(&mut segment, 1, &generated);
                let start = output.len();
                push_bounded(&mut output, segment, "X12")?;
                transaction = Some(Message { start, reference });
            }
            "SE" => {
                finish_x12_transaction(&mut output, &mut transaction, &mut group, Some(segment))?
            }
            "GE" => {
                finish_x12_transaction(&mut output, &mut transaction, &mut group, None)?;
                finish_x12_group(&mut output, &mut group, &mut interchange, Some(segment))?;
            }
            "IEA" => {
                finish_x12_transaction(&mut output, &mut transaction, &mut group, None)?;
                finish_x12_group(&mut output, &mut group, &mut interchange, None)?;
                finish_x12_interchange(&mut output, &mut interchange, Some(segment))?;
            }
            _ => push_bounded(&mut output, segment, "X12")?,
        }
    }
    finish_x12_transaction(&mut output, &mut transaction, &mut group, None)?;
    finish_x12_group(&mut output, &mut group, &mut interchange, None)?;
    finish_x12_interchange(&mut output, &mut interchange, None)?;
    Ok(output)
}

fn finish_x12_transaction(
    output: &mut Vec<Segment>,
    transaction: &mut Option<Message>,
    group: &mut Option<Group>,
    trailer: Option<Segment>,
) -> Result<(), EdiFormatError> {
    let Some(transaction) = transaction.take() else {
        return reject_orphan_trailer(trailer, "X12", "SE has no preceding ST");
    };
    let count = output
        .len()
        .checked_sub(transaction.start)
        .and_then(|count| count.checked_add(1))
        .ok_or(autocomplete_error(
            "X12",
            "transaction segment count overflow",
        ))?;
    let mut trailer = trailer.unwrap_or_else(|| control_segment("SE", 2));
    ensure_value(&mut trailer, 0, &count.to_string());
    ensure_value(&mut trailer, 1, &transaction.reference);
    push_bounded(output, trailer, "X12")?;
    let group = group.as_mut().ok_or(autocomplete_error(
        "X12",
        "transaction set is not owned by a functional group",
    ))?;
    group.messages = checked_increment(group.messages, "X12")?;
    Ok(())
}

fn finish_x12_group(
    output: &mut Vec<Segment>,
    group: &mut Option<Group>,
    interchange: &mut Option<Interchange>,
    trailer: Option<Segment>,
) -> Result<(), EdiFormatError> {
    let Some(group) = group.take() else {
        return reject_orphan_trailer(trailer, "X12", "GE has no preceding GS");
    };
    let mut trailer = trailer.unwrap_or_else(|| control_segment("GE", 2));
    ensure_value(&mut trailer, 0, &group.messages.to_string());
    ensure_value(&mut trailer, 1, &group.reference);
    push_bounded(output, trailer, "X12")?;
    let interchange = interchange.as_mut().ok_or(autocomplete_error(
        "X12",
        "functional group is not owned by an interchange",
    ))?;
    interchange.groups = checked_increment(interchange.groups, "X12")?;
    Ok(())
}

fn finish_x12_interchange(
    output: &mut Vec<Segment>,
    interchange: &mut Option<Interchange>,
    trailer: Option<Segment>,
) -> Result<(), EdiFormatError> {
    let Some(interchange) = interchange.take() else {
        return reject_orphan_trailer(trailer, "X12", "IEA has no preceding ISA");
    };
    let mut trailer = trailer.unwrap_or_else(|| control_segment("IEA", 2));
    ensure_value(&mut trailer, 0, &interchange.groups.to_string());
    ensure_value(&mut trailer, 1, &interchange.reference);
    push_bounded(output, trailer, "X12")
}

fn control_segment(id: &str, elements: usize) -> Segment {
    Segment {
        id: id.to_string(),
        elements: (0..elements).map(|_| vec![vec![String::new()]]).collect(),
    }
}

fn ensure_value(segment: &mut Segment, index: usize, default: &str) -> String {
    while segment.elements.len() <= index {
        segment.elements.push(vec![vec![String::new()]]);
    }
    let element = &mut segment.elements[index];
    if element.is_empty() {
        element.push(vec![String::new()]);
    }
    if element[0].is_empty() {
        element[0].push(String::new());
    }
    if element[0][0].is_empty() {
        element[0][0] = default.to_string();
    }
    element[0][0].clone()
}

struct Timestamp {
    date_yyyy_mm_dd: String,
    date_yy_mm_dd: String,
    time_hh_mm: String,
    time_hh_mm_ss: String,
}

impl Timestamp {
    fn parse(value: &str, dialect: &'static str) -> Result<Self, EdiFormatError> {
        let bytes = value.as_bytes();
        let valid = bytes.len() >= 19
            && bytes.get(4) == Some(&b'-')
            && bytes.get(7) == Some(&b'-')
            && bytes.get(10) == Some(&b'T')
            && bytes.get(13) == Some(&b':')
            && bytes.get(16) == Some(&b':')
            && bytes[..19].iter().enumerate().all(|(index, byte)| {
                matches!(index, 4 | 7 | 10 | 13 | 16) || byte.is_ascii_digit()
            });
        if !valid {
            return Err(autocomplete_error(
                dialect,
                "current dateTime must begin with YYYY-MM-DDTHH:MM:SS",
            ));
        }
        Ok(Self {
            date_yyyy_mm_dd: format!("{}{}{}", &value[..4], &value[5..7], &value[8..10]),
            date_yy_mm_dd: format!("{}{}{}", &value[2..4], &value[5..7], &value[8..10]),
            time_hh_mm: format!("{}{}", &value[11..13], &value[14..16]),
            time_hh_mm_ss: format!("{}{}{}", &value[11..13], &value[14..16], &value[17..19]),
        })
    }
}

fn complete_edifact_header(
    segment: &mut Segment,
    timestamp: &Timestamp,
    syntax_level: Option<&str>,
    syntax_version: Option<&str>,
    controlling_agency: Option<&str>,
) {
    if let (Some(agency), Some(level)) = (controlling_agency, syntax_level) {
        let identifier = format!("{agency}{level}");
        ensure_components(
            segment,
            0,
            &[identifier.as_str(), syntax_version.unwrap_or_default()],
        );
    }
    let syntax_version = segment
        .elements
        .first()
        .and_then(|element| element.first())
        .and_then(|components| components.get(1))
        .map(String::as_str);
    let date = if syntax_version.is_some_and(|version| version >= "4") {
        &timestamp.date_yyyy_mm_dd
    } else {
        &timestamp.date_yy_mm_dd
    };
    ensure_components(segment, 3, &[date, &timestamp.time_hh_mm]);
}

fn complete_edifact_message_header(
    segment: &mut Segment,
    message_type: Option<&str>,
    controlling_agency: Option<&str>,
) {
    let message_agency = controlling_agency
        .and_then(|agency| agency.strip_suffix('O'))
        .or(controlling_agency)
        .unwrap_or_default();
    ensure_components(
        segment,
        1,
        &[message_type.unwrap_or_default(), "", "", message_agency],
    );
}

fn ensure_components(segment: &mut Segment, index: usize, defaults: &[&str]) {
    while segment.elements.len() <= index {
        segment.elements.push(Vec::new());
    }
    let element = &mut segment.elements[index];
    if element.is_empty() {
        element.push(Vec::new());
    }
    for (component_index, default) in defaults.iter().enumerate() {
        while element[0].len() <= component_index {
            element[0].push(String::new());
        }
        if element[0][component_index].is_empty() {
            element[0][component_index].push_str(default);
        }
    }
}

fn complete_x12_header(
    segment: &mut Segment,
    timestamp: &Timestamp,
    request_acknowledgement: bool,
) -> Result<(), EdiFormatError> {
    let defaults = [
        "00",
        "          ",
        "00",
        "          ",
        "ZZ",
        "",
        "ZZ",
        "",
        &timestamp.date_yy_mm_dd,
        &timestamp.time_hh_mm,
        "",
        "",
        "000000000",
        if request_acknowledgement { "1" } else { "0" },
        "P",
        "",
    ];
    for (index, default) in defaults.into_iter().enumerate() {
        ensure_value(segment, index, default);
    }
    normalize_x12_date(segment, 8, true)?;
    normalize_x12_time(segment, 9, true)?;
    for (index, width) in [(1, 10), (3, 10), (5, 15), (7, 15)] {
        pad_x12_element(segment, index, width)?;
    }
    pad_x12_numeric_element(segment, 12, 9)?;
    for (index, width) in [
        (0, 2),
        (2, 2),
        (4, 2),
        (6, 2),
        (8, 6),
        (9, 4),
        (10, 1),
        (11, 5),
        (13, 1),
        (14, 1),
        (15, 1),
    ] {
        validate_x12_width(segment, index, width)?;
    }
    Ok(())
}

fn normalize_x12_date(
    segment: &mut Segment,
    index: usize,
    short: bool,
) -> Result<(), EdiFormatError> {
    let value = ensure_value(segment, index, "");
    let compact = if value.len() == 10
        && value.as_bytes().get(4) == Some(&b'-')
        && value.as_bytes().get(7) == Some(&b'-')
    {
        format!("{}{}{}", &value[..4], &value[5..7], &value[8..10])
    } else {
        value
    };
    let compact = if short && compact.len() == 8 {
        compact[2..].to_string()
    } else {
        compact
    };
    let width = if short { 6 } else { 8 };
    if compact.len() != width || !compact.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(EdiFormatError::InvalidEnvelopeElement {
            element: format!("{}{:02}", segment.id, index + 1),
            value: compact,
            reason: "date must be YYYY-MM-DD or the dialect's compact numeric form",
        });
    }
    segment.elements[index][0][0] = compact;
    Ok(())
}

fn normalize_x12_time(
    segment: &mut Segment,
    index: usize,
    short: bool,
) -> Result<(), EdiFormatError> {
    let value = ensure_value(segment, index, "");
    let mut compact = String::with_capacity(6);
    for character in value.chars() {
        if matches!(character, '+' | '-' | 'Z') {
            break;
        }
        if character.is_ascii_digit() {
            compact.push(character);
        } else if !matches!(character, ':' | '?') {
            compact.clear();
            break;
        }
    }
    if short && compact.len() == 6 {
        compact.truncate(4);
    }
    let width = if short { 4 } else { 6 };
    if compact.len() != width || !compact.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(EdiFormatError::InvalidEnvelopeElement {
            element: format!("{}{:02}", segment.id, index + 1),
            value: compact,
            reason: "time must be HH:MM[:SS] or the dialect's compact numeric form",
        });
    }
    segment.elements[index][0][0] = compact;
    Ok(())
}

fn pad_x12_numeric_element(
    segment: &mut Segment,
    index: usize,
    width: usize,
) -> Result<(), EdiFormatError> {
    let value = ensure_value(segment, index, "");
    if value.is_empty() || value.len() > width || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(EdiFormatError::InvalidEnvelopeElement {
            element: format!("ISA{:02}", index + 1),
            value,
            reason: "control number must contain at most nine ASCII digits",
        });
    }
    segment.elements[index][0][0] = format!("{value:0>width$}");
    Ok(())
}

fn pad_x12_element(
    segment: &mut Segment,
    index: usize,
    width: usize,
) -> Result<(), EdiFormatError> {
    let value = ensure_value(segment, index, "");
    if !value.is_ascii() || value.len() > width {
        return Err(EdiFormatError::InvalidEnvelopeElement {
            element: format!("ISA{:02}", index + 1),
            value,
            reason: "fixed-width value must be ASCII and may not exceed its declared width",
        });
    }
    segment.elements[index][0][0].extend(std::iter::repeat_n(' ', width - value.len()));
    Ok(())
}

fn validate_x12_width(
    segment: &mut Segment,
    index: usize,
    width: usize,
) -> Result<(), EdiFormatError> {
    let value = ensure_value(segment, index, "");
    if !value.is_ascii() || value.len() != width {
        return Err(EdiFormatError::InvalidEnvelopeElement {
            element: format!("ISA{:02}", index + 1),
            value,
            reason: "fixed-width value has the wrong width",
        });
    }
    Ok(())
}

fn push_bounded(
    output: &mut Vec<Segment>,
    segment: Segment,
    dialect: &'static str,
) -> Result<(), EdiFormatError> {
    if output.len() >= MAX_COMPLETED_SEGMENTS {
        return Err(autocomplete_error(
            dialect,
            "completed segment count exceeds 1,000,000",
        ));
    }
    output.push(segment);
    Ok(())
}

fn bounded_input(segments: &[Segment], dialect: &'static str) -> Result<(), EdiFormatError> {
    if segments.len() > MAX_COMPLETED_SEGMENTS {
        return Err(autocomplete_error(
            dialect,
            "input segment count exceeds 1,000,000",
        ));
    }
    Ok(())
}

fn checked_increment(value: usize, dialect: &'static str) -> Result<usize, EdiFormatError> {
    value
        .checked_add(1)
        .ok_or(autocomplete_error(dialect, "control count overflow"))
}

fn require_interchange<T>(
    interchange: &Option<T>,
    reason: &'static str,
) -> Result<(), EdiFormatError> {
    interchange.as_ref().map(|_| ()).ok_or(autocomplete_error(
        if reason.starts_with("X12") {
            "X12"
        } else {
            "EDIFACT"
        },
        reason,
    ))
}

fn reject_orphan_trailer(
    trailer: Option<Segment>,
    dialect: &'static str,
    reason: &'static str,
) -> Result<(), EdiFormatError> {
    if trailer.is_some() {
        Err(autocomplete_error(dialect, reason))
    } else {
        Ok(())
    }
}

const fn autocomplete_error(dialect: &'static str, reason: &'static str) -> EdiFormatError {
    EdiFormatError::EnvelopeAutocomplete { dialect, reason }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn segment(id: &str, values: &[&str]) -> Segment {
        Segment {
            id: id.to_string(),
            elements: values
                .iter()
                .map(|value| vec![vec![(*value).to_string()]])
                .collect(),
        }
    }

    fn values(segment: &Segment) -> Vec<&str> {
        segment
            .elements
            .iter()
            .map(|element| element[0][0].as_str())
            .collect()
    }

    #[test]
    fn edifact_completes_each_message_and_interchange() {
        let completed = edifact(
            vec![
                segment("UNB", &["UNOA:4", "S", "R", "date", "77"]),
                segment("UNH", &["001"]),
                segment("BGM", &["order"]),
                segment("UNH", &["002"]),
                segment("DTM", &["date"]),
            ],
            "2026-07-18T12:34:56-07:00",
            None,
            None,
            None,
            None,
        )
        .unwrap();
        assert_eq!(
            completed
                .iter()
                .map(|segment| segment.id.as_str())
                .collect::<Vec<_>>(),
            ["UNB", "UNH", "BGM", "UNT", "UNH", "DTM", "UNT", "UNZ"]
        );
        assert_eq!(values(&completed[3]), ["3", "001"]);
        assert_eq!(values(&completed[6]), ["3", "002"]);
        assert_eq!(values(&completed[7]), ["2", "77"]);
    }

    #[test]
    fn x12_completes_transaction_group_and_interchange_controls() {
        let completed = x12(
            vec![
                segment(
                    "ISA",
                    &[
                        "",
                        "",
                        "",
                        "",
                        "",
                        "",
                        "",
                        "",
                        "",
                        "",
                        "^",
                        "00501",
                        "000000777",
                        "0",
                        "P",
                        ":",
                    ],
                ),
                segment("GS", &["PO", "S", "R", "20260718", "123456", "9"]),
                segment("ST", &["850", "12345"]),
                segment("BEG", &["00"]),
            ],
            "2026-07-18T12:34:56-07:00",
            false,
            None,
        )
        .unwrap();
        assert_eq!(
            completed
                .iter()
                .map(|segment| segment.id.as_str())
                .collect::<Vec<_>>(),
            ["ISA", "GS", "ST", "BEG", "SE", "GE", "IEA"]
        );
        assert_eq!(values(&completed[4]), ["3", "12345"]);
        assert_eq!(values(&completed[5]), ["1", "9"]);
        assert_eq!(values(&completed[6]), ["1", "000000777"]);
    }

    #[test]
    fn explicit_nonempty_trailer_values_are_preserved() {
        let completed = x12(
            vec![
                segment(
                    "ISA",
                    &[
                        "",
                        "",
                        "",
                        "",
                        "",
                        "",
                        "",
                        "",
                        "",
                        "",
                        "^",
                        "00501",
                        "000000001",
                        "0",
                        "P",
                        ":",
                    ],
                ),
                segment("GS", &["PO", "S", "R", "20260718", "123456", "1"]),
                segment("ST", &["850", "0001"]),
                segment("SE", &["99", "manual"]),
                segment("GE", &["8", "manual-group"]),
                segment("IEA", &["7", "manual-interchange"]),
            ],
            "2026-07-18T12:34:56-07:00",
            false,
            None,
        )
        .unwrap();
        assert_eq!(values(&completed[3]), ["99", "manual"]);
        assert_eq!(values(&completed[4]), ["8", "manual-group"]);
        assert_eq!(values(&completed[5]), ["7", "manual-interchange"]);
    }

    #[test]
    fn x12_normalizes_mapped_envelope_lexicals_without_replacing_controls() {
        let completed = x12(
            vec![
                segment(
                    "ISA",
                    &[
                        "00",
                        "",
                        "00",
                        "",
                        "ZZ",
                        "Sender",
                        "ZZ",
                        "Receiver",
                        "2004-04-30",
                        "17:42:00-09:00",
                        "^",
                        "00501",
                        "1",
                        "1",
                        "P",
                        ":",
                    ],
                ),
                segment("GS", &["PO", "S", "R", "2004-04-30", "17:42:00-09:00", "1"]),
                segment("ST", &["", "12345"]),
            ],
            "2026-07-18T12:34:56-07:00",
            true,
            Some("850"),
        )
        .unwrap();

        assert_eq!(
            &values(&completed[0])[8..14],
            ["040430", "1742", "^", "00501", "000000001", "1"]
        );
        assert_eq!(&values(&completed[1])[3..6], ["20040430", "174200", "1"]);
        assert_eq!(values(&completed[2]), ["850", "12345"]);
    }
}
