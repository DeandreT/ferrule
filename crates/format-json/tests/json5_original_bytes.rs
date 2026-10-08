//! Native JSON5 original-byte accounting; generated adapters remain separate.
//! The ignored eight-call cohort requires a serial, externally supervised root run.

use std::error::Error;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use format_json::JsonFormatError;
use ir::{Instance, ScalarType, SchemaNode, Value};
use serde_json::{Value as JsonValue, json};

type TestResult<T = ()> = Result<T, Box<dyn Error>>;
const LIMIT: usize = 64 * 1024 * 1024;
const RESERVE_KIB: u64 = 20 * 1024 * 1024;
const BOM: &[u8] = b"\xef\xbb\xbf";
// The comment has two ASCII delimiter pairs and one two-byte UTF-8 character.
// Its six bytes plus the independent three-byte finite literal total nine.
const BASE: &[u8] = b"/*\xc3\xa9*/1.5";

const SOURCE_BODIES: [(&str, &[u8]); 7] = [
    (
        "crates/format-json/src/lib.rs",
        include_bytes!("../src/lib.rs"),
    ),
    (
        "crates/format-json/src/json5_finite.rs",
        include_bytes!("../src/json5_finite.rs"),
    ),
    (
        "crates/format-json/tests/json5_original_bytes.rs",
        include_bytes!("json5_original_bytes.rs"),
    ),
    (
        "crates/format-json/Cargo.toml",
        include_bytes!("../Cargo.toml"),
    ),
    ("Cargo.toml", include_bytes!("../../../Cargo.toml")),
    ("Cargo.lock", include_bytes!("../../../Cargo.lock")),
    (
        "docs/memory-and-limits.md",
        include_bytes!("../../../docs/memory-and-limits.md"),
    ),
];

