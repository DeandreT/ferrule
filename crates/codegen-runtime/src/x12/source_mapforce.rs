//! Explicit tolerant source value projection, not strict X12 validation.
use super::*;
use mapping::EdiLexicalKind;

/// Read a MapForce-envelope source without eagerly converting numeric values or
/// validating date/time calendars. Empty physical values become absent; source
/// dates/times receive the native lexical view, independently of reached casts.
/// A flat descriptor keeps UTF-8 fallback. An explicit native descriptor binds
/// UTF-8 or Latin-1 fallback; Unicode BOMs override either choice. This does not
/// acquire files, validate every native parser rule, or publish.
pub fn parse_mapforce_x12_source(
    descriptor_text: &str,
    bytes: &[u8],
) -> Result<Instance, X12BoundaryError> {
    let (profile, encoding) = source_profile(descriptor_text)?;
    input_size(bytes.len())?;
    let text = decode_source(bytes, encoding);
    input_size(text.len())?;
    let text = source_wire_text(&text);
    let lexical = format_edi::x12::from_str_lexical(&text, &profile.schema)?;
    let formats = profile
        .lexical
        .iter()
        .map(|format| (format.path(), format.kind()))
        .collect::<BTreeMap<_, _>>();
    let mut budget = SourceBudget::default();
    project(
        SchemaCursor::root(&profile.schema),
        &lexical,
        &[],
        &formats,
        &mut budget,
        0,
    )
}

pub(super) fn source_profile(
    text: &str,
) -> Result<(Profile, mapping::X12SourceEncoding), X12BoundaryError> {
    if text.len() > MAX_EMBEDDED_X12_SCHEMA_BYTES {
        return Err(descriptor(
            "native source descriptor exceeds its byte limit",
        ));
    }
    let value: Json = serde_json::from_str(text).map_err(|error| descriptor(error.to_string()))?;
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct NativeSourceDescriptor {
        boundary: String,
        source_encoding: mapping::X12SourceEncoding,
    }
    // Deserialize the original text, not the Value, so duplicate properties
    // cannot silently replace the bound source policy.
    let native = value
        .get("boundary")
        .map(|_| serde_json::from_str::<NativeSourceDescriptor>(text))
        .transpose()
        .map_err(|error| descriptor(error.to_string()))?;
    let boundary = native
        .as_ref()
        .map_or(text, |source| source.boundary.as_str());
    let encoding = native
        .as_ref()
        .map_or(mapping::X12SourceEncoding::Utf8, |source| {
            source.source_encoding
        });
    let profile = Profile::parse(boundary)?;
    validate_profile(&profile)?;
    Ok((profile, encoding))
}

fn decode_source(bytes: &[u8], encoding: mapping::X12SourceEncoding) -> String {
    let bom = bytes.starts_with(&[0xef, 0xbb, 0xbf])
        || bytes.starts_with(&[0xff, 0xfe])
        || bytes.starts_with(&[0xfe, 0xff])
        || bytes.starts_with(&[0, 0, 0xfe, 0xff]);
    if encoding == mapping::X12SourceEncoding::Latin1 && !bom {
        bytes.iter().map(|byte| char::from(*byte)).collect()
    } else {
        format_csv::decode_bom_aware_non_flushing(bytes)
    }
}

pub(super) fn validate_profile(profile: &Profile) -> Result<(), X12BoundaryError> {
    if profile.envelope != EnvelopeProfile::MapForce
        || profile.implied.iter().any(|value| value.places() != 0)
    {
        return Err(descriptor(
            "MapForce source projection requires a MapForce envelope without fractional implied decimals",
        ));
    }
    Ok(())
}

#[derive(Default)]
struct SourceBudget {
    structure: Budget,
    text: usize,
}
impl SourceBudget {
    fn text(&mut self, value: &str) -> Result<(), X12BoundaryError> {
        self.text = self
            .text
            .checked_add(value.len())
            .ok_or_else(|| descriptor("source projection text overflow"))?;
        resource(
            "x12_source_projected_text",
            self.text,
            MAX_X12_PROJECTED_SCALAR_BYTES,
        )
    }
}

