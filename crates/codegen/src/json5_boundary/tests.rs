use std::collections::BTreeMap;
use std::error::Error;

use ir::{ScalarType, SchemaNode, Value};
use mapping::{FormatOptions, FunctionId};

use crate::{
    Binding, DynamicSourceProgram, Expression, ExpressionNode, FailureIteration, FailureRule,
    FailureSelection, IterationPlan, NamedSourceProgram, NamedTargetProgram, RuntimeValue,
    ScalarTargetDomain, SourceIteration, TargetConstruction, TargetScope, UserFunctionProgram,
};

use super::*;

fn program() -> Program {
    Program {
        xml_boundary: None,
        source: SchemaNode::group("Source", vec![SchemaNode::scalar("n", ScalarType::String)]),
        extra_sources: Vec::new(),
        target: SchemaNode::group(
            "Target",
            vec![SchemaNode::scalar("out", ScalarType::String)],
        ),
        expressions: vec![ExpressionNode {
            id: 1,
            expression: Expression::Const {
                value: Value::String("value".into()),
            },
        }],
        user_functions: Vec::new(),
        failure_rules: Vec::new(),
        root: TargetScope {
            target_field: String::new(),
            repeating: false,
            iteration: None,
            construction: TargetConstruction::Group,
            bindings: vec![Binding {
                target_field: "out".into(),
                expression: 1,
                target_domain: ScalarTargetDomain::Single(ScalarType::String),
                repeating: false,
            }],
            children: Vec::new(),
        },
        extra_targets: Vec::new(),
    }
}

#[test]
fn closed_program_retains_complete_ordinary_descriptors_and_all_lowering_bodies() {
    let program = program();
    let before = program.clone();
    let result = prepare_json5_boundary(&program);
    eprintln!("original profile={result:?}");
    let profile = result.unwrap();
    assert_eq!(
        profile.source_descriptor,
        codegen_schema::encode(&program.source, 1_048_576).unwrap()
    );
    assert_eq!(
        profile.target_descriptor,
        codegen_schema::encode(&program.target, 1_048_576).unwrap()
    );
    assert_eq!(program, before);
    let result = validate_json5_boundary(&program);
    eprintln!("original validate-only={result:?}");
    assert!(result.is_ok());
    let project = mapping::Project {
        source: program.source.clone(),
        target: program.target.clone(),
        source_path: None,
        target_path: None,
        source_options: FormatOptions::default(),
        target_options: FormatOptions::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        user_functions: BTreeMap::new(),
        failure_rules: Vec::new(),
        graph: mapping::Graph {
            nodes: BTreeMap::from([(
                1,
                mapping::Node::Const {
                    value: Value::String("value".into()),
                },
            )]),
        },
        root: mapping::Scope {
            bindings: vec![mapping::Binding {
                target_field: "out".into(),
                node: 1,
            }],
            ..mapping::Scope::default()
        },
    };
    let first = crate::lower(&project);
    eprintln!("original lower before policy={first:?}");
    let first = first.unwrap();
    let selected = prepare_json5_boundary(&first);
    eprintln!("original lowered selected={selected:?}");
    assert!(selected.is_ok());
    let second = crate::lower(&project);
    eprintln!("original lower after policy={second:?}");
    assert_eq!(second.unwrap(), first);
}

