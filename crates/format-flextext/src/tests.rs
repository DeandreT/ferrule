use std::num::NonZeroU32;

use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{
    DelimitedDialect, DelimitedRecordField, FixedWidthRecordField, FlexCommand, FlexLineEnding,
    FlexTextLayout, ManySplitter, OnceSplitter, StoreTrim, SwitchArm, SwitchMode, TrimSide,
};

use crate::{
    FlexTextError, MAX_INPUT_BYTES, MAX_RECORDS, compile_switch_regex, from_str, split_many,
    to_string,
};

fn nonzero(value: u32) -> NonZeroU32 {
    match NonZeroU32::new(value) {
        Some(value) => value,
        None => panic!("test count must be nonzero"),
    }
}

fn layout(command: FlexCommand) -> FlexTextLayout {
    match FlexTextLayout::new("document", command, FlexLineEnding::Lf, false) {
        Ok(layout) => layout,
        Err(error) => panic!("test layout should be valid: {error}"),
    }
}

fn parse(layout: &FlexTextLayout, input: &str) -> Instance {
    match from_str(input, &layout.schema(), layout) {
        Ok(instance) => instance,
        Err(error) => panic!("test input should parse: {error}"),
    }
}

fn scalar(value: Value) -> Instance {
    Instance::Scalar(value)
}

fn group(fields: Vec<(&str, Instance)>) -> Instance {
    Instance::Group(
        fields
            .into_iter()
            .map(|(name, value)| (name.to_string(), value))
            .collect(),
    )
}

#[test]
fn split_once_consumes_delimiter_and_store_trims_a_unicode_set() {
    let trim = match StoreTrim::new(TrimSide::Both, " _") {
        Ok(trim) => trim,
        Err(error) => panic!("trim should be valid: {error}"),
    };
    let layout = layout(FlexCommand::SplitOnce {
        name: "parts".into(),
        splitter: OnceSplitter::Delimiter("||".into()),
        first: Box::new(FlexCommand::store(
            "left",
            ScalarType::String,
            Some(trim.clone()),
        )),
        second: Box::new(FlexCommand::store("right", ScalarType::String, Some(trim))),
    });

    assert_eq!(
        parse(&layout, "\u{feff} _α_ || _β_ "),
        group(vec![(
            "parts",
            group(vec![
                ("left", scalar(Value::String("α".into()))),
                ("right", scalar(Value::String("β".into()))),
            ]),
        )])
    );
}

#[test]
fn split_once_counts_unicode_columns_and_preserves_line_terminators() {
    let columns = layout(FlexCommand::SplitOnce {
        name: "parts".into(),
        splitter: OnceSplitter::FixedColumns(nonzero(2)),
        first: Box::new(FlexCommand::store("head", ScalarType::String, None)),
        second: Box::new(FlexCommand::store("tail", ScalarType::String, None)),
    });
    assert_eq!(
        parse(&columns, "李αrest")
            .field("parts")
            .and_then(|parts| parts.field("head")),
        Some(&scalar(Value::String("李α".into())))
    );

    let lines = layout(FlexCommand::SplitOnce {
        name: "parts".into(),
        splitter: OnceSplitter::FixedLines(nonzero(2)),
        first: Box::new(FlexCommand::store("head", ScalarType::String, None)),
        second: Box::new(FlexCommand::store("tail", ScalarType::String, None)),
    });
    assert_eq!(
        parse(&lines, "one\r\ntwo\nthree")
            .field("parts")
            .and_then(|parts| parts.field("head")),
        Some(&scalar(Value::String("one\r\ntwo\n".into())))
    );

    let marker = layout(FlexCommand::SplitOnce {
        name: "parts".into(),
        splitter: OnceSplitter::LineStartingWith("@".into()),
        first: Box::new(FlexCommand::store("prefix", ScalarType::String, None)),
        second: Box::new(FlexCommand::store("marked", ScalarType::String, None)),
    });
    let parsed = parse(&marker, "prefix\r\n@record\nvalue");
    assert_eq!(
        parsed
            .field("parts")
            .and_then(|parts| parts.field("prefix")),
        Some(&scalar(Value::String("prefix\r\n".into())))
    );
    assert_eq!(
        parsed
            .field("parts")
            .and_then(|parts| parts.field("marked")),
        Some(&scalar(Value::String("@record\nvalue".into())))
    );
}

