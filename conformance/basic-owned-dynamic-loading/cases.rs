//! Independently authored inputs, host documents and complete expected
//! outcomes. Every case is called through all outputs, the Primary target and
//! the `audit` target, each with a fresh recording host loader. `calls` lists
//! every attempted callback; `reads` lists the physical reads left after the
//! host's success-only cache reuses a repeated successful document.
use super::*;
use projects::{AUDIT, DOC_PATH, TITLE};

pub fn text(value: &str) -> Instance {
    Instance::Scalar(Value::String(value.into()))
}
fn int(value: i64) -> Instance {
    Instance::Scalar(Value::Int(value))
}
pub fn group(fields: Vec<(&str, Instance)>) -> Instance {
    Instance::Group(
        fields
            .into_iter()
            .map(|(name, value)| (name.into(), value))
            .collect::<Vec<_>>()
            .into(),
    )
}
fn repeated(items: Vec<Instance>) -> Instance {
    Instance::Repeated(items)
}

/// Driver rows: an absent path omits the field.
fn batch_rows(rows: Vec<(Option<Value>, &str)>) -> Instance {
    group(vec![(
        "Files",
        repeated(
            rows.into_iter()
                .map(|(path, label)| match path {
                    Some(path) => group(vec![
                        ("Path", Instance::Scalar(path)),
                        ("Label", text(label)),
                    ]),
                    None => group(vec![("Label", text(label))]),
                })
                .collect(),
        ),
    )])
}
fn batch(files: &[(&str, &str)]) -> Instance {
    batch_rows(
        files
            .iter()
            .map(|(path, label)| (Some(Value::String((*path).into())), *label))
            .collect(),
    )
}
fn catalog(files: &[(&str, &str)]) -> Vec<(String, Instance)> {
    vec![(projects::CATALOG.into(), batch(files))]
}
fn doc(title: &str, lines: &[&str], parts: &[&str]) -> Instance {
    group(vec![
        ("Title", text(title)),
        (
            "Lines",
            repeated(
                lines
                    .iter()
                    .map(|code| group(vec![("Code", text(code))]))
                    .collect(),
            ),
        ),
        (
            "Parts",
            repeated(
                parts
                    .iter()
                    .map(|path| group(vec![("Path", text(path))]))
                    .collect(),
            ),
        ),
    ])
}
/// The host's documents: Docs `a`, `b`, `c` and Parts `p1`, `p2`.
pub fn documents() -> Vec<(String, String, Instance)> {
    [
        ("Docs", "a.json", doc("A", &["a1", "a2"], &["p1.json"])),
        ("Docs", "b.json", doc("B", &["b1"], &["p1.json", "p2.json"])),
        ("Docs", "c.json", doc("C", &[], &[])),
        ("Parts", "p1.json", group(vec![("Name", text("P1"))])),
        ("Parts", "p2.json", group(vec![("Name", text("P2"))])),
    ]
    .into_iter()
    .map(|(source, path, document)| (source.into(), path.into(), document))
    .collect()
}

fn line(code: &str, position: i64) -> Instance {
    group(vec![("Code", text(code)), ("Position", int(position))])
}
fn part(name: &str, doc: &str, label: &str) -> Instance {
    group(vec![
        ("Name", text(name)),
        ("Doc", text(doc)),
        ("Label", text(label)),
    ])
}
fn out_doc(
    title: &str,
    label: &str,
    driver: i64,
    lines: Vec<Instance>,
    parts: Vec<Instance>,
) -> Instance {
    group(vec![
        ("Title", text(title)),
        ("Label", text(label)),
        ("Driver", int(driver)),
        ("Lines", repeated(lines)),
        ("Parts", repeated(parts)),
    ])
}
fn out(docs: Vec<Instance>) -> Instance {
    group(vec![("Docs", repeated(docs))])
}
fn labels(labels: &[&str]) -> Instance {
    group(vec![(
        "Labels",
        repeated(
            labels
                .iter()
                .map(|label| group(vec![("Label", text(label))]))
                .collect(),
        ),
    )])
}
fn doc_a(label: &str, driver: i64) -> Instance {
    out_doc(
        "A",
        label,
        driver,
        vec![line("a1", 1), line("a2", 2)],
        vec![part("P1", "A", label)],
    )
}
fn doc_b(label: &str, driver: i64) -> Instance {
    out_doc(
        "B",
        label,
        driver,
        vec![line("b1", 1)],
        vec![part("P1", "B", label), part("P2", "B", label)],
    )
}

fn missing_loader() -> Json {
    json!({"kind":"MissingLoader","source":"Docs"})
}
fn unavailable(owner: &str, node: Option<u32>) -> Json {
    json!({"kind":"OwnedSource","cause":"SourceUnavailable","owner":owner,"node":node})
}

