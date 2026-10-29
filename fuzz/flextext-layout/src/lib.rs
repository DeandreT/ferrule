//! Shared invariants for the FlexText layout-and-input fuzz target.
//!
//! An input is either a JSON layout (it starts with `{`; an optional NUL byte
//! separates the input text) or a compact [`program`] that builds a layout
//! through the public `mapping` constructors and carries its input text.
//!
//! Layout construction must succeed or return a typed rejection, the same way
//! through `FlexTextLayout::new` and through JSON deserialization. A validated
//! layout must respect the documented layout limits and survive a JSON round
//! trip. Its input text is then read with `format_flextext::from_str`, which
//! must return a well-shaped bounded instance or a typed data or limit error,
//! deterministically. A parsed instance is written back with
//! `format_flextext::to_string`, which must likewise succeed within the output
//! limit or return a typed error. Anything else, including a panic, is a
//! finding.

use std::collections::HashSet;

use format_flextext::{
    FlexTextError, MAX_INSTANCE_NODES, MAX_OUTPUT_BYTES, MAX_RECORDS, MAX_VALUE_BYTES, from_str,
    to_string,
};
use ir::{Instance, ScalarType, Value};
use mapping::{
    FlexCommand, FlexLineEnding, FlexTextLayout, MAX_FLEXTEXT_LAYOUT_DEPTH,
    MAX_FLEXTEXT_LAYOUT_NODES, MAX_FLEXTEXT_LAYOUT_STRING_BYTES, ManySplitter, OnceSplitter,
    SwitchMode,
};
use serde::Serialize;

pub mod program;
pub mod seeds;

use program::Program;

/// Largest input the fuzz target examines. Larger inputs are ignored so a
/// run's per-input cost stays proportional to this bound. The README's
/// bounded smoke run passes a smaller `-max_len`.
pub const MAX_INPUT_BYTES: usize = 64 * 1024;

/// Separates a JSON layout from its input text.
pub const JSON_TEXT_SEPARATOR: u8 = 0;

const BOM: char = '\u{feff}';

/// The validator's limit on distinct switch regular expressions per layout.
/// `mapping` names it only in the `TooManySwitchRegexes` message; a seed test
/// keeps this copy in step with that message.
pub const MAX_SWITCH_REGEXES: usize = 64;

/// The validator's limit on a layout's serialized JSON nesting, counting the
/// layout object. `mapping` names it only in the `SerializedLayoutTooDeep`
/// message; a seed test keeps this copy in step with that message.
pub const MAX_LAYOUT_JSON_DEPTH: usize = 96;

/// serde_json refuses documents nesting this many arrays or objects before
/// any `Deserialize` impl runs. Each switch arm costs three levels (command,
/// `arms` array, arm), so a command tree the constructor rejects for depth can
/// have a JSON form this deep; serde then refuses it with its own error.
pub const SERDE_JSON_DEPTH_LIMIT: usize = 128;

/// Deepest instance node a validated layout can produce: the root group is
/// level zero and each command adds at most a sequence and an item group
/// above its children.
pub const MAX_PARSED_DEPTH: usize = 2 * MAX_FLEXTEXT_LAYOUT_DEPTH + 1;

/// Result of checking one fuzz input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    /// `Ok` for a validated layout, otherwise its rejection message.
    pub layout: Result<(), String>,
    /// How the input text read against a validated layout.
    pub input: Option<Result<Parsed, String>>,
    /// How a parsed instance wrote back.
    pub render: Option<Result<usize, String>>,
    /// Whether the JSON form of a program the constructor rejected nests past
    /// serde_json's limit, so serde refused it before the constructor ran.
    pub json_refused_by_serde: bool,
}

impl Outcome {
    fn rejected(message: String) -> Self {
        Self {
            layout: Err(message),
            input: None,
            render: None,
            json_refused_by_serde: false,
        }
    }
}

/// Measurements of one parsed instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Parsed {
    pub nodes: usize,
    pub depth: usize,
    pub records: usize,
}

/// Checks one fuzz input. Panics with a descriptive message when an invariant
/// fails, which is how libFuzzer records a finding.
pub fn check(bytes: &[u8]) -> Outcome {
    if bytes.first() == Some(&b'{') {
        check_json(bytes)
    } else {
        check_program(bytes)
    }
}

