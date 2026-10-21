//! Independently authored inputs and complete expected outcomes. Calls select
//! all outputs, the Primary target or the `audit` target; a call may supply
//! explicit generated limits, with the interpreter's own outcome beside it.
use super::*;
use projects::{AUDIT, LITERAL, LOWER, REF, SNOW, UNUSED, VALUE};
use serde_json::{Value as Json, json};

pub fn text(value: &str) -> Instance {
    Instance::Scalar(Value::String(value.into()))
}
fn int(value: i64) -> Instance {
    Instance::Scalar(Value::Int(value))
}
fn float(value: f64) -> Instance {
    Instance::Scalar(Value::Float(value))
}
fn null() -> Instance {
    Instance::Scalar(Value::Null)
}
fn group(fields: Vec<(&str, Instance)>) -> Instance {
    Instance::Group(
        fields
            .into_iter()
            .map(|(name, value)| (name.into(), value))
            .collect::<Vec<_>>()
            .into(),
    )
}
fn rep(items: Vec<Instance>) -> Instance {
    Instance::Repeated(items)
}
fn row(value: &str, items: Option<&[&str]>) -> Instance {
    let mut fields = vec![("Value", text(value))];
    if let Some(items) = items {
        fields.push((
            "Items",
            rep(items
                .iter()
                .map(|item| group(vec![("Value", text(item))]))
                .collect()),
        ));
    }
    group(fields)
}
/// One endpoint; `tag` prefixes its distinct values.
fn endpoint(
    tag: &str,
    count: Instance,
    ratio: Instance,
    flag: Instance,
    rows: Vec<Instance>,
) -> Instance {
    group(vec![
        ("Value", text(tag)),
        ("Count", count),
        ("Ratio", ratio),
        ("Flag", flag),
        ("Ref", group(vec![("Value", text(&format!("{tag}.ref")))])),
        ("Rows", rep(rows)),
    ])
}
fn primary_rows() -> Vec<Instance> {
    vec![
        row("r1", Some(&["i1", "i2"])),
        row("r2", Some(&[])),
        row("r3", None),
    ]
}
pub fn primary() -> Instance {
    endpoint(
        "P",
        int(7),
        float(3.0),
        Instance::Scalar(Value::Bool(true)),
        primary_rows(),
    )
}
/// Primary with `name` replaced, or removed when `value` is `None`.
fn primary_with(name: &str, value: Option<Instance>) -> Instance {
    let Instance::Group(fields) = primary() else {
        unreachable!("authored primary group")
    };
    group(
        fields
            .iter()
            .filter_map(|(field, current)| match field == name {
                true => value.clone().map(|value| (field.as_str(), value)),
                false => Some((field.as_str(), current.clone())),
            })
            .collect(),
    )
}
fn named(name: &str) -> Instance {
    endpoint(
        &format!("N.{name}"),
        int(9),
        float(3.0),
        Instance::Scalar(Value::Bool(false)),
        vec![row("n1", None), row("n2", Some(&["m1"]))],
    )
}
fn named_with(name: &str, field: &str, value: Option<Instance>) -> Instance {
    let Instance::Group(fields) = named(name) else {
        unreachable!("authored named group")
    };
    group(
        fields
            .iter()
            .filter_map(|(current, item)| match current == field {
                true => value.clone().map(|value| (current.as_str(), value)),
                false => Some((current.as_str(), item.clone())),
            })
            .collect(),
    )
}
fn inputs() -> Vec<(String, Instance)> {
    [REF, SNOW, LOWER, LITERAL, UNUSED]
        .into_iter()
        .map(|name| (name.to_owned(), named(name)))
        .collect()
}
fn without(name: &str) -> Vec<(String, Instance)> {
    inputs()
        .into_iter()
        .filter(|(input, _)| input != name)
        .collect()
}
fn replace(name: &str, value: Instance) -> Vec<(String, Instance)> {
    inputs()
        .into_iter()
        .map(|(input, tree)| match input == name {
            true => (input, value.clone()),
            false => (input, tree),
        })
        .collect()
}

