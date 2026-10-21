//! Independently authored inputs and complete expected outcomes. Every case is
//! called through all outputs, the Primary target and the `audit` target.
use super::*;
use projects::{
    AUDIT, AUDIT_NODE, FILTER_READ, FIRST_NODE, LOOKUP, NEEDLE_READ, RAISE, SETTINGS, TREE,
};

pub fn text(value: &str) -> Instance {
    Instance::Scalar(Value::String(value.into()))
}
fn null() -> Instance {
    Instance::Scalar(Value::Null)
}
fn json_null() -> Instance {
    Instance::Scalar(Value::json_null())
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
fn row(id: &str, pick: &str, props: Vec<(&str, Instance)>) -> Instance {
    group(vec![
        ("Id", text(id)),
        ("Pick", text(pick)),
        ("Props", group(props)),
    ])
}
fn endpoint(
    key: Option<&str>,
    flag: bool,
    props: Option<Vec<(&str, Instance)>>,
    rows: Vec<Instance>,
) -> Instance {
    let mut fields = Vec::new();
    if let Some(key) = key {
        fields.push(("Key", text(key)));
    }
    fields.push(("Flag", Instance::Scalar(Value::Bool(flag))));
    if let Some(props) = props {
        fields.push(("Props", group(props)));
    }
    fields.push(("Rows", Instance::Repeated(rows)));
    group(fields)
}
fn primary_props() -> Vec<(&'static str, Instance)> {
    vec![
        ("Id", text("P.id")),
        ("color", text("P.red")),
        ("blank", json_null()),
        ("nested", group(vec![("x", text("structural"))])),
    ]
}
fn primary_rows() -> Vec<Instance> {
    vec![
        row(
            "P1",
            "size",
            vec![("color", text("P.row1")), ("size", text("P.s1"))],
        ),
        row("P2", "size", vec![("size", text("P.s2"))]),
        row("P3", "color", vec![("color", text("P.row3"))]),
    ]
}
fn primary_with(key: &str, flag: bool, rows: Vec<Instance>) -> Instance {
    endpoint(Some(key), flag, Some(primary_props()), rows)
}
/// Primary: Key `color`, no `size` property and three rows.
pub fn primary() -> Instance {
    primary_with("color", false, primary_rows())
}
fn lookup_props() -> Vec<(&'static str, Instance)> {
    vec![("Id", text("N.id")), ("size", text("N.small"))]
}
fn lookup_rows() -> Vec<Instance> {
    vec![row("N1", "color", vec![("color", text("N.row1"))])]
}
/// Lookup: Key `size`, no `color` property and one row.
pub fn lookup() -> Instance {
    endpoint(Some("size"), true, Some(lookup_props()), lookup_rows())
}
fn settings() -> Instance {
    group(vec![
        ("mode", text("S.fast")),
        ("color", text("S.color")),
        ("pick", text("color")),
    ])
}
/// One directory: its name, leaf file names and child directories.
fn dir(name: &str, files: &[&str], children: Vec<Instance>) -> Instance {
    group(vec![
        ("name", text(name)),
        (
            "file",
            Instance::Repeated(
                files
                    .iter()
                    .map(|file| group(vec![("name", text(file))]))
                    .collect(),
            ),
        ),
        ("Dir", Instance::Repeated(children)),
    ])
}
fn tree() -> Instance {
    group(vec![(
        "Dir",
        Instance::Repeated(vec![dir("a", &["x", "y"], vec![dir("b", &["z"], vec![])])]),
    )])
}
fn inputs() -> Vec<(String, Instance)> {
    vec![
        (LOOKUP.into(), lookup()),
        (SETTINGS.into(), settings()),
        (TREE.into(), tree()),
    ]
}
fn replace(name: &str, value: Option<Instance>) -> Vec<(String, Instance)> {
    inputs()
        .into_iter()
        .filter_map(|(input, tree)| match input == name {
            true => value.clone().map(|value| (input, value)),
            false => Some((input, tree)),
        })
        .collect()
}

pub enum Outcome {
    /// The Primary output tree, with the default audit value.
    Outputs(Instance),
    OutputsWithAudit(Instance, Instance),
    /// A Primary-target error; the audit target still succeeds.
    PrimaryError(Json),
    /// The Primary target succeeds; the audit target and so all outputs fail
    /// at the unsupplied Lookup.
    AuditUnsupplied(Instance),
    /// The Primary target fails with `recipe`; the audit target fails at the
    /// unsupplied Lookup.
    BothFail(Json),
}
fn audit(first: Instance) -> Instance {
    group(vec![("First", first)])
}
fn default_audit() -> Instance {
    audit(text("N.small"))
}
fn values(values: Vec<Instance>) -> Instance {
    group(
        values
            .into_iter()
            .enumerate()
            .map(|(index, value)| (["V1", "V2", "V3", "V4", "V5", "V6"][index], value))
            .collect(),
    )
}
fn items(field: &str, rows: Vec<Vec<Instance>>) -> Instance {
    group(vec![(
        field,
        Instance::Repeated(rows.into_iter().map(values).collect()),
    )])
}
fn unavailable(owner: &str, node: u32) -> Json {
    json!({"kind":"OwnedSource","cause":"SourceUnavailable","owner":owner,"node":node})
}
fn calls(outcome: &Outcome) -> Vec<Json> {
    ["All", "Primary", AUDIT]
        .into_iter()
        .map(|selection| {
            let ok = |primary: &Instance, audit: &Instance| {
                let primary = json!([null, codec::encode(primary)]);
                let audit = json!([AUDIT, codec::encode(audit)]);
                match selection {
                    "All" => json!({"Ok":[primary, audit]}),
                    "Primary" => json!({"Ok":[primary]}),
                    _ => json!({"Ok":[audit]}),
                }
            };
            let lookup_unavailable = json!({ "Err": unavailable(LOOKUP, AUDIT_NODE) });
            let expected = match outcome {
                Outcome::Outputs(primary) => ok(primary, &default_audit()),
                Outcome::OutputsWithAudit(primary, audit) => ok(primary, audit),
                Outcome::PrimaryError(_) if selection == AUDIT => {
                    ok(&group(vec![]), &default_audit())
                }
                Outcome::PrimaryError(recipe) => json!({ "Err": recipe }),
                Outcome::AuditUnsupplied(primary) if selection == "Primary" => {
                    ok(primary, &default_audit())
                }
                Outcome::AuditUnsupplied(_) => lookup_unavailable,
                Outcome::BothFail(_) if selection == AUDIT => lookup_unavailable,
                Outcome::BothFail(recipe) => json!({ "Err": recipe }),
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
    let unsupplied = || replace(LOOKUP, None);
    vec![
        // D01: one Primary object yields every result kind.
        case(
            "D01",
            "D01-primary-object",
            primary(),
            inputs(),
            Outcome::Outputs(values(vec![
                text("P.red"),
                null(),
                json_null(),
                null(),
                null(),
                text("P.id"),
            ])),
        ),
        // Reading only Primary never inspects the unsupplied Lookup.
        case(
            "D01-lookup-unsupplied",
            "D01-primary-object",
            primary(),
            unsupplied(),
            Outcome::AuditUnsupplied(values(vec![
                text("P.red"),
                null(),
                json_null(),
                null(),
                null(),
                text("P.id"),
            ])),
        ),
        // A missing selected object is absent; Lookup's object never stands in.
        case(
            "D01-no-props",
            "D01-primary-object",
            endpoint(Some("color"), false, None, primary_rows()),
            inputs(),
            Outcome::Outputs(values(vec![null(); 6])),
        ),
        // D02: Lookup's missing `color` never borrows Primary's, and Primary's
        // missing `size` never borrows Lookup's.
        case(
            "D02",
            "D02-named-object",
            primary(),
            inputs(),
            Outcome::Outputs(values(vec![null(), text("N.small"), text("N.id"), null()])),
        ),
        case(
            "D02-lookup-no-props",
            "D02-named-object",
            primary(),
            replace(
                LOOKUP,
                Some(endpoint(Some("size"), true, None, lookup_rows())),
            ),
            Outcome::OutputsWithAudit(values(vec![null(); 4]), audit(null())),
        ),
        case(
            "D02-lookup-unsupplied",
            "D02-named-object",
            primary(),
            unsupplied(),
            Outcome::BothFail(unavailable(LOOKUP, FIRST_NODE)),
        ),
        // D03: an open endpoint root, its explicit root pin and source keys.
        case(
            "D03",
            "D03-open-roots-and-source-keys",
            primary(),
            inputs(),
            Outcome::Outputs(values(vec![
                text("S.fast"),
                text("S.color"),
                text("N.small"),
                null(),
                null(),
            ])),
        ),
        case(
            "D03-swapped-keys",
            "D03-open-roots-and-source-keys",
            primary_with("size", false, primary_rows()),
            replace(
                LOOKUP,
                Some(endpoint(
                    Some("color"),
                    true,
                    Some(lookup_props()),
                    lookup_rows(),
                )),
            ),
            Outcome::Outputs(values(vec![
                text("S.fast"),
                null(),
                null(),
                text("P.red"),
                text("N.small"),
            ])),
        ),
        // An absent key is not a string, so neither of its reads looks up.
        case(
            "D03-absent-lookup-key",
            "D03-open-roots-and-source-keys",
            primary(),
            replace(
                LOOKUP,
                Some(endpoint(None, true, Some(lookup_props()), lookup_rows())),
            ),
            Outcome::Outputs(values(vec![
                text("S.fast"),
                text("S.color"),
                null(),
                null(),
                null(),
            ])),
        ),
        case(
            "D03-settings-unsupplied",
            "D03-open-roots-and-source-keys",
            primary(),
            replace(SETTINGS, None),
            Outcome::PrimaryError(unavailable(SETTINGS, FIRST_NODE)),
        ),
        // D04: each Primary row object, its root object and Lookup's root.
        case(
            "D04",
            "D04-primary-rows",
            primary(),
            inputs(),
            Outcome::Outputs(items(
                "Items",
                vec![
                    vec![
                        text("P.row1"),
                        text("P.s1"),
                        text("P.red"),
                        text("N.small"),
                        text("P.s1"),
                    ],
                    vec![
                        null(),
                        text("P.s2"),
                        text("P.red"),
                        text("N.small"),
                        text("P.s2"),
                    ],
                    vec![
                        text("P.row3"),
                        null(),
                        text("P.red"),
                        text("N.small"),
                        text("P.row3"),
                    ],
                ],
            )),
        ),
        case(
            "D04-no-rows",
            "D04-primary-rows",
            primary_with("color", false, Vec::new()),
            inputs(),
            Outcome::Outputs(items("Items", Vec::new())),
        ),
        case(
            "D04-lookup-unsupplied",
            "D04-primary-rows",
            primary(),
            unsupplied(),
            Outcome::BothFail(unavailable(LOOKUP, FIRST_NODE + 3)),
        ),
        // D05: Lookup row objects while Primary keeps its root object.
        case(
            "D05",
            "D05-named-rows",
            primary(),
            inputs(),
            Outcome::Outputs(items(
                "Items",
                vec![vec![text("N.row1"), text("P.red"), text("N.small")]],
            )),
        ),
        case(
            "D05-two-rows",
            "D05-named-rows",
            primary(),
            replace(
                LOOKUP,
                Some(endpoint(
                    Some("size"),
                    true,
                    Some(lookup_props()),
                    vec![
                        row("N1", "color", vec![("color", text("N.row1"))]),
                        row("N2", "size", Vec::new()),
                    ],
                )),
            ),
            Outcome::Outputs(items(
                "Items",
                vec![
                    vec![text("N.row1"), text("P.red"), text("N.small")],
                    vec![null(), text("P.red"), text("N.small")],
                ],
            )),
        ),
        // D06: a Primary row never supplies Lookup's row object.
        case(
            "D06-uniterated",
            "D06-foreign-row-frames",
            primary_with("color", true, primary_rows()),
            inputs(),
            Outcome::PrimaryError(json!({"kind":"OwnedSource","cause":"UniteratedRepetition",
                "owner":LOOKUP,"node":110,"path":["Rows","Props"],"segment":1})),
        ),
        case(
            "D06-inactive",
            "D06-foreign-row-frames",
            primary(),
            inputs(),
            Outcome::PrimaryError(json!({"kind":"OwnedSource","cause":"InactiveSourceFrame",
                "owner":LOOKUP,"node":111,"collection":["Rows"]})),
        ),
        case(
            "D06-no-primary-rows",
            "D06-foreign-row-frames",
            primary_with("color", true, Vec::new()),
            inputs(),
            Outcome::Outputs(items("Items", Vec::new())),
        ),
        // D07: untaken branches neither evaluate keys nor inspect Lookup.
        case(
            "D07-untaken",
            "D07-lazy-keys",
            primary(),
            inputs(),
            Outcome::Outputs(values(vec![text("skip"), text("taken"), null()])),
        ),
        case(
            "D07-untaken-unsupplied",
            "D07-lazy-keys",
            primary(),
            unsupplied(),
            Outcome::AuditUnsupplied(values(vec![text("skip"), text("taken"), null()])),
        ),
        // The raising key precedes any owner inspection.
        case(
            "D07-taken",
            "D07-lazy-keys",
            primary_with("color", true, primary_rows()),
            inputs(),
            Outcome::PrimaryError(json!({"kind":"MappingException","node":RAISE})),
        ),
        case(
            "D07-taken-unsupplied",
            "D07-lazy-keys",
            primary_with("color", true, primary_rows()),
            unsupplied(),
            Outcome::BothFail(json!({"kind":"MappingException","node":RAISE})),
        ),
        // D08: each group exposes its first member's row object.
        case(
            "D08",
            "D08-grouped-members",
            primary(),
            inputs(),
            Outcome::Outputs(items(
                "Groups",
                vec![
                    vec![text("P.row1"), text("P.s1"), text("size")],
                    vec![text("P.row3"), null(), text("color")],
                ],
            )),
        ),
        case(
            "D08-reordered",
            "D08-grouped-members",
            primary_with("color", false, {
                let mut rows = primary_rows();
                rows.swap(0, 1);
                rows
            }),
            inputs(),
            Outcome::Outputs(items(
                "Groups",
                vec![
                    vec![null(), text("P.s2"), text("size")],
                    vec![text("P.row3"), null(), text("color")],
                ],
            )),
        ),
        // D09: the composed recursive filter reads Settings for every file.
        case(
            "D09",
            "D09-recursive-filter-predicate",
            primary(),
            inputs(),
            Outcome::Outputs(group(vec![
                ("V1", text("P.red")),
                (
                    "Dir",
                    Instance::Repeated(vec![dir("a", &["x", "y"], vec![dir("b", &["z"], vec![])])]),
                ),
            ])),
        ),
        case(
            "D09-no-mode",
            "D09-recursive-filter-predicate",
            primary(),
            replace(SETTINGS, Some(group(vec![("color", text("S.color"))]))),
            Outcome::Outputs(group(vec![
                ("V1", text("P.red")),
                (
                    "Dir",
                    Instance::Repeated(vec![dir("a", &[], vec![dir("b", &[], vec![])])]),
                ),
            ])),
        ),
        case(
            "D09-settings-unsupplied",
            "D09-recursive-filter-predicate",
            primary(),
            replace(SETTINGS, None),
            Outcome::PrimaryError(unavailable(SETTINGS, FILTER_READ)),
        ),
        // D10: each owned lookup searches only its own rows for the needle.
        case(
            "D10",
            "D10-lookup-needle",
            primary(),
            inputs(),
            Outcome::Outputs(values(vec![text("N1"), text("P3")])),
        ),
        case(
            "D10-no-pick",
            "D10-lookup-needle",
            primary(),
            replace(SETTINGS, Some(group(vec![("mode", text("S.fast"))]))),
            Outcome::Outputs(values(vec![null(), null()])),
        ),
        case(
            "D10-settings-unsupplied",
            "D10-lookup-needle",
            primary(),
            replace(SETTINGS, None),
            Outcome::PrimaryError(unavailable(SETTINGS, NEEDLE_READ)),
        ),
    ]
}