fn check_json(bytes: &[u8]) -> Outcome {
    let (json, text) = match bytes.iter().position(|byte| *byte == JSON_TEXT_SEPARATOR) {
        Some(index) => (&bytes[..index], &bytes[index + 1..]),
        None => (bytes, &[][..]),
    };
    let decoded = serde_json::from_slice::<FlexTextLayout>(json);
    let again = serde_json::from_slice::<FlexTextLayout>(json);
    match (decoded, again) {
        (Ok(layout), Ok(again)) => {
            assert_eq!(layout, again, "JSON layout decoding is not deterministic");
            check_layout(&layout, &String::from_utf8_lossy(text))
        }
        (Err(error), Err(again)) => {
            assert_eq!(
                error.to_string(),
                again.to_string(),
                "JSON layout rejection is not deterministic"
            );
            Outcome::rejected(error.to_string())
        }
        (first, second) => {
            panic!("JSON layout decoding is not deterministic: {first:?} / {second:?}")
        }
    }
}

fn check_program(bytes: &[u8]) -> Outcome {
    let program = Program::decode(bytes);
    let built = build(&program);
    assert_eq!(
        built,
        build(&program),
        "layout validation is not deterministic"
    );
    match built {
        Ok(layout) => check_layout(&layout, &program.text),
        Err(error) => {
            let json_refused_by_serde = match &program.command {
                Ok(command) => check_json_rejects(&program, command, &error),
                Err(_) => false,
            };
            Outcome {
                json_refused_by_serde,
                ..Outcome::rejected(error.to_string())
            }
        }
    }
}

fn build(program: &Program) -> Result<FlexTextLayout, mapping::FlexTextLayoutError> {
    program.command.clone().and_then(|command| {
        FlexTextLayout::new(
            program.root_name.clone(),
            command,
            program.line_ending,
            program.write_bom,
        )
    })
}

/// A command tree that `FlexTextLayout::new` rejects must also be rejected,
/// with the same message, when it arrives as a JSON layout. The one distinct
/// outcome is a JSON form nesting past serde_json's own limit, which serde
/// refuses before the constructor runs; returns whether that happened.
fn check_json_rejects(
    program: &Program,
    command: &FlexCommand,
    error: &mapping::FlexTextLayoutError,
) -> bool {
    /// The wire form of `FlexTextLayout`, borrowed from an unvalidated tree.
    #[derive(Serialize)]
    struct LayoutWire<'a> {
        root_name: &'a str,
        command: &'a FlexCommand,
        output_line_ending: FlexLineEnding,
        write_bom: bool,
    }

    let wire = LayoutWire {
        root_name: &program.root_name,
        command,
        output_line_ending: program.line_ending,
        write_bom: program.write_bom,
    };
    let json = match serde_json::to_string(&wire) {
        Ok(json) => json,
        Err(error) => panic!("command does not serialize: {error}"),
    };
    let expected = error.to_string();
    match serde_json::from_str::<FlexTextLayout>(&json) {
        Ok(layout) => {
            panic!("JSON accepted a layout the constructor rejects ({expected}): {layout:?}")
        }
        Err(json_error) => {
            let refused_by_serde = json_error
                .to_string()
                .starts_with("recursion limit exceeded");
            if refused_by_serde {
                assert!(
                    json_depth(&json) >= SERDE_JSON_DEPTH_LIMIT,
                    "serde refused `{json_error}` a layout nesting only {} levels",
                    json_depth(&json)
                );
            } else {
                assert!(
                    json_error.to_string().contains(&expected),
                    "JSON rejection `{json_error}` differs from constructor rejection `{expected}`"
                );
            }
            refused_by_serde
        }
    }
}

