//! Independently authored inputs and complete expected outcomes. Every case is
//! called through all outputs, the Primary target and the `audit` target.
use super::*;
use projects::{AUDIT, LOOKUP};

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
fn rows(rows: &[(Option<i64>, &str)]) -> Instance {
    Instance::Repeated(
        rows.iter()
            .map(|(id, code)| {
                group(vec![
                    ("id", Instance::Scalar(id.map_or(Value::Null, Value::Int))),
                    ("code", text(code)),
                    ("name", text(&format!("name-{code}"))),
                ])
            })
            .collect(),
    )
}
fn endpoint(header: &str, rows: Instance) -> Instance {
    group(vec![("Header", group(vec![("code", text(header))])), ("Rows", rows)])
}
/// Primary ids 1,2,2,null,3 with codes a-e.
fn primary() -> Instance {
    endpoint(
        "H",
        rows(&[(Some(1), "a"), (Some(2), "b"), (Some(2), "c"), (None, "d"), (Some(3), "e")]),
    )
}
/// Lookup ids 2,1,2,null,4 with codes x,y,b,z,w and header `b`.
fn lookup() -> Instance {
    endpoint(
        "b",
        rows(&[(Some(2), "x"), (Some(1), "y"), (Some(2), "b"), (None, "z"), (Some(4), "w")]),
    )
}
fn inputs(lookup: Instance) -> Vec<(String, Instance)> {
    vec![(LOOKUP.into(), lookup)]
}
/// One order: its id, code and `(order, sku)` lines.
type Order<'a> = (i64, &'a str, &'a [(i64, &'a str)]);

