use super::*;
use mapping::{
    FailureIteration, FailureRule, FailureSelection, OwnedPosition, OwnedSourceField,
    OwnedSourceRef, SourceOwner,
};

fn path(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|part| (*part).into()).collect()
}
fn named() -> SourceOwner {
    SourceOwner::Named { name: "Ref".into() }
}
fn field(owner: SourceOwner, parts: &[&str], frame: Option<&[&str]>) -> Node {
    Node::OwnedSourceField(OwnedSourceField {
        source: OwnedSourceRef {
            owner,
            path: path(parts),
        },
        frame: frame.map(path),
    })
}
fn constant(value: Value) -> Node {
    Node::Const { value }
}
fn rule(selection: FailureSelection, message: Option<u32>) -> FailureRule {
    FailureRule {
        iteration: FailureIteration::Source {
            collection: path(&["Rows"]),
        },
        selection,
        message,
    }
}
fn scope(node: u32) -> Scope {
    Scope {
        bindings: vec![Binding {
            target_field: "Value".into(),
            node,
        }],
        ..Scope::default()
    }
}
pub(super) fn projects() -> Vec<Project> {
    let nodes = BTreeMap::from([
        (1, field(SourceOwner::Primary, &["Rows", "Reject"], None)),
        (2, field(SourceOwner::Primary, &["Rows", "Code"], None)),
        (
            3,
            field(SourceOwner::Primary, &["Rows", "Code"], Some(&["Rows"])),
        ),
        (
            4,
            Node::OwnedPosition(OwnedPosition {
                owner: SourceOwner::Primary,
                collection: None,
            }),
        ),
        (
            5,
            Node::OwnedPosition(OwnedPosition {
                owner: SourceOwner::Primary,
                collection: Some(path(&["Rows"])),
            }),
        ),
        (6, field(SourceOwner::Primary, &["Code"], None)),
        (7, field(named(), &["Code"], None)),
        (8, field(named(), &["Code"], Some(&[]))),
        (9, constant(Value::String("|".into()))),
        (
            13,
            field(SourceOwner::Primary, &["Rows", "PoisonPredicate"], None),
        ),
        (
            14,
            Node::If {
                condition: 13,
                then: 30,
                else_: 1,
            },
        ),
        (
            15,
            field(SourceOwner::Primary, &["Rows", "PoisonMessage"], None),
        ),
        (
            16,
            Node::If {
                condition: 15,
                then: 31,
                else_: 18,
            },
        ),
        (
            17,
            field(SourceOwner::Primary, &["Rows", "Valid"], Some(&["Rows"])),
        ),
        (
            18,
            Node::Call {
                function: "concat".into(),
                args: vec![2, 9, 3, 9, 4, 9, 5, 9, 6, 9, 7, 9, 8],
            },
        ),
        (28, constant(Value::String("message.poison".into()))),
        (29, constant(Value::String("predicate.poison".into()))),
        (30, Node::Raise { message: Some(29) }),
        (31, Node::Raise { message: Some(28) }),
        (38, field(SourceOwner::Primary, &["PrimaryPoison"], None)),
        (39, field(SourceOwner::Primary, &["NamedPoison"], None)),
        (
            40,
            Node::If {
                condition: 38,
                then: 42,
                else_: 43,
            },
        ),
        (
            41,
            Node::If {
                condition: 39,
                then: 44,
                else_: 45,
            },
        ),
        (42, Node::Raise { message: None }),
        (43, constant(Value::String("primary.ok 雪".into()))),
        (44, Node::Raise { message: None }),
        (45, constant(Value::String("named.ok 🙂".into()))),
    ]);
    let target = SchemaNode::group("Output", vec![string("Value")]);
    let primary = Project {
        source: SchemaNode::group(
            "Input",
            vec![
                string("Code"),
                bool_("PrimaryPoison"),
                bool_("NamedPoison"),
                SchemaNode::group("Ref", vec![string("Code")]),
                SchemaNode::group(
                    "Rows",
                    vec![
                        string("Code"),
                        bool_("Reject"),
                        bool_("Valid"),
                        bool_("PoisonPredicate"),
                        bool_("PoisonMessage"),
                    ],
                )
                .repeating(),
            ],
        ),
        target: target.clone(),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: ["Ref", "Rows"]
            .into_iter()
            .map(|name| mapping::NamedSource {
                name: name.into(),
                path: format!("unused/{name}.json"),
                schema: SchemaNode::group("Reference", vec![string("Code"), bool_("Reject")]),
                options: Default::default(),
                dynamic_path: None,
            })
            .collect(),
        extra_targets: vec![mapping::NamedTarget {
            name: "audit".into(),
            path: None,
            schema: target,
            options: Default::default(),
            root: scope(41),
        }],
        failure_rules: vec![
            rule(FailureSelection::WhenTrue { predicate: 14 }, Some(16)),
            rule(FailureSelection::WhenFalse { predicate: 17 }, Some(3)),
        ],
        user_functions: Default::default(),
        graph: Graph { nodes },
        root: scope(40),
    };
    let mut position = primary.clone();
    position.failure_rules = vec![rule(FailureSelection::All, Some(5))];
    let mut no_message = primary.clone();
    no_message.failure_rules = vec![rule(FailureSelection::All, None)];
    // This rule has no owned expression and keeps the ordinary eager-alias
    // contract. It does not exercise the independently declared Rows collision.
    no_message
        .extra_sources
        .retain(|source| source.name != "Rows");
    vec![primary, position, no_message]
}

