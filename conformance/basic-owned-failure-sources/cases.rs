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
pub fn row(flag: bool, code: &str, message: &str) -> Instance {
    group(vec![
        ("Flag", Instance::Scalar(Value::Bool(flag))),
        ("Code", text(code)),
        ("Message", text(message)),
    ])
}
fn line(flag: bool, code: &str) -> Instance {
    group(vec![
        ("Flag", Instance::Scalar(Value::Bool(flag))),
        ("Code", text(code)),
    ])
}
fn order(id: &str, lines: Vec<Instance>) -> Instance {
    group(vec![("Id", text(id)), ("Lines", Instance::Repeated(lines))])
}
pub fn endpoint(note: &str, rows: Vec<Instance>, orders: Vec<Instance>) -> Instance {
    group(vec![
        ("Note", text(note)),
        ("Rows", Instance::Repeated(rows)),
        ("Orders", Instance::Repeated(orders)),
    ])
}
/// Primary's first true row is P2 and first false row is P1.
pub fn primary() -> Instance {
    endpoint(
        "P.note",
        vec![row(false, "A", "P1"), row(true, "B", "P2"), row(true, "C", "P3")],
        vec![order("PA", vec![line(true, "pa1")])],
    )
}
fn primary_false() -> Instance {
    endpoint(
        "P.note",
        vec![row(false, "A", "P1"), row(false, "B", "P2")],
        vec![],
    )
}
/// Lookup's flags are opposite: its first true row is N1, first false N2.
pub fn lookup() -> Instance {
    endpoint(
        "N.note",
        vec![row(true, "A", "N1"), row(false, "X", "N2"), row(false, "B", "N3")],
        vec![
            order("A", vec![line(false, "a1")]),
            order("B", vec![line(false, "b1"), line(true, "b2")]),
        ],
    )
}
fn lookup_rows(rows: Vec<Instance>) -> Instance {
    endpoint("N.note", rows, vec![])
}
fn batch() -> Instance {
    Instance::Repeated(vec![row(true, "a", "B1"), row(false, "b", "B2")])
}
fn inputs() -> Vec<(String, Instance)> {
    vec![(LOOKUP.into(), lookup()), (BATCH.into(), batch())]
}
fn with_lookup(lookup: Instance) -> Vec<(String, Instance)> {
    vec![(LOOKUP.into(), lookup), (BATCH.into(), batch())]
}