#[test]
fn identities_do_not_select_adapters_and_every_actual_default_is_preserved() {
    for side in [Json5BoundarySide::Source, Json5BoundarySide::Target] {
        for json_document in [false, true] {
            for json5 in [false, true] {
                let options = FormatOptions {
                    json_document,
                    json5,
                    ..FormatOptions::default()
                };
                let before = options.clone();
                let result = validate_json5_format_options(&options, side);
                eprintln!("original identities={options:?} side={side:?} result={result:?}");
                assert!(result.is_ok());
                assert_eq!(options, before);
            }
        }
    }
    let mut cases: Vec<(&str, FormatOptions)> = Vec::new();
    macro_rules! boolean { ($($field:ident),+ $(,)?) => { $( { let mut options = FormatOptions::default(); options.$field = true; cases.push((stringify!($field), options)); } )+ }; }
    boolean!(
        lenient_segments,
        xml_document,
        xml_allow_inactive_root_type_members,
        xml_root_view_read_policy,
        local_xml_file_set,
        csv_quote_disabled,
        csv_utf8_bom,
        csv_preserve_empty_strings,
        json_lines,
        xlsx_update_existing
    );
    let options = FormatOptions {
        delimiter: Some(','),
        ..FormatOptions::default()
    };
    cases.push(("delimiter", options));
    let options = FormatOptions {
        csv_quote: Some('"'),
        ..FormatOptions::default()
    };
    cases.push(("csv_quote", options));
    let options = FormatOptions {
        has_header_row: Some(true),
        ..FormatOptions::default()
    };
    cases.push(("has_header_row", options));
    let options = FormatOptions {
        xlsx_start_row: Some(1),
        ..FormatOptions::default()
    };
    cases.push(("xlsx_start_row", options));
    let options = FormatOptions {
        xlsx_sheet: Some("Sheet1".into()),
        ..FormatOptions::default()
    };
    cases.push(("xlsx_sheet", options));
    let options = FormatOptions {
        xlsx_columns: vec![1],
        ..FormatOptions::default()
    };
    cases.push(("xlsx_columns", options));
    let options = FormatOptions {
        xlsx_headers: vec!["Name".into()],
        ..FormatOptions::default()
    };
    cases.push(("xlsx_headers", options));
    let options = FormatOptions {
        xlsx_rows: vec![1],
        ..FormatOptions::default()
    };
    cases.push(("xlsx_rows", options));
    let options = FormatOptions {
        json_schema_unresolved_reference: Some("schema.json".into()),
        ..FormatOptions::default()
    };
    cases.push(("json_schema_unresolved_reference", options));
    let options = FormatOptions {
        mfd_decimal_input_names: BTreeMap::from([(1, "decimal".into())]),
        ..FormatOptions::default()
    };
    cases.push(("mfd_decimal_input_names", options));
    let options = FormatOptions {
        edi_config_reference: Some(mapping::EdiConfigDependency::missing_configuration()),
        ..FormatOptions::default()
    };
    cases.push(("edi_config_reference", options));
    assert_eq!(cases.len(), 21);
    for (expected, options) in cases {
        for side in [Json5BoundarySide::Source, Json5BoundarySide::Target] {
            let result = validate_json5_format_options(&options, side);
            eprintln!("original option field={expected} side={side:?} result={result:?}");
            assert!(
                matches!(result, Err(Json5BoundaryPolicyError::FormatOption { side: actual, field }) if actual == side && field == expected)
            );
        }
    }
}

#[test]
fn extra_static_dynamic_inputs_and_targets_refuse_before_graph_validation() {
    for dynamic in [
        None,
        Some(DynamicSourceProgram {
            path: 999,
            driver: SourceIteration::new(Vec::new()),
        }),
    ] {
        let mut candidate = program();
        candidate.extra_sources.push(NamedSourceProgram {
            name: "Other".into(),
            source: candidate.source.clone(),
            dynamic,
        });
        let result = prepare_json5_boundary(&candidate);
        eprintln!("original extra input={result:?}");
        assert!(matches!(
            result,
            Err(Json5BoundaryPolicyError::ProgramField {
                field: "extra_sources"
            })
        ));
    }
    let mut candidate = program();
    candidate.xml_boundary = Some(crate::XmlBoundaryProgram {
        input: crate::XmlInputPolicy {
            allow_inactive_root_type_members: false,
            root_view_policy: false,
        },
        extra_inputs: Vec::new(),
        output: crate::XmlOutputPolicy {
            declaration: true,
            indent: true,
            default_namespace: None,
            schema_hints: None,
        },
        extra_outputs: Vec::new(),
    });
    let result = prepare_json5_boundary(&candidate);
    eprintln!("original explicit XML program={result:?}");
    assert!(matches!(
        result,
        Err(Json5BoundaryPolicyError::ProgramField {
            field: "xml_boundary"
        })
    ));
    let mut candidate = program();
    candidate.extra_targets.push(NamedTargetProgram {
        name: "Other".into(),
        target: candidate.target.clone(),
        root: candidate.root.clone(),
    });
    let result = prepare_json5_boundary(&candidate);
    eprintln!("original extra target={result:?}");
    assert!(matches!(
        result,
        Err(Json5BoundaryPolicyError::ProgramField {
            field: "extra_targets"
        })
    ));
}