#[test]
fn line_containing_split_starts_at_the_full_matching_line_and_round_trips() {
    let layout = layout(FlexCommand::SplitOnce {
        name: "parts".into(),
        splitter: OnceSplitter::LineContaining("ITEM".into()),
        first: Box::new(FlexCommand::store("prefix", ScalarType::String, None)),
        second: Box::new(FlexCommand::store("matched", ScalarType::String, None)),
    });
    let parsed = parse(&layout, "header\r\nnotes\n  row ITEM 1\r\ntail");
    let parts = parsed.field("parts").unwrap();
    assert_eq!(
        parts.field("prefix"),
        Some(&scalar(Value::String("header\r\nnotes\n".into())))
    );
    assert_eq!(
        parts.field("matched"),
        Some(&scalar(Value::String("  row ITEM 1\r\ntail".into())))
    );

    let rendered = to_string(&layout.schema(), &parsed, &layout).unwrap();
    assert_eq!(rendered, "header\nnotes\n  row ITEM 1\ntail");
    assert_eq!(
        parse(&layout, &rendered),
        group(vec![(
            "parts",
            group(vec![
                ("prefix", scalar(Value::String("header\nnotes\n".into()))),
                (
                    "matched",
                    scalar(Value::String("  row ITEM 1\ntail".into()))
                ),
            ]),
        )])
    );

    let misplaced = group(vec![(
        "parts",
        group(vec![
            ("prefix", scalar(Value::String("header".into()))),
            (
                "matched",
                scalar(Value::String("not yet\nlater ITEM".into())),
            ),
        ]),
    )]);
    assert!(matches!(
        to_string(&layout.schema(), &misplaced, &layout),
        Err(FlexTextError::Data { .. })
    ));
}

#[test]
fn line_containing_split_keeps_all_input_when_no_line_matches() {
    let layout = layout(FlexCommand::SplitOnce {
        name: "parts".into(),
        splitter: OnceSplitter::LineContaining("ITEM".into()),
        first: Box::new(FlexCommand::store("prefix", ScalarType::String, None)),
        second: Box::new(FlexCommand::store("matched", ScalarType::String, None)),
    });
    let parsed = parse(&layout, "header\nnotes");
    let parts = parsed.field("parts").unwrap();
    assert_eq!(
        parts.field("prefix"),
        Some(&scalar(Value::String("header\nnotes".into())))
    );
    assert_eq!(
        parts.field("matched"),
        Some(&scalar(Value::String(String::new())))
    );
}

#[test]
fn line_marker_split_retains_markers_and_discards_leading_prefix() {
    let layout = layout(FlexCommand::SplitMany {
        name: "records".into(),
        splitter: ManySplitter::LinesStartingWith("@".into()),
        child: Box::new(FlexCommand::store("raw", ScalarType::String, None)),
    });
    let parsed = parse(&layout, "ignored\r\n@one\r\ndata\n@two");
    let records = match parsed.field("records") {
        Some(Instance::Repeated(records)) => records,
        other => panic!("records should repeat, got {other:?}"),
    };
    assert_eq!(records.len(), 2);
    assert_eq!(
        records[0].field("raw"),
        Some(&scalar(Value::String("@one\r\ndata\n".into())))
    );
    assert_eq!(
        records[1].field("raw"),
        Some(&scalar(Value::String("@two".into())))
    );
}

fn delimiter_split_layout() -> FlexTextLayout {
    layout(FlexCommand::SplitMany {
        name: "records".into(),
        splitter: ManySplitter::Delimiter("|".into()),
        child: Box::new(FlexCommand::store("raw", ScalarType::String, None)),
    })
}

