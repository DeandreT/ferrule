mod support;

use std::path::Path;

use ir::{Instance, Value};
use support::{
    ConnectionStyle, GeneratedAggregate, GeneratedSequence, ScalarContext,
    ScalarLiteral as Literal, ScalarMfdBuilder,
};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

#[derive(Clone)]
struct FunctionCase {
    name: &'static str,
    library: &'static str,
    arguments: Vec<Literal>,
    output_type: &'static str,
    nondeterministic: bool,
}

#[derive(Clone, Copy)]
struct GraphEncoding {
    style: ConnectionStyle,
    reverse_components: bool,
    key_offset: u32,
}

impl Default for GraphEncoding {
    fn default() -> Self {
        Self {
            style: ConnectionStyle::Graph,
            reverse_components: false,
            key_offset: 0,
        }
    }
}

impl FunctionCase {
    fn new(
        name: &'static str,
        library: &'static str,
        arguments: Vec<Literal>,
        output_type: &'static str,
    ) -> Self {
        Self {
            name,
            library,
            arguments,
            output_type,
            nondeterministic: false,
        }
    }

    fn nondeterministic(mut self) -> Self {
        self.nondeterministic = true;
        self
    }
}

fn text(value: &str) -> Literal {
    Literal::string(value)
}

fn int(value: i64) -> Literal {
    Literal::Integer(value)
}

fn decimal(value: f64) -> Literal {
    Literal::Decimal(value)
}

fn boolean(value: bool) -> Literal {
    Literal::Boolean(value)
}