fn tree(name: Option<&str>, value: &Instance) -> Json {
    json!([name, codec::encode(value)])
}
fn ok(trees: &[(Option<&str>, Instance)]) -> Json {
    json!({"Ok": trees.iter().map(|(name, value)| tree(*name, value)).collect::<Vec<_>>()})
}
fn err(recipe: Json) -> Json {
    json!({ "Err": recipe })
}
fn call(selection: &str, expected: Json) -> Json {
    json!({"selection": selection, "expected": expected})
}
/// A generated-only limit call; the interpreter has no owned-source limits.
fn limited(selection: &str, limits: [u64; 5], expected: Json, native: Json) -> Json {
    json!({"selection": selection, "limits": limits, "expected": expected, "native": native})
}
/// The all-output, Primary and audit calls of one input.
fn three(primary: Result<Instance, Json>, audit: Result<Instance, Json>) -> Vec<Json> {
    let all = match (&primary, &audit) {
        (Ok(primary), Ok(audit)) => ok(&[(None, primary.clone()), (Some(AUDIT), audit.clone())]),
        (Err(error), _) | (Ok(_), Err(error)) => err(error.clone()),
    };
    let single = |name: Option<&str>, result: Result<Instance, Json>| match result {
        Ok(value) => ok(&[(name, value)]),
        Err(error) => err(error),
    };
    vec![
        call("All", all),
        call("Primary", single(None, primary)),
        call(AUDIT, single(Some(AUDIT), audit)),
    ]
}
fn unavailable(owner: &str, node: u32) -> Json {
    json!({"kind": "SourceUnavailable", "owner": owner, "node": node})
}
fn limit(kind: &str, used: u64, requested: u64, maximum: u64) -> Json {
    err(
        json!({"kind": "MetadataLimit", "limit": kind, "used": used, "requested": requested, "maximum": maximum}),
    )
}
/// A limit charged while evaluating `node`, which each backend attaches.
fn limit_at(node: u32, kind: &str, used: u64, requested: u64, maximum: u64) -> Json {
    err(
        json!({"kind": "MetadataLimit", "node": node, "limit": kind, "used": used, "requested": requested, "maximum": maximum}),
    )
}
/// Walk contexts are owned metadata in Rust and a paired bridge budget in C#,
/// each with its own exact accounting.
fn walk_limit(rust: [u64; 3], csharp: [u64; 3]) -> Json {
    let counts = |[used, requested, maximum]: [u64; 3]| json!({"used": used, "requested": requested, "maximum": maximum});
    err(json!({"kind": "WalkLimit", "rust": counts(rust), "csharp": counts(csharp)}))
}
const GENEROUS: [u64; 5] = [16, 32, 200, 16_777_216, 512];