/// Deepest array or object nesting in a well-formed JSON document.
fn json_depth(json: &str) -> usize {
    let (mut depth, mut deepest) = (0_usize, 0_usize);
    let (mut in_string, mut escaped) = (false, false);
    for character in json.chars() {
        if in_string {
            match character {
                _ if escaped => escaped = false,
                '\\' => escaped = true,
                '"' => in_string = false,
                _ => {}
            }
            continue;
        }
        match character {
            '"' => in_string = true,
            '{' | '[' => {
                depth += 1;
                deepest = deepest.max(depth);
            }
            '}' | ']' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    deepest
}

fn check_layout(layout: &FlexTextLayout, text: &str) -> Outcome {
    check_layout_limits(layout);
    check_layout_round_trip(layout);
    let (input, render) = check_input(layout, text, true);
    Outcome {
        layout: Ok(()),
        input: Some(input),
        render,
        json_refused_by_serde: false,
    }
}

/// A validated layout must respect every documented layout limit.
fn check_layout_limits(layout: &FlexTextLayout) {
    let stats = LayoutStats::of(layout);
    assert!(
        stats.depth <= MAX_FLEXTEXT_LAYOUT_DEPTH,
        "validated layout nests {} commands deep, over {MAX_FLEXTEXT_LAYOUT_DEPTH}",
        stats.depth
    );
    assert!(
        stats.nodes <= MAX_FLEXTEXT_LAYOUT_NODES,
        "validated layout has {} commands, over {MAX_FLEXTEXT_LAYOUT_NODES}",
        stats.nodes
    );
    assert!(
        stats.string_bytes <= MAX_FLEXTEXT_LAYOUT_STRING_BYTES,
        "validated layout holds {} string bytes, over {MAX_FLEXTEXT_LAYOUT_STRING_BYTES}",
        stats.string_bytes
    );
    assert!(
        stats.regexes.len() <= MAX_SWITCH_REGEXES,
        "validated layout has {} distinct regular expressions, over {MAX_SWITCH_REGEXES}",
        stats.regexes.len()
    );
}

/// A validated layout must serialize to JSON within the validator's nesting
/// limit that decodes to an equal layout.
fn check_layout_round_trip(layout: &FlexTextLayout) {
    let json = match serde_json::to_string(layout) {
        Ok(json) => json,
        Err(error) => panic!("validated layout does not serialize: {error}"),
    };
    let depth = json_depth(&json);
    assert!(
        depth <= MAX_LAYOUT_JSON_DEPTH,
        "validated layout's JSON nests {depth} levels, over {MAX_LAYOUT_JSON_DEPTH}"
    );
    match serde_json::from_str::<FlexTextLayout>(&json) {
        Ok(decoded) => assert_eq!(&decoded, layout, "JSON round trip changed the layout"),
        Err(error) => panic!("validated layout does not decode from its own JSON: {error}"),
    }
}

/// Layout measurements, counted the way the validator counts them.
#[derive(Default)]
struct LayoutStats {
    nodes: usize,
    depth: usize,
    string_bytes: usize,
    regexes: HashSet<String>,
}

impl LayoutStats {
    fn of(layout: &FlexTextLayout) -> Self {
        let mut stats = Self {
            string_bytes: layout.root_name().len(),
            ..Self::default()
        };
        stats.visit(layout.command(), 1);
        stats
    }

    fn emitted(&mut self, value: &str) -> usize {
        value.len()
    }

    fn visit(&mut self, command: &FlexCommand, depth: usize) {
        self.nodes += 1;
        self.depth = self.depth.max(depth);
        self.string_bytes += command.output_name().map_or(0, str::len);
        match command {
            FlexCommand::SplitOnce {
                splitter,
                first,
                second,
                ..
            } => {
                self.string_bytes += match splitter {
                    OnceSplitter::Delimiter(value) => self.emitted(value),
                    OnceSplitter::LineStartingWith(value) | OnceSplitter::LineContaining(value) => {
                        value.len()
                    }
                    OnceSplitter::FixedLines(_) | OnceSplitter::FixedColumns(_) => 0,
                };
                self.visit(first, depth + 1);
                self.visit(second, depth + 1);
            }
            FlexCommand::SplitMany {
                splitter, child, ..
            } => {
                self.string_bytes += match splitter {
                    ManySplitter::Delimiter(value) => self.emitted(value),
                    ManySplitter::LinesStartingWith(value) => value.len(),
                    ManySplitter::FixedLines(_) => 0,
                };
                self.visit(child, depth + 1);
            }
            FlexCommand::Store { trim, .. } => {
                self.string_bytes += trim.as_ref().map_or(0, |trim| trim.characters().len());
            }
            FlexCommand::Ignore => {}
            FlexCommand::FixedWidthRecords {
                fields, fill_char, ..
            } => {
                self.string_bytes += fill_char.len_utf8();
                self.string_bytes += fields.iter().map(|field| field.name().len()).sum::<usize>();
            }
            FlexCommand::DelimitedRecords {
                dialect, fields, ..
            } => {
                self.string_bytes +=
                    dialect.field_separator().len() + self.emitted(dialect.record_separator());
                self.string_bytes += fields.iter().map(|field| field.name().len()).sum::<usize>();
            }
            FlexCommand::Switch { arms, default, .. } => {
                for arm in arms {
                    self.string_bytes += arm.prefix().len();
                    if arm.contains_regex() {
                        self.regexes.insert(arm.prefix().to_string());
                    }
                    self.visit(arm.command(), depth + 1);
                }
                if let Some(default) = default {
                    self.visit(default, depth + 1);
                }
            }
        }
    }
}

type InputResult = Result<Parsed, String>;
type RenderResult = Option<Result<usize, String>>;

/// Reads `text` against a validated layout and checks the result. With
/// `render`, a parsed instance is also written back and the output re-read.
fn check_input(layout: &FlexTextLayout, text: &str, render: bool) -> (InputResult, RenderResult) {
    let schema = layout.schema();
    let parsed = from_str(text, &schema, layout);
    let again = from_str(text, &schema, &layout.clone());
    assert_same(&parsed, &again, "reading the same input twice differs");
    // A read that reached the record or node limit walked about a million
    // items; a third read would add seconds per input under sanitizers.
    let at_limit = matches!(
        parsed,
        Err(FlexTextError::TooManyRecords | FlexTextError::TooManyNodes)
    );
    if !at_limit && !text.starts_with(BOM) {
        let with_bom = format!("{BOM}{text}");
        let bom_parsed = from_str(&with_bom, &schema, layout);
        assert_same(
            &parsed,
            &bom_parsed,
            "a leading byte-order mark changed the result",
        );
    }

    let instance = match parsed {
        Ok(instance) => instance,
        Err(error) => {
            check_read_error(&error, layout.root_name());
            return (Err(error.to_string()), None);
        }
    };

    let stats = measure(&instance, text.len());
    assert!(
        stats.nodes <= MAX_INSTANCE_NODES,
        "{} instance nodes exceed {MAX_INSTANCE_NODES}",
        stats.nodes
    );
    assert!(
        stats.records <= MAX_RECORDS,
        "{} records exceed {MAX_RECORDS}",
        stats.records
    );
    assert!(
        stats.depth <= MAX_PARSED_DEPTH,
        "instance depth {} exceeds {MAX_PARSED_DEPTH}",
        stats.depth
    );
    let Instance::Group(fields) = &instance else {
        panic!("document root is not a group: {instance:?}");
    };
    check_children(fields, &[layout.command()], false, layout.root_name());

    let rendered = render.then(|| check_render(layout, &instance));
    (Ok(stats), rendered)
}

fn assert_same(
    left: &Result<Instance, FlexTextError>,
    right: &Result<Instance, FlexTextError>,
    context: &str,
) {
    match (left, right) {
        (Ok(left), Ok(right)) => assert_eq!(left, right, "{context}"),
        (Err(left), Err(right)) => {
            assert_eq!(format!("{left:?}"), format!("{right:?}"), "{context}");
        }
        _ => panic!("{context}: {left:?} / {right:?}"),
    }
}

/// Reading in-memory text against a validated layout and its own schema can
/// only fail on the data or on the record and node limits. The input stays
/// below the input and value byte limits, regular expressions were compiled
/// by the validator under the same limits, and the validator bounds command
/// depth by the same limit the reader enforces.
fn check_read_error(error: &FlexTextError, root: &str) {
    match error {
        FlexTextError::Data { path, message } => {
            assert!(
                path.starts_with(&format!("{root}/")),
                "data error path `{path}` is outside the document root `{root}` ({message})"
            );
            assert!(!message.is_empty(), "data error at `{path}` has no message");
        }
        FlexTextError::TooManyRecords | FlexTextError::TooManyNodes => {}
        other => panic!("reading produced an unexpected error: {other:?}"),
    }
}

/// Counts instance nodes, records (sequence items) and depth without
/// recursing, and checks every scalar string against the value limit.
fn measure(instance: &Instance, text_len: usize) -> Parsed {
    let mut stats = Parsed {
        nodes: 0,
        depth: 0,
        records: 0,
    };
    let mut pending = vec![(instance, 0_usize)];
    while let Some((node, depth)) = pending.pop() {
        stats.nodes += 1;
        stats.depth = stats.depth.max(depth);
        match node {
            Instance::Group(fields) => {
                pending.extend(fields.iter().map(|(_, value)| (value, depth + 1)));
            }
            Instance::Repeated(items) | Instance::MappedSequence(items) => {
                stats.records += items.len();
                pending.extend(items.iter().map(|item| (item, depth + 1)));
            }
            Instance::Scalar(Value::String(value)) => assert!(
                value.len() <= MAX_VALUE_BYTES && value.len() <= text_len,
                "a {}-byte value exceeds its {text_len}-byte input or the value limit",
                value.len()
            ),
            Instance::Scalar(_) | Instance::DocumentSet(_) => {}
        }
    }
    stats
}

/// Checks a group's fields against the commands that produced them: each
/// field belongs to a distinct command, in command order, and every command
/// other than `Ignore` or a `Switch` (which may match nothing) is present
/// unless the parent is itself a switch.
fn check_children(
    fields: &[(String, Instance)],
    commands: &[&FlexCommand],
    from_switch: bool,
    path: &str,
) {
    let mut next = 0;
    for (name, value) in fields {
        let Some(offset) = commands[next..]
            .iter()
            .position(|command| command.output_name() == Some(name.as_str()))
        else {
            panic!("{path}: field `{name}` is unexpected, repeated or out of order");
        };
        for skipped in &commands[next..next + offset] {
            check_may_be_absent(skipped, from_switch, path);
        }
        let field_path = format!("{path}/{name}");
        check_value(commands[next + offset], value, &field_path);
        next += offset + 1;
    }
    for skipped in &commands[next..] {
        check_may_be_absent(skipped, from_switch, path);
    }
}

fn check_may_be_absent(command: &FlexCommand, from_switch: bool, path: &str) {
    assert!(
        from_switch || matches!(command, FlexCommand::Ignore | FlexCommand::Switch { .. }),
        "{path}: missing output of {command:?}"
    );
}

fn check_value(command: &FlexCommand, value: &Instance, path: &str) {
    match command {
        FlexCommand::Ignore => panic!("{path}: ignore command produced a value"),
        FlexCommand::Store { ty, .. } => check_scalar(value, *ty, false, path),
        FlexCommand::SplitOnce { first, second, .. } => {
            let fields = group(value, path);
            check_children(fields, &[first.as_ref(), second.as_ref()], false, path);
        }
        FlexCommand::SplitMany { child, .. } => {
            for (index, item) in repeated(value, path).iter().enumerate() {
                let item_path = format!("{path}[{}]", index + 1);
                let fields = group(item, &item_path);
                assert!(!fields.is_empty(), "{item_path}: empty split item was kept");
                check_children(fields, &[child.as_ref()], false, &item_path);
            }
        }
        FlexCommand::FixedWidthRecords {
            fields,
            treat_empty_as_absent,
            ..
        } => {
            let columns = fields
                .iter()
                .map(|field| (field.name(), field.ty()))
                .collect::<Vec<_>>();
            check_records(value, &columns, *treat_empty_as_absent, path);
        }
        FlexCommand::DelimitedRecords { fields, .. } => {
            let columns = fields
                .iter()
                .map(|field| (field.name(), field.ty()))
                .collect::<Vec<_>>();
            check_records(value, &columns, true, path);
        }
        FlexCommand::Switch {
            mode,
            arms,
            default,
            ..
        } => {
            let fields = group(value, path);
            assert!(!fields.is_empty(), "{path}: empty switch group was kept");
            let commands = arms
                .iter()
                .map(mapping::SwitchArm::command)
                .chain(default.as_deref())
                .collect::<Vec<_>>();
            check_children(fields, &commands, true, path);
            let default_name = default.as_deref().and_then(FlexCommand::output_name);
            let arm_outputs = fields
                .iter()
                .filter(|(name, _)| Some(name.as_str()) != default_name)
                .count();
            assert!(
                arm_outputs == fields.len() || arm_outputs == 0,
                "{path}: switch holds both arm and default output"
            );
            assert!(
                *mode == SwitchMode::AllPossible || arm_outputs <= 1,
                "{path}: first-match switch holds {arm_outputs} arm outputs"
            );
        }
    }
}

fn check_records(value: &Instance, columns: &[(&str, ScalarType)], nullable: bool, path: &str) {
    for (index, item) in repeated(value, path).iter().enumerate() {
        let item_path = format!("{path}[{}]", index + 1);
        let fields = group(item, &item_path);
        assert_eq!(
            fields.len(),
            columns.len(),
            "{item_path}: record has the wrong number of fields"
        );
        for ((name, value), (column, ty)) in fields.iter().zip(columns) {
            assert_eq!(name, column, "{item_path}: record field order");
            check_scalar(value, *ty, nullable, &format!("{item_path}/{name}"));
        }
    }
}

fn check_scalar(value: &Instance, ty: ScalarType, nullable: bool, path: &str) {
    let matches = match value {
        Instance::Scalar(Value::Null) => nullable,
        Instance::Scalar(Value::String(_)) => ty == ScalarType::String,
        Instance::Scalar(Value::Int(_)) => ty == ScalarType::Int,
        Instance::Scalar(Value::Float(value)) => ty == ScalarType::Float && value.is_finite(),
        Instance::Scalar(Value::Bool(_)) => ty == ScalarType::Bool,
        _ => false,
    };
    assert!(matches, "{path}: {value:?} is not a {ty:?} value");
}

fn group<'a>(value: &'a Instance, path: &str) -> &'a [(String, Instance)] {
    match value {
        Instance::Group(fields) => fields,
        other => panic!("{path}: expected a group, got {other:?}"),
    }
}