fn scalar_cases() -> Vec<FunctionCase> {
    vec![
        FunctionCase::new(
            "concat",
            "core",
            vec![text("left"), text("right")],
            "string",
        ),
        FunctionCase::new("add", "core", vec![int(7), int(5)], "integer"),
        FunctionCase::new("subtract", "core", vec![int(7), int(5)], "integer"),
        FunctionCase::new("multiply", "core", vec![int(7), int(5)], "integer"),
        FunctionCase::new("divide", "core", vec![int(10), int(4)], "decimal"),
        FunctionCase::new("equal", "core", vec![int(7), int(7)], "boolean"),
        FunctionCase::new("not-equal", "core", vec![int(7), int(5)], "boolean"),
        FunctionCase::new("greater", "core", vec![int(7), int(5)], "boolean"),
        FunctionCase::new("less", "core", vec![int(5), int(7)], "boolean"),
        FunctionCase::new("greater-or-equal", "core", vec![int(7), int(7)], "boolean"),
        FunctionCase::new("less-or-equal", "core", vec![int(5), int(7)], "boolean"),
        FunctionCase::new(
            "logical-and",
            "core",
            vec![boolean(true), boolean(false)],
            "boolean",
        ),
        FunctionCase::new(
            "logical-or",
            "core",
            vec![boolean(true), boolean(false)],
            "boolean",
        ),
        FunctionCase::new("logical-not", "core", vec![boolean(false)], "boolean"),
        FunctionCase::new("string-length", "xpath2", vec![text("hello")], "integer"),
        FunctionCase::new(
            "contains",
            "xpath2",
            vec![text("hello"), text("ell")],
            "boolean",
        ),
        FunctionCase::new(
            "starts-with",
            "xpath2",
            vec![text("hello"), text("he")],
            "boolean",
        ),
        FunctionCase::new(
            "ends-with",
            "xpath2",
            vec![text("hello"), text("lo")],
            "boolean",
        ),
        FunctionCase::new(
            "matches",
            "xpath2",
            vec![text("hello"), text("^h.*o$")],
            "boolean",
        ),
        FunctionCase::new(
            "replace",
            "xpath2",
            vec![text("hello"), text("ell"), text("ipp")],
            "string",
        ),
        FunctionCase::new("upper-case", "xpath2", vec![text("Mixed")], "string"),
        FunctionCase::new("lower-case", "xpath2", vec![text("Mixed")], "string"),
        FunctionCase::new("string", "xpath2", vec![int(42)], "string"),
        FunctionCase::new("number", "xpath2", vec![text("42.5")], "decimal"),
        FunctionCase::new("numeric", "core", vec![text("42.5")], "boolean"),
        FunctionCase::new("boolean", "xpath2", vec![text("false")], "boolean"),
        FunctionCase::new("positive", "core", vec![decimal(-2.5)], "decimal"),
        FunctionCase::new("floor", "xpath2", vec![decimal(-2.5)], "decimal"),
        FunctionCase::new("create-guid", "lang", Vec::new(), "string").nondeterministic(),
        FunctionCase::new(
            "format-number",
            "xpath2",
            vec![decimal(1234.5), text("#,##0.0")],
            "string",
        ),
        FunctionCase::new(
            "format-date",
            "xpath2",
            vec![text("2024-02-29"), text("[Y]-[M01]-[D01]")],
            "string",
        ),
        FunctionCase::new(
            "format-dateTime",
            "xpath2",
            vec![
                text("2024-02-29T13:14:15Z"),
                text("[Y]-[M01]-[D01] [H01]:[m01]"),
            ],
            "string",
        ),
        FunctionCase::new(
            "format-time",
            "xpath2",
            vec![text("13:14:15Z"), text("[H01]:[m01]:[s01]")],
            "string",
        ),
        FunctionCase::new("trim", "lang", vec![text("  hello  ")], "string"),
        FunctionCase::new("left", "lang", vec![text("hello"), int(2)], "string"),
        FunctionCase::new("right", "lang", vec![text("hello"), int(2)], "string"),
        FunctionCase::new("left-trim", "lang", vec![text("  hello  ")], "string"),
        FunctionCase::new("right-trim", "lang", vec![text("  hello  ")], "string"),
        FunctionCase::new(
            "pad-string-left",
            "lang",
            vec![text("7"), int(3), text("0")],
            "string",
        ),
        FunctionCase::new(
            "pad-string-right",
            "lang",
            vec![text("7"), int(3), text("0")],
            "string",
        ),
        FunctionCase::new(
            "substring",
            "xpath2",
            vec![text("hello"), int(2), int(3)],
            "string",
        ),
        FunctionCase::new(
            "substring-before",
            "xpath2",
            vec![text("left:right"), text(":")],
            "string",
        ),
        FunctionCase::new(
            "substring-after",
            "xpath2",
            vec![text("left:right"), text(":")],
            "string",
        ),
        FunctionCase::new(
            "normalize-space",
            "xpath2",
            vec![text(" a \t b ")],
            "string",
        ),
        FunctionCase::new("empty", "xpath2", vec![text("")], "boolean"),
        FunctionCase::new(
            "get-folder",
            "lang",
            vec![text("folder/file.txt")],
            "string",
        ),
        FunctionCase::new(
            "remove-folder",
            "lang",
            vec![text("folder/file.txt")],
            "string",
        ),
        FunctionCase::new(
            "resolve-filepath",
            "lang",
            vec![text("folder/base"), text("../file.txt")],
            "string",
        ),
        FunctionCase::new("is-xsi-nil", "core", vec![text("present")], "boolean"),
        FunctionCase::new(
            "substitute-missing-with-xsi-nil",
            "core",
            vec![text("present")],
            "string",
        ),
        FunctionCase::new("exists", "core", vec![text("present")], "boolean"),
        FunctionCase::new(
            "round-precision",
            "xpath2",
            vec![decimal(2.55), int(1)],
            "decimal",
        ),
        FunctionCase::new(
            "date-from-datetime",
            "xpath2",
            vec![text("2024-02-29T13:14:15Z")],
            "string",
        ),
        FunctionCase::new(
            "year-from-datetime",
            "xpath2",
            vec![text("2024-02-29T13:14:15Z")],
            "integer",
        ),
        FunctionCase::new(
            "month-from-datetime",
            "xpath2",
            vec![text("2024-02-29T13:14:15Z")],
            "integer",
        ),
        FunctionCase::new(
            "day-from-datetime",
            "xpath2",
            vec![text("2024-02-29T13:14:15Z")],
            "integer",
        ),
        FunctionCase::new(
            "weekday",
            "lang",
            vec![text("2024-02-29T13:14:15Z")],
            "integer",
        ),
        FunctionCase::new(
            "hours-from-datetime",
            "xpath2",
            vec![text("2024-02-29T13:14:15Z")],
            "integer",
        ),
        FunctionCase::new(
            "minutes-from-datetime",
            "xpath2",
            vec![text("2024-02-29T13:14:15Z")],
            "integer",
        ),
        FunctionCase::new(
            "time-from-datetime",
            "xpath2",
            vec![text("2024-02-29T13:14:15Z")],
            "string",
        ),
        FunctionCase::new(
            "datetime-from-date-and-time",
            "lang",
            vec![text("2024-02-29"), text("13:14:15Z")],
            "string",
        ),
        FunctionCase::new(
            "datetime-from-parts",
            "lang",
            vec![int(2024), int(2), int(29), int(13), int(14), int(15)],
            "string",
        ),
        FunctionCase::new(
            "duration-from-parts",
            "lang",
            vec![int(1), int(2), int(3), int(4), int(5), int(6)],
            "string",
        ),
        FunctionCase::new(
            "datetime-add",
            "lang",
            vec![text("2024-02-29T13:14:15Z"), text("P1D")],
            "string",
        ),
        FunctionCase::new(
            "parse-date",
            "lang",
            vec![text("29/02/2024"), text("[D01]/[M01]/[Y]")],
            "string",
        ),
        FunctionCase::new(
            "parse-dateTime",
            "lang",
            vec![
                text("29/02/2024 13:14:15"),
                text("[D01]/[M01]/[Y] [H01]:[m01]:[s01]"),
            ],
            "string",
        ),
        FunctionCase::new(
            "parse-time",
            "lang",
            vec![text("13:14:15"), text("[H01]:[m01]:[s01]")],
            "string",
        ),
        FunctionCase::new(
            "to-datetime",
            "edifact",
            vec![text("20240229131415"), text("204")],
            "string",
        ),
        FunctionCase::new(
            "substitute-null",
            "db",
            vec![text("present"), text("fallback")],
            "string",
        ),
        FunctionCase::new(
            "get-fileext",
            "lang",
            vec![text("folder/archive.tar.gz")],
            "string",
        ),
        FunctionCase::new("sleep", "core", vec![text("value"), int(0)], "string"),
        FunctionCase::new("is-null", "db", vec![text("present")], "boolean"),
        FunctionCase::new("is-not-null", "db", vec![text("present")], "boolean"),
        FunctionCase::new("now", "lang", Vec::new(), "string"),
        FunctionCase::new("current-dateTime", "xpath2", Vec::new(), "string"),
        FunctionCase::new("mfd-filepath", "core", Vec::new(), "string"),
        FunctionCase::new("main-mfd-filepath", "core", Vec::new(), "string"),
        FunctionCase::new("set-empty", "core", Vec::new(), "string"),
        FunctionCase::new("set-xsi-nil", "core", Vec::new(), "string"),
        FunctionCase::new("not-exists", "core", vec![text("present")], "boolean"),
        FunctionCase::new("xbrl-measure-currency", "xbrl", vec![text("USD")], "string"),
        FunctionCase::new("xbrl-measure-shares", "xbrl", Vec::new(), "string"),
    ]
}

