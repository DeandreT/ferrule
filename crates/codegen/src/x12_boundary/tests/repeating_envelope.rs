//! X12 interchange admission: explicit selection, the retained version
//! and exact typed refusals for every ownership, width and type rule.
use super::*;

const SCHEMAS: [(&str, &str, &str); 3] = [
    (
        "00401",
        "004010",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../codegen-csharp/tests/x12/repeating-envelope/fixtures/004010-schema.json"
        )),
    ),
    (
        "00501",
        "005010",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../codegen-csharp/tests/x12/repeating-envelope/fixtures/005010-schema.json"
        )),
    ),
    (
        "00604",
        "006040",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../codegen-csharp/tests/x12/repeating-envelope/fixtures/006040-schema.json"
        )),
    ),
];

fn repeating_envelope_schema() -> SchemaNode {
    serde_json::from_str(SCHEMAS[1].2).unwrap()
}

fn repeating_options(version: &str) -> X12BoundaryOptions {
    X12BoundaryOptions {
        envelope_profile: X12EnvelopeProfile::MapForceInterchanges,
        interchange_version: Some(version.into()),
        ..Default::default()
    }
}

fn node_mut<'a>(schema: &'a mut SchemaNode, path: &[&str]) -> &'a mut SchemaNode {
    let mut node = schema;
    for name in path {
        node = node.child_mut(name).unwrap();
    }
    node
}

fn children_mut<'a>(schema: &'a mut SchemaNode, path: &[&str]) -> &'a mut Vec<SchemaNode> {
    let SchemaKind::Group { children, .. } = &mut node_mut(schema, path).kind else {
        panic!("repeating-interchange test owner must be a group");
    };
    children
}

#[test]
fn repeating_envelope_admission_is_explicit_and_uses_the_retained_version() {
    for (isa, version, literal) in SCHEMAS {
        let candidate: SchemaNode = serde_json::from_str(literal).unwrap();
        for side in [X12BoundarySide::Source, X12BoundarySide::Target] {
            let encoded =
                retained_profile("repeating-envelope-version", &candidate, &repeating_options(isa), side).unwrap();
            let descriptor: serde_json::Value = serde_json::from_str(&encoded).unwrap();
            assert_eq!(descriptor["envelope_profile"], "mapforce_interchanges");
            assert_eq!(descriptor["version"], version);
            let decoded = codegen_schema::decode(
                descriptor["schema"].as_str().unwrap(),
                MAX_EMBEDDED_X12_DESCRIPTOR_BYTES,
            )
            .unwrap();
            assert_eq!(decoded, candidate);
        }
        // Agreeing fixed ISA12 and GS08 metadata remains admitted.
        let mut fixed = candidate.clone();
        node_mut(&mut fixed, &["Interchange", "ISA", "FI11"]).fixed = Some(isa.into());
        node_mut(&mut fixed, &["Interchange", "Group", "GS", "F480"]).fixed = Some(version.into());
        retained_profile(
            "repeating-envelope-fixed-version",
            &fixed,
            &repeating_options(isa),
            X12BoundarySide::Target,
        )
        .unwrap();
        // Message configurations may declare SE optional.
        let mut optional_se = candidate.clone();
        node_mut(
            &mut optional_se,
            &["Interchange", "Group", "Message_945", "SE"],
        )
        .repeating = true;
        retained_profile(
            "repeating-envelope-optional-se",
            &optional_se,
            &repeating_options(isa),
            X12BoundarySide::Target,
        )
        .unwrap();
    }
    // The shape is never inferred from repeating envelope segments.
    for profile in [
        X12EnvelopeProfile::SingleTransaction,
        X12EnvelopeProfile::GroupedTransactions,
    ] {
        let result = retained_profile(
            "repeating-envelope-not-inferred",
            &repeating_envelope_schema(),
            &X12BoundaryOptions {
                envelope_profile: profile,
                interchange_version: Some("00501".into()),
                ..Default::default()
            },
            X12BoundarySide::Source,
        );
        assert!(
            matches!(result, Err(X12BoundaryPolicyError::Schema { .. })),
            "{profile:?}: {result:?}"
        );
    }
    let program = Program {
        source: repeating_envelope_schema(),
        ..program()
    };
    let policy = X12BoundaryPolicy {
        source: Some(repeating_options("00501")),
        target: None,
    };
    let profile = prepare_x12_boundary(&program, &policy).unwrap();
    assert!(profile.source_x12 && !profile.target_x12);
}

