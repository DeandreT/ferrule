//! Native JSON5 finite-number admission witnesses. Generated adapters are separate.
use format_json::JsonFormatError;
use ir::{Instance, ScalarType, SchemaKind, SchemaNode, Value};
use std::error::Error;
use std::path::PathBuf;

type TestResult<T = ()> = Result<T, Box<dyn Error>>;
const NONFINITE: [&str; 8] = [
    "Infinity",
    "+Infinity",
    "-Infinity",
    "NaN",
    "+NaN",
    "-NaN",
    "1e999",
    "-1e999",
];

enum Expected {
    Value(Instance),
    Refusal,
    Shape {
        expected: &'static str,
        got: &'static str,
    },
    Undeclared,
    Nesting,
    Parser,
}
struct Case {
    id: String,
    source: String,
    schema: SchemaNode,
    expected: Expected,
}
fn case(id: &str, source: impl Into<String>, schema: SchemaNode, expected: Expected) -> Case {
    Case {
        id: id.into(),
        source: source.into(),
        schema,
        expected,
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
            .collect::<Vec<_>>()
            .into(),
    )
}
fn nullable_float(name: &str) -> SchemaNode {
    SchemaNode::scalar(name, ScalarType::Float)
        .nullable()
        .unwrap()
}
fn nullable_object(name: &str) -> SchemaNode {
    SchemaNode::group(name, vec![nullable_float("n")])
        .nullable_container()
        .unwrap()
}
fn nullable_array(name: &str) -> SchemaNode {
    nullable_float(name)
        .repeating()
        .nullable_container()
        .unwrap()
}
fn any(name: &str) -> SchemaNode {
    SchemaNode::scalar(name, ScalarType::String)
        .json_any()
        .unwrap()
}
fn open(dynamic_schema: SchemaNode) -> SchemaNode {
    let mut schema = SchemaNode::group("Root", Vec::new());
    if let SchemaKind::Group { dynamic, .. } = &mut schema.kind {
        *dynamic = Some(Box::new(dynamic_schema));
    }
    schema
}
fn object_n() -> SchemaNode {
    SchemaNode::group("Root", vec![nullable_float("n")])
}