fn execute_case(
    case: &FunctionCase,
    context: ScalarContext,
    style: ConnectionStyle,
    reverse_components: bool,
    key_offset: u32,
) -> TestResult<Value> {
    let fixture = ScalarMfdBuilder::new(
        format!("{}_{}", case.name, context_name(context)),
        case.name,
        case.library,
        case.arguments.clone(),
        case.output_type,
    )
    .context(context)
    .connection_style(style)
    .reverse_components(reverse_components)
    .key_offset(key_offset)
    .write()?;
    let imported = mfd::import(fixture.design())?;
    assert!(
        imported.warnings.is_empty(),
        "{} in {context:?}: {:?}",
        case.name,
        imported.warnings
    );
    let validation = engine::validate(&imported.project);
    assert!(
        validation.is_empty(),
        "{} in {context:?}: {validation:?}",
        case.name
    );

    let source = format_xml::from_str(
        "<Source><Seed>fixture</Seed></Source>",
        &imported.project.source,
    )?;
    let execution = engine::ExecutionContext::new(Path::new("/fixtures/scenario.mfd"))
        .with_current_datetime("2026-07-24T12:34:56-07:00");
    let output = engine::run_with_context(&imported.project, &source, &execution)?;
    output
        .field("Result")
        .and_then(Instance::as_scalar)
        .cloned()
        .ok_or_else(|| format!("{} in {context:?} produced no Result scalar", case.name).into())
}

fn context_name(context: ScalarContext) -> &'static str {
    match context {
        ScalarContext::Main => "main",
        ScalarContext::UserDefined => "udf",
        ScalarContext::NestedUserDefined => "nested_udf",
    }
}

fn assert_guid(value: &Value, context: ScalarContext) {
    let Value::String(value) = value else {
        panic!("create-guid in {context:?} did not produce text");
    };
    assert_eq!(value.len(), 32, "create-guid in {context:?}");
    assert!(
        value.bytes().all(|byte| byte.is_ascii_hexdigit()),
        "create-guid in {context:?}"
    );
}

fn execute_generated_item_at(
    sequence: GeneratedSequence,
    arguments: Vec<Literal>,
    context: ScalarContext,
    style: ConnectionStyle,
    reverse_components: bool,
    key_offset: u32,
) -> TestResult<(Value, mapping::Project)> {
    let fixture = ScalarMfdBuilder::generated_item_at(
        format!("generated_item_at_{sequence:?}_{context:?}"),
        sequence,
        arguments,
        match sequence {
            GeneratedSequence::Generate => "integer",
            GeneratedSequence::Tokenize
            | GeneratedSequence::TokenizeByLength
            | GeneratedSequence::TokenizeRegex => "string",
        },
    )
    .context(context)
    .connection_style(style)
    .reverse_components(reverse_components)
    .key_offset(key_offset)
    .write()?;
    let imported = mfd::import(fixture.design())?;
    assert!(
        imported.warnings.is_empty(),
        "{sequence:?} in {context:?}: {:?}",
        imported.warnings
    );
    let validation = engine::validate(&imported.project);
    assert!(
        validation.is_empty(),
        "{sequence:?} in {context:?}: {validation:?}"
    );
    codegen::lower(&imported.project)
        .map_err(|diagnostics| format!("{sequence:?} did not lower: {diagnostics:?}"))?;
    let source = format_xml::from_str(
        "<Source><Seed>fixture</Seed></Source>",
        &imported.project.source,
    )?;
    let output = engine::run(&imported.project, &source)?;
    let value = output
        .field("Result")
        .and_then(Instance::as_scalar)
        .cloned()
        .ok_or_else(|| format!("{sequence:?} in {context:?} produced no Result scalar"))?;

    let roundtrip = fixture.design().with_file_name("roundtrip.mfd");
    let export_warnings = mfd::export(&imported.project, &roundtrip)?;
    assert!(
        export_warnings.is_empty(),
        "{sequence:?} in {context:?} export: {export_warnings:?}"
    );
    let reimported = mfd::import(&roundtrip)?;
    assert!(
        reimported.warnings.is_empty(),
        "{sequence:?} in {context:?} re-import: {:?}",
        reimported.warnings
    );
    let rerun = engine::run(&reimported.project, &source)?;
    assert_eq!(
        rerun.field("Result").and_then(Instance::as_scalar),
        Some(&value),
        "{sequence:?} in {context:?} changed after export and re-import"
    );
    Ok((value, imported.project))
}