fn project(
    cursor: SchemaCursor<'_>,
    value: &Instance,
    path: &[String],
    formats: &BTreeMap<&[String], EdiLexicalKind>,
    budget: &mut SourceBudget,
    depth: usize,
) -> Result<Instance, X12BoundaryError> {
    budget.structure.node(depth)?;
    match value {
        Instance::Repeated(items) => {
            budget.structure.loops(items.len())?;
            items
                .iter()
                .map(|item| project(cursor, item, path, formats, budget, depth + 1))
                .collect::<Result<Vec<_>, _>>()
                .map(Instance::Repeated)
        }
        Instance::Scalar(Value::Null) => Ok(Instance::Scalar(Value::Null)),
        Instance::Scalar(Value::String(text)) => {
            if text.is_empty() {
                return Ok(Instance::Scalar(Value::Null));
            }
            let text = match formats.get(path) {
                Some(EdiLexicalKind::CompactDate6 | EdiLexicalKind::CompactDate8) => {
                    source_date(text)
                }
                Some(EdiLexicalKind::CompactTime { .. }) => source_time(text),
                Some(EdiLexicalKind::Decimal { .. }) => source_number(text),
                _ => text.clone(),
            };
            budget.text(&text)?;
            Ok(Instance::Scalar(Value::String(text)))
        }
        Instance::Group(fields) => {
            let mut result = Vec::new();
            for child in cursor.children() {
                let name = &child.schema().name;
                let mut next = path.to_vec();
                next.push(name.clone());
                let value = fields
                    .iter()
                    .find(|(key, _)| key == name)
                    .map(|(_, value)| value)
                    .ok_or_else(|| {
                        descriptor("lexical source field is missing from its schema-shaped view")
                    })?;
                budget
                    .structure
                    .name(name, MAX_X12_MATERIALIZED_NAME_BYTES)?;
                result.push((
                    name.clone(),
                    project(child, value, &next, formats, budget, depth + 1)?,
                ));
            }
            // A positional composite with no physical values is absent; an
            // empty segment/container is still a physical occurrence.
            if matches!(
                cursor.role(),
                SchemaRole::Element { .. } | SchemaRole::Component
            ) && result
                .iter()
                .all(|(_, value)| matches!(value, Instance::Scalar(Value::Null)))
            {
                return Ok(Instance::Scalar(Value::Null));
            }
            Ok(Instance::Group(result.into()))
        }
        _ => Err(descriptor("unsupported lexical source value")),
    }
}

// Match .NET Char.IsWhiteSpace rather than a locale or a broad text trim.
fn whitespace(ch: char) -> bool {
    matches!(ch, '\u{0009}'..='\u{000d}' | '\u{0020}' | '\u{0085}' | '\u{00a0}' | '\u{1680}' | '\u{2000}'..='\u{200a}' | '\u{2028}' | '\u{2029}' | '\u{202f}' | '\u{205f}' | '\u{3000}')
}

fn source_wire_text(text: &str) -> String {
    let first = text.trim_start_matches(whitespace);
    let terminator = first
        .chars()
        .nth(3)
        .and_then(|element| first.split(element).nth(16))
        .and_then(|tail| tail.chars().nth(1));
    text.chars()
        .filter(|ch| !matches!(ch, '\t' | '\r' | '\n') || Some(*ch) == terminator)
        .collect()
}