#[test]
fn delimiter_split_roundtrips_representable_empty_items() {
    let layout = delimiter_split_layout();
    let instance = group(vec![(
        "records",
        Instance::Repeated(vec![
            group(vec![("raw", scalar(Value::String("first".into())))]),
            group(vec![("raw", scalar(Value::String(String::new())))]),
            group(vec![("raw", scalar(Value::String("third".into())))]),
        ]),
    )]);

    let output = to_string(&layout.schema(), &instance, &layout).unwrap();
    assert_eq!(output, "first||third");
    assert_eq!(parse(&layout, &output), instance);
}

#[test]
fn delimiter_split_rejects_unrepresentable_writes() {
    let layout = delimiter_split_layout();
    let containing_delimiter = group(vec![(
        "records",
        Instance::Repeated(vec![group(vec![(
            "raw",
            scalar(Value::String("first|second".into())),
        )])]),
    )]);
    let error = to_string(&layout.schema(), &containing_delimiter, &layout).unwrap_err();
    assert!(matches!(
        error,
        FlexTextError::Data { message, .. } if message.contains("contains the record delimiter")
    ));

    let trailing_empty = group(vec![(
        "records",
        Instance::Repeated(vec![
            group(vec![("raw", scalar(Value::String("first".into())))]),
            group(vec![("raw", scalar(Value::String(String::new())))]),
        ]),
    )]);
    let error = to_string(&layout.schema(), &trailing_empty, &layout).unwrap_err();
    assert!(matches!(
        error,
        FlexTextError::Data { message, .. } if message.contains("final delimiter-split item is empty")
    ));
}

#[test]
fn delimiter_split_stops_at_the_record_limit() {
    let input = "|".repeat(MAX_RECORDS + 1);
    let error = split_many(&input, &ManySplitter::Delimiter("|".into())).unwrap_err();
    assert!(matches!(error, FlexTextError::TooManyRecords));
}

#[test]
fn fixed_line_writes_preserve_chunk_boundaries_without_a_trailing_delimiter() {
    let layout = layout(FlexCommand::SplitMany {
        name: "records".into(),
        splitter: ManySplitter::FixedLines(nonzero(1)),
        child: Box::new(FlexCommand::store("raw", ScalarType::String, None)),
    });
    let instance = group(vec![(
        "records",
        Instance::Repeated(vec![
            group(vec![("raw", scalar(Value::String("first".into())))]),
            group(vec![("raw", scalar(Value::String("second".into())))]),
        ]),
    )]);

    let output = to_string(&layout.schema(), &instance, &layout).unwrap();

    assert_eq!(output, "first\nsecond");
    let reparsed = parse(&layout, &output);
    let records = reparsed
        .field("records")
        .and_then(Instance::as_repeated)
        .unwrap();
    assert_eq!(
        records[0].field("raw"),
        Some(&scalar(Value::String("first\n".into())))
    );
}

#[test]
fn switch_runs_all_matching_arms_and_default_only_when_none_match() {
    let arms = vec![
        SwitchArm::new(
            "A",
            FlexCommand::store("starts_a", ScalarType::String, None),
        )
        .unwrap(),
        SwitchArm::new(
            "AB",
            FlexCommand::store("starts_ab", ScalarType::String, None),
        )
        .unwrap(),
    ];
    let command = FlexCommand::Switch {
        name: "choice".into(),
        mode: SwitchMode::AllPossible,
        arms,
        default: Some(Box::new(FlexCommand::store(
            "fallback",
            ScalarType::String,
            None,
        ))),
    };
    let layout = layout(command);
    let matched = parse(&layout, "AB-value");
    let choice = matched.field("choice").expect("choice should exist");
    assert!(choice.field("starts_a").is_some());
    assert!(choice.field("starts_ab").is_some());
    assert!(choice.field("fallback").is_none());
    assert_eq!(
        to_string(&layout.schema(), &matched, &layout).unwrap(),
        "AB-value"
    );
    assert_eq!(parse(&layout, "AB-value"), matched);

    let defaulted = parse(&layout, "Z-value");
    let choice = defaulted.field("choice").expect("choice should exist");
    assert!(choice.field("starts_a").is_none());
    assert_eq!(
        choice.field("fallback"),
        Some(&scalar(Value::String("Z-value".into())))
    );
}