fn execute_generated_exists(
    sequence: GeneratedSequence,
    arguments: Vec<Literal>,
    compare_position: bool,
    context: ScalarContext,
    style: ConnectionStyle,
    reverse_components: bool,
    key_offset: u32,
) -> TestResult<(Value, mapping::Project)> {
    let tag = format!("generated_exists_{sequence:?}_{context:?}");
    let builder = if compare_position {
        ScalarMfdBuilder::generated_exists_at_position(tag, sequence, arguments)
    } else {
        ScalarMfdBuilder::generated_exists(tag, sequence, arguments)
    };
    let fixture = builder
        .context(context)
        .connection_style(style)
        .reverse_components(reverse_components)
        .key_offset(key_offset)
        .write()?;
    let imported = mfd::import(fixture.design())?;
    assert!(
        imported.warnings.is_empty(),
        "{sequence:?} in {context:?}: {:?}",
        imported.warnings
    );
    let validation = engine::validate(&imported.project);
    assert!(
        validation.is_empty(),
        "{sequence:?} in {context:?}: {validation:?}"
    );
    codegen::lower(&imported.project)
        .map_err(|diagnostics| format!("{sequence:?} did not lower: {diagnostics:?}"))?;
    let source = format_xml::from_str(
        "<Source><Seed>fixture</Seed></Source>",
        &imported.project.source,
    )?;
    let output = engine::run(&imported.project, &source)?;
    let value = output
        .field("Result")
        .and_then(Instance::as_scalar)
        .cloned()
        .ok_or_else(|| format!("{sequence:?} in {context:?} produced no Result scalar"))?;

    let roundtrip = fixture.design().with_file_name("roundtrip.mfd");
    let export_warnings = mfd::export(&imported.project, &roundtrip)?;
    assert!(
        export_warnings.is_empty(),
        "{sequence:?} in {context:?} export: {export_warnings:?}"
    );
    let reimported = mfd::import(&roundtrip)?;
    assert!(
        reimported.warnings.is_empty(),
        "{sequence:?} in {context:?} re-import: {:?}",
        reimported.warnings
    );
    let rerun = engine::run(&reimported.project, &source)?;
    assert_eq!(
        rerun.field("Result").and_then(Instance::as_scalar),
        Some(&value),
        "{sequence:?} in {context:?} changed after export and re-import"
    );
    Ok((value, imported.project))
}

fn execute_generated_aggregate(
    sequence: GeneratedSequence,
    operation: GeneratedAggregate,
    arguments: Vec<Literal>,
    output_type: &str,
    context: ScalarContext,
    encoding: GraphEncoding,
) -> TestResult<(Value, mapping::Project)> {
    let fixture = ScalarMfdBuilder::generated_aggregate(
        format!("generated_{operation:?}_{sequence:?}_{context:?}"),
        sequence,
        operation,
        arguments,
        output_type,
    )
    .context(context)
    .connection_style(encoding.style)
    .reverse_components(encoding.reverse_components)
    .key_offset(encoding.key_offset)
    .write()?;
    let imported = mfd::import(fixture.design())?;
    assert!(
        imported.warnings.is_empty(),
        "{operation:?} over {sequence:?} in {context:?}: {:?}",
        imported.warnings
    );
    let validation = engine::validate(&imported.project);
    assert!(
        validation.is_empty(),
        "{operation:?} over {sequence:?} in {context:?}: {validation:?}"
    );
    codegen::lower(&imported.project).map_err(|diagnostics| {
        format!("{operation:?} over {sequence:?} did not lower: {diagnostics:?}")
    })?;
    let source = format_xml::from_str(
        "<Source><Seed>fixture</Seed></Source>",
        &imported.project.source,
    )?;
    let output = engine::run(&imported.project, &source)?;
    let value = output
        .field("Result")
        .and_then(Instance::as_scalar)
        .cloned()
        .ok_or_else(|| {
            format!("{operation:?} over {sequence:?} in {context:?} produced no Result scalar")
        })?;

    let roundtrip = fixture.design().with_file_name("roundtrip.mfd");
    let export_warnings = mfd::export(&imported.project, &roundtrip)?;
    assert!(
        export_warnings.is_empty(),
        "{operation:?} over {sequence:?} in {context:?} export: {export_warnings:?}"
    );
    let reimported = mfd::import(&roundtrip)?;
    assert!(
        reimported.warnings.is_empty(),
        "{operation:?} over {sequence:?} in {context:?} re-import: {:?}",
        reimported.warnings
    );
    let rerun = engine::run(&reimported.project, &source)?;
    assert_eq!(
        rerun.field("Result").and_then(Instance::as_scalar),
        Some(&value),
        "{operation:?} over {sequence:?} in {context:?} changed after export and re-import"
    );
    Ok((value, imported.project))
}