/// One target's outcome with its attempted callbacks and physical reads.
pub struct Target {
    result: Result<Instance, Json>,
    calls: Vec<&'static str>,
    reads: Vec<&'static str>,
}
fn ok(tree: Instance, calls: Vec<&'static str>, reads: Vec<&'static str>) -> Target {
    Target {
        result: Ok(tree),
        calls,
        reads,
    }
}
fn quiet(tree: Instance) -> Target {
    ok(tree, Vec::new(), Vec::new())
}
fn fails(recipe: Json, calls: Vec<&'static str>, reads: Vec<&'static str>) -> Target {
    Target {
        result: Err(recipe),
        calls,
        reads,
    }
}

/// All outputs run Primary, then `audit`, and stop at the first failure.
fn calls(primary: Target, audit: Target) -> Vec<Json> {
    let tree = |name: Option<&str>, tree: &Instance| json!([name, codec::encode(tree)]);
    let single = |name: Option<&str>, target: &Target| {
        let expected = match &target.result {
            Ok(value) => json!({ "Ok": [tree(name, value)] }),
            Err(recipe) => json!({ "Err": recipe }),
        };
        (expected, target.calls.clone(), target.reads.clone())
    };
    let all = match (&primary.result, &audit.result) {
        (Err(recipe), _) => (
            json!({ "Err": recipe }),
            primary.calls.clone(),
            primary.reads.clone(),
        ),
        (Ok(first), result) => {
            let calls = primary
                .calls
                .iter()
                .chain(&audit.calls)
                .copied()
                .collect::<Vec<_>>();
            let reads = primary
                .reads
                .iter()
                .chain(&audit.reads)
                .copied()
                .collect::<Vec<_>>();
            let expected = match result {
                Ok(second) => json!({ "Ok": [tree(None, first), tree(Some(AUDIT), second)] }),
                Err(recipe) => json!({ "Err": recipe }),
            };
            (expected, calls, reads)
        }
    };
    [("All", all), ("Primary", single(None, &primary)), (AUDIT, single(Some(AUDIT), &audit))]
        .into_iter()
        .map(|(selection, (expected, calls, reads))| {
            json!({"selection":selection,"expected":expected,"calls":calls,"reads":reads})
        })
        .collect()
}
fn case(
    id: &'static str,
    profile: &'static str,
    source: Instance,
    inputs: Vec<(String, Instance)>,
    loader: bool,
    primary: Target,
    audit: Target,
) -> Case {
    Case {
        id,
        profile,
        source,
        inputs,
        loader,
        calls: calls(primary, audit),
    }
}

