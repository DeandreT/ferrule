//! Independently authored inputs and complete expected outcomes. Every case is
//! called through all outputs, the Primary target and the `audit` target.
use super::*;
use projects::{AUDIT, BATCH, LOOKUP};

pub fn text(value: &str) -> Instance {
    Instance::Scalar(Value::String(value.into()))
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
fn line(sku: &str, qty: i64) -> Instance {
    group(vec![("Sku", text(sku)), ("Qty", Instance::Scalar(Value::Int(qty)))])
}
fn row(code: &str, name: &str, rank: i64, lines: Vec<Instance>) -> Instance {
    group(vec![
        ("Code", text(code)),
        ("Name", text(name)),
        ("Rank", Instance::Scalar(Value::Int(rank))),
        ("Lines", Instance::Repeated(lines)),
    ])
}
pub fn endpoint(note: &str, rows: Vec<Instance>) -> Instance {
    group(vec![("Note", text(note)), ("Rows", Instance::Repeated(rows))])
}
/// Primary repeats code A; only Lookup's second row reuses a Primary SKU.
fn primary(note: &str) -> Instance {
    endpoint(
        note,
        vec![
            row("A", "P-A", 1, vec![line("a1", 1), line("a2", 5)]),
            row("B", "P-B", 2, vec![line("b1", 7)]),
            row("A", "P-A2", 3, vec![]),
        ],
    )
}
fn lookup_rows() -> Vec<Instance> {
    vec![
        row("C", "N-C", 1, vec![line("c1", 3)]),
        row("D", "N-D", 2, vec![line("a1", 9)]),
    ]
}
fn lookup() -> Instance {
    endpoint("N.note", lookup_rows())
}
fn batch() -> Instance {
    let item = |code: &str, name: &str| group(vec![("Code", text(code)), ("Name", text(name))]);
    Instance::Repeated(vec![item("a", "B-a"), item("b", "B-b")])
}
/// Lookup rows plus a repeating scalar path collection.
fn paths(paths: &[&str]) -> Instance {
    group(vec![
        ("Note", text("N.note")),
        ("Rows", Instance::Repeated(lookup_rows())),
        ("Paths", Instance::Repeated(paths.iter().map(|path| text(path)).collect())),
    ])
}
/// One recursive directory with its files and child directories.
fn directory(note: &str, files: &[&str], children: Vec<Instance>) -> Instance {
    group(vec![
        ("Note", text(note)),
        (
            "file",
            Instance::Repeated(files.iter().map(|file| group(vec![("Note", text(file))])).collect()),
        ),
        ("Dir", Instance::Repeated(children)),
    ])
}
/// Lookup rows plus one recursive directory tree.
fn dirs() -> Instance {
    group(vec![
        ("Note", text("N.note")),
        ("Rows", Instance::Repeated(lookup_rows())),
        (
            "Dir",
            Instance::Repeated(vec![directory("n", &["m"], vec![directory("o", &["k"], vec![])])]),
        ),
    ])
}
/// One owned path-hierarchy directory with its files and subdirectories.
fn tree(name: &str, files: &[&str], directories: Vec<Instance>) -> Instance {
    group(vec![
        (
            "file",
            Instance::Repeated(files.iter().map(|file| group(vec![("name", text(file))])).collect()),
        ),
        ("directory", Instance::Repeated(directories)),
        ("name", text(name)),
    ])
}
fn inputs() -> Vec<(String, Instance)> {
    vec![(LOOKUP.into(), lookup()), (BATCH.into(), batch())]
}
fn with_lookup(lookup: Instance) -> Vec<(String, Instance)> {
    vec![(LOOKUP.into(), lookup), (BATCH.into(), batch())]
}

/// One target's complete expected tree or typed error recipe.
type Target = Result<Instance, Json>;

fn scalar(value: Option<&str>) -> Instance {
    match value {
        Some(value) => text(value),
        None => Instance::Scalar(Value::Null),
    }
}
fn output(value: Option<&str>) -> Target {
    Ok(group(vec![("Value", scalar(value))]))
}
fn output_items(value: &str, items: &[(&str, Option<&str>)]) -> Target {
    Ok(group(vec![
        ("Value", text(value)),
        (
            "Items",
            Instance::Repeated(
                items
                    .iter()
                    .map(|(code, value)| group(vec![("Code", text(code)), ("Value", scalar(*value))]))
                    .collect(),
            ),
        ),
    ]))
}
fn audit(value: Option<&str>) -> Target {
    Ok(group(vec![("Value", scalar(value))]))
}
fn unavailable(owner: &str, node: u32) -> Json {
    json!({"kind":"OwnedSource","cause":"SourceUnavailable","owner":owner,"node":node})
}
fn not_bool(node: u32) -> Json {
    json!({"kind":"NotABool","node":node,"found":"string"})
}
fn failure(rule: usize, message: &str) -> Json {
    json!({"kind":"MappingFailure","rule":rule,"message":message})
}

/// All outputs evaluate Primary before audit, so Primary's error wins.
fn calls(primary: &Target, audit: &Target) -> Vec<Json> {
    let expected = |target: &Target, name: Option<&str>| match target {
        Ok(tree) => json!({"Ok":[[name, codec::encode(tree)]]}),
        Err(recipe) => json!({ "Err": recipe }),
    };
    let all = match (primary, audit) {
        (Err(recipe), _) | (Ok(_), Err(recipe)) => json!({ "Err": recipe }),
        (Ok(primary), Ok(audit)) => json!({"Ok":[
            [null, codec::encode(primary)],
            [AUDIT, codec::encode(audit)],
        ]}),
    };
    vec![
        json!({"selection":"All","expected":all}),
        json!({"selection":"Primary","expected":expected(primary, None)}),
        json!({"selection":AUDIT,"expected":expected(audit, Some(AUDIT))}),
    ]
}
fn case(
    id: &'static str,
    profile: &'static str,
    source: Instance,
    inputs: Vec<(String, Instance)>,
    primary: Target,
    audit: Target,
) -> Case {
    Case {
        id,
        profile,
        calls: calls(&primary, &audit),
        source,
        inputs,
    }
}

pub fn cases() -> Vec<Case> {
    let l01 = "L01-lookup-by-parent-needle";
    let keyless = group(vec![
        ("Note", text("N.note")),
        (
            "Rows",
            Instance::Repeated(vec![
                group(vec![("Name", text("no-code"))]),
                group(vec![("Code", Instance::Scalar(Value::Null)), ("Name", text("null-code"))]),
            ]),
        ),
    ]);
    vec![
        // First match in source order; each owner reads only its own rows.
        case("L01-primary", l01, primary("A"), inputs(), output(Some("P-A")), audit(None)),
        case("L01-foreign", l01, primary("C"), inputs(), output(None), audit(Some("N-C"))),
        case(
            "L01-empty",
            l01,
            primary("B"),
            with_lookup(endpoint("N.note", vec![])),
            output(Some("P-B")),
            audit(None),
        ),
        case(
            "L01-absent",
            l01,
            primary("B"),
            with_lookup(group(vec![("Note", text("N.note"))])),
            output(Some("P-B")),
            audit(None),
        ),
        case(
            "L01-null-rows",
            l01,
            primary("B"),
            with_lookup(group(vec![("Rows", Instance::Scalar(Value::Null))])),
            output(Some("P-B")),
            audit(None),
        ),
        // A missing needle never selects absent or Null keys.
        case(
            "L01-null-needle",
            l01,
            group(vec![("Rows", Instance::Repeated(vec![]))]),
            with_lookup(keyless),
            output(None),
            audit(None),
        ),
        case(
            "L01-unsupplied",
            l01,
            primary("A"),
            vec![(BATCH.into(), batch())],
            output(Some("P-A")),
            Err(unavailable(LOOKUP, 91)),
        ),
        case(
            "L02-primary",
            "L02-find-with-owned-ordinal",
            primary("B"),
            inputs(),
            output(Some("P-B:2")),
            audit(None),
        ),
        case(
            "L02-named",
            "L02-find-with-owned-ordinal",
            primary("D"),
            inputs(),
            output(None),
            audit(Some("N-D:2")),
        ),
        case(
            "L03-primary",
            "L03-find-with-legacy-item-reads",
            primary("B"),
            inputs(),
            output(Some("P-B")),
            audit(None),
        ),
        case(
            "L03-named",
            "L03-find-with-legacy-item-reads",
            primary("D"),
            inputs(),
            output(None),
            audit(Some("N-D")),
        ),
        // Primary lines come from the current row; Lookup lines are flattened.
        case(
            "L04",
            "L04-nested-parent-lines",
            primary("P.note"),
            inputs(),
            output_items("lines", &[("A", Some("1/9")), ("B", Some("/9")), ("A", Some("/9"))]),
            // Target adaptation keeps the found integer.
            Ok(group(vec![("Value", Instance::Scalar(Value::Int(9)))])),
        ),
        case(
            "L04-empty-named",
            "L04-nested-parent-lines",
            primary("P.note"),
            with_lookup(endpoint("N.note", vec![])),
            output_items("lines", &[("A", Some("1/")), ("B", Some("/")), ("A", Some("/"))]),
            audit(None),
        ),
        case(
            "L05",
            "L05-find-reads-parent-and-root-pins",
            primary("P.note"),
            inputs(),
            output_items(
                "ranks",
                &[("A", Some("N-C/P-A/N.note")), ("B", Some("N-D/P-B/N.note")), ("A", None)],
            ),
            audit(Some("N-D/N.note")),
        ),
        case(
            "L06-first-match",
            "L06-predicate-policy",
            primary("C"),
            inputs(),
            output(Some("P-B")),
            audit(Some("N-C")),
        ),
        case(
            "L06-not-bool",
            "L06-predicate-policy",
            primary("D"),
            inputs(),
            output(Some("P-B")),
            Err(not_bool(53)),
        ),
        case(
            "L06-empty",
            "L06-predicate-policy",
            primary("D"),
            with_lookup(endpoint("N.note", vec![])),
            output(Some("P-B")),
            audit(None),
        ),
        case(
            "L07",
            "L07-repeated-named-root",
            primary("b"),
            inputs(),
            output(Some("B-b")),
            audit(Some("B-b:2")),
        ),
        case(
            "L07-unsupplied",
            "L07-repeated-named-root",
            primary("b"),
            vec![(LOOKUP.into(), lookup())],
            Err(unavailable(BATCH, 90)),
            Err(unavailable(BATCH, 91)),
        ),
        // Group A holds P-A and P-A2; only P-A2 ranks above 2.
        case(
            "L08",
            "L08-find-within-owned-groups",
            primary("D"),
            inputs(),
            Ok(group(vec![
                ("Value", text("N-D")),
                (
                    "Items",
                    Instance::Repeated(vec![
                        group(vec![("Code", text("A")), ("Value", text("P-A2:2"))]),
                        group(vec![("Code", text("B")), ("Value", Instance::Scalar(Value::Null))]),
                    ]),
                ),
            ])),
            audit(Some("P-A2:3")),
        ),
        case(
            "L09",
            "L09-failure-walk-beside-search",
            primary("C"),
            inputs(),
            output(None),
            audit(Some("N-C")),
        ),
        case(
            "L09-failure",
            "L09-failure-walk-beside-search",
            primary("C"),
            with_lookup(endpoint(
                "N.note",
                vec![row("C", "N-C", 1, vec![]), row("X", "N-X", 2, vec![])],
            )),
            Err(failure(1, "N-X")),
            Err(failure(1, "N-X")),
        ),
        case(
            "L10",
            "L10-root-identity-lookup",
            primary("A"),
            inputs(),
            output(Some("A")),
            audit(None),
        ),
        case(
            "L11",
            "L11-search-beside-owned-hierarchy",
            primary("D"),
            with_lookup(paths(&["n/x.txt", "n/y/z.txt"])),
            output(Some("N-D")),
            Ok(tree("n", &["x.txt"], vec![tree("y", &["z.txt"], vec![])])),
        ),
        case(
            "L11-miss",
            "L11-search-beside-owned-hierarchy",
            primary("A"),
            with_lookup(paths(&["n/x.txt"])),
            output(None),
            Ok(tree("n", &["x.txt"], vec![])),
        ),
        // Blocks of two hold [P-A, P-B] and [P-A2]; positions count members.
        case(
            "L12",
            "L12-find-within-owned-blocks",
            primary("P.note"),
            inputs(),
            output_items("blocks", &[("A", Some("P-B:2")), ("A", Some("P-A2:1"))]),
            audit(Some("P-B:2")),
        ),
        case(
            "L13",
            "L13-search-beside-owned-recursive-walk",
            primary("D"),
            with_lookup(dirs()),
            output(Some("/n/o/k")),
            audit(Some("N-D")),
        ),
        // Primary has no directories; only the Lookup walk supplies values.
        case(
            "L13-short",
            "L13-search-beside-owned-recursive-walk",
            primary("C"),
            with_lookup(endpoint("N.note", lookup_rows())),
            output(None),
            audit(Some("N-C")),
        ),
        // Only file `k` survives; emptied directories keep their structure.
        case(
            "L14",
            "L14-find-beside-owned-recursive-filter",
            primary("D"),
            with_lookup(dirs()),
            output(Some("N-D")),
            Ok(group(vec![(
                "Dir",
                Instance::Repeated(vec![directory(
                    "n",
                    &[],
                    vec![directory("o", &["k"], vec![])],
                )]),
            )])),
        ),
        case(
            "L14-no-dirs",
            "L14-find-beside-owned-recursive-filter",
            primary("A"),
            inputs(),
            output(None),
            Ok(group(vec![("Dir", Instance::Repeated(vec![]))])),
        ),
        case(
            "L10-same-note",
            "L10-root-identity-lookup",
            primary("N.note"),
            inputs(),
            output(Some("N.note")),
            audit(Some("N.note")),
        ),
    ]
}