#[test]
fn scalar_functions_are_equivalent_in_main_udf_and_nested_udf_contexts() -> TestResult {
    for (case_index, case) in scalar_cases().iter().enumerate() {
        let baseline = execute_case(case, ScalarContext::Main, ConnectionStyle::Graph, false, 0)?;
        let udf = execute_case(
            case,
            ScalarContext::UserDefined,
            ConnectionStyle::Graph,
            false,
            1_000 + case_index as u32,
        )?;
        let nested = execute_case(
            case,
            ScalarContext::NestedUserDefined,
            ConnectionStyle::Graph,
            false,
            2_000 + case_index as u32,
        )?;

        if case.nondeterministic {
            assert_guid(&baseline, ScalarContext::Main);
            assert_guid(&udf, ScalarContext::UserDefined);
            assert_guid(&nested, ScalarContext::NestedUserDefined);
        } else {
            assert_eq!(udf, baseline, "{} differs in a scalar UDF", case.name);
            assert_eq!(
                nested, baseline,
                "{} differs in a nested scalar UDF",
                case.name
            );
        }
    }
    Ok(())
}

#[test]
fn scalar_import_is_invariant_under_ids_order_and_connection_encoding() -> TestResult {
    let case = FunctionCase::new(
        "concat",
        "core",
        vec![text("left<&\""), text("right")],
        "string",
    );
    let variants = [
        (ConnectionStyle::Graph, false, 0),
        (ConnectionStyle::Graph, true, 3_000),
        (ConnectionStyle::Legacy, false, 6_000),
        (ConnectionStyle::Legacy, true, 9_000),
    ];
    let mut baseline = None;
    for (style, reverse_components, key_offset) in variants {
        let value = execute_case(
            &case,
            ScalarContext::NestedUserDefined,
            style,
            reverse_components,
            key_offset,
        )?;
        match &baseline {
            Some(baseline) => assert_eq!(
                &value, baseline,
                "semantic output changed for {style:?}, reverse={reverse_components}, offset={key_offset}"
            ),
            None => baseline = Some(value),
        }
    }
    assert_eq!(baseline, Some(Value::String("left<&\"right".to_string())));
    Ok(())
}

#[test]
fn generated_item_at_executes_in_scalar_and_nested_udfs() -> TestResult {
    let cases = [
        (
            GeneratedSequence::Tokenize,
            vec![text("alpha|beta|gamma"), text("|"), int(2)],
            Value::String("beta".to_string()),
        ),
        (
            GeneratedSequence::TokenizeByLength,
            vec![text("abcdef"), int(2), int(3)],
            Value::String("ef".to_string()),
        ),
        (
            GeneratedSequence::TokenizeRegex,
            vec![text("alpha--beta::gamma"), text("[-:]+"), text(""), int(2)],
            Value::String("beta".to_string()),
        ),
        (
            GeneratedSequence::Generate,
            vec![int(4), int(7), int(3)],
            Value::Int(6),
        ),
    ];
    for (sequence, arguments, expected) in cases {
        for context in [ScalarContext::UserDefined, ScalarContext::NestedUserDefined] {
            let (actual, project) = execute_generated_item_at(
                sequence,
                arguments.clone(),
                context,
                ConnectionStyle::Graph,
                false,
                0,
            )?;
            assert_eq!(actual, expected, "{sequence:?} in {context:?}");
            assert!(
                project
                    .graph
                    .nodes
                    .values()
                    .any(|node| matches!(node, mapping::Node::SequenceItemAt { .. })),
                "{sequence:?} in {context:?} was not lowered to SequenceItemAt"
            );
        }
    }
    Ok(())
}