#[test]
fn switch_regex_arms_search_the_complete_input() {
    let command = FlexCommand::Switch {
        name: "choice".into(),
        mode: SwitchMode::AllPossible,
        arms: vec![
            SwitchArm::new_contains_regex(
                r"status=[0-9]{3}\b",
                FlexCommand::store("status", ScalarType::String, None),
            )
            .unwrap(),
        ],
        default: Some(Box::new(FlexCommand::store(
            "fallback",
            ScalarType::String,
            None,
        ))),
    };
    let layout = layout(command);
    let matched = parse(&layout, "prefix status=204 suffix");
    let choice = matched.field("choice").unwrap();
    assert_eq!(
        choice.field("status"),
        Some(&scalar(Value::String("prefix status=204 suffix".into())))
    );

    let defaulted = parse(&layout, "status=ok");
    assert!(
        defaulted
            .field("choice")
            .and_then(|choice| choice.field("fallback"))
            .is_some()
    );
}

#[test]
fn runtime_regex_compile_failures_are_typed() {
    let error = compile_switch_regex("[", "document/choice").unwrap_err();
    assert!(matches!(
        error,
        FlexTextError::SwitchRegex { path, .. } if path == "document/choice"
    ));
}

#[test]
fn first_match_switch_stops_after_the_first_matching_arm() {
    let layout = layout(FlexCommand::Switch {
        name: "choice".into(),
        mode: SwitchMode::FirstMatch,
        arms: vec![
            SwitchArm::new("A", FlexCommand::store("first", ScalarType::String, None)).unwrap(),
            SwitchArm::new("AB", FlexCommand::store("second", ScalarType::String, None)).unwrap(),
        ],
        default: None,
    });

    let parsed = parse(&layout, "AB-value");
    let choice = parsed.field("choice").unwrap();
    assert!(choice.field("first").is_some());
    assert!(choice.field("second").is_none());
    assert_eq!(
        to_string(&layout.schema(), &parsed, &layout).unwrap(),
        "AB-value"
    );

    let impossible = group(vec![(
        "choice",
        group(vec![
            ("first", scalar(Value::String("AB-value".into()))),
            ("second", scalar(Value::String("AB-value".into()))),
        ]),
    )]);
    assert!(matches!(
        to_string(&layout.schema(), &impossible, &layout),
        Err(FlexTextError::Data { message, .. })
            if message.contains("more than one arm")
    ));
}

#[test]
fn ignored_split_items_are_pruned_but_an_empty_stored_string_is_retained() {
    let trim = StoreTrim::new(TrimSide::Both, "\r\n").unwrap();
    let child = FlexCommand::Switch {
        name: "selected".into(),
        mode: SwitchMode::AllPossible,
        arms: vec![SwitchArm::new("skip", FlexCommand::Ignore).unwrap()],
        default: Some(Box::new(FlexCommand::store(
            "value",
            ScalarType::String,
            Some(trim),
        ))),
    };
    let layout = layout(FlexCommand::SplitMany {
        name: "lines".into(),
        splitter: ManySplitter::FixedLines(nonzero(1)),
        child: Box::new(child),
    });
    let parsed = parse(&layout, "skip this\n\nkeep\n");
    let lines = match parsed.field("lines") {
        Some(Instance::Repeated(lines)) => lines,
        other => panic!("lines should repeat, got {other:?}"),
    };
    assert_eq!(lines.len(), 2);
    assert_eq!(
        lines[0]
            .field("selected")
            .and_then(|selected| selected.field("value")),
        Some(&scalar(Value::String(String::new())))
    );
}