pub fn cases() -> Vec<Case> {
    let case = |id, profile, source, inputs, calls| Case {
        id,
        profile,
        source,
        inputs,
        calls,
    };
    let audit = || Ok(text("N.Ref"));
    let p_value = || Ok(text("P"));
    let mut cases = vec![
        case(
            "S01-all-inputs",
            "S01-primary-root",
            primary(),
            inputs(),
            three(p_value(), audit()),
        ),
        // An absent selected root fails only where it is read.
        case(
            "S01-absent-named-root",
            "S01-primary-root",
            primary(),
            without(REF),
            three(p_value(), Err(unavailable(REF, projects::AUDIT_VALUE))),
        ),
        case(
            "S01-absent-field",
            "S01-primary-root",
            primary_with("Value", None),
            inputs(),
            three(Ok(null()), audit()),
        ),
        // Explicit limits: the five-field policy is charged before evaluation.
        case(
            "S01-explicit-limits",
            "S01-primary-root",
            primary(),
            inputs(),
            vec![
                limited(
                    "Primary",
                    GENEROUS,
                    ok(&[(None, text("P"))]),
                    ok(&[(None, text("P"))]),
                ),
                limited(
                    "Primary",
                    [5, 32, 200, 16_777_216, 512],
                    limit("Roots", 0, 6, 5),
                    ok(&[(None, text("P"))]),
                ),
                // Declared names are charged in declaration order: 22 bytes precede `Unused`.
                limited(
                    "Primary",
                    [16, 32, 200, 27, 512],
                    limit("MetadataBytes", 22, 6, 27),
                    ok(&[(None, text("P"))]),
                ),
                limited(
                    "Primary",
                    [16, 32, 0, 16_777_216, 512],
                    limit_at(VALUE, "PathSegments", 0, 1, 0),
                    ok(&[(None, text("P"))]),
                ),
            ],
        ),
        case(
            "S02-collisions",
            "S02-named-collision",
            primary(),
            inputs(),
            three(Ok(text("N.Ref|P.ref|P.ref|N.Ref.ref")), audit()),
        ),
        // Without Primary's own `Ref`, only the legacy prefix reaches the named root.
        case(
            "S02-primary-ref-absent",
            "S02-named-collision",
            primary_with("Ref", None),
            inputs(),
            three(Ok(text("N.Ref|N.Ref||N.Ref.ref")), audit()),
        ),
        case(
            "S03-exact-names",
            "S03-literal-names",
            primary(),
            inputs(),
            three(Ok(text("N. Ref 雪 |N.ref|N.primary")), audit()),
        ),
        // `ref` never resolves to `Ref`.
        case(
            "S03-lowercase-absent",
            "S03-literal-names",
            primary(),
            without(LOWER),
            three(Err(unavailable(LOWER, 11)), audit()),
        ),
        case(
            "S04-unavailable",
            "S04-absent-owner",
            primary(),
            without(UNUSED),
            three(Err(unavailable(UNUSED, VALUE)), audit()),
        ),
        case(
            "S04-supplied",
            "S04-absent-owner",
            primary(),
            inputs(),
            three(Ok(text("N.Unused")), audit()),
        ),
        case(
            "S04-supplied-without-field",
            "S04-absent-owner",
            primary(),
            replace(UNUSED, named_with(UNUSED, "Value", None)),
            three(Ok(null()), audit()),
        ),
        case(
            "S05-then-skips-unavailable-else",
            "S05-lazy-branch",
            primary(),
            without(UNUSED),
            three(p_value(), audit()),
        ),
        case(
            "S05-else-reaches-unavailable",
            "S05-lazy-branch",
            primary_with("Flag", Some(Instance::Scalar(Value::Bool(false)))),
            without(UNUSED),
            three(Err(unavailable(UNUSED, 12)), audit()),
        ),
        case(
            "S05-else-supplied",
            "S05-lazy-branch",
            primary_with("Flag", Some(Instance::Scalar(Value::Bool(false)))),
            inputs(),
            three(Ok(text("N.Unused")), audit()),
        ),
        case(
            "S05-not-a-bool",
            "S05-lazy-branch",
            primary_with("Flag", Some(text("yes"))),
            without(UNUSED),
            three(
                Err(json!({"kind": "NotABool", "node": 10, "found": "string"})),
                audit(),
            ),
        ),
        case(
            "S06-first-argument-fails-first",
            "S06-argument-order",
            primary_with("Ref", Some(text("flat"))),
            without(UNUSED),
            three(Err(unavailable(UNUSED, 10)), audit()),
        ),
        case(
            "S06-second-argument-traversal",
            "S06-argument-order",
            primary_with("Ref", Some(text("flat"))),
            inputs(),
            three(
                Err(json!({"kind": "Traversal", "owner": "Primary", "node": 11,
                    "path": ["Ref", "Value"], "segment": 1, "found": "scalar"})),
                audit(),
            ),
        ),
        case(
            "S06-both-arguments",
            "S06-argument-order",
            primary(),
            inputs(),
            three(Ok(text("N.UnusedP.ref")), audit()),
        ),
        case(
            "S07-exact-adaptation",
            "S07-numeric-adaptation",
            primary(),
            inputs(),
            three(Ok(float(7.0)), Ok(int(3))),
        ),
        // Inexact values keep their runtime tag; absence stays Null.
        case(
            "S07-inexact-and-absent",
            "S07-numeric-adaptation",
            primary_with("Count", None),
            replace(REF, named_with(REF, "Ratio", Some(float(2.5)))),
            three(Ok(null()), Ok(float(2.5))),
        ),
        case(
            "S07-unrepresentable",
            "S07-numeric-adaptation",
            primary_with("Count", Some(int(9_007_199_254_740_993))),
            replace(REF, named_with(REF, "Ratio", Some(float(1e300)))),
            three(Ok(int(9_007_199_254_740_993)), Ok(float(1e300))),
        ),
    ];
    let nested = group(vec![
        (
            "Item",
            rep(vec![
                group(vec![
                    ("Label", text("r1#1")),
                    ("Entry", rep(vec![text("r1:i1:1.1"), text("r1:i2:1.2")])),
                    ("Plain", text("r1")),
                ]),
                group(vec![
                    ("Label", text("r2#2")),
                    ("Entry", rep(vec![])),
                    ("Plain", text("r2")),
                ]),
                group(vec![
                    ("Label", text("r3#3")),
                    ("Entry", rep(vec![])),
                    ("Plain", text("r3")),
                ]),
            ]),
        ),
        ("Named", rep(vec![text("n1@1/P"), text("n2@2/P")])),
        (
            "Legacy",
            rep(vec![text("r1~P"), text("r2~P"), text("r3~P")]),
        ),
    ]);
    let nested_ok = || ok(&[(None, nested.clone())]);
    cases.extend([
        case(
            "S08-nested",
            "S08-nested-repetitions",
            primary(),
            inputs(),
            three(Ok(nested.clone()), audit()),
        ),
        case(
            "S08-nested-limits",
            "S08-nested-repetitions",
            primary(),
            inputs(),
            vec![
                limited("Primary", GENEROUS, nested_ok(), nested_ok()),
                limited(
                    "Primary",
                    [16, 0, 200, 16_777_216, 512],
                    limit("Frames", 0, 1, 0),
                    nested_ok(),
                ),
                limited(
                    "Primary",
                    [16, 32, 200, 16_777_216, 1],
                    walk_limit([1, 1, 1], [0, 2, 1]),
                    nested_ok(),
                ),
            ],
        ),
        case(
            "S09-scalar-primary",
            "S09-scalar-primary",
            text("P.root"),
            Vec::new(),
            vec![
                call("All", ok(&[(None, text("P.root"))])),
                call("Primary", ok(&[(None, text("P.root"))])),
                // One root and no frame, segment, metadata byte or walk.
                limited(
                    "Primary",
                    [1, 0, 0, 0, 0],
                    ok(&[(None, text("P.root"))]),
                    ok(&[(None, text("P.root"))]),
                ),
                limited(
                    "Primary",
                    [0, 0, 0, 0, 0],
                    limit("Roots", 0, 1, 0),
                    ok(&[(None, text("P.root"))]),
                ),
            ],
        ),
        case(
            "S09-null-primary",
            "S09-scalar-primary",
            null(),
            Vec::new(),
            vec![
                call("All", ok(&[(None, null())])),
                call("Primary", ok(&[(None, null())])),
            ],
        ),
    ]);
    cases
}