#[test]
fn generated_item_at_is_invariant_under_ids_order_and_connection_encoding() -> TestResult {
    let variants = [
        (ConnectionStyle::Graph, false, 0),
        (ConnectionStyle::Graph, true, 3_000),
        (ConnectionStyle::Legacy, false, 6_000),
        (ConnectionStyle::Legacy, true, 9_000),
    ];
    for (style, reverse_components, key_offset) in variants {
        let (actual, _) = execute_generated_item_at(
            GeneratedSequence::TokenizeRegex,
            vec![text("one  TWO three"), text(r"\s+"), text("i"), int(2)],
            ScalarContext::NestedUserDefined,
            style,
            reverse_components,
            key_offset,
        )?;
        assert_eq!(
            actual,
            Value::String("TWO".to_string()),
            "{style:?}, reverse={reverse_components}, offset={key_offset}"
        );
    }
    Ok(())
}

#[test]
fn generated_exists_executes_in_scalar_and_nested_udfs() -> TestResult {
    let cases = [
        (
            GeneratedSequence::Tokenize,
            vec![text("alpha|beta|gamma"), text("|"), text("beta")],
            true,
        ),
        (
            GeneratedSequence::TokenizeByLength,
            vec![text("abcdef"), int(2), text("gh")],
            false,
        ),
        (
            GeneratedSequence::TokenizeRegex,
            vec![
                text("alpha--beta::gamma"),
                text("[-:]+"),
                text(""),
                text("gamma"),
            ],
            true,
        ),
        (
            GeneratedSequence::Generate,
            vec![int(4), int(7), int(6)],
            true,
        ),
    ];
    for (sequence, arguments, expected) in cases {
        for context in [ScalarContext::UserDefined, ScalarContext::NestedUserDefined] {
            let (actual, project) = execute_generated_exists(
                sequence,
                arguments.clone(),
                false,
                context,
                ConnectionStyle::Graph,
                false,
                0,
            )?;
            assert_eq!(actual, Value::Bool(expected), "{sequence:?} in {context:?}");
            assert!(
                project
                    .graph
                    .nodes
                    .values()
                    .any(|node| matches!(node, mapping::Node::SequenceExists { .. })),
                "{sequence:?} in {context:?} was not lowered to SequenceExists"
            );
        }
    }
    Ok(())
}

#[test]
fn generated_exists_is_invariant_under_ids_order_and_connection_encoding() -> TestResult {
    let variants = [
        (ConnectionStyle::Graph, false, 0),
        (ConnectionStyle::Graph, true, 3_000),
        (ConnectionStyle::Legacy, false, 6_000),
        (ConnectionStyle::Legacy, true, 9_000),
    ];
    for (style, reverse_components, key_offset) in variants {
        let (actual, _) = execute_generated_exists(
            GeneratedSequence::TokenizeRegex,
            vec![text("one  TWO three"), text(r"\s+"), text("i"), text("TWO")],
            false,
            ScalarContext::NestedUserDefined,
            style,
            reverse_components,
            key_offset,
        )?;
        assert_eq!(
            actual,
            Value::Bool(true),
            "{style:?}, reverse={reverse_components}, offset={key_offset}"
        );
    }
    Ok(())
}

#[test]
fn generated_position_exists_executes_in_scalar_and_nested_udfs() -> TestResult {
    let cases = [
        (
            GeneratedSequence::Tokenize,
            vec![text("alpha|beta|gamma"), text("|"), int(2)],
            true,
        ),
        (
            GeneratedSequence::TokenizeByLength,
            vec![text("abcdef"), int(2), int(4)],
            false,
        ),
        (
            GeneratedSequence::TokenizeRegex,
            vec![text("alpha--beta::gamma"), text("[-:]+"), text(""), int(3)],
            true,
        ),
        (
            GeneratedSequence::Generate,
            vec![int(4), int(7), int(4)],
            true,
        ),
    ];
    for (sequence, arguments, expected) in cases {
        for context in [ScalarContext::UserDefined, ScalarContext::NestedUserDefined] {
            let (actual, project) = execute_generated_exists(
                sequence,
                arguments.clone(),
                true,
                context,
                ConnectionStyle::Graph,
                false,
                0,
            )?;
            assert_eq!(actual, Value::Bool(expected), "{sequence:?} in {context:?}");
            assert!(
                project.graph.nodes.values().any(
                    |node| matches!(node, mapping::Node::Position { collection } if collection.is_empty())
                ),
                "{sequence:?} in {context:?} was not lowered to generated-item Position"
            );
        }
    }
    Ok(())
}

#[test]
fn generated_position_exists_is_invariant_under_graph_encoding() -> TestResult {
    let variants = [
        (ConnectionStyle::Graph, false, 0),
        (ConnectionStyle::Graph, true, 3_000),
        (ConnectionStyle::Legacy, false, 6_000),
        (ConnectionStyle::Legacy, true, 9_000),
    ];
    for (style, reverse_components, key_offset) in variants {
        let (actual, _) = execute_generated_exists(
            GeneratedSequence::TokenizeRegex,
            vec![text("one  two three"), text(r"\s+"), text(""), int(2)],
            true,
            ScalarContext::NestedUserDefined,
            style,
            reverse_components,
            key_offset,
        )?;
        assert_eq!(
            actual,
            Value::Bool(true),
            "{style:?}, reverse={reverse_components}, offset={key_offset}"
        );
    }
    Ok(())
}