// Each group retains all complete originals before its first comparison.
// The only automatically removed directory is this test's fresh successful
// private directory, unless explicit retention is requested. Failures survive.
struct Originals {
    root: PathBuf,
    files: usize,
    bytes: usize,
    completed: bool,
}
impl Originals {
    fn new(label: &str) -> TestResult<Self> {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "ferrule-json5-native130-{label}-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir(&root)?;
        println!("retained native JSON5 originals: {}", root.display());
        Ok(Self {
            root,
            files: 0,
            bytes: 0,
            completed: false,
        })
    }
    fn save(&mut self, name: &str, bytes: &[u8]) -> TestResult {
        let next = self
            .bytes
            .checked_add(bytes.len())
            .ok_or("retention size overflow")?;
        if self.files >= 1600 || bytes.len() > 1024 * 1024 || next > 16 * 1024 * 1024 {
            return Err("small original evidence bound exceeded without truncation".into());
        }
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(self.root.join(name))?;
        file.write_all(bytes)?;
        file.sync_all()?;
        self.files += 1;
        self.bytes = next;
        Ok(())
    }
    fn outcome(&mut self, id: &str, actual: &Result<Instance, JsonFormatError>) -> TestResult {
        self.save(
            &format!("{id}-ORIGINAL-RESULT.debug.txt"),
            format!("{actual:#?}\n").as_bytes(),
        )?;
        if let Err(error) = actual {
            let mut text = format!("original: {error}\n");
            let mut cause = error.source();
            let mut count = 0;
            while let Some(original) = cause {
                if count == 16 {
                    return Err("original error chain bound exceeded".into());
                }
                text.push_str(&format!(
                    "cause {count}: {original:#?}\ndisplay: {original}\n"
                ));
                cause = original.source();
                count += 1;
            }
            self.save(&format!("{id}-ORIGINAL-ERROR-CHAIN.txt"), text.as_bytes())?;
        }
        Ok(())
    }
}
impl Drop for Originals {
    fn drop(&mut self) {
        if self.completed
            && !std::thread::panicking()
            && std::env::var("FERRULE_FORMAT_JSON_KEEP_ARTIFACTS").as_deref() != Ok("1")
        {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }
}
fn run(label: &str, cases: Vec<Case>) -> TestResult {
    if cases.len() > 256 {
        return Err("small case census exceeded".into());
    }
    let mut originals = Originals::new(label)?;
    let mut observed = Vec::with_capacity(cases.len());
    for case in cases {
        originals.save(&format!("{}-INPUT.json5", case.id), case.source.as_bytes())?;
        originals.save(
            &format!("{}-SCHEMA.json", case.id),
            &serde_json::to_vec_pretty(&case.schema)?,
        )?;
        let actual = format_json::from_json5_str(&case.source, &case.schema);
        originals.outcome(&case.id, &actual)?;
        observed.push((case, actual));
    }
    for (case, actual) in observed {
        match case.expected {
            Expected::Value(expected) => assert_eq!(actual.unwrap(), expected, "{}", case.id),
            // Native numeric admission retains the parser error before typed
            // projection; every original refusal and cause is retained.
            Expected::Refusal => assert!(
                matches!(actual, Err(JsonFormatError::Json5(_))),
                "{} lost numeric nonfinite parser refusal: {actual:#?}",
                case.id
            ),
            Expected::Shape { expected, got } => assert!(
                matches!(actual, Err(JsonFormatError::Shape { expected: e, got: g, .. }) if e == expected && g == got),
                "{}",
                case.id
            ),
            Expected::Undeclared => assert!(
                matches!(actual, Err(JsonFormatError::UndeclaredProperty { ref object, ref property }) if object == "Root" && property == "extra"),
                "{}",
                case.id
            ),
            Expected::Nesting => assert!(
                matches!(
                    actual,
                    Err(JsonFormatError::Json5NestingLimit { limit: 128 })
                ),
                "{}",
                case.id
            ),
            Expected::Parser => assert!(
                matches!(actual, Err(JsonFormatError::Json5(_))),
                "{}",
                case.id
            ),
        }
    }
    originals.completed = true;
    Ok(())
}

#[test]
fn json5_nonfinite_nullable_scalar_object_array_and_dynamic_positions_refuse() -> TestResult {
    let mut cases = Vec::new();
    for (index, token) in NONFINITE.into_iter().enumerate() {
        let positions = [
            ("scalar", token.into(), nullable_float("Value")),
            ("object-container", token.into(), nullable_object("Root")),
            ("array-container", token.into(), nullable_array("Items")),
            (
                "array-item",
                format!("[1.5,{token},2.5]"),
                nullable_float("Items").repeating(),
            ),
            ("object-field", format!("{{n:{token}}}"), object_n()),
            (
                "nested-object",
                format!("{{box:{token}}}"),
                SchemaNode::group("Root", vec![nullable_object("box")]),
            ),
            (
                "nested-array",
                format!("{{items:{token}}}"),
                SchemaNode::group("Root", vec![nullable_array("items")]),
            ),
            (
                "dynamic-scalar",
                format!("{{entry:{token}}}"),
                open(nullable_float("Value")),
            ),
            (
                "dynamic-object",
                format!("{{entry:{token}}}"),
                open(nullable_object("Value")),
            ),
            (
                "dynamic-array",
                format!("{{entry:{token}}}"),
                open(nullable_array("Value")),
            ),
            (
                "dynamic-any",
                format!("{{entry:[{token}]}}"),
                open(any("Value")),
            ),
        ];
        for (name, source, schema) in positions {
            cases.push(case(
                &format!("{name}-{index}"),
                source,
                schema,
                Expected::Refusal,
            ));
        }
    }
    run("positions", cases)
}

#[test]
fn json5_nonfinite_overwritten_nested_and_undeclared_members_refuse() -> TestResult {
    let mut cases = Vec::new();
    for (index, token) in NONFINITE.into_iter().enumerate() {
        for (name, source, schema) in [
            ("overwritten", format!("{{n:{token},n:1.5}}"), object_n()),
            (
                "escaped-overwrite",
                format!("{{n:{token},\\u006e:1.5}}"),
                object_n(),
            ),
            (
                "nested-object-overwrite",
                format!("{{n:{{masked:{token}}},n:{{value:1.5}}}}"),
                SchemaNode::group("Root", vec![any("n")]),
            ),
            (
                "nested-array-overwrite",
                format!("{{n:[{token}],n:[1.5]}}"),
                SchemaNode::group("Root", vec![any("n")]),
            ),
            (
                "dynamic-overwrite",
                format!("{{entry:{token},entry:1.5}}"),
                open(any("Value")),
            ),
            // A numeric nonfinite value is rejected before closed-object
            // projection; finite unknown fields retain their shape refusal.
            ("undeclared", format!("{{n:1.5,extra:{token}}}"), object_n()),
        ] {
            cases.push(case(
                &format!("{name}-{index}"),
                source,
                schema,
                Expected::Refusal,
            ));
        }
    }
    run("overwritten", cases)
}

#[test]
fn json5_legitimate_null_absence_empty_and_finite_values_remain_distinct() -> TestResult {
    let null = || scalar(Value::json_null());
    let finite = || scalar(Value::Float(1.5));
    let cases = vec![
        case(
            "scalar-null",
            "null",
            nullable_float("Value"),
            Expected::Value(null()),
        ),
        case(
            "object-null",
            "null",
            nullable_object("Root"),
            Expected::Value(null()),
        ),
        case(
            "array-null",
            "null",
            nullable_array("Items"),
            Expected::Value(null()),
        ),
        case(
            "array-null-item",
            "[null,1.5]",
            nullable_float("Items").repeating(),
            Expected::Value(Instance::Repeated(vec![null(), finite()])),
        ),
        case(
            "field-null",
            "{n:null}",
            object_n(),
            Expected::Value(group(vec![("n", null())])),
        ),
        case(
            "nested-object-null",
            "{box:null}",
            SchemaNode::group("Root", vec![nullable_object("box")]),
            Expected::Value(group(vec![("box", null())])),
        ),
        case(
            "nested-array-null",
            "{items:null}",
            SchemaNode::group("Root", vec![nullable_array("items")]),
            Expected::Value(group(vec![("items", null())])),
        ),
        case(
            "missing",
            "{}",
            SchemaNode::group(
                "Root",
                vec![
                    nullable_float("n"),
                    nullable_object("box"),
                    nullable_array("items"),
                ],
            ),
            Expected::Value(group(vec![
                ("n", scalar(Value::Null)),
                ("box", scalar(Value::Null)),
                ("items", scalar(Value::Null)),
            ])),
        ),
        case(
            "empty-object",
            "{}",
            SchemaNode::group("Root", Vec::new())
                .nullable_container()
                .unwrap(),
            Expected::Value(group(Vec::new())),
        ),
        case(
            "empty-array",
            "[]",
            nullable_array("Items"),
            Expected::Value(Instance::Repeated(Vec::new())),
        ),
        case(
            "finite",
            "1.5",
            nullable_float("Value"),
            Expected::Value(finite()),
        ),
        case(
            "hex-finite",
            "0x10",
            nullable_float("Value"),
            Expected::Value(scalar(Value::Float(16.0))),
        ),
        case(
            "dynamic-order",
            "{z:null,a:1.5}",
            open(nullable_float("Value")),
            Expected::Value(group(vec![("z", null()), ("a", finite())])),
        ),
        case(
            "any-null",
            "{entry:null}",
            open(any("Value")),
            Expected::Value(group(vec![("entry", scalar(Value::String("null".into())))])),
        ),
        case(
            "any-finite-object",
            "{entry:{n:1.5,other:null}}",
            open(any("Value")),
            Expected::Value(group(vec![(
                "entry",
                scalar(Value::String("{\"n\":1.5,\"other\":null}".into())),
            )])),
        ),
        case(
            "nonnullable-null",
            "null",
            SchemaNode::scalar("Value", ScalarType::Float),
            Expected::Shape {
                expected: "number",
                got: "null",
            },
        ),
        case(
            "undeclared-finite",
            "{n:1.5,extra:2.5}",
            object_n(),
            Expected::Undeclared,
        ),
    ];
    run("legitimate", cases)
}

#[test]
fn json5_quoted_comment_unicode_identifier_and_duplicate_controls_are_literal() -> TestResult {
    let string_schema = || SchemaNode::scalar("Value", ScalarType::String);
    let numeric = || SchemaNode::group("Root", vec![SchemaNode::scalar("n", ScalarType::Float)]);
    let number = |value| group(vec![("n", scalar(Value::Float(value)))]);
    let mut cases = Vec::new();
    for (index, token) in NONFINITE.into_iter().enumerate() {
        cases.push(case(
            &format!("quoted-{index}"),
            format!("'{token}'"),
            string_schema(),
            Expected::Value(scalar(Value::String(token.into()))),
        ));
    }
    cases.extend([
        case(
            "comment-line",
            "// Infinity +Infinity NaN 1e999\n{n:1.5}",
            numeric(),
            Expected::Value(number(1.5)),
        ),
        case(
            "comment-block",
            "/* -Infinity -NaN */{n:1.5}/* -1e999 */",
            numeric(),
            Expected::Value(number(1.5)),
        ),
        case(
            "quoted-object-text",
            "'{n:Infinity}'",
            string_schema(),
            Expected::Value(scalar(Value::String("{n:Infinity}".into()))),
        ),
        case(
            "escaped-string",
            r#"'Infinity\x20NaN\u0020雪'"#,
            string_schema(),
            Expected::Value(scalar(Value::String("Infinity NaN 雪".into()))),
        ),
        case(
            "continued-string",
            "'Infi\\\nnity NaN'",
            string_schema(),
            Expected::Value(scalar(Value::String("Infinity NaN".into()))),
        ),
        case(
            "escaped-key",
            r#"{\u006e:1.5}"#,
            numeric(),
            Expected::Value(number(1.5)),
        ),
        case(
            "unicode-key",
            "{雪:1.5}",
            SchemaNode::group("Root", vec![SchemaNode::scalar("雪", ScalarType::Float)]),
            Expected::Value(group(vec![("雪", scalar(Value::Float(1.5)))])),
        ),
        case(
            "escaped-unicode-key",
            r#"{\u96ea:1.5}"#,
            open(nullable_float("Value")),
            Expected::Value(group(vec![("雪", scalar(Value::Float(1.5)))])),
        ),
        case(
            "numeric-spelling-keys",
            "{Infinity:'finite',NaN:'quoted'}",
            open(SchemaNode::scalar("Value", ScalarType::String)),
            Expected::Value(group(vec![
                ("Infinity", scalar(Value::String("finite".into()))),
                ("NaN", scalar(Value::String("quoted".into()))),
            ])),
        ),
        case(
            "duplicate-finite",
            "{n:1.5,n:2.5}",
            numeric(),
            Expected::Value(number(2.5)),
        ),
        case(
            "duplicate-escaped-key",
            r#"{n:1.5,\u006e:2.5}"#,
            numeric(),
            Expected::Value(number(2.5)),
        ),
        case(
            "bom",
            "\u{feff}{n:1.5}",
            numeric(),
            Expected::Value(number(1.5)),
        ),
        case(
            "line-separator",
            "// Infinity\u{2028}{n:1.5}",
            numeric(),
            Expected::Value(number(1.5)),
        ),
        case(
            "paragraph-separator",
            "// NaN\u{2029}{n:1.5}",
            numeric(),
            Expected::Value(number(1.5)),
        ),
    ]);
    run("trivia", cases)
}

#[test]
fn json5_public_file_reader_retains_nonfinite_null_finite_and_utf8_outcomes() -> TestResult {
    let mut originals = Originals::new("files")?;
    let schema = nullable_float("Value");
    originals.save("SCHEMA.json", &serde_json::to_vec_pretty(&schema)?)?;
    let mut observed = Vec::new();
    for (index, text) in NONFINITE.into_iter().chain(["null", "1.5"]).enumerate() {
        let id = format!("file-{index}");
        let path = originals.root.join(format!("{id}.json5"));
        originals.save(&format!("{id}.json5"), text.as_bytes())?;
        let actual = format_json::read_json5(&path, &schema);
        originals.outcome(&id, &actual)?;
        observed.push((index, actual));
    }
    originals.save("invalid-utf8.json5", &[0xff])?;
    let invalid = format_json::read_json5(&originals.root.join("invalid-utf8.json5"), &schema);
    originals.outcome("invalid-utf8", &invalid)?;
    for (index, actual) in observed {
        if index < 8 {
            assert!(
                matches!(actual, Err(JsonFormatError::Json5(_))),
                "file-{index}: {actual:#?}"
            );
        } else {
            assert_eq!(
                actual.unwrap(),
                scalar(if index == 8 {
                    Value::json_null()
                } else {
                    Value::Float(1.5)
                })
            );
        }
    }
    assert!(
        matches!(invalid, Err(JsonFormatError::Io(ref original)) if original.kind() == std::io::ErrorKind::InvalidData)
    );
    originals.completed = true;
    Ok(())
}

#[test]
fn json5_original_typed_nesting_and_parser_refusals_remain_small() -> TestResult {
    let nested = format!("{}null{}", "[".repeat(129), "]".repeat(129));
    run(
        "limits",
        vec![
            case("nesting", nested.clone(), any("Root"), Expected::Nesting),
            case(
                "nesting-after-comment",
                format!("// Infinity\u{2028}{nested}"),
                any("Root"),
                Expected::Nesting,
            ),
            case(
                "trailing-root",
                "null null",
                nullable_float("Value"),
                Expected::Parser,
            ),
            case("malformed", "{n: }", object_n(), Expected::Parser),
        ],
    )
}