pub enum Outcome {
    Outputs,
    Failure(usize, Option<&'static str>),
    Error(Json),
}
fn failed(rule: usize, message: &'static str) -> Outcome {
    Outcome::Failure(rule, Some(message))
}
fn unavailable(owner: &str) -> Outcome {
    Outcome::Error(json!({"kind":"OwnedSource","cause":"SourceUnavailable","owner":owner}))
}
fn not_bool(node: u32, found: &str) -> Outcome {
    Outcome::Error(json!({"kind":"NotABool","node":node,"found":found}))
}

fn outputs(selection: &str) -> Json {
    let primary = json!([null, codec::encode(&group(vec![("Value", text("P.note"))]))]);
    let audit = json!([AUDIT, codec::encode(&group(vec![("Value", text("N.note"))]))]);
    match selection {
        "All" => json!({"Ok":[primary, audit]}),
        "Primary" => json!({"Ok":[primary]}),
        _ => json!({"Ok":[audit]}),
    }
}
fn calls(outcome: &Outcome) -> Vec<Json> {
    ["All", "Primary", AUDIT]
        .into_iter()
        .map(|selection| {
            let expected = match outcome {
                Outcome::Outputs => outputs(selection),
                Outcome::Failure(rule, message) => {
                    json!({"Err":{"kind":"MappingFailure","rule":rule,"message":message}})
                }
                Outcome::Error(recipe) => json!({ "Err": recipe }),
            };
            json!({"selection":selection,"expected":expected})
        })
        .collect()
}
fn case(
    id: &'static str,
    profile: &'static str,
    source: Instance,
    inputs: Vec<(String, Instance)>,
    outcome: Outcome,
) -> Case {
    Case {
        id,
        profile,
        calls: calls(&outcome),
        source,
        inputs,
    }
}

pub fn cases() -> Vec<Case> {
    let no_x = || lookup_rows(vec![row(false, "A", "N1"), row(true, "B", "N2")]);
    vec![
        case("F01", "F01-primary-when-true", primary(), inputs(), failed(1, "P2")),
        case("F01-none", "F01-primary-when-true", primary_false(), inputs(), Outcome::Outputs),
        case("F02", "F02-named-when-true", primary(), inputs(), failed(1, "N1")),
        case(
            "F02-none",
            "F02-named-when-true",
            primary(),
            with_lookup(lookup_rows(vec![row(false, "A", "N1")])),
            Outcome::Outputs,
        ),
        case("F03", "F03-primary-when-false", primary(), inputs(), failed(1, "P1")),
        case("F04", "F04-named-when-false", primary(), inputs(), failed(1, "N2")),
        case("F05", "F05-named-all-ordinal", primary(), inputs(), failed(1, "1:N1:P.note")),
        case(
            "F05-empty",
            "F05-named-all-ordinal",
            primary(),
            with_lookup(lookup_rows(vec![])),
            Outcome::Outputs,
        ),
        case(
            "F06",
            "F06-primary-all-absent-message",
            primary(),
            inputs(),
            Outcome::Failure(1, None),
        ),
        case(
            "F06-empty",
            "F06-primary-all-absent-message",
            endpoint("P.note", vec![], vec![]),
            inputs(),
            Outcome::Outputs,
        ),
        case("F07", "F07-legacy-row-reads", primary(), inputs(), failed(1, "N1")),
        case("F08-first", "F08-rule-order", primary(), inputs(), failed(1, "N2")),
        case("F08-second", "F08-rule-order", primary(), with_lookup(no_x()), failed(2, "P:P2")),
        case(
            "F08-legacy",
            "F08-rule-order",
            primary_false(),
            with_lookup(no_x()),
            failed(3, "legacy"),
        ),
        case(
            "F08-none",
            "F08-rule-order",
            endpoint("P.note", vec![], vec![]),
            with_lookup(no_x()),
            Outcome::Outputs,
        ),
        // N2's predicate is not Boolean, but N1 already selected the failure.
        case("F09-first", "F09-lazy-predicate-and-message", primary(), inputs(), failed(1, "N1")),
        case(
            "F09-not-bool",
            "F09-lazy-predicate-and-message",
            primary(),
            with_lookup(lookup_rows(vec![row(false, "A", "N1"), row(true, "B", "N2")])),
            not_bool(50, "string"),
        ),
        case(
            "F09-empty",
            "F09-lazy-predicate-and-message",
            primary(),
            with_lookup(lookup_rows(vec![])),
            Outcome::Outputs,
        ),
        // Absent selected collections are empty; Primary rows never repair them.
        case(
            "F10-absent-rows",
            "F10-absent-owner",
            primary(),
            with_lookup(group(vec![("Note", text("N.note"))])),
            failed(2, "P2"),
        ),
        case(
            "F10-empty-rows",
            "F10-absent-owner",
            primary(),
            with_lookup(lookup_rows(vec![])),
            failed(2, "P2"),
        ),
        case(
            "F10-unsupplied",
            "F10-absent-owner",
            primary(),
            vec![(BATCH.into(), batch())],
            unavailable(LOOKUP),
        ),
        case(
            "F10-none",
            "F10-absent-owner",
            primary_false(),
            with_lookup(group(vec![("Note", text("N.note"))])),
            Outcome::Outputs,
        ),
        case("F11", "F11-named-repeated-root", primary(), inputs(), failed(1, "B2:2")),
        case(
            "F11-unsupplied",
            "F11-named-repeated-root",
            primary(),
            vec![(LOOKUP.into(), lookup())],
            unavailable(BATCH),
        ),
        case(
            "F11-empty",
            "F11-named-repeated-root",
            primary(),
            vec![(LOOKUP.into(), lookup()), (BATCH.into(), Instance::Repeated(vec![]))],
            Outcome::Outputs,
        ),
        case(
            "F12",
            "F12-primary-root-identity",
            primary(),
            inputs(),
            failed(1, "P.note:P.note"),
        ),
        case("F13", "F13-nested-lines", primary(), inputs(), failed(1, "B:2:2:b2")),
        case(
            "F13-none",
            "F13-nested-lines",
            primary(),
            with_lookup(endpoint("N.note", vec![], vec![order("A", vec![line(false, "a1")])])),
            Outcome::Outputs,
        ),
        case("F14-owned", "F14-legacy-alias-beside-owned", primary(), inputs(), failed(1, "P2")),
        case(
            "F14-alias",
            "F14-legacy-alias-beside-owned",
            primary_false(),
            inputs(),
            failed(2, "N1"),
        ),
        case(
            "F15-primary-row",
            "F15-primary-row-rule-beside-owned",
            primary(),
            inputs(),
            failed(1, "P2"),
        ),
        case(
            "F15-owned",
            "F15-primary-row-rule-beside-owned",
            primary_false(),
            inputs(),
            failed(2, "N1"),
        ),
        case(
            "F16",
            "F16-cross-owner-root-pins",
            primary(),
            inputs(),
            failed(1, "N1:P.note:N.note:1"),
        ),
    ]
}