#[test]
fn generated_aggregates_execute_in_scalar_and_nested_udfs() -> TestResult {
    let cases = [
        (
            GeneratedSequence::Tokenize,
            GeneratedAggregate::Count,
            vec![text("alpha|beta|gamma"), text("|")],
            "integer",
            Value::Int(3),
        ),
        (
            GeneratedSequence::Generate,
            GeneratedAggregate::Sum,
            vec![int(1), int(4)],
            "integer",
            Value::Int(10),
        ),
        (
            GeneratedSequence::Generate,
            GeneratedAggregate::Avg,
            vec![int(1), int(4)],
            "decimal",
            Value::Float(2.5),
        ),
        (
            GeneratedSequence::TokenizeByLength,
            GeneratedAggregate::Min,
            vec![text("090204"), int(2)],
            "integer",
            Value::Int(2),
        ),
        (
            GeneratedSequence::TokenizeRegex,
            GeneratedAggregate::Max,
            vec![text("3,11,7"), text(","), text("")],
            "integer",
            Value::Int(11),
        ),
        (
            GeneratedSequence::Tokenize,
            GeneratedAggregate::Join,
            vec![text("alpha|beta|gamma"), text("|"), text(" / ")],
            "string",
            Value::String("alpha / beta / gamma".to_string()),
        ),
    ];
    for (sequence, operation, arguments, output_type, expected) in cases {
        for context in [ScalarContext::UserDefined, ScalarContext::NestedUserDefined] {
            let (actual, project) = execute_generated_aggregate(
                sequence,
                operation,
                arguments.clone(),
                output_type,
                context,
                GraphEncoding::default(),
            )?;
            assert_eq!(
                actual, expected,
                "{operation:?} over {sequence:?} in {context:?}"
            );
            assert!(
                project
                    .graph
                    .nodes
                    .values()
                    .any(|node| matches!(node, mapping::Node::SequenceAggregate { .. })),
                "{operation:?} over {sequence:?} in {context:?} was not lowered to SequenceAggregate"
            );
        }
    }
    Ok(())
}

#[test]
fn generated_aggregate_is_invariant_under_graph_encoding() -> TestResult {
    let variants = [
        (ConnectionStyle::Graph, false, 0),
        (ConnectionStyle::Graph, true, 3_000),
        (ConnectionStyle::Legacy, false, 6_000),
        (ConnectionStyle::Legacy, true, 9_000),
    ];
    for (style, reverse_components, key_offset) in variants {
        let (actual, _) = execute_generated_aggregate(
            GeneratedSequence::TokenizeRegex,
            GeneratedAggregate::Join,
            vec![text("one  two three"), text(r"\s+"), text(""), text(",")],
            "string",
            ScalarContext::NestedUserDefined,
            GraphEncoding {
                style,
                reverse_components,
                key_offset,
            },
        )?;
        assert_eq!(
            actual,
            Value::String("one,two,three".to_string()),
            "{style:?}, reverse={reverse_components}, offset={key_offset}"
        );
    }
    Ok(())
}

#[test]
fn computed_generated_aggregate_executes_and_roundtrips() -> TestResult {
    let fixture = ScalarMfdBuilder::generated_computed_aggregate(
        "computed_generated_sum",
        GeneratedSequence::Generate,
        GeneratedAggregate::Sum,
        vec![int(1), int(4), int(3)],
        "integer",
    )
    .context(ScalarContext::NestedUserDefined)
    .write()?;
    let imported = mfd::import(fixture.design())?;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(
        engine::validate(&imported.project).is_empty(),
        "{:?}",
        engine::validate(&imported.project)
    );
    codegen::lower(&imported.project)
        .map_err(|diagnostics| format!("computed aggregate did not lower: {diagnostics:?}"))?;
    assert!(imported.project.graph.nodes.values().any(|node| {
        matches!(
            node,
            mapping::Node::SequenceAggregate {
                expression: Some(_),
                ..
            }
        )
    }));

    let source = format_xml::from_str(
        "<Source><Seed>fixture</Seed></Source>",
        &imported.project.source,
    )?;
    let output = engine::run(&imported.project, &source)?;
    assert_eq!(
        output.field("Result").and_then(Instance::as_scalar),
        Some(&Value::Int(30))
    );

    let roundtrip = fixture.design().with_file_name("roundtrip.mfd");
    let export_warnings = mfd::export(&imported.project, &roundtrip)?;
    assert!(export_warnings.is_empty(), "{export_warnings:?}");
    let reimported = mfd::import(&roundtrip)?;
    assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
    assert!(reimported.project.graph.nodes.values().any(|node| {
        matches!(
            node,
            mapping::Node::SequenceAggregate {
                expression: Some(_),
                ..
            }
        )
    }));
    let rerun = engine::run(&reimported.project, &source)?;
    assert_eq!(
        rerun.field("Result").and_then(Instance::as_scalar),
        Some(&Value::Int(30))
    );
    Ok(())
}