#[test]
fn repeating_envelope_refusals_name_their_exact_owner_and_reason() {
    type Case = (
        &'static str,
        SchemaMutation,
        Option<&'static str>,
        Vec<&'static str>,
        &'static str,
    );
    let cases: Vec<Case> = vec![
        (
            "acknowledgment-branch",
            Box::new(|schema| {
                children_mut(schema, &["Interchange", "Group"]).insert(
                    2,
                    SchemaNode::group(
                        "ParserErrors_Message",
                        vec![SchemaNode::scalar("Detail", ScalarType::String)],
                    ),
                )
            }),
            Some("00501"),
            vec!["Interchange", "Group", "ParserErrors_Message"],
            "acknowledgment branches are outside the generated profile",
        ),
        (
            "extra-root-child",
            Box::new(|schema| {
                children_mut(schema, &[]).push(SchemaNode::group(
                    "Trailer",
                    vec![SchemaNode::scalar("Note", ScalarType::String)],
                ))
            }),
            Some("00501"),
            vec![],
            "a repeating-interchange X12 schema requires one repeating interchange container below the root",
        ),
        (
            "singular-interchange",
            Box::new(|schema| node_mut(schema, &["Interchange"]).repeating = false),
            Some("00501"),
            vec!["Interchange"],
            "interchange, group and message owners in a repeating interchange must be non-segment repeating containers",
        ),
        (
            "extra-interchange-child",
            Box::new(|schema| {
                let note =
                    SchemaNode::group("NTE", vec![SchemaNode::scalar("NTE01", ScalarType::String)]);
                children_mut(schema, &["Interchange"]).insert(1, note.repeating())
            }),
            Some("00501"),
            vec!["Interchange"],
            "each repeating interchange requires exactly ISA, one functional-group container and IEA",
        ),
        (
            "second-message-container",
            Box::new(|schema| {
                let mut other = node_mut(schema, &["Interchange", "Group", "Message_945"]).clone();
                other.name = "Message_940".into();
                children_mut(schema, &["Interchange", "Group"]).insert(2, other)
            }),
            Some("00501"),
            vec!["Interchange", "Group"],
            "each functional group in a repeating interchange must have exactly one message container",
        ),
        (
            "extra-group-segment",
            Box::new(|schema| {
                let note =
                    SchemaNode::group("NTE", vec![SchemaNode::scalar("NTE01", ScalarType::String)]);
                children_mut(schema, &["Interchange", "Group"]).insert(1, note.repeating())
            }),
            Some("00501"),
            vec!["Interchange", "Group"],
            "each functional group in a repeating interchange requires exactly GS, one message container and GE",
        ),
        (
            "singular-group",
            Box::new(|schema| node_mut(schema, &["Interchange", "Group"]).repeating = false),
            Some("00501"),
            vec!["Interchange", "Group"],
            "interchange, group and message owners in a repeating interchange must be non-segment repeating containers",
        ),
        (
            "singular-message",
            Box::new(|schema| {
                node_mut(schema, &["Interchange", "Group", "Message_945"]).repeating = false
            }),
            Some("00501"),
            vec!["Interchange", "Group", "Message_945"],
            "interchange, group and message owners in a repeating interchange must be non-segment repeating containers",
        ),
        (
            "singular-isa",
            Box::new(|schema| node_mut(schema, &["Interchange", "ISA"]).repeating = false),
            Some("00501"),
            vec!["Interchange", "ISA"],
            "ISA, GS, GE and IEA must be optional repeating segments in a repeating interchange",
        ),
        (
            "singular-gs",
            Box::new(|schema| node_mut(schema, &["Interchange", "Group", "GS"]).repeating = false),
            Some("00501"),
            vec!["Interchange", "Group", "GS"],
            "ISA, GS, GE and IEA must be optional repeating segments in a repeating interchange",
        ),
        (
            "singular-ge",
            Box::new(|schema| node_mut(schema, &["Interchange", "Group", "GE"]).repeating = false),
            Some("00501"),
            vec!["Interchange", "Group", "GE"],
            "ISA, GS, GE and IEA must be optional repeating segments in a repeating interchange",
        ),
        (
            "singular-iea",
            Box::new(|schema| node_mut(schema, &["Interchange", "IEA"]).repeating = false),
            Some("00501"),
            vec!["Interchange", "IEA"],
            "ISA, GS, GE and IEA must be optional repeating segments in a repeating interchange",
        ),
        (
            "repeating-st",
            Box::new(|schema| {
                node_mut(schema, &["Interchange", "Group", "Message_945", "ST"]).repeating = true
            }),
            Some("00501"),
            vec!["Interchange", "Group", "Message_945"],
            "each message in a repeating interchange requires a singular ST first and SE last",
        ),
        (
            "missing-se",
            Box::new(|schema| {
                children_mut(schema, &["Interchange", "Group", "Message_945"]).pop();
            }),
            Some("00501"),
            vec!["Interchange", "Group", "Message_945"],
            "each message in a repeating interchange requires a singular ST first and SE last",
        ),
        (
            "envelope-in-body",
            Box::new(|schema| {
                let ge = node_mut(schema, &["Interchange", "Group", "GE"]).clone();
                children_mut(schema, &["Interchange", "Group", "Message_945", "Detail"]).push(ge)
            }),
            Some("00501"),
            vec!["Interchange", "Group", "Message_945", "Detail", "GE"],
            "message bodies in a repeating interchange cannot contain envelope segments",
        ),
        (
            "gs-width",
            Box::new(|schema| {
                children_mut(schema, &["Interchange", "Group", "GS"]).pop();
            }),
            Some("00501"),
            vec!["Interchange", "Group", "GS"],
            "repeating-interchange envelopes require 16 elements in ISA, 8 in GS, 2 or 3 in ST and 2 each in SE, GE and IEA",
        ),
        (
            "st-width",
            Box::new(|schema| {
                children_mut(schema, &["Interchange", "Group", "Message_945", "ST"])
                    .push(SchemaNode::scalar("F9999", ScalarType::String))
            }),
            Some("00501"),
            vec!["Interchange", "Group", "Message_945", "ST"],
            "repeating-interchange envelopes require 16 elements in ISA, 8 in GS, 2 or 3 in ST and 2 each in SE, GE and IEA",
        ),
        (
            "numeric-identity",
            Box::new(|schema| {
                node_mut(schema, &["Interchange", "ISA", "FI06"]).kind = SchemaKind::Scalar {
                    ty: ScalarType::Float,
                }
            }),
            Some("00501"),
            vec!["Interchange", "ISA"],
            "repeating-interchange controls and counts permit String, Int or Float; other envelope elements require String",
        ),
        (
            "numeric-st02",
            Box::new(|schema| {
                node_mut(
                    schema,
                    &["Interchange", "Group", "Message_945", "ST", "F329"],
                )
                .kind = SchemaKind::Scalar {
                    ty: ScalarType::Int,
                }
            }),
            Some("00501"),
            vec!["Interchange", "Group", "Message_945", "ST"],
            "repeating-interchange controls and counts permit String, Int or Float; other envelope elements require String",
        ),
        (
            "boolean-control",
            Box::new(|schema| {
                node_mut(schema, &["Interchange", "IEA", "FI12"]).kind = SchemaKind::Scalar {
                    ty: ScalarType::Bool,
                }
            }),
            Some("00501"),
            vec!["Interchange", "IEA", "FI12"],
            "elements require String/Int/Float scalars or one flat composite level",
        ),
        (
            "missing-version",
            Box::new(|_| {}),
            None,
            vec![],
            "repeating interchanges require a retained interchange version of 00401, 00501 or 00604",
        ),
        (
            "unsupported-version",
            Box::new(|_| {}),
            Some("00402"),
            vec![],
            "repeating interchanges require a retained interchange version of 00401, 00501 or 00604",
        ),
        (
            "fixed-isa12-conflict",
            Box::new(|schema| {
                node_mut(schema, &["Interchange", "ISA", "FI11"]).fixed = Some("00604".into())
            }),
            Some("00501"),
            vec![],
            "fixed ISA12 or GS08 must agree with the retained interchange version",
        ),
        (
            "fixed-gs08-conflict",
            Box::new(|schema| {
                node_mut(schema, &["Interchange", "Group", "GS", "F480"]).fixed =
                    Some("005010X999".into())
            }),
            Some("00501"),
            vec![],
            "fixed ISA12 or GS08 must agree with the retained interchange version",
        ),
    ];
    for (label, mutate, version, path, reason) in cases {
        let mut candidate = repeating_envelope_schema();
        mutate(&mut candidate);
        let options = X12BoundaryOptions {
            interchange_version: version.map(Into::into),
            ..repeating_options("00501")
        };
        for side in [X12BoundarySide::Source, X12BoundarySide::Target] {
            let result = retained_profile(label, &candidate, &options, side);
            assert!(
                matches!(&result, Err(X12BoundaryPolicyError::Schema { side: actual, path: found, reason: found_reason })
                    if *actual == side && *found == path && *found_reason == reason),
                "{label} {side:?}: {result:?}"
            );
        }
    }
}