fn metadata(path: &Path) -> TestResult<JsonValue> {
    let value = std::fs::symlink_metadata(path)?;
    if !value.is_file() || value.file_type().is_symlink() {
        return Err("regular non-symlink evidence/source file required".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        Ok(json!({
            "dev": value.dev(), "ino": value.ino(), "mode": value.mode(),
            "nlink": value.nlink(), "size": value.size(),
            "mtime": value.mtime(), "mtime_nsec": value.mtime_nsec(),
            "ctime": value.ctime(), "ctime_nsec": value.ctime_nsec()
        }))
    }
    #[cfg(not(unix))]
    {
        Ok(
            json!({"size": value.len(), "readonly": value.permissions().readonly(),
            "modified": format!("{:?}", value.modified())}),
        )
    }
}

struct Originals {
    root: PathBuf,
    maximum: usize,
    bytes: usize,
    files: usize,
}
impl Originals {
    fn new(label: &str, physical: bool) -> TestResult<Self> {
        let parent = if physical {
            let selected = PathBuf::from(std::env::var("FERRULE_NATIVE_JSON5_EVIDENCE_DIR")?);
            if !selected.is_absolute()
                || !selected.is_dir()
                || std::fs::canonicalize(&selected)? != selected
            {
                return Err("existing absolute canonical evidence directory required".into());
            }
            selected
        } else {
            std::env::temp_dir()
        };
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let root = parent.join(format!(
            "ferrule-native-json5-bytes163-{label}-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir(&root)?;
        println!(
            "retained complete native JSON5 byte originals: {}",
            root.display()
        );
        Ok(Self {
            root,
            maximum: if physical {
                4 * (LIMIT + 1) + 16 * 1024 * 1024
            } else {
                16 * 1024 * 1024
            },
            bytes: 0,
            files: 0,
        })
    }
    fn save(&mut self, name: &str, bytes: &[u8]) -> TestResult<PathBuf> {
        use std::io::Write;
        let next = self
            .bytes
            .checked_add(bytes.len())
            .ok_or("evidence byte overflow")?;
        if name.contains('/')
            || name.contains('\\')
            || self.files >= 256
            || bytes.len() > LIMIT + 1
            || next > self.maximum
        {
            return Err("finite evidence bound exceeded without truncation".into());
        }
        let path = self.root.join(name);
        let mut file = std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&path)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        self.bytes = next;
        self.files += 1;
        Ok(path)
    }
    fn record(&mut self, name: &str, value: &impl serde::Serialize) -> TestResult {
        self.save(name, &serde_json::to_vec_pretty(value)?)?;
        Ok(())
    }
    fn outcome(&mut self, id: &str, actual: &Result<Instance, JsonFormatError>) -> TestResult {
        self.save(
            &format!("{id}-ORIGINAL-RESULT.debug.txt"),
            format!("{actual:#?}\n").as_bytes(),
        )?;
        if let Err(error) = actual {
            let mut text = format!("original debug: {error:#?}\noriginal display: {error}\n");
            let mut cause = error.source();
            let mut count = 0;
            while let Some(original) = cause {
                if count == 16 {
                    return Err("complete error chain bound exceeded".into());
                }
                text.push_str(&format!(
                    "cause {count}: {original:#?}\ndisplay: {original}\n"
                ));
                cause = original.source();
                count += 1;
            }
            self.save(&format!("{id}-ORIGINAL-CAUSES.txt"), text.as_bytes())?;
        }
        Ok(())
    }
}

struct SourceGuard(Vec<(PathBuf, JsonValue, Vec<u8>)>);
impl SourceGuard {
    fn before(originals: &mut Originals) -> TestResult<Self> {
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let mut selected = Vec::new();
        for (index, (relative, compiled)) in SOURCE_BODIES.into_iter().enumerate() {
            let path = workspace.join(relative);
            let before = metadata(&path)?;
            let bytes = std::fs::read(&path)?;
            let after = metadata(&path)?;
            originals.save(&format!("SOURCE-{index}-ORIGINAL.bin"), &bytes)?;
            originals.record(
                &format!("SOURCE-{index}-BEFORE.json"),
                &json!({
                    "path": path, "before": before, "after": after,
                    "complete_compiled_bytes_equal": bytes == compiled
                }),
            )?;
            if before != after || bytes != compiled {
                return Err("source changed or differs from complete compiled body".into());
            }
            selected.push((path, after, bytes));
        }
        Ok(Self(selected))
    }
    fn after(&self, originals: &mut Originals) -> TestResult {
        let mut complete = true;
        for (index, (path, expected, bytes)) in self.0.iter().enumerate() {
            let observed = (|| -> TestResult<(JsonValue, Vec<u8>, JsonValue)> {
                let before = metadata(path)?;
                let actual = std::fs::read(path)?;
                Ok((before, actual, metadata(path)?))
            })();
            originals.save(
                &format!("SOURCE-{index}-AFTER-OBSERVATION.debug.txt"),
                format!("{observed:#?}\n").as_bytes(),
            )?;
            let exact = observed.as_ref().is_ok_and(|(before, actual, after)| {
                before == expected && after == expected && actual == bytes
            });
            complete &= exact;
            originals.record(
                &format!("SOURCE-{index}-AFTER.json"),
                &json!({"path": path, "complete_equal": exact}),
            )?;
        }
        if !complete {
            return Err(
                "complete source after guard refused; original observations retained".into(),
            );
        }
        Ok(())
    }
}

fn retained(
    label: &str,
    physical: bool,
    body: impl FnOnce(&mut Originals) -> TestResult,
) -> TestResult {
    let mut originals = Originals::new(label, physical)?;
    let guard = SourceGuard::before(&mut originals)?;
    let primary = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| body(&mut originals)));
    let after = guard.after(&mut originals);
    originals.save(
        "COMPLETE-AFTER-GUARD-RESULT.debug.txt",
        format!("{after:#?}\n").as_bytes(),
    )?;
    match primary {
        Ok(result) => {
            originals.save(
                "ORIGINAL-PRIMARY-RESULT.debug.txt",
                format!("{result:#?}\n").as_bytes(),
            )?;
            result?;
            after
        }
        Err(payload) => {
            let message = payload
                .downcast_ref::<String>()
                .map(String::as_str)
                .or_else(|| payload.downcast_ref::<&str>().copied())
                .unwrap_or("opaque original panic payload; not fabricated");
            originals.save("ORIGINAL-PANIC.txt", message.as_bytes())?;
            // The original assertion remains primary after source observation.
            std::panic::resume_unwind(payload)
        }
    }
}