fn repeated<'a>(value: &'a Instance, path: &str) -> &'a [Instance] {
    match value {
        Instance::Repeated(items) => {
            assert!(items.len() <= MAX_RECORDS, "{path}: too many records");
            items
        }
        other => panic!("{path}: expected a sequence, got {other:?}"),
    }
}

/// Writes a parsed instance back and checks the output.
fn check_render(layout: &FlexTextLayout, instance: &Instance) -> Result<usize, String> {
    let schema = layout.schema();
    let rendered = to_string(&schema, instance, layout);
    let again = to_string(&schema, instance, layout);
    match (&rendered, &again) {
        (Ok(left), Ok(right)) => assert_eq!(left, right, "writing is not deterministic"),
        (Err(left), Err(right)) => assert_eq!(
            format!("{left:?}"),
            format!("{right:?}"),
            "writing is not deterministic"
        ),
        _ => panic!("writing is not deterministic: {rendered:?} / {again:?}"),
    }
    match rendered {
        Ok(output) => {
            // Data may itself begin with U+FEFF, so only a layout that writes
            // a byte-order mark fixes the first character.
            let body = if layout.write_bom() {
                match output.strip_prefix(BOM) {
                    Some(body) => body,
                    None => panic!("layout writes a byte-order mark but the output lacks one"),
                }
            } else {
                output.as_str()
            };
            assert!(
                body.len() <= MAX_OUTPUT_BYTES,
                "{}-byte output exceeds {MAX_OUTPUT_BYTES}",
                body.len()
            );
            check_line_endings(body, layout);
            if output.len() <= MAX_INPUT_BYTES {
                // The output need not read back to the same instance; it must
                // still satisfy every reading invariant.
                let _ = check_input(layout, &output, false);
            }
            Ok(output.len())
        }
        Err(error) => {
            match &error {
                FlexTextError::Data { .. }
                | FlexTextError::OutputTooLarge
                | FlexTextError::InstanceTooDeep => {}
                other => {
                    panic!("writing a parsed instance produced an unexpected error: {other:?}")
                }
            }
            Err(error.to_string())
        }
    }
}

/// LF output has no `\r\n` and CRLF output ends every line with `\r\n`.
fn check_line_endings(output: &str, layout: &FlexTextLayout) {
    let ending = layout.output_line_ending();
    for (index, _) in output.match_indices('\n') {
        let carriage_return = index > 0 && output.as_bytes()[index - 1] == b'\r';
        assert_eq!(
            carriage_return,
            ending == FlexLineEnding::Crlf,
            "line ending at byte {index} does not follow {ending:?}"
        );
    }
}
