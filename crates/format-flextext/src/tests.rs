use std::num::NonZeroU32;

use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{
    DelimitedDialect, DelimitedRecordField, FixedWidthRecordField, FlexCommand, FlexLineEnding,
    FlexTextLayout, ManySplitter, OnceSplitter, StoreTrim, SwitchArm, TrimSide,
};

use crate::{FlexTextError, MAX_INPUT_BYTES, from_str, to_string};

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

    let defaulted = parse(&layout, "Z-value");
    let choice = defaulted.field("choice").expect("choice should exist");
    assert!(choice.field("starts_a").is_none());
    assert_eq!(
        choice.field("fallback"),
        Some(&scalar(Value::String("Z-value".into())))
    );
}

#[test]
fn ignored_split_items_are_pruned_but_an_empty_stored_string_is_retained() {
    let trim = StoreTrim::new(TrimSide::Both, "\r\n").unwrap();
    let child = FlexCommand::Switch {
        name: "selected".into(),
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