fn fixed_layout(line_ending: FlexLineEnding, bom: bool) -> FlexTextLayout {
    let fields = vec![
        FixedWidthRecordField::new("name", ScalarType::String, nonzero(3)).unwrap(),
        FixedWidthRecordField::new("count", ScalarType::Int, nonzero(2)).unwrap(),
    ];
    FlexTextLayout::new(
        "document",
        FlexCommand::FixedWidthRecords {
            name: "rows".into(),
            fields,
            fill_char: ' ',
            record_delimiters: true,
            treat_empty_as_absent: true,
        },
        line_ending,
        bom,
    )
    .unwrap()
}

#[test]
fn fixed_width_records_use_unicode_columns_alignment_bom_and_crlf_output() {
    let layout = fixed_layout(FlexLineEnding::Crlf, true);
    let parsed = parse(&layout, "\u{feff}李   7\r\nAda12");
    let rows = match parsed.field("rows") {
        Some(Instance::Repeated(rows)) => rows,
        other => panic!("rows should repeat, got {other:?}"),
    };
    assert_eq!(
        rows[0].field("name"),
        Some(&scalar(Value::String("李".into())))
    );
    assert_eq!(rows[0].field("count"), Some(&scalar(Value::Int(7))));

    let output = to_string(&layout.schema(), &parsed, &layout).unwrap();
    assert_eq!(output, "\u{feff}李   7\r\nAda12");
    assert!(!output.ends_with("\r\n"));
}

#[test]
fn delimited_fixed_width_records_allow_omitted_final_string_padding() {
    let layout = FlexTextLayout::new(
        "document",
        FlexCommand::FixedWidthRecords {
            name: "rows".into(),
            fields: vec![
                FixedWidthRecordField::new("count", ScalarType::Int, nonzero(2)).unwrap(),
                FixedWidthRecordField::new("name", ScalarType::String, nonzero(3)).unwrap(),
            ],
            fill_char: ' ',
            record_delimiters: true,
            treat_empty_as_absent: true,
        },
        FlexLineEnding::Lf,
        false,
    )
    .unwrap();
    let parsed = parse(&layout, "12Ada\n 7Bo");
    let rows = parsed
        .field("rows")
        .and_then(Instance::as_repeated)
        .unwrap();

    assert_eq!(
        rows[1].field("name"),
        Some(&scalar(Value::String("Bo".into())))
    );
    assert_eq!(rows[1].field("count"), Some(&scalar(Value::Int(7))));
}

#[test]
fn fixed_width_records_keep_non_final_and_contiguous_short_fields_strict() {
    let layout = fixed_layout(FlexLineEnding::Lf, false);
    assert!(from_str("Bo\n", &layout.schema(), &layout).is_err());

    let contiguous = FlexTextLayout::new(
        "document",
        FlexCommand::FixedWidthRecords {
            name: "rows".into(),
            fields: vec![
                FixedWidthRecordField::new("count", ScalarType::Int, nonzero(2)).unwrap(),
                FixedWidthRecordField::new("name", ScalarType::String, nonzero(3)).unwrap(),
            ],
            fill_char: ' ',
            record_delimiters: false,
            treat_empty_as_absent: true,
        },
        FlexLineEnding::Lf,
        false,
    )
    .unwrap();
    assert!(from_str("12Bo", &contiguous.schema(), &contiguous).is_err());
}

#[test]
fn fixed_width_records_honor_fill_empty_and_contiguous_record_settings() {
    let layout = FlexTextLayout::new(
        "document",
        FlexCommand::FixedWidthRecords {
            name: "rows".into(),
            fields: vec![
                FixedWidthRecordField::new("value", ScalarType::String, nonzero(3)).unwrap(),
            ],
            fill_char: '_',
            record_delimiters: false,
            treat_empty_as_absent: false,
        },
        FlexLineEnding::Lf,
        false,
    )
    .unwrap();
    let parsed = parse(&layout, "A_____B__");
    let rows = parsed
        .field("rows")
        .and_then(Instance::as_repeated)
        .unwrap();
    assert_eq!(rows.len(), 3);
    assert_eq!(
        rows[1].field("value"),
        Some(&scalar(Value::String(String::new())))
    );
    assert_eq!(
        to_string(&layout.schema(), &parsed, &layout).unwrap(),
        "A_____B__"
    );
}