#[test]
fn computed_generated_aggregate_imports_in_main_mapping() -> TestResult {
    let fixture = ScalarMfdBuilder::generated_computed_aggregate(
        "main_computed_generated_sum",
        GeneratedSequence::Generate,
        GeneratedAggregate::Sum,
        vec![int(1), int(4), int(3)],
        "integer",
    )
    .context(ScalarContext::Main)
    .write()?;
    let imported = mfd::import(fixture.design())?;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(
        engine::validate(&imported.project).is_empty(),
        "{:?}",
        engine::validate(&imported.project)
    );
    assert!(imported.project.graph.nodes.values().any(|node| {
        matches!(
            node,
            mapping::Node::SequenceAggregate {
                expression: Some(_),
                ..
            }
        )
    }));
    let source = format_xml::from_str(
        "<Source><Seed>fixture</Seed></Source>",
        &imported.project.source,
    )?;
    let output = engine::run(&imported.project, &source)?;
    assert_eq!(
        output.field("Result").and_then(Instance::as_scalar),
        Some(&Value::Int(30))
    );
    Ok(())
}

#[test]
fn filtered_computed_generated_aggregate_executes_and_roundtrips() -> TestResult {
    let fixture = ScalarMfdBuilder::generated_filtered_computed_aggregate(
        "filtered_computed_generated_sum",
        GeneratedSequence::Generate,
        GeneratedAggregate::Sum,
        vec![int(1), int(4), int(3), int(2)],
        "integer",
    )
    .context(ScalarContext::NestedUserDefined)
    .write()?;
    let imported = mfd::import(fixture.design())?;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(
        engine::validate(&imported.project).is_empty(),
        "{:?}",
        engine::validate(&imported.project)
    );
    codegen::lower(&imported.project)
        .map_err(|diagnostics| format!("filtered computed aggregate: {diagnostics:?}"))?;
    assert!(imported.project.graph.nodes.values().any(|node| {
        matches!(
            node,
            mapping::Node::SequenceAggregate {
                predicate: Some(_),
                expression: Some(_),
                ..
            }
        )
    }));

    let source = format_xml::from_str(
        "<Source><Seed>fixture</Seed></Source>",
        &imported.project.source,
    )?;
    let output = engine::run(&imported.project, &source)?;
    assert_eq!(
        output.field("Result").and_then(Instance::as_scalar),
        Some(&Value::Int(6))
    );

    let roundtrip = fixture.design().with_file_name("roundtrip.mfd");
    assert!(
        mfd::export(&imported.project, &roundtrip)?.is_empty(),
        "filtered computed aggregate export warned"
    );
    let reimported = mfd::import(&roundtrip)?;
    assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
    let rerun = engine::run(&reimported.project, &source)?;
    assert_eq!(
        rerun.field("Result").and_then(Instance::as_scalar),
        Some(&Value::Int(6))
    );
    Ok(())
}

#[test]
fn filtered_generated_aggregate_executes_and_roundtrips() -> TestResult {
    let fixture = ScalarMfdBuilder::generated_filtered_aggregate(
        "filtered_generated_join",
        GeneratedSequence::Tokenize,
        GeneratedAggregate::Join,
        vec![
            text("alpha|beta|alpha"),
            text("|"),
            text("alpha"),
            text("/"),
        ],
        "string",
    )
    .context(ScalarContext::NestedUserDefined)
    .write()?;
    let imported = mfd::import(fixture.design())?;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(
        engine::validate(&imported.project).is_empty(),
        "{:?}",
        engine::validate(&imported.project)
    );
    codegen::lower(&imported.project)
        .map_err(|diagnostics| format!("filtered aggregate did not lower: {diagnostics:?}"))?;
    assert!(imported.project.graph.nodes.values().any(|node| {
        matches!(
            node,
            mapping::Node::SequenceAggregate {
                predicate: Some(_),
                ..
            }
        )
    }));

    let source = format_xml::from_str(
        "<Source><Seed>fixture</Seed></Source>",
        &imported.project.source,
    )?;
    let output = engine::run(&imported.project, &source)?;
    assert_eq!(
        output.field("Result").and_then(Instance::as_scalar),
        Some(&Value::String("alpha/alpha".to_string()))
    );

    let roundtrip = fixture.design().with_file_name("roundtrip.mfd");
    let export_warnings = mfd::export(&imported.project, &roundtrip)?;
    assert!(export_warnings.is_empty(), "{export_warnings:?}");
    let reimported = mfd::import(&roundtrip)?;
    assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
    let rerun = engine::run(&reimported.project, &source)?;
    assert_eq!(
        rerun.field("Result").and_then(Instance::as_scalar),
        Some(&Value::String("alpha/alpha".to_string()))
    );
    Ok(())
}