fn text(value: &str) -> Instance {
    Instance::Scalar(Value::String(value.into()))
}
fn boolean(value: bool) -> Instance {
    Instance::Scalar(Value::Bool(value))
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
fn row(code: &str, reject: bool, valid: bool) -> Instance {
    group(vec![
        ("Code", text(code)),
        ("Reject", boolean(reject)),
        ("Valid", boolean(valid)),
        ("PoisonPredicate", boolean(false)),
        ("PoisonMessage", boolean(false)),
    ])
}
fn replace(mut instance: Instance, name: &str, value: Instance) -> Instance {
    let Instance::Group(fields) = &mut instance else {
        panic!("fixture group")
    };
    fields.iter_mut().find(|(key, _)| key == name).unwrap().1 = value;
    instance
}
fn source(rows: Option<Instance>, primary_poison: bool, named_poison: bool) -> Instance {
    let mut fields = vec![
        ("Code", text("P.root")),
        ("PrimaryPoison", boolean(primary_poison)),
        ("NamedPoison", boolean(named_poison)),
        ("Ref", group(vec![("Code", text("P.Ref.shadow"))])),
    ];
    if let Some(rows) = rows {
        fields.push(("Rows", rows));
    }
    group(fields)
}
fn rows(items: Vec<Instance>) -> Instance {
    Instance::Repeated(items)
}
fn inputs(reference: Option<Instance>, collision: bool) -> Vec<(String, Instance)> {
    let mut supplied = Vec::new();
    if let Some(reference) = reference {
        supplied.push(("Ref".into(), reference));
    }
    if collision {
        supplied.push((
            "Rows".into(),
            group(vec![
                ("Code", text("foreign.Rows.root")),
                ("Reject", boolean(true)),
            ]),
        ));
    }
    supplied
}
fn reference() -> Instance {
    group(vec![("Code", text(" N.雪 ")), ("Reject", boolean(false))])
}
fn failure(rule: usize, message: Option<&str>) -> Json {
    json!({"kind":"MappingFailure","rule":rule,"message":message})
}
fn raise(node: u32, message: Option<&str>) -> Json {
    json!({"kind":"MappingException","node":node,"message":message})
}
fn not_bool(found: &str) -> Json {
    json!({"kind":"NotABool","node":14,"found":found})
}
fn access(
    node: u32,
    owner: &str,
    cause: &str,
    full: &[&str],
    suffix: &[&str],
    segment: Option<usize>,
    found: Option<&str>,
) -> Json {
    json!({"kind":"OwnedSource","node":node,"owner":owner,"cause":cause,"path":full,"native_path":suffix,"segment":segment,"found":found})
}
fn okay(selection: &str) -> Json {
    let primary = group(vec![("Value", text("primary.ok 雪"))]);
    let audit = group(vec![("Value", text("named.ok 🙂"))]);
    let outputs = match selection {
        "All" => vec![(None::<String>, primary), (Some("audit".into()), audit)],
        "Primary" => vec![(None, primary)],
        "audit" => vec![(Some("audit".into()), audit)],
        _ => panic!("frozen selection"),
    };
    json!({"Ok":outputs})
}
fn case(
    id: &str,
    model: usize,
    source: Instance,
    inputs: Vec<(String, Instance)>,
    error: Option<Json>,
) -> Json {
    let calls = ["All","Primary","audit"].into_iter().map(|selection| json!({
        "selection":selection,"expected":error.as_ref().map_or_else(||okay(selection),|error|json!({"Err":error}))
    })).collect::<Vec<_>>();
    json!({"id":id,"model":model,"source":source,"inputs":inputs,"calls":calls})
}
pub(super) fn controls() -> Json {
    let selected = || {
        rows(vec![
            row("unread", false, true),
            row("B🙂", true, true),
            row("later", true, false),
        ])
    };
    let one = || rows(vec![row("A", true, true)]);
    let safe = || rows(vec![row("unused", false, true)]);
    let rich = "B🙂|B🙂|2|2|P.root| N.雪 | N.雪 ";
    let mut cases = vec![
        case(
            "selected-rich-owner-pins-and-positions",
            0,
            source(Some(selected()), false, false),
            inputs(Some(reference()), true),
            Some(failure(1, Some(rich))),
        ),
        case(
            "first-selected-row-before-later-malformed",
            0,
            source(
                Some(rows(vec![row("A", true, true), text("never-read")])),
                false,
                false,
            ),
            inputs(Some(reference()), true),
            Some(failure(1, Some("A|A|1|1|P.root| N.雪 | N.雪 "))),
        ),
        case(
            "stored-rule-before-earlier-item-of-rule-two",
            0,
            source(
                Some(rows(vec![
                    row("invalid-first", false, false),
                    row("skip", false, true),
                    row("third", true, true),
                ])),
                false,
                false,
            ),
            inputs(Some(reference()), true),
            Some(failure(1, Some("third|third|3|3|P.root| N.雪 | N.雪 "))),
        ),
        case(
            "when-false-second-rule-lazy-first-message",
            0,
            source(
                Some(rows(vec![
                    row("first", false, true),
                    row("second", false, false),
                ])),
                true,
                true,
            ),
            vec![],
            Some(failure(2, Some("second"))),
        ),
        case(
            "success-skips-malformed-unselected-message-and-missing-ref",
            0,
            source(
                Some(rows(vec![replace(
                    row("unused", false, true),
                    "Code",
                    group(vec![]),
                )])),
                false,
                false,
            ),
            vec![],
            None,
        ),
        case(
            "success-empty-skips-predicates-and-messages",
            0,
            source(Some(rows(vec![])), false, false),
            vec![],
            None,
        ),
        case(
            "success-absent-collection",
            0,
            source(None, false, false),
            vec![],
            None,
        ),
        case(
            "present-null-terminal-reaches-predicate-condition",
            0,
            source(Some(Instance::Scalar(Value::Null)), false, false),
            vec![],
            // Source failures use the observed legacy walk (strict=false):
            // a present Null terminal is a singleton, not an absent collection.
            // Its owned field is Null, so If(14) rejects condition node 13.
            Some(json!({"kind":"NotABool","node":13,"found":"null"})),
        ),
        case(
            "predicate-original-raise-before-target",
            0,
            source(
                Some(rows(vec![
                    row("skip", false, true),
                    replace(row("P", true, true), "PoisonPredicate", boolean(true)),
                ])),
                true,
                true,
            ),
            vec![],
            Some(raise(30, Some("predicate.poison"))),
        ),
        case(
            "selected-message-original-raise-before-target",
            0,
            source(
                Some(rows(vec![replace(
                    row("M", true, true),
                    "PoisonMessage",
                    boolean(true),
                )])),
                true,
                true,
            ),
            vec![],
            Some(raise(31, Some("message.poison"))),
        ),
        case(
            "predicate-null-is-typed-not-bool",
            0,
            source(
                Some(rows(vec![replace(
                    row("N", true, true),
                    "Reject",
                    Instance::Scalar(Value::Null),
                )])),
                true,
                true,
            ),
            vec![],
            Some(not_bool("null")),
        ),
        case(
            "predicate-string-is-typed-not-bool",
            0,
            source(
                Some(rows(vec![replace(
                    row("S", true, true),
                    "Reject",
                    text("not-bool"),
                )])),
                true,
                true,
            ),
            vec![],
            Some(not_bool("string")),
        ),
        case(
            "malformed-row-keeps-owned-traversal-cause",
            0,
            source(Some(rows(vec![text("bad-row")])), true, true),
            vec![],
            Some(access(
                13,
                "Primary",
                "CannotTraverse",
                &["Rows", "PoisonPredicate"],
                &["PoisonPredicate"],
                Some(1),
                Some("Scalar"),
            )),
        ),
        case(
            "structural-predicate-keeps-owned-terminal-cause",
            0,
            source(
                Some(rows(vec![replace(
                    row("G", true, true),
                    "PoisonPredicate",
                    group(vec![]),
                )])),
                true,
                true,
            ),
            vec![],
            Some(access(
                13,
                "Primary",
                "ExpectedScalar",
                &["Rows", "PoisonPredicate"],
                &["PoisonPredicate"],
                None,
                Some("Group"),
            )),
        ),
        case(
            "repeated-predicate-does-not-take-first",
            0,
            source(
                Some(rows(vec![replace(
                    row("R", true, true),
                    "Reject",
                    rows(vec![boolean(true), boolean(false)]),
                )])),
                true,
                true,
            ),
            vec![],
            Some(access(
                1,
                "Primary",
                "UniteratedRepetition",
                &["Rows", "Reject"],
                &["Reject"],
                Some(2),
                None,
            )),
        ),
        case(
            "selected-missing-named-root-is-not-primary-shadow",
            0,
            source(Some(one()), true, true),
            vec![],
            Some(access(7, "Ref", "SourceUnavailable", &[], &[], None, None)),
        ),
        case(
            "selected-named-scalar-keeps-full-cause",
            0,
            source(Some(one()), true, true),
            inputs(Some(text("bad-ref")), false),
            Some(access(
                7,
                "Ref",
                "CannotTraverse",
                &["Code"],
                &["Code"],
                Some(0),
                Some("Scalar"),
            )),
        ),
        case(
            "selected-named-structural-leaf-keeps-full-cause",
            0,
            source(Some(one()), true, true),
            inputs(Some(group(vec![("Code", group(vec![]))])), false),
            Some(access(
                7,
                "Ref",
                "ExpectedScalar",
                &["Code"],
                &["Code"],
                None,
                Some("Group"),
            )),
        ),
        case(
            "selected-repeated-named-root-is-not-first-item",
            0,
            source(Some(one()), true, true),
            inputs(Some(rows(vec![reference(), reference()])), false),
            Some(access(
                7,
                "Ref",
                "UniteratedRepetition",
                &["Code"],
                &["Code"],
                Some(0),
                None,
            )),
        ),
        case(
            "selected-row-message-keeps-original-owned-cause",
            0,
            source(
                Some(rows(vec![replace(
                    row("G", true, true),
                    "Code",
                    group(vec![]),
                )])),
                true,
                true,
            ),
            inputs(Some(reference()), false),
            Some(access(
                2,
                "Primary",
                "ExpectedScalar",
                &["Rows", "Code"],
                &["Code"],
                None,
                Some("Group"),
            )),
        ),
        case(
            "primary-target-poison-only-after-rules",
            0,
            source(Some(safe()), true, false),
            vec![],
            Some(raise(42, None)),
        ),
        case(
            "named-target-poison-only-after-rules",
            0,
            source(Some(safe()), false, true),
            vec![],
            Some(raise(44, None)),
        ),
        case(
            "success-does-not-read-malformed-named-root",
            0,
            source(Some(safe()), false, false),
            inputs(Some(text("unread-ref")), false),
            None,
        ),
        case(
            "mapping-failure-precedes-both-target-poisons",
            0,
            source(Some(selected()), true, true),
            inputs(Some(reference()), true),
            Some(failure(1, Some(rich))),
        ),
        case(
            "all-selection-position-counts-null-structural-row",
            1,
            source(
                Some(rows(vec![
                    Instance::Scalar(Value::Null),
                    row("later", true, true),
                ])),
                true,
                true,
            ),
            vec![],
            Some(failure(1, Some("1"))),
        ),
        case(
            "all-position-empty-is-lazy",
            1,
            source(Some(rows(vec![])), false, false),
            vec![],
            None,
        ),
        case(
            "all-position-absent-is-lazy",
            1,
            source(None, false, false),
            vec![],
            None,
        ),
        case(
            "present-null-terminal-has-no-structural-position",
            1,
            source(Some(Instance::Scalar(Value::Null)), false, false),
            vec![],
            // The same singleton is selected by All, but only a Repeated
            // traversal creates the Rows position frame required by node 5.
            Some(json!({"kind":"InactivePosition","node":5,"collection":["Rows"]})),
        ),
        case(
            "singleton-does-not-invent-structural-position",
            1,
            source(Some(row("singleton", true, true)), true, true),
            vec![],
            Some(json!({"kind":"InactivePosition","node":5,"collection":["Rows"]})),
        ),
        case(
            "all-no-message-does-not-evaluate-any-row-leaf",
            2,
            source(
                Some(rows(vec![text("not-read"), row("later", true, true)])),
                true,
                true,
            ),
            vec![],
            Some(failure(1, None)),
        ),
        case(
            "all-no-message-empty-success",
            2,
            source(Some(rows(vec![])), false, false),
            vec![],
            None,
        ),
    ];
    // These success alternatives are independently literal selected-target
    // controls, not outputs copied from a native or generated execution.
    cases[20]["calls"][2]["expected"] = okay("audit");
    cases[21]["calls"][1]["expected"] = okay("Primary");
    json!({"version":1,"cases":cases})
}
