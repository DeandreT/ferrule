//! Independently authored inputs and complete expected outcomes. Every case is
//! called through all outputs, the Primary target and the `audit` target.
use super::*;
use projects::{AUDIT, BATCH, REF};

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
fn line(sku: &str, qty: i64) -> Instance {
    group(vec![("Sku", text(sku)), ("Qty", int(qty))])
}
/// One row; `price` is omitted when absent.
fn row(code: &str, name: &str, rank: i64, price: Option<&str>, lines: Vec<Instance>, tags: &[&str]) -> Instance {
    let mut fields = vec![("Code", text(code)), ("Name", text(name)), ("Rank", int(rank))];
    if let Some(price) = price {
        fields.push(("Price", text(price)));
    }
    fields.push(("Lines", Instance::Repeated(lines)));
    fields.push(("Tags", Instance::Repeated(tags.iter().map(|tag| text(tag)).collect())));
    group(fields)
}
pub fn endpoint(note: &str, rows: Vec<Instance>) -> Instance {
    group(vec![("Note", text(note)), ("Rows", Instance::Repeated(rows))])
}
/// Primary repeats code A and carries one non-numeric price.
fn primary(note: &str) -> Instance {
    endpoint(
        note,
        vec![
            row("A", "P-A", 1, Some("1.5"), vec![line("a1", 1), line("a2", 5)], &["x", "y"]),
            row("B", "P-B", 2, Some("2"), vec![line("b1", 7)], &[]),
            row("A", "P-A2", 3, Some("oops"), vec![], &["z"]),
        ],
    )
}
/// Ref's second row has no price.
fn reference_rows() -> Vec<Instance> {
    vec![
        row("C", "R-C", 4, Some("10"), vec![line("c1", 3)], &["n1"]),
        row("D", "R-D", 5, None, vec![line("d1", 9), line("d2", 2)], &[]),
    ]
}
fn batch() -> Instance {
    let item = |code: &str, name: &str| group(vec![("Code", text(code)), ("Name", text(name))]);
    Instance::Repeated(vec![item("a", "B-a"), item("b", "B-b")])
}
fn inputs() -> Vec<(String, Instance)> {
    with_ref(endpoint("R.note", reference_rows()))
}
fn with_ref(reference: Instance) -> Vec<(String, Instance)> {
    vec![(REF.into(), reference), (BATCH.into(), batch())]
}
fn empty_ref() -> Vec<(String, Instance)> {
    with_ref(endpoint("R.note", vec![]))
}

/// One target's complete expected tree or typed error recipe.
type Target = Result<Instance, Json>;