#[test]
fn scope_construction_iteration_members_and_domains_are_checked_before_copying() {
    for (candidate, expected) in [
        (
            {
                let mut p = program();
                p.root.iteration = Some(IterationPlan::source(Vec::new()));
                p
            },
            "iteration",
        ),
        (
            {
                let mut p = program();
                p.root.repeating = true;
                p
            },
            "repeating",
        ),
        (
            {
                let mut p = program();
                p.root.construction = TargetConstruction::CopyCurrentSource;
                p
            },
            "construction",
        ),
        (
            {
                let mut p = program();
                p.root.construction = TargetConstruction::Scalar {
                    expression: 1,
                    target_domain: ScalarTargetDomain::Single(ScalarType::String),
                };
                p
            },
            "construction",
        ),
        (
            {
                let mut p = program();
                p.root.construction = TargetConstruction::DynamicGroup {
                    fixed_fields: vec!["out".into()],
                    bindings: Vec::new(),
                    children: Vec::new(),
                    merge: false,
                };
                p
            },
            "construction",
        ),
        (
            {
                let mut p = program();
                p.root.bindings[0].target_domain = ScalarTargetDomain::Single(ScalarType::Float);
                p
            },
            "binding_domain",
        ),
        (
            {
                let mut p = program();
                p.root.bindings[0].repeating = true;
                p
            },
            "binding_repeating",
        ),
        (
            {
                let mut p = program();
                p.root.bindings[0].target_field = "missing".into();
                p
            },
            "binding_field",
        ),
        (
            {
                let mut p = program();
                p.root.bindings.push(p.root.bindings[0].clone());
                p
            },
            "member_count",
        ),
    ] {
        let result = prepare_json5_boundary(&candidate);
        eprintln!("original scope field={expected} result={result:?}");
        assert!(
            matches!(result, Err(Json5BoundaryPolicyError::TargetScope { field }) if field == expected)
        );
    }
    let mut duplicate = program();
    duplicate.target = SchemaNode::group(
        "Target",
        vec![
            SchemaNode::scalar("out", ScalarType::String),
            SchemaNode::scalar("other", ScalarType::String),
        ],
    );
    duplicate
        .root
        .bindings
        .push(duplicate.root.bindings[0].clone());
    let result = prepare_json5_boundary(&duplicate);
    eprintln!("original duplicate under member cap={result:?}");
    assert!(matches!(
        result,
        Err(Json5BoundaryPolicyError::TargetScope {
            field: "duplicate_member"
        })
    ));
    let mut nested = program();
    nested.target = SchemaNode::group(
        "Target",
        vec![SchemaNode::group(
            "Details",
            vec![SchemaNode::scalar("out", ScalarType::String)],
        )],
    );
    let mut child = nested.root.clone();
    child.target_field = "Details".into();
    nested.root.bindings.clear();
    nested.root.children.push(child);
    let result = prepare_json5_boundary(&nested);
    eprintln!("original nested static scope={result:?}");
    assert!(result.is_ok());
    nested.root.children.push(nested.root.children[0].clone());
    let result = prepare_json5_boundary(&nested);
    eprintln!("original duplicate nested scope={result:?}");
    assert!(matches!(
        result,
        Err(Json5BoundaryPolicyError::TargetScope {
            field: "member_count"
        })
    ));
}