fn delimited_layout() -> FlexTextLayout {
    let dialect = DelimitedDialect::new(';', "\n", '"', '\\').unwrap();
    let fields = vec![
        DelimitedRecordField::new("text", ScalarType::String).unwrap(),
        DelimitedRecordField::new("count", ScalarType::Int).unwrap(),
    ];
    FlexTextLayout::new(
        "document",
        FlexCommand::DelimitedRecords {
            name: "rows".into(),
            dialect,
            fields,
        },
        FlexLineEnding::Crlf,
        false,
    )
    .unwrap()
}

#[test]
fn delimited_records_honor_separator_quote_escape_and_lf_or_crlf_input() {
    let layout = delimited_layout();
    let parsed = parse(&layout, "\"a;b\";2\r\n\"say \\\"hi\\\"\";3");
    let rows = match parsed.field("rows") {
        Some(Instance::Repeated(rows)) => rows,
        other => panic!("rows should repeat, got {other:?}"),
    };
    assert_eq!(
        rows[1].field("text"),
        Some(&scalar(Value::String("say \"hi\"".into())))
    );
    assert_eq!(
        to_string(&layout.schema(), &parsed, &layout).unwrap(),
        "\"a;b\";2\r\n\"say \\\"hi\\\"\";3"
    );
}

#[test]
fn delimited_records_support_multi_character_field_separators() {
    let dialect = DelimitedDialect::new_with_field_separator("*#*", "\n", '"', '\\').unwrap();
    let layout = FlexTextLayout::new(
        "document",
        FlexCommand::DelimitedRecords {
            name: "rows".into(),
            dialect,
            fields: vec![
                DelimitedRecordField::new("text", ScalarType::String).unwrap(),
                DelimitedRecordField::new("count", ScalarType::Int).unwrap(),
            ],
        },
        FlexLineEnding::Lf,
        false,
    )
    .unwrap();
    let parsed = parse(&layout, "Ada*#*3\nGrace*#*5");
    let rows = parsed
        .field("rows")
        .and_then(Instance::as_repeated)
        .unwrap();
    assert_eq!(
        rows[1].field("count").and_then(Instance::as_scalar),
        Some(&Value::Int(5))
    );
    assert_eq!(
        to_string(&layout.schema(), &parsed, &layout).unwrap(),
        "Ada*#*3\nGrace*#*5"
    );
}

#[test]
fn writer_lexically_coerces_string_values_for_typed_scalars() {
    let cases = [
        (ScalarType::Int, " 42 ", "42"),
        (ScalarType::Float, " 1.5 ", "1.5"),
        (ScalarType::Bool, " true ", "true"),
    ];
    for (ty, input, expected) in cases {
        let layout = layout(FlexCommand::store("value", ty, None));
        let instance = group(vec![("value", scalar(Value::String(input.into())))]);
        assert_eq!(
            to_string(&layout.schema(), &instance, &layout).unwrap(),
            expected
        );
    }

    for (ty, input) in [
        (ScalarType::Int, "12.5"),
        (ScalarType::Float, "NaN"),
        (ScalarType::Bool, "TRUE"),
    ] {
        let layout = layout(FlexCommand::store("value", ty, None));
        let instance = group(vec![("value", scalar(Value::String(input.into())))]);
        assert!(matches!(
            to_string(&layout.schema(), &instance, &layout),
            Err(FlexTextError::Data { .. })
        ));
    }
}

#[test]
fn runtime_rejects_schema_mismatches_and_oversized_input() {
    let layout = layout(FlexCommand::store("value", ScalarType::String, None));
    let wrong = SchemaNode::group("other", vec![]);
    assert!(matches!(
        from_str("x", &wrong, &layout),
        Err(FlexTextError::SchemaMismatch)
    ));

    let oversized = "x".repeat(MAX_INPUT_BYTES + 1);
    assert!(matches!(
        from_str(&oversized, &layout.schema(), &layout),
        Err(FlexTextError::InputTooLarge)
    ));
}