#[derive(Debug)]
enum Expected {
    Value(Instance),
    Json5,
    Limit,
    Nesting,
    Undeclared,
    InvalidUtf8,
}
struct Case {
    id: String,
    input: Vec<u8>,
    schema: SchemaNode,
    expected: Expected,
}
fn scalar(value: Value) -> Expected {
    Expected::Value(Instance::Scalar(value))
}
fn float_schema() -> TestResult<SchemaNode> {
    Ok(SchemaNode::scalar("Value", ScalarType::Float)
        .nullable()
        .ok_or("nullable Float schema required")?)
}
fn compare(id: &str, actual: &Result<Instance, JsonFormatError>, expected: &Expected) {
    match expected {
        Expected::Value(value) => assert!(
            actual.as_ref().is_ok_and(|observed| observed == value),
            "{id}: {actual:#?}"
        ),
        Expected::Json5 => assert!(
            matches!(actual, Err(JsonFormatError::Json5(_))),
            "{id}: {actual:#?}"
        ),
        Expected::Limit => assert!(
            matches!(actual, Err(JsonFormatError::Json5DocumentLimit { limit }) if *limit == LIMIT),
            "{id}: {actual:#?}"
        ),
        Expected::Nesting => assert!(
            matches!(
                actual,
                Err(JsonFormatError::Json5NestingLimit { limit: 128 })
            ),
            "{id}: {actual:#?}"
        ),
        Expected::Undeclared => assert!(
            matches!(actual, Err(JsonFormatError::UndeclaredProperty { object, property }) if object == "Root" && property == "extra"),
            "{id}: {actual:#?}"
        ),
        Expected::InvalidUtf8 => assert!(
            matches!(actual, Err(JsonFormatError::Io(error)) if error.kind() == std::io::ErrorKind::InvalidData),
            "{id}: {actual:#?}"
        ),
    }
}
fn invoke(
    originals: &mut Originals,
    id: &str,
    input: &[u8],
    path: &Path,
    schema: &SchemaNode,
    text: bool,
) -> TestResult<Result<Instance, JsonFormatError>> {
    let before = metadata(path)?;
    let actual = if text {
        format_json::from_json5_str(std::str::from_utf8(input)?, schema)
    } else {
        format_json::read_json5(path, schema)
    };
    originals.outcome(id, &actual)?;
    let observed = std::fs::read(path)?;
    let after = metadata(path)?;
    originals.record(
        &format!("{id}-INPUT-AFTER.json"),
        &json!({
            "path": path, "original_bytes": input.len(), "observed_bytes": observed.len(),
            "complete_byte_equal": observed == input, "before": before, "after": after
        }),
    )?;
    if observed != input || before != after {
        return Err("input changed; original typed result retained before refusal".into());
    }
    Ok(actual)
}
fn padded(total: usize, bom: bool) -> TestResult<Vec<u8>> {
    let prefix = if bom { 3 + 9 } else { 9 };
    let padding = total
        .checked_sub(prefix)
        .ok_or("manual recipe needs nonnegative padding")?;
    let mut input = Vec::with_capacity(total);
    if bom {
        input.extend_from_slice(BOM);
    }
    input.extend_from_slice(BASE);
    input.resize(prefix + padding, b' ');
    Ok(input)
}