pub fn cases() -> Vec<Case> {
    let two = || batch(&[("a.json", "first"), ("b.json", "second")]);
    let nested_calls = vec![
        "Docs:a.json",
        "Docs:b.json",
        "Parts:p1.json",
        "Parts:p1.json",
        "Parts:p2.json",
    ];
    let nested_reads = vec![
        "Docs:a.json",
        "Docs:b.json",
        "Parts:p1.json",
        "Parts:p2.json",
    ];
    vec![
        // Each loaded document keeps its own driver row and identity through
        // nested loading; the repeated p1 callback is one physical read.
        case(
            "L01-nested",
            "L01-primary-driver",
            two(),
            catalog(&[("c.json", "catalog")]),
            true,
            ok(
                out(vec![doc_a("first", 1), doc_b("second", 2)]),
                nested_calls,
                nested_reads,
            ),
            quiet(labels(&["first", "second"])),
        ),
        // Absent and null paths skip their driver row; the driver position
        // stays the raw row's position.
        case(
            "L01-null-paths",
            "L01-primary-driver",
            batch_rows(vec![
                (None, "row1"),
                (Some(Value::Null), "row2"),
                (Some(Value::json_null()), "row3"),
                (Some(Value::String("b.json".into())), "row4"),
            ]),
            Vec::new(),
            true,
            ok(
                out(vec![doc_b("row4", 4)]),
                vec!["Docs:b.json", "Parts:p1.json", "Parts:p2.json"],
                vec!["Docs:b.json", "Parts:p1.json", "Parts:p2.json"],
            ),
            quiet(labels(&["row1", "row2", "row3", "row4"])),
        ),
        // A non-string path fails after earlier requests in driver order.
        case(
            "L01-int-path",
            "L01-primary-driver",
            batch_rows(vec![
                (Some(Value::String("a.json".into())), "first"),
                (Some(Value::Int(7)), "second"),
                (Some(Value::String("b.json".into())), "third"),
            ]),
            Vec::new(),
            true,
            fails(
                json!({"kind":"DynamicPath","source":"Docs","node":DOC_PATH,"found":"int"}),
                vec!["Docs:a.json"],
                vec!["Docs:a.json"],
            ),
            quiet(labels(&["first", "second", "third"])),
        ),
        // A host failure keeps its source and path and is never cached.
        case(
            "L01-load-failure",
            "L01-primary-driver",
            batch(&[("a.json", "first"), ("missing.json", "second")]),
            Vec::new(),
            true,
            fails(
                json!({"kind":"Load","source":"Docs","path":"missing.json",
                    "message":"no document Docs:missing.json"}),
                vec!["Docs:a.json", "Docs:missing.json"],
                vec!["Docs:a.json", "Docs:missing.json"],
            ),
            quiet(labels(&["first", "second"])),
        ),
        // A reached loading scope needs a loader; the audit target never does.
        case(
            "L01-no-loader",
            "L01-primary-driver",
            batch(&[("a.json", "first")]),
            Vec::new(),
            false,
            fails(missing_loader(), Vec::new(), Vec::new()),
            quiet(labels(&["first"])),
        ),
        case(
            "L01-empty-driver",
            "L01-primary-driver",
            batch(&[]),
            Vec::new(),
            true,
            quiet(out(Vec::new())),
            quiet(labels(&[])),
        ),
        // The same-shaped named driver supplies only its own rows.
        case(
            "L02-catalog",
            "L02-named-driver",
            two(),
            catalog(&[("c.json", "catalog")]),
            true,
            ok(
                out(vec![out_doc("C", "catalog", 1, Vec::new(), Vec::new())]),
                vec!["Docs:c.json"],
                vec!["Docs:c.json"],
            ),
            quiet(labels(&["first", "second"])),
        ),
        // An unsupplied driver endpoint is never repaired by Primary rows.
        case(
            "L02-unsupplied",
            "L02-named-driver",
            two(),
            Vec::new(),
            true,
            fails(unavailable(projects::CATALOG, None), Vec::new(), Vec::new()),
            quiet(labels(&["first", "second"])),
        ),
        case(
            "L02-empty-catalog",
            "L02-named-driver",
            two(),
            vec![(projects::CATALOG.into(), group(Vec::new()))],
            true,
            quiet(out(Vec::new())),
            quiet(labels(&["first", "second"])),
        ),
        // Every document is requested before the filter and sort; only kept
        // documents load their Parts, and the driver position is compacted.
        case(
            "L03-filter-sort",
            "L03-filter-sort",
            batch(&[("a.json", "m"), ("b.json", "k"), ("c.json", "z")]),
            Vec::new(),
            true,
            ok(
                out(vec![
                    out_doc("C", "z", 1, Vec::new(), Vec::new()),
                    doc_b("k", 2),
                ]),
                vec![
                    "Docs:a.json",
                    "Docs:b.json",
                    "Docs:c.json",
                    "Parts:p1.json",
                    "Parts:p2.json",
                ],
                vec![
                    "Docs:a.json",
                    "Docs:b.json",
                    "Docs:c.json",
                    "Parts:p1.json",
                    "Parts:p2.json",
                ],
            ),
            quiet(labels(&["m", "k", "z"])),
        ),
        case(
            "L04-skip-first",
            "L04-window",
            two(),
            Vec::new(),
            true,
            ok(
                out(vec![doc_b("second", 1)]),
                vec![
                    "Docs:a.json",
                    "Docs:b.json",
                    "Parts:p1.json",
                    "Parts:p2.json",
                ],
                vec![
                    "Docs:a.json",
                    "Docs:b.json",
                    "Parts:p1.json",
                    "Parts:p2.json",
                ],
            ),
            quiet(labels(&["first", "second"])),
        ),
        // Only the selected named target loads.
        case(
            "L05-audit-only",
            "L05-audit-loads",
            batch(&[("a.json", "first")]),
            Vec::new(),
            true,
            quiet(labels(&["first"])),
            ok(
                out(vec![doc_a("first", 1)]),
                vec!["Docs:a.json", "Parts:p1.json"],
                vec!["Docs:a.json", "Parts:p1.json"],
            ),
        ),
        case(
            "L05-audit-no-loader",
            "L05-audit-loads",
            batch(&[("a.json", "first")]),
            Vec::new(),
            false,
            quiet(labels(&["first"])),
            fails(missing_loader(), Vec::new(), Vec::new()),
        ),
        // Outside its loading scope a loaded owner has no document.
        case(
            "L06-outside",
            "L06-outside-read",
            batch(&[("a.json", "first")]),
            Vec::new(),
            true,
            fails(
                unavailable(projects::DOCS, Some(TITLE)),
                Vec::new(),
                Vec::new(),
            ),
            quiet(labels(&["first"])),
        ),
    ]
}
