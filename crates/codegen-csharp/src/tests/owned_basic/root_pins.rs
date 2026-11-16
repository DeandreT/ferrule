use super::*;

fn root_pin_632_source(label: &str, program: &Program) -> String {
    eprintln!("INPUT {label} complete_program={program:#?}");
    let result = crate::owned_basic::render_public_basic_owned_mapping(program);
    eprintln!("RAW {label} complete_public_mapping_or_error={result:#?}");
    result.expect("complete source return retained before comparison")
}

#[test]
fn public_basic_empty_root_pins_emit_exact_owner_schema_proof() {
    let mut p = basic(
        vec![
            expression(1, field(SourceOwner::Primary, &["Value"], Some(&[]))),
            expression(2, field(named("Ref"), &["Value"], Some(&[]))),
        ],
        group(vec![
            binding("Primary", 1, ScalarType::String),
            binding("Named", 2, ScalarType::String),
        ]),
    );
    p.extra_sources[1].source.repeating = true;
    let source = root_pin_632_source("E632-CS01", &p);
    assert_eq!(
        source
            .matches("private static bool OwnedRootIsDeclaredNonrepeating(")
            .count(),
        1
    );
    assert_eq!(
        source
            .matches("context.ResolveOwnedFieldWithRootPin(")
            .count(),
        2
    );
    assert!(source.contains("if (owner.IsPrimary) return true;"));
    assert!(
        source.contains(
            "owner.Name, \"Ref\", global::System.StringComparison.Ordinal)) return false;"
        )
    );
    assert!(source.contains(
        "owner.Name, \" Ref \\u96EA \", global::System.StringComparison.Ordinal)) return true;"
    ));
    assert!(source.contains(
        "OwnedRootIsDeclaredNonrepeating(global::Ferrule.Runtime.FerruleOwnedSourceOwner.Primary)"
    ));
    assert!(source.contains("OwnedRootIsDeclaredNonrepeating(global::Ferrule.Runtime.FerruleOwnedSourceOwner.Named(\"Ref\"))"));
}

#[test]
fn public_basic_root_pin_schema_proof_preserves_scalar_union_admission_and_repetition() {
    let mut p = basic(
        vec![expression(
            1,
            field(SourceOwner::Primary, &["Value"], Some(&[])),
        )],
        group(vec![binding("Value", 1, ScalarType::String)]),
    );
    p.extra_sources.clear();
    let union = ir::ScalarTypeSet::new([ScalarType::String, ScalarType::Int]).unwrap();
    p.source = SchemaNode::group("Input", vec![SchemaNode::scalar_union("Value", union)]);
    let nonrepeating = root_pin_632_source("E632-CS02-union-child", &p);
    assert!(nonrepeating.contains("if (owner.IsPrimary) return true;"));
    p.source = SchemaNode::scalar_union("Input", union);
    p.expressions[0].expression = field(SourceOwner::Primary, &[], Some(&[]));
    let refused = crate::owned_basic::render_public_basic_owned_mapping(&p);
    eprintln!("INPUT E632-CS02-union-root complete_program={p:#?}");
    eprintln!("RAW E632-CS02-union-root complete_refusal={refused:#?}");
    let expected = ProgramValidationError::OwnedSource {
        diagnostic: Box::new(codegen::OwnedSourceDiagnostic::Structure {
            location: codegen::OwnedSourceDiagnosticLocation::Expression {
                namespace: codegen::OwnedSourceNamespace::Main,
                node: 1,
                scope: None,
            },
            request: codegen::OwnedSourceDiagnosticRequest::Field {
                owner: codegen::OwnedSourceDiagnosticOwner::Primary,
                path: vec![],
                frame: Some(vec![]),
            },
            reason: codegen::OwnedSourceStructureReason::InvalidOwnedCollection {
                found: codegen::OwnedSourceSchemaShape::ScalarUnion { repeating: false },
            },
        }),
    };
    assert!(matches!(refused, Err(EmitError::ProgramValidation(actual)) if actual == expected));
    p.source.repeating = true;
    let repeating = root_pin_632_source("E632-CS02-repeating-union-root", &p);
    assert!(repeating.contains("if (owner.IsPrimary) return false;"));
    assert!(repeating.contains("context.ResolveOwnedFieldWithRootPin("));
}

#[test]
fn public_basic_none_and_nonempty_pins_keep_previous_field_and_position_routes() {
    for (label, frame, parts) in [
        ("E632-CS03-none", None, &["Value"][..]),
        (
            "E632-CS03-nonempty",
            Some(&["Rows"][..]),
            &["Rows", "Value"][..],
        ),
    ] {
        let p = basic(
            vec![expression(1, field(SourceOwner::Primary, parts, frame))],
            group(vec![binding("Value", 1, ScalarType::String)]),
        );
        let source = root_pin_632_source(label, &p);
        assert!(!source.contains("OwnedRootIsDeclaredNonrepeating"));
        assert!(!source.contains("ResolveOwnedFieldWithRootPin"));
        assert!(source.contains("context.ResolveOwnedField("));
    }
    let p = basic(
        vec![expression(1, position(SourceOwner::Primary, Some(&[])))],
        group(vec![binding("Value", 1, ScalarType::Int)]),
    );
    let position = root_pin_632_source("E632-CS03-position", &p);
    assert!(!position.contains("OwnedRootIsDeclaredNonrepeating"));
    assert!(position.contains("context.OwnedPosition("));
}

#[test]
fn public_basic_unused_empty_pin_does_not_add_source_metadata_or_open_ordinary_gate() {
    let p = basic(
        vec![
            expression(
                1,
                Expression::Const {
                    value: Value::String("ordinary".into()),
                },
            ),
            expression(2, field(SourceOwner::Primary, &["Value"], Some(&[]))),
        ],
        group(vec![binding("Value", 1, ScalarType::String)]),
    );
    let source = root_pin_632_source("E632-CS04-unused", &p);
    assert!(!source.contains("OwnedRootIsDeclaredNonrepeating"));
    assert!(!source.contains("ResolveOwnedFieldWithRootPin"));
    let owned = basic(
        vec![expression(
            1,
            field(SourceOwner::Primary, &["Value"], Some(&[])),
        )],
        group(vec![binding("Value", 1, ScalarType::String)]),
    );
    let ordinary = crate::emit(&owned);
    eprintln!("RAW E632-CS04 ordinary_emission_complete_error={ordinary:#?}");
    let expected = ProgramValidationError::OwnedSource {
        diagnostic: Box::new(codegen::OwnedSourceDiagnostic::Capability {
            location: codegen::OwnedSourceDiagnosticLocation::Expression {
                namespace: codegen::OwnedSourceNamespace::Main,
                node: 1,
                scope: None,
            },
            request: Some(codegen::OwnedSourceDiagnosticRequest::Field {
                owner: codegen::OwnedSourceDiagnosticOwner::Primary,
                path: path(&["Value"]),
                frame: Some(vec![]),
            }),
            reason: codegen::OwnedSourceCapabilityReason::UnsupportedBasicContext,
            origin: codegen::OwnedSourceCapabilityOrigin::RetainedExpression {
                namespace: codegen::OwnedSourceNamespace::Main,
                node: 1,
            },
        }),
    };
    assert!(matches!(ordinary, Err(EmitError::ProgramValidation(actual)) if actual == expected));
}

// Public Basic leaf provenance is separate from ordinary/private rendering.