#[test]
fn json5_original_bytes_small_bom_utf8_null_finite_and_typed_errors() -> TestResult {
    let mut cases = Vec::new();
    for bom in [false, true] {
        let prefix = if bom { "\u{feff}" } else { "" };
        let role = if bom { "bom" } else { "plain" };
        for (label, text, expected) in [
            ("null", "null", scalar(Value::json_null())),
            ("finite", "1.5", scalar(Value::Float(1.5))),
            ("nan", "NaN", Expected::Json5),
            ("overflow", "1e999", Expected::Json5),
            ("malformed", "{", Expected::Json5),
        ] {
            cases.push(Case {
                id: format!("{role}-{label}"),
                input: format!("{prefix}{text}").into_bytes(),
                schema: float_schema()?,
                expected,
            });
        }
        cases.push(Case {
            id: format!("{role}-unicode"),
            input: format!("{prefix}'é'").into_bytes(),
            schema: SchemaNode::scalar("Text", ScalarType::String),
            expected: scalar(Value::String("é".into())),
        });
        cases.push(Case {
            id: format!("{role}-depth129"),
            input: format!("{prefix}{}0{}", "[".repeat(129), "]".repeat(129)).into_bytes(),
            schema: float_schema()?,
            expected: Expected::Nesting,
        });
        cases.push(Case {
            id: format!("{role}-unknown-finite"),
            input: format!("{prefix}{{n:1.5,extra:1}}").into_bytes(),
            schema: SchemaNode::group("Root", vec![SchemaNode::scalar("n", ScalarType::Float)]),
            expected: Expected::Undeclared,
        });
        cases.push(Case {
            id: format!("{role}-absent-field"),
            input: format!("{prefix}{{}}").into_bytes(),
            schema: SchemaNode::group(
                "Root",
                vec![
                    SchemaNode::scalar("n", ScalarType::Float)
                        .nullable()
                        .ok_or("nullable Float schema required")?,
                ],
            ),
            expected: Expected::Value(Instance::Group(
                vec![("n".into(), Instance::Scalar(Value::Null))].into(),
            )),
        });
        for total in [17, 18] {
            cases.push(Case {
                id: format!("{role}-manual-byte-recipe-{total}"),
                input: padded(total, bom)?,
                schema: float_schema()?,
                expected: scalar(Value::Float(1.5)),
            });
        }
    }
    retained("small", false, |originals| {
        let mut observed = Vec::new();
        for case in cases {
            let path = originals.save(&format!("{}-INPUT.json5", case.id), &case.input)?;
            originals.record(&format!("{}-SCHEMA.json", case.id), &case.schema)?;
            originals.save(
                &format!("{}-EXPECTED.debug.txt", case.id),
                format!("{:#?}\n", case.expected).as_bytes(),
            )?;
            for text in [true, false] {
                let id = format!("{}-{}", case.id, if text { "text" } else { "file" });
                let actual = invoke(originals, &id, &case.input, &path, &case.schema, text)?;
                observed.push((id, actual));
            }
            // Keep complete expected values owned alongside these observations.
            for (id, actual) in observed.drain(..) {
                compare(&id, &actual, &case.expected);
            }
        }
        let path = originals.save("invalid-utf8-INPUT.json5", &[0xff])?;
        let schema = float_schema()?;
        originals.record("invalid-utf8-SCHEMA.json", &schema)?;
        originals.save(
            "invalid-utf8-EXPECTED.debug.txt",
            format!("{:#?}\n", Expected::InvalidUtf8).as_bytes(),
        )?;
        let actual = invoke(
            originals,
            "invalid-utf8-file",
            &[0xff],
            &path,
            &schema,
            false,
        )?;
        compare("invalid-utf8-file", &actual, &Expected::InvalidUtf8);
        Ok(())
    })
}

fn deadline() -> TestResult<Instant> {
    if std::env::var("FERRULE_NATIVE_JSON5_PHYSICAL_OPT_IN").as_deref() != Ok("1")
        || std::env::var("FERRULE_NATIVE_JSON5_ROOT_EXCLUSIVE").as_deref() != Ok("1")
    {
        return Err("explicit root exclusive/serial supervised opt-in required".into());
    }
    let absolute = std::env::var("FERRULE_NATIVE_JSON5_DEADLINE_UNIX")?.parse::<u64>()?;
    let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    let remaining = absolute
        .checked_sub(now)
        .filter(|seconds| *seconds > 0 && *seconds <= 600)
        .ok_or("positive root outer deadline within600 seconds required")?;
    Instant::now()
        .checked_add(Duration::from_secs(remaining))
        .ok_or_else(|| "monotonic deadline overflow".into())
}
fn time_left(end: Instant) -> TestResult {
    if Instant::now() >= end {
        return Err("root deadline exhausted; no next physical call".into());
    }
    Ok(())
}
fn reserve(originals: &mut Originals, label: &str, end: Instant) -> TestResult {
    time_left(end)?;
    let tool = PathBuf::from(std::env::var("FERRULE_NATIVE_JSON5_DF")?);
    if !tool.is_absolute() || !tool.is_file() {
        return Err("actual root-selected absolute df path required".into());
    }
    let mut command = Command::new(&tool);
    command.args(["-Pk"]).arg(&originals.root);
    originals.save(
        &format!("{label}-DF-COMMAND.debug.txt"),
        format!("{command:#?}\n").as_bytes(),
    )?;
    let actual = command.output();
    originals.save(
        &format!("{label}-DF-ORIGINAL-RESULT.debug.txt"),
        format!("{actual:#?}\n").as_bytes(),
    )?;
    let result = actual?;
    originals.save(&format!("{label}-DF-stdout.bin"), &result.stdout)?;
    originals.save(&format!("{label}-DF-stderr.bin"), &result.stderr)?;
    time_left(end)?;
    if !result.status.success() {
        return Err("actual disk observation failed".into());
    }
    let text = std::str::from_utf8(&result.stdout)?;
    let values = text
        .lines()
        .skip(1)
        .map(|row| {
            row.split_whitespace()
                .nth(3)
                .ok_or("disk available field absent")?
                .parse::<u64>()
                .map_err(|error| error.into())
        })
        .collect::<TestResult<Vec<_>>>()?;
    if values.is_empty() || values.iter().any(|available| *available < RESERVE_KIB) {
        return Err("20GiB free disk reserve required before generation/each physical call".into());
    }
    Ok(())
}