fn orders(orders: &[Order<'_>]) -> Instance {
    Instance::Repeated(
        orders
            .iter()
            .map(|(id, code, lines)| {
                group(vec![
                    ("id", int(*id)),
                    ("code", text(code)),
                    (
                        "Lines",
                        Instance::Repeated(
                            lines
                                .iter()
                                .map(|(order, sku)| {
                                    group(vec![("order", int(*order)), ("sku", text(sku))])
                                })
                                .collect(),
                        ),
                    ),
                ])
            })
            .collect(),
    )
}

/// One target's complete expected tree or typed error recipe.
type Target = Result<Instance, Json>;
type Tuple<'a> = (&'a str, &'a str, i64, i64, i64);

fn tuple(row: &Tuple<'_>) -> Instance {
    let (left, right, left_at, right_at, position) = *row;
    group(vec![
        ("left", text(left)),
        ("right", text(right)),
        ("left_at", int(left_at)),
        ("right_at", int(right_at)),
        ("tuple", int(position)),
    ])
}
fn tuples(rows: &[Tuple<'_>]) -> Instance {
    Instance::Repeated(rows.iter().map(tuple).collect())
}
fn output(rows: &[Tuple<'_>]) -> Target {
    Ok(group(vec![("Row", tuples(rows))]))
}
fn audit(rows: &[Tuple<'_>]) -> Target {
    Ok(group(vec![("Row", tuples(rows))]))
}
/// `Row` items holding a tuple position and a static `Pair` descendant.
fn pairs(rows: &[(i64, &str, &str)]) -> Instance {
    group(vec![(
        "Row",
        Instance::Repeated(
            rows.iter()
                .map(|(position, left, right)| {
                    group(vec![
                        ("tuple", int(*position)),
                        ("Pair", group(vec![("left", text(left)), ("right", text(right))])),
                    ])
                })
                .collect(),
        ),
    )])
}
fn unavailable(owner: &str) -> Json {
    json!({"kind":"OwnedSource","cause":"SourceUnavailable","owner":owner})
}
fn not_bool(node: u32) -> Json {
    json!({"kind":"NotABool","node":node,"found":"string"})
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
    let j01 = "J01-same-path-rows";
    let j02 = "J02-composite-and-singleton-stages";
    let j03 = "J03-controls-and-positions";
    let j04 = "J04-correlated-active-orders";
    let j05 = "J05-first-and-scalar-output";
    let j06 = "J06-static-descendants-and-windows";
    let j07 = "J07-filter-policy";
    // Primary-first and Lookup-first tuples over the base rows.
    let forward: &[Tuple<'_>] = &[
        ("a", "y", 1, 2, 1),
        ("b", "x", 2, 1, 2),
        ("b", "b", 2, 3, 3),
        ("c", "x", 3, 1, 4),
        ("c", "b", 3, 3, 5),
    ];
    let reverse: &[Tuple<'_>] = &[
        ("b", "x", 2, 1, 1),
        ("c", "x", 3, 1, 2),
        ("a", "y", 1, 2, 3),
        ("b", "b", 2, 3, 4),
        ("c", "b", 3, 3, 5),
    ];
    let order_lines = group(vec![(
        "Orders",
        orders(&[
            (7, "p7", &[(7, "a"), (9, "b"), (7, "b")]),
            (9, "p9", &[(9, "a"), (9, "c")]),
        ]),
    )]);
    let order_lookup = |codes: &[&str]| {
        let rows = codes
            .iter()
            .enumerate()
            .map(|(index, code)| (Some(index as i64 + 1), *code))
            .collect::<Vec<_>>();
        group(vec![
            ("Header", group(vec![("code", text("H"))])),
            ("Rows", self::rows(&rows)),
            (
                "Orders",
                orders(&[(5, "o5", &[(5, "q"), (6, "r")]), (6, "o6", &[(6, "s")])]),
            ),
        ])
    };
    let order = |id: i64, rows: &[Tuple<'_>]| group(vec![("id", int(id)), ("Row", tuples(rows))]);
    vec![
        // Same-path sources: each owner supplies only its own rows; null ids never match.
        case("J01-rows", j01, primary(), inputs(lookup()), output(forward), audit(reverse)),
        case(
            "J01-empty-lookup",
            j01,
            primary(),
            inputs(endpoint("b", Instance::Repeated(vec![]))),
            output(&[]),
            audit(&[]),
        ),
        // An absent Lookup collection is empty; Primary rows never stand in.
        case(
            "J01-absent-lookup-rows",
            j01,
            primary(),
            inputs(group(vec![("Header", group(vec![("code", text("b"))]))])),
            output(&[]),
            audit(&[]),
        ),
        case(
            "J01-empty-primary",
            j01,
            endpoint("H", Instance::Repeated(vec![])),
            inputs(lookup()),
            output(&[]),
            audit(&[]),
        ),
        case(
            "J01-unavailable",
            j01,
            primary(),
            vec![],
            Err(unavailable(LOOKUP)),
            Err(unavailable(LOOKUP)),
        ),
        // Only Primary b/Lookup b shares id and code, and the Lookup header is `b`.
        case(
            "J02-composite",
            j02,
            primary(),
            inputs(lookup()),
            output(&[("b", "b", 2, 3, 1)]),
            audit(&[]),
        ),
        // A header that matches no Primary code removes the composite tuple;
        // the Primary header joins every Lookup row coded `H`.
        case(
            "J02-singletons",
            j02,
            primary(),
            inputs(endpoint(
                "a",
                rows(&[(Some(2), "b"), (Some(9), "H"), (Some(2), "c"), (None, "H")]),
            )),
            output(&[]),
            audit(&[("H", "H", 1, 2, 1), ("H", "H", 1, 4, 2)]),
        ),
        // Sorted by descending Lookup position, sorted tuple 2 is dropped and
        // the first three survivors compact; audit drops original tuple 2.
        case(
            "J03-controls",
            j03,
            primary(),
            inputs(lookup()),
            output(&[("b", "b", 2, 3, 1), ("a", "y", 1, 2, 2), ("b", "x", 2, 1, 3)]),
            audit(&[
                ("a", "y", 1, 2, 1),
                ("b", "x", 2, 1, 2),
                ("b", "b", 2, 3, 3),
                ("c", "b", 3, 3, 4),
            ]),
        ),
        case(
            "J03-single",
            j03,
            endpoint("H", rows(&[(Some(1), "a")])),
            inputs(endpoint("b", rows(&[(Some(1), "y")]))),
            output(&[("a", "y", 1, 1, 1)]),
            audit(&[("a", "y", 1, 1, 1)]),
        ),
        // Order 7 keeps lines 1 and 3 by key, order 9 line 1 by Lookup code;
        // audit lines keep their raw positions within each Lookup order.
        case(
            "J04-orders",
            j04,
            order_lines.clone(),
            inputs(order_lookup(&["a", "b"])),
            Ok(group(vec![(
                "Order",
                Instance::Repeated(vec![
                    order(7, &[("a", "a", 1, 1, 1), ("b", "b", 1, 3, 2)]),
                    order(9, &[("a", "a", 2, 1, 1)]),
                ]),
            )])),
            audit(&[("q", "o5", 1, 1, 1), ("r", "o6", 2, 2, 2), ("s", "o6", 1, 2, 3)]),
        ),
        case(
            "J04-missing-lookup-code",
            j04,
            order_lines,
            inputs(order_lookup(&["c", "x"])),
            Ok(group(vec![(
                "Order",
                Instance::Repeated(vec![order(7, &[]), order(9, &[("c", "c", 2, 2, 1)])]),
            )])),
            audit(&[("q", "o5", 1, 1, 1), ("r", "o6", 2, 2, 2), ("s", "o6", 1, 2, 3)]),
        ),
        case(
            "J05-best-and-codes",
            j05,
            primary(),
            inputs(lookup()),
            Ok(group(vec![
                ("Best", tuple(&("b", "b", 2, 3, 1))),
                (
                    "Codes",
                    Instance::Repeated(
                        ["a-y", "b-x", "b-b", "c-x", "c-b"]
                            .into_iter()
                            .map(text)
                            .collect(),
                    ),
                ),
            ])),
            Ok(group(vec![("Value", text("audit"))])),
        ),
        case(
            "J05-no-tuples",
            j05,
            primary(),
            inputs(endpoint("b", Instance::Repeated(vec![]))),
            Ok(group(vec![
                ("Best", group(vec![])),
                ("Codes", Instance::Repeated(vec![])),
            ])),
            Ok(group(vec![("Value", text("audit"))])),
        ),
        case(
            "J06-pairs",
            j06,
            primary(),
            inputs(lookup()),
            Ok(pairs(&[(1, "c", "x"), (2, "c", "b")])),
            Ok(pairs(&[(1, "c", "x"), (2, "a", "y")])),
        ),
        case(
            "J07-filters",
            j07,
            primary(),
            inputs(lookup()),
            output(&[("b", "x", 2, 1, 1), ("b", "b", 2, 3, 2)]),
            Err(not_bool(52)),
        ),
        case(
            "J07-single-audit-tuple",
            j07,
            primary(),
            inputs(endpoint("b", rows(&[(Some(1), "y")]))),
            output(&[]),
            audit(&[("a", "y", 1, 1, 1)]),
        ),
    ]
}