fn source_number(text: &str) -> String {
    let trimmed = text.trim_matches(whitespace);
    if trimmed.is_empty() {
        return text.into();
    }
    let negative = trimmed.starts_with('-');
    let text = trimmed.strip_prefix(['+', '-']).unwrap_or(trimmed);
    if text.is_empty() {
        return String::new();
    }
    let separator = text.rfind('.').or_else(|| text.rfind(','));
    let (integer, fraction) =
        separator.map_or((text, ""), |index| (&text[..index], &text[index + 1..]));
    let integer = if integer.is_empty() {
        "0".into()
    } else {
        integer.replace(['.', ','], "")
    };
    if !integer.bytes().all(|ch| ch.is_ascii_digit())
        || !fraction.bytes().all(|ch| ch.is_ascii_digit())
    {
        return text.into();
    }
    format!(
        "{}{}{}{}",
        if negative { "-" } else { "" },
        integer,
        if fraction.is_empty() { "" } else { "." },
        fraction
    )
}
fn source_date(text: &str) -> String {
    let text = text.trim_start_matches(whitespace);
    if !matches!(text.len(), 6 | 8) || !text.bytes().all(|ch| ch.is_ascii_digit()) {
        return text.into();
    }
    let expanded = if text.len() == 6 {
        format!("{}{text}", if &text[..2] < "50" { "20" } else { "19" })
    } else {
        text.into()
    };
    format!("{}-{}-{}", &expanded[..4], &expanded[4..6], &expanded[6..])
}
fn source_time(text: &str) -> String {
    let text = text.trim_start_matches(whitespace);
    let mut units = text.encode_utf16().collect::<Vec<_>>();
    if units.len() < 4 || units.len() == 5 {
        return text.into();
    }
    if units.len() == 4 {
        units.extend_from_slice(&[u16::from(b'0'); 2]);
    }
    if units.len() > 6 {
        units.insert(6, u16::from(b'.'));
    }
    units.insert(4, u16::from(b':'));
    units.insert(2, u16::from(b':'));
    // The value view is carried in Unicode scalar strings, as in the existing
    // wire reader. Split surrogate pairs become replacement characters.
    String::from_utf16_lossy(&units)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ir::ScalarType;
    #[test]
    fn projection_is_separate_from_raw_lexicals_and_preserves_ownership() {
        let fields = |name: &str, count: usize| {
            SchemaNode::group(
                name,
                (1..=count)
                    .map(|i| SchemaNode::scalar(format!("E{i:02}"), ScalarType::String))
                    .collect(),
            )
        };
        let schema = SchemaNode::group(
            "Envelope",
            vec![
                SchemaNode::group(
                    "Interchange",
                    vec![
                        fields("ISA", 16).repeating(),
                        SchemaNode::group(
                            "Group",
                            vec![
                                fields("GS", 8).repeating(),
                                SchemaNode::group(
                                    "Message_850",
                                    vec![fields("ST", 2), fields("N9", 3), fields("SE", 2)],
                                )
                                .repeating(),
                                fields("GE", 2).repeating(),
                            ],
                        )
                        .repeating(),
                        fields("IEA", 2).repeating(),
                    ],
                )
                .repeating(),
            ],
        );
        let formats = vec![
            mapping::EdiLexicalFormat::new(
                vec![
                    "Interchange".into(),
                    "Group".into(),
                    "Message_850".into(),
                    "N9".into(),
                    "E02".into(),
                ],
                EdiLexicalKind::CompactDate8,
            )
            .unwrap(),
            mapping::EdiLexicalFormat::new(
                vec![
                    "Interchange".into(),
                    "Group".into(),
                    "Message_850".into(),
                    "N9".into(),
                    "E03".into(),
                ],
                EdiLexicalKind::CompactTime {
                    min_digits: 4,
                    max_digits: 8,
                },
            )
            .unwrap(),
        ];
        let descriptor = serde_json::json!({"schema":codegen_schema::encode(&schema,MAX_EMBEDDED_X12_SCHEMA_BYTES).unwrap(),"version":"006040","envelope_profile":"mapforce_interchanges","constraints":[],"implied_decimals":[],"lexical_formats":formats,"autocomplete":null,"lenient_segments":false,"separators":null}).to_string();
        let wire = b"ISA*00*          *00*          *ZZ*ALPHA          *ZZ*BETA           *300615*0945*U*00401*000000123*0*T*:~GS*PO*ALPHA*BETA*20300615*0945*7*X*004010~ST*850*0007~N9**491399*abCDef~SE*3*0007~GE*1*7~IEA*1*000000123~";
        let raw = parse_lexical_x12_source(&descriptor, wire).unwrap();
        let native = parse_mapforce_x12_source(&descriptor, wire).unwrap();
        let field = |tree: &Instance, name: &str| {
            tree.field("Interchange").unwrap().as_repeated().unwrap()[0]
                .field("Group")
                .unwrap()
                .as_repeated()
                .unwrap()[0]
                .field("Message_850")
                .unwrap()
                .as_repeated()
                .unwrap()[0]
                .field("N9")
                .unwrap()
                .field(name)
                .unwrap()
                .clone()
        };
        assert_eq!(
            field(&raw, "E02"),
            Instance::Scalar(Value::String("491399".into()))
        );
        assert_eq!(
            field(&native, "E02"),
            Instance::Scalar(Value::String("2049-13-99".into()))
        );
        assert_eq!(
            field(&native, "E03"),
            Instance::Scalar(Value::String("ab:CD:ef".into()))
        );
        assert_eq!(field(&native, "E01"), Instance::Scalar(Value::Null));
    }
    #[test]
    fn native_dates_do_not_validate_calendars_and_keep_bad_lexicals() {
        for (input, expected) in [
            ("491231", "2049-12-31"),
            ("501231", "1950-12-31"),
            ("000000", "2000-00-00"),
            ("20269999", "2026-99-99"),
            ("20260229", "2026-02-29"),
            ("abcdefgh", "abcdefgh"),
            (" 20261023 ", "20261023 "),
            ("\u{a0}20261023", "2026-10-23"),
        ] {
            assert_eq!(source_date(input), expected);
        }
    }

    #[test]
    fn source_encoding_is_explicit_and_boms_override_the_fallback() {
        use mapping::X12SourceEncoding::{Latin1, Utf8};
        assert_eq!(
            decode_source(&[b'c', b'a', b'f', 0xe9], Latin1),
            "caf\u{e9}"
        );
        assert_eq!(
            decode_source("caf\u{e9}".as_bytes(), Latin1),
            "caf\u{c3}\u{a9}"
        );
        assert_eq!(decode_source("caf\u{e9}".as_bytes(), Utf8), "caf\u{e9}");
        assert_eq!(
            decode_source(&[0xef, 0xbb, 0xbf, b'c', b'a', b'f', 0xc3, 0xa9], Latin1),
            "caf\u{e9}"
        );
        assert_eq!(
            decode_source(&[0xff, 0xfe, b'c', 0, b'a', 0, b'f', 0, 0xe9, 0], Latin1),
            "caf\u{e9}"
        );
        assert_eq!(
            decode_source(&[0, 0, 0xfe, 0xff, 0, 0, 0, 0xe9], Latin1),
            "\u{e9}"
        );
        assert!(source_profile(r#"{"boundary":"{}","source_encoding":"windows1252"}"#).is_err());
    }
    #[test]
    fn native_times_format_by_utf16_width_without_validation() {
        for (input, expected) in [
            ("1200", "12:00:00"),
            ("12001", "12001"),
            ("abcdef", "ab:cd:ef"),
            ("246099", "24:60:99"),
            ("12000123456789", "12:00:01.23456789"),
            ("12:00:01", "12::0:0:.01"),
            ("\u{a0}1200", "12:00:00"),
            ("a😀b", "a�:�b:00"),
        ] {
            assert_eq!(source_time(input), expected);
        }
    }

    #[test]
    fn native_number_normalization_does_not_parse_or_reject_invalid_values() {
        for (input, expected) in [
            (" +2 ", "2"),
            ("  ", "  "),
            ("+", ""),
            ("-bad", "bad"),
            ("1,234", "1.234"),
            ("1,234.50", "1234.50"),
            ("1.2.3", "12.3"),
            ("1.", "1"),
            (".5", "0.5"),
            ("1.234,50", "1.234,50"),
            (" -1e3 ", "1e3"),
            ("..", ""),
            ("-0.0", "-0.0"),
        ] {
            assert_eq!(source_number(input), expected);
        }
        assert_eq!(
            source_wire_text("ISA*1*2*3*4*5*6*7*8*9*10*11*12*13*14*15*:~N9*A\tB\r\nC~"),
            "ISA*1*2*3*4*5*6*7*8*9*10*11*12*13*14*15*:~N9*ABC~"
        );
    }
}