fn null() -> Value {
    Value::Null
}
fn string(value: &str) -> Value {
    Value::String(value.into())
}
/// A target holding exactly these scalar fields, in target schema order.
fn scalars(fields: &[(&str, Value)]) -> Instance {
    group(
        fields
            .iter()
            .map(|(name, value)| (*name, Instance::Scalar(value.clone())))
            .collect(),
    )
}
fn output(fields: &[(&str, Value)]) -> Target {
    Ok(scalars(fields))
}
fn output_items(value: Value, items: &[(&str, &str)]) -> Target {
    Ok(group(vec![
        ("Value", Instance::Scalar(value)),
        (
            "Items",
            Instance::Repeated(
                items
                    .iter()
                    .map(|(code, value)| group(vec![("Code", text(code)), ("Value", text(value))]))
                    .collect(),
            ),
        ),
    ]))
}
/// Every reducer field of one empty collection, after `Value`.
fn empty_reducers(value: Value) -> Target {
    output(&[
        ("Value", value),
        ("Count", Value::Int(0)),
        ("Sum", Value::Int(0)),
        ("Avg", null()),
        ("Min", null()),
        ("Max", null()),
        ("Join", string("")),
        ("ItemAt", null()),
    ])
}
fn unavailable(owner: &str, node: u32) -> Json {
    json!({"kind":"OwnedSource","cause":"SourceUnavailable","owner":owner,"node":node})
}
fn raised(node: u32, message: &str) -> Json {
    json!({"kind":"MappingException","node":node,"message":message})
}
fn reducer_failure(kind: &str) -> Json {
    json!({"kind":kind,"function":"Sum"})
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
    let c01 = "C01-every-operator-root";
    let c02 = "C02-nested-and-scalar-collections";
    let c03 = "C03-computed-values-and-parent-arguments";
    let c07 = "C07-repeated-named-root";
    let c08 = "C08-evaluation-order-and-failures";
    let c09 = "C09-empty-and-value-policies";
    let raising = || {
        with_ref(endpoint(
            "R.note",
            vec![
                row("C", "R-C", 4, Some("10"), vec![], &[]),
                row("X", "R-X", 1, Some("1"), vec![], &[]),
            ],
        ))
    };
    let c08_audit = |value: i64, sum: i64| output(&[("Value", Value::Int(value)), ("Sum", Value::Int(sum))]);
    let policy = with_ref(group(vec![(
        "Rows",
        Instance::Repeated(vec![
            group(vec![("Code", text("n")), ("Rank", int(1)), ("Price", Instance::Scalar(Value::Null))]),
            group(vec![("Code", Instance::Scalar(Value::Null)), ("Rank", int(2)), ("Price", text(" 4 "))]),
            group(vec![("Code", text("m")), ("Rank", int(4)), ("Price", text("2.5"))]),
        ]),
    )]));
    let primary_avg = Value::Float(1.75);
    vec![
        // Every reducer reads only its endpoint; numeric strings parse.
        case(
            "C01",
            c01,
            primary("P.note"),
            inputs(),
            output(&[
                ("Value", Value::Float(3.5)),
                ("Count", Value::Int(3)),
                ("Sum", Value::Int(6)),
                ("Avg", Value::Float(2.0)),
                ("Min", Value::Float(1.5)),
                ("Max", Value::Int(2)),
                ("Join", string("P-A,P-B,P-A2")),
                ("ItemAt", string("P-B")),
            ]),
            output(&[
                ("Value", Value::Int(10)),
                ("Count", Value::Int(2)),
                ("Sum", Value::Int(9)),
                ("Avg", Value::Float(4.5)),
                ("Min", Value::Int(10)),
                ("Max", Value::Int(10)),
                ("Join", string("R-C,R-D")),
                ("ItemAt", string("R-D")),
            ]),
        ),
        // Primary rows with the same path never fill an empty Ref collection.
        case(
            "C01-empty-ref",
            c01,
            primary("P.note"),
            empty_ref(),
            output(&[
                ("Value", Value::Float(3.5)),
                ("Count", Value::Int(3)),
                ("Sum", Value::Int(6)),
                ("Avg", Value::Float(2.0)),
                ("Min", Value::Float(1.5)),
                ("Max", Value::Int(2)),
                ("Join", string("P-A,P-B,P-A2")),
                ("ItemAt", string("P-B")),
            ]),
            empty_reducers(Value::Int(0)),
        ),
        case(
            "C02",
            c02,
            primary("P.note"),
            inputs(),
            output(&[
                ("Value", Value::Int(3)),
                ("Count", Value::Int(3)),
                ("Sum", Value::Int(13)),
                ("Join", string("x+y+z")),
            ]),
            output(&[
                ("Value", Value::Int(1)),
                ("Count", Value::Int(3)),
                ("Sum", Value::Int(14)),
                ("Join", string("n1")),
            ]),
        ),
        case(
            "C02-absent-ref",
            c02,
            primary("P.note"),
            with_ref(group(vec![("Note", text("R.note"))])),
            output(&[
                ("Value", Value::Int(3)),
                ("Count", Value::Int(3)),
                ("Sum", Value::Int(13)),
                ("Join", string("x+y+z")),
            ]),
            output(&[
                ("Value", Value::Int(0)),
                ("Count", Value::Int(0)),
                ("Sum", Value::Int(0)),
                ("Join", string("")),
            ]),
        ),
        // The third Primary row's rank selects past the two Ref rows.
        case(
            "C03",
            c03,
            primary("P.note"),
            inputs(),
            output_items(
                Value::Int(60),
                &[
                    ("A", "R-C|CAD|CA1R-C,DA2R-D"),
                    ("B", "R-D|CBD|CB1R-C,DB2R-D"),
                    ("A", "|CAD|CA1R-C,DA2R-D"),
                ],
            ),
            output(&[("Value", Value::Int(90))]),
        ),
        case(
            "C03-empty-ref",
            c03,
            primary("P.note"),
            empty_ref(),
            output_items(Value::Int(60), &[("A", "||"), ("B", "||"), ("A", "||")]),
            output(&[("Value", Value::Int(0))]),
        ),
        // Group A holds P-A and P-A2.
        case(
            "C04",
            "C04-grouped-subset",
            primary("P.note"),
            inputs(),
            output_items(Value::Int(3), &[("A", "2/2/6/P-A+P-A2"), ("B", "1/2/7/P-B")]),
            output(&[("Value", Value::Int(3))]),
        ),
        case(
            "C05",
            "C05-row-anchored-walks",
            primary("P.note"),
            inputs(),
            output_items(
                Value::Int(13),
                &[("A", "1/2/6/P-A"), ("B", "1/2/7/P-B"), ("A", "1/2/0/P-A2")],
            ),
            output(&[("Value", Value::Int(14))]),
        ),
        // Blocks of two hold [P-A, P-B] and [P-A2].
        case(
            "C06",
            "C06-block-members",
            primary("P.note"),
            inputs(),
            output_items(string("P-B"), &[("A", "2/2/13/P-A+P-B"), ("A", "1/2/0/P-A2")]),
            output(&[("Value", string("R-D"))]),
        ),
        case(
            "C07",
            c07,
            primary("P.note"),
            inputs(),
            output(&[("Value", string("B-a,B-b")), ("Count", Value::Int(2))]),
            output(&[("Value", string("B-b"))]),
        ),
        case(
            "C07-unsupplied",
            c07,
            primary("P.note"),
            vec![(REF.into(), endpoint("R.note", reference_rows()))],
            Err(unavailable(BATCH, 90)),
            Err(unavailable(BATCH, 91)),
        ),
        case("C08", c08, primary("P.note"), inputs(), output(&[("Value", string("R-C,R-D"))]), c08_audit(9, 10)),
        case("C08-item-raise", c08, primary("P.note"), raising(), Err(raised(61, "R-X")), c08_audit(5, 11)),
        case("C08-arg-raise", c08, primary("bad"), inputs(), Err(raised(64, "bad")), c08_audit(9, 10)),
        case("C08-item-before-arg", c08, primary("bad"), raising(), Err(raised(61, "R-X")), c08_audit(5, 11)),
        // The argument still evaluates when no item does.
        case("C08-empty-arg-raise", c08, primary("bad"), empty_ref(), Err(raised(64, "bad")), c08_audit(0, 0)),
        // An unsupplied owner fails before any item value or argument.
        case(
            "C08-unsupplied",
            c08,
            primary("bad"),
            vec![(BATCH.into(), batch())],
            Err(unavailable(REF, 90)),
            Err(unavailable(REF, 91)),
        ),
        case(
            "C08-overflow",
            c08,
            primary("P.note"),
            with_ref(endpoint(
                "R.note",
                vec![row("C", "R-C", i64::MAX, None, vec![], &[]), row("D", "R-D", 1, None, vec![], &[])],
            )),
            output(&[("Value", string("R-C,R-D"))]),
            Err(reducer_failure("AggregateIntegerOverflow")),
        ),
        case(
            "C08-non-finite",
            c08,
            primary("P.note"),
            with_ref(endpoint(
                "R.note",
                vec![row("C", "R-C", 4, Some("1e999"), vec![], &[]), row("D", "R-D", 5, None, vec![], &[])],
            )),
            output(&[("Value", string("R-C,R-D"))]),
            Err(reducer_failure("AggregateNonFinite")),
        ),
        // Index 2.6 rounds to the third item, past Ref's two rows.
        case(
            "C09",
            c09,
            primary("P.note"),
            inputs(),
            output(&[
                ("Value", primary_avg.clone()),
                ("Count", Value::Int(2)),
                ("Sum", Value::Int(10)),
                ("Avg", Value::Float(4.5)),
                ("Min", Value::Int(10)),
                ("Max", Value::Int(10)),
                ("Join", string("C-D")),
                ("ItemAt", null()),
            ]),
            output(&[("Value", Value::Int(3))]),
        ),
        // Count counts items; Null values are skipped and padded numbers parse.
        case(
            "C09-policy",
            c09,
            primary("P.note"),
            policy,
            output(&[
                ("Value", primary_avg.clone()),
                ("Count", Value::Int(3)),
                ("Sum", Value::Float(6.5)),
                ("Avg", Value::Float(7.0 / 3.0)),
                ("Min", Value::Float(2.5)),
                ("Max", Value::Int(4)),
                ("Join", string("n-m")),
                ("ItemAt", string("m")),
            ]),
            output(&[("Value", Value::Int(3))]),
        ),
        case("C09-empty", c09, primary("P.note"), empty_ref(), empty_reducers(primary_avg.clone()), output(&[("Value", Value::Int(3))])),
        case(
            "C09-absent",
            c09,
            primary("P.note"),
            with_ref(group(vec![("Note", text("R.note"))])),
            empty_reducers(primary_avg.clone()),
            output(&[("Value", Value::Int(3))]),
        ),
        case(
            "C09-null-rows",
            c09,
            primary("P.note"),
            with_ref(group(vec![("Rows", Instance::Scalar(Value::Null))])),
            empty_reducers(primary_avg.clone()),
            output(&[("Value", Value::Int(3))]),
        ),
        case(
            "C09-unsupplied",
            c09,
            primary("P.note"),
            vec![(BATCH.into(), batch())],
            Err(unavailable(REF, 92)),
            output(&[("Value", Value::Int(3))]),
        ),
    ]
}