#[test]
#[ignore = "eight physical original-byte calls require small tests first, root-exclusive serial outer timeout,20GiB reserve and explicit opt-in"]
fn json5_original_bytes_physical_exact_and_plus_one_text_file_bom_plain() -> TestResult {
    let end = deadline()?;
    retained("physical", true, |originals| {
        let schema = float_schema()?;
        originals.record("PHYSICAL-SCHEMA.json", &schema)?;
        originals.record(
            "MANUAL-BYTE-RECIPES.json",
            &json!({
                "limit": 67108864, "bom_bytes": 3, "base_bytes": 9,
                "base_literal": "/*é*/1.5", "padding": "ASCIIspace",
                "totals": [67108864,67108865], "bom": [false,true],
                "public_routes": ["from_json5_str","read_json5"], "calls": 8,
                "expected_exact": "Scalar(Float(1.5))",
                "expected_plus_one": "Json5DocumentLimit{limit:67108864}",
                "measurement": "No RSS measurement or total-memory guarantee is made by this test"
            }),
        )?;
        reserve(originals, "PRE-GENERATION", end)?;
        let mut corpus = Vec::new();
        for bom in [false, true] {
            for total in [LIMIT, LIMIT + 1] {
                time_left(end)?;
                let id = format!(
                    "{}-{}",
                    if bom { "bom" } else { "plain" },
                    if total == LIMIT { "exact" } else { "plus-one" }
                );
                let input = padded(total, bom)?;
                let path = originals.save(&format!("{id}-INPUT.json5"), &input)?;
                originals.record(
                    &format!("{id}-MANUAL-RECIPE.json"),
                    &json!({
                        "original_bytes": total, "bom_bytes": if bom { 3 } else { 0 },
                        "utf8_comment_bytes": 6, "finite_literal_bytes": 3,
                        "ascii_padding_bytes": total - if bom { 12 } else { 9 }
                    }),
                )?;
                corpus.push((id, total, path));
            }
        }
        let mut observed = Vec::new();
        for (id, total, path) in corpus {
            let input = std::fs::read(&path)?;
            if input.len() != total {
                return Err("full original fixture length changed".into());
            }
            let expected = if total == LIMIT {
                scalar(Value::Float(1.5))
            } else {
                Expected::Limit
            };
            for text in [true, false] {
                let role = format!("{id}-{}", if text { "text" } else { "file" });
                reserve(originals, &format!("PRE-{role}"), end)?;
                let actual = invoke(originals, &role, &input, &path, &schema, text)?;
                time_left(end)?;
                observed.push((role, actual));
            }
            // Both route originals and their complete input afterguards precede comparisons.
            for (role, actual) in observed.drain(..) {
                compare(&role, &actual, &expected);
            }
        }
        originals.record(
            "ALL-EIGHT-PUBLIC-CALLS-COMPLETE.json",
            &json!({"calls":8,"assertions_passed":true,"actual_runtime_only":true}),
        )?;
        Ok(())
    })
}