#[test]
fn existing_lazy_raise_user_functions_context_and_global_rules_keep_ordinary_validation() {
    let mut candidate = program();
    let function = FunctionId::new(50);
    candidate.user_functions.push(UserFunctionProgram {
        id: function,
        library: "self".into(),
        name: "constant".into(),
        parameters: Vec::new(),
        output_type: ScalarType::String,
        expressions: vec![ExpressionNode {
            id: 1,
            expression: Expression::Const {
                value: Value::String("from function".into()),
            },
        }],
        output: 1,
    });
    candidate.expressions.extend([
        ExpressionNode {
            id: 2,
            expression: Expression::Const {
                value: Value::Bool(false),
            },
        },
        ExpressionNode {
            id: 3,
            expression: Expression::Raise { message: Some(1) },
        },
        ExpressionNode {
            id: 4,
            expression: Expression::UserFunctionCall {
                function,
                args: Vec::new(),
            },
        },
        ExpressionNode {
            id: 5,
            expression: Expression::RuntimeValue {
                value: RuntimeValue::MappingFilePath,
            },
        },
        ExpressionNode {
            id: 6,
            expression: Expression::If {
                condition: 2,
                then: 3,
                else_: 4,
            },
        },
    ]);
    candidate.root.bindings[0].expression = 6;
    candidate.failure_rules.push(FailureRule {
        iteration: FailureIteration::Source(SourceIteration::new(Vec::new())),
        selection: FailureSelection::WhenTrue(2),
        message: Some(5),
    });
    let original = validate_program(&candidate);
    eprintln!("original ordinary graph validation={original:?}");
    assert!(original.is_ok());
    let result = prepare_json5_boundary(&candidate);
    eprintln!("original admitted existing expression domains={result:?}");
    assert!(result.is_ok());
    candidate
        .expressions
        .retain(|expression| expression.id != 1);
    let original = validate_program(&candidate);
    eprintln!("original ordinary missing cause={original:?}");
    let result = prepare_json5_boundary(&candidate);
    eprintln!("original opt-in missing cause={result:?}");
    assert!(result.as_ref().unwrap_err().source().is_some());
    match result {
        Err(Json5BoundaryPolicyError::Validation(error)) => assert_eq!(Err(error), original),
        _ => panic!("the exact ordinary typed validation cause must propagate"),
    }
}

#[test]
fn schema_owner_codec_depth_and_budget_errors_remain_typed() {
    let mut candidate = program();
    candidate.source.nullable = true;
    let result = prepare_json5_boundary(&candidate);
    eprintln!("original source schema refusal={result:?}");
    assert!(matches!(
        result,
        Err(Json5BoundaryPolicyError::Schema {
            side: Json5BoundarySide::Source,
            error: Json5ProfileError::UnsupportedMetadata {
                field: "nullable_group"
            }
        })
    ));
    let mut candidate = program();
    candidate.target.name = "x".repeat(4097);
    let result = prepare_json5_boundary(&candidate);
    eprintln!("original target name refusal={result:?}");
    assert!(matches!(
        result,
        Err(Json5BoundaryPolicyError::Schema {
            side: Json5BoundarySide::Target,
            error: Json5ProfileError::Limit {
                resource: codegen_schema::json5_profile::Json5ProfileResource::NameLength,
                requested: 4097,
                max: 4096
            }
        })
    ));
    let mut candidate = program();
    let mut deep = SchemaNode::scalar("n", ScalarType::String);
    for _ in 1..64 {
        deep = SchemaNode::group("g", vec![deep]);
    }
    candidate.source = deep;
    let result = prepare_json5_boundary(&candidate);
    eprintln!("original source actual codec depth refusal={result:?}");
    assert!(matches!(
        result,
        Err(Json5BoundaryPolicyError::Schema {
            side: Json5BoundarySide::Source,
            error: Json5ProfileError::Codec(_)
        })
    ));
}