#[test]
fn repeating_envelope_envelope_elements_refuse_implied_decimals_and_release_syntax() {
    let path = vec![
        "Interchange".to_owned(),
        "ISA".to_owned(),
        "FI12".to_owned(),
    ];
    for side in [X12BoundarySide::Source, X12BoundarySide::Target] {
        let options = X12BoundaryOptions {
            implied_decimals: vec![EdiImpliedDecimal::new(path.clone(), 2).unwrap()],
            ..repeating_options("00501")
        };
        let result = retained_profile("repeating-envelope-implied", &repeating_envelope_schema(), &options, side);
        assert!(
            matches!(&result, Err(X12BoundaryPolicyError::Schema { path: found, reason, .. })
                if *found == path && *reason == "implied decimals cannot apply to repeating-interchange envelope elements"),
            "{side:?}: {result:?}"
        );
    }
    // Body numeric elements keep their ordinary implied-decimal rules.
    let body = vec![
        "Interchange".to_owned(),
        "Group".to_owned(),
        "Message_945".to_owned(),
        "W03".to_owned(),
        "F81".to_owned(),
    ];
    retained_profile(
        "repeating-envelope-body-implied",
        &repeating_envelope_schema(),
        &X12BoundaryOptions {
            implied_decimals: vec![EdiImpliedDecimal::new(body, 2).unwrap()],
            ..repeating_options("00501")
        },
        X12BoundarySide::Source,
    )
    .unwrap();
    let release = X12BoundaryOptions {
        separators: Some(X12Separators {
            element: '*',
            component: ':',
            segment: '~',
            repetition: Some('^'),
            release: Some('?'),
        }),
        ..repeating_options("00501")
    };
    assert!(matches!(
        retained_profile(
            "repeating-envelope-release",
            &repeating_envelope_schema(),
            &release,
            X12BoundarySide::Target
        ),
        Err(X12BoundaryPolicyError::Separators { .. })
    ));
}
