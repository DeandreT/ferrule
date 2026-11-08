use super::*;

pub(super) fn filter644_schema() -> SchemaNode {
    SchemaNode::group(
        "Input",
        vec![
            SchemaNode::scalar("Value", ScalarType::String),
            SchemaNode::group("Ref", vec![SchemaNode::scalar("Value", ScalarType::String)]),
            SchemaNode::group(
                "Rows",
                vec![
                    SchemaNode::scalar("Value", ScalarType::Int),
                    SchemaNode::scalar("Flag", ScalarType::Bool),
                    SchemaNode::group(
                        "Rows",
                        vec![
                            SchemaNode::scalar("Value", ScalarType::Int),
                            SchemaNode::scalar("Flag", ScalarType::Bool),
                        ],
                    )
                    .repeating(),
                ],
            )
            .repeating(),
        ],
    )
}
pub(super) fn filter644_program(owner: SourceOwner) -> Program {
    let mut p = basic(
        vec![
            node(1, owned(owner.clone(), &["Rows", "Value"], Some(&["Rows"]))),
            node(2, position(owner.clone(), Some(&["Rows"]))),
            node(
                3,
                Expression::Const {
                    value: Value::Int(1),
                },
            ),
            node(
                4,
                Expression::Call {
                    function: ScalarFunction::GreaterThan,
                    args: vec![2, 3],
                },
            ),
            node(5, owned(owner.clone(), &["Value"], Some(&[]))),
        ],
        &[
            ("Value", 1, ScalarType::Int),
            ("Position", 2, ScalarType::Int),
            ("Root", 5, ScalarType::String),
        ],
    );
    p.source = filter644_schema();
    for source in &mut p.extra_sources {
        source.source = filter644_schema();
    }
    p.target.repeating = true;
    p.root.repeating = true;
    p.root.iteration = Some(IterationPlan::new(
        codegen::OwnedSourceIteration::new(OwnedSourceRef {
            owner,
            path: path(&["Rows"]),
        }),
        Some(4),
        None,
        Vec::new(),
        IterationOutput::Repeated,
    ));
    p
}
pub(super) fn filter644_result(
    label: &str,
    p: &Program,
    options: &Options,
) -> Result<ArtifactSet, EmitError> {
    eprintln!("INPUT F644 {label} complete_program={p:#?} complete_options={options:#?}");
    let result = emit_with_basic_owned_filters(p, options);
    eprintln!("RAW F644 {label} complete_artifacts_or_error={result:#?}");
    if let Err(error) = &result {
        eprintln!(
            "RAW F644 {label} cause={:?}",
            error.source().map(ToString::to_string)
        );
    }
    result
}
pub(super) fn filter644_invalid(
    actual: Result<ArtifactSet, EmitError>,
    expected: ProgramValidationError,
) {
    match actual {
        Err(EmitError::InvalidProgram(error)) => assert_eq!(error, expected),
        other => panic!("complete recorded refusal differs: {other:#?}"),
    }
}

#[test]
fn public_basic_filter_profile_preserves_complete_control_free_artifacts_and_policy_entries() {
    for p in [
        public519_outputs_program(),
        root_pin632_rows_program(),
        public582_lazy_program(),
    ] {
        let options = public519_options("public_filter_control_free_644");
        let old = public519_result("F644 control-free baseline", &p, &options).unwrap();
        let opted = filter644_result("control-free explicit profile", &p, &options).unwrap();
        assert_control_free_typed_artifacts_match(&p, &old, &opted);
    }
    let p = filter644_program(SourceOwner::Primary);
    let files = filter644_result(
        "checked input/target policy",
        &p,
        &public519_options("public_filter_policy_644"),
    )
    .unwrap();
    let source = public519_source(&files);
    assert!(
        source.contains("execute_checked_private_owned_basic(source, supplied, execution, limits)")
    );
    assert!(source.contains("execute_selected_checked_private_owned_basic(source, supplied, execution, limits, selection)"));
    assert!(!source.contains("with_dynamic_source_loader"));
    let mut invalid = public519_options("valid");
    invalid.package_name = "0bad".into();
    match filter644_result("package before renderer", &p, &invalid) {
        Err(EmitError::InvalidPackageName(name)) => assert_eq!(name, "0bad"),
        other => panic!("complete package refusal changed: {other:#?}"),
    }
    let mut structural = p;
    structural.expressions[0].expression = owned(named("Absent"), &["Rows", "Value"], None);
    let expected = codegen::validate_program_with_basic_owned_filters(&structural).unwrap_err();
    filter644_invalid(
        filter644_result("structure before package", &structural, &invalid),
        expected,
    );
}

pub(super) fn assert_control_free_typed_artifacts_match(
    program: &Program,
    with_json: &ArtifactSet,
    typed: &ArtifactSet,
) {
    let core = crate::owned_basic::render_public_basic_owned_source(program).unwrap();
    let source = public519_source(with_json);
    assert!(source.starts_with(&core));
    assert!(source.contains("pub fn execute_json_outputs_with_basic_owned_sources("));
    assert_eq!(public519_source(typed), core);
    assert_eq!(
        with_json
            .files()
            .iter()
            .filter(|file| file.path.as_str() != "src/lib.rs")
            .collect::<Vec<_>>(),
        typed
            .files()
            .iter()
            .filter(|file| file.path.as_str() != "src/lib.rs")
            .collect::<Vec<_>>(),
    );
}

#[test]
fn public_basic_filters_keep_sort_windows_grouping_and_first_output_closed_with_full_diagnostics() {
    for control in 0..9 {
        let mut p = filter644_program(SourceOwner::Primary);
        let sort = (control == 0).then(|| {
            codegen::SortPlan::new(
                codegen::SortKey {
                    expression: 1,
                    descending: false,
                },
                Vec::new(),
                SortFilterOrder::FilterThenSort,
            )
        });
        let windows = match control {
            1 => vec![SequenceWindow::SkipFirst { count: 3 }],
            2 => vec![SequenceWindow::First { count: 3 }],
            3 => vec![SequenceWindow::From { position: 3 }],
            4 => vec![SequenceWindow::FromTo { first: 3, last: 3 }],
            5 => vec![SequenceWindow::Last { count: 3 }],
            _ => Vec::new(),
        };
        let output = if control == 8 {
            p.root.repeating = false;
            p.target.repeating = false;
            IterationOutput::First
        } else {
            IterationOutput::Repeated
        };
        let mut iteration = IterationPlan::new(
            codegen::OwnedSourceIteration::new(OwnedSourceRef {
                owner: SourceOwner::Primary,
                path: path(&["Rows"]),
            }),
            Some(4),
            sort,
            windows,
            output,
        );
        if control == 6 {
            iteration = iteration.with_grouping(GroupingPlan::By { key: 1 });
        }
        if control == 7 {
            iteration = iteration.with_filtered_grouping(GroupingPlan::By { key: 1 }, 4);
        }
        p.root.iteration = Some(iteration);
        let expected = codegen::validate_basic_owned_filter_emission(&p).unwrap_err();
        let actual = filter644_result(
            "all other scope controls stay closed",
            &p,
            &public519_options("public_filter_closed_644"),
        );
        filter644_invalid(actual, expected);
    }
}

#[test]
fn public_basic_filter_source_keeps_one_structural_predicate_and_immediate_compacted_output() {
    let p = filter644_program(SourceOwner::Primary);
    let files = filter644_result(
        "complete streaming scope",
        &p,
        &public519_options("public_filter_stream_644"),
    )
    .unwrap();
    let source = public519_source(&files);
    let start = source.find("fn scope_root(").unwrap();
    let end = source[start..].find("\n}\n").unwrap() + start + "\n}\n".len();
    let expected = "fn scope_root(context: &BasicScopeContext<'_>) -> Result<Instance, RuntimeError> {\n    let cohort = context.walk_owned_source_cohort(SourceOwnerRef::Primary, &[\"Rows\"])?;\n    let mut outputs = Vec::new();\n    context.visit_filtered_source_cohort(\n        &cohort,\n        |predicate_context| {\n            let value = expression_4(predicate_context)?;\n            require_bool(4, value)\n        },\n        |item_context| {\n            let value_0 = adapt_target_value(expression_1(item_context)?, ScalarType::Int);\n            let value_1 = adapt_target_value(expression_2(item_context)?, ScalarType::Int);\n            let value_2 = adapt_target_value(expression_5(item_context)?, ScalarType::String);\n            let output = group([\n                field(\"Value\", scalar(value_0)),\n                field(\"Position\", scalar(value_1)),\n                field(\"Root\", scalar(value_2)),\n            ]);\n            if outputs.capacity() == 0 { outputs = Vec::with_capacity(cohort.len()); }\n            outputs.push(output);\n            Ok(())\n        },\n    )?;\n    Ok(repeated(outputs))\n}\n";
    assert_eq!(&source[start..end], expected);
    assert!(source.contains(".with_owned_source_node(1)"));
    assert!(source.contains(".with_owned_source_node(2)"));
    assert!(source.contains(".with_owned_source_node(5)"));
}

#[test]
fn public_basic_filter_predicate_unsigned_nodes_and_potential_owned_pins_are_retained() {
    for id in [0, u32::MAX] {
        let mut p = filter644_program(SourceOwner::Primary);
        p.expressions.retain(|node| node.id != 4);
        p.expressions.push(node(
            id,
            owned(named("Ref"), &["Rows", "Flag"], Some(&["Rows"])),
        ));
        p.root.iteration = Some(IterationPlan::new(
            codegen::OwnedSourceIteration::new(OwnedSourceRef {
                owner: SourceOwner::Primary,
                path: path(&["Rows"]),
            }),
            Some(id),
            None,
            Vec::new(),
            IterationOutput::Repeated,
        ));
        let files = filter644_result(
            "unsigned predicate/leaf",
            &p,
            &public519_options("public_filter_unsigned_644"),
        )
        .unwrap();
        let source = public519_source(&files);
        assert!(source.contains(&format!("let value = expression_{id}(predicate_context)?;")));
        assert!(source.contains(&format!("require_bool({id}, value)")));
        assert!(public582_map_body(source, id).contains(&format!(".with_owned_source_node({id})")));
    }
    let mut p = filter644_program(SourceOwner::Primary);
    p.expressions.extend([
        node(0, owned(named("Ref"), &["Rows", "Flag"], Some(&["Rows"]))),
        node(
            6,
            Expression::Const {
                value: Value::Bool(false),
            },
        ),
        node(
            7,
            Expression::If {
                condition: 6,
                then: 0,
                else_: 6,
            },
        ),
    ]);
    p.root.iteration = Some(IterationPlan::new(
        codegen::OwnedSourceIteration::new(OwnedSourceRef {
            owner: SourceOwner::Primary,
            path: path(&["Rows"]),
        }),
        Some(7),
        None,
        Vec::new(),
        IterationOutput::Repeated,
    ));
    let files = filter644_result(
        "inactive potential leaf stays lazy",
        &p,
        &public519_options("public_filter_lazy_644"),
    )
    .unwrap();
    assert!(public519_source(&files).contains("fn expression_0("));
    assert!(public582_map_body(public519_source(&files), 7).contains("expression_0(context)"));
}

#[test]
fn public_basic_filter_named_catalogues_preserve_predicate_legacy_requirements_and_owned_optional_inputs()
 {
    let mut legacy = filter644_program(SourceOwner::Primary);
    legacy.expressions.push(node(
        6,
        Expression::SourceField {
            path: path(&["Ref", "Rows", "Flag"]),
            frame: None,
        },
    ));
    legacy.root.iteration = Some(IterationPlan::new(
        codegen::OwnedSourceIteration::new(OwnedSourceRef {
            owner: SourceOwner::Primary,
            path: path(&["Rows"]),
        }),
        Some(6),
        None,
        Vec::new(),
        IterationOutput::Repeated,
    ));
    let plan =
        codegen::legacy_named_input_admission_plan_with_basic_owned_filters(&legacy).unwrap();
    assert_eq!(
        plan.primary
            .entries
            .iter()
            .find(|entry| entry.name == "Ref")
            .unwrap()
            .availability,
        codegen::NamedInputAvailability::RequiredLegacy
    );
    let owned = filter644_program(named("Ref"));
    let plan = codegen::legacy_named_input_admission_plan_with_basic_owned_filters(&owned).unwrap();
    assert_eq!(
        plan.primary
            .entries
            .iter()
            .find(|entry| entry.name == "Ref")
            .unwrap()
            .availability,
        codegen::NamedInputAvailability::Optional
    );
    assert!(
        filter644_result(
            "legacy predicate admission",
            &legacy,
            &public519_options("public_filter_legacy_644")
        )
        .is_ok()
    );
}

fn filter644_predicate(p: &mut Program, predicate: u32) {
    let input = p.root.iteration.as_ref().unwrap().input().clone();
    p.root.iteration = Some(IterationPlan::new(
        input,
        Some(predicate),
        None,
        Vec::new(),
        IterationOutput::Repeated,
    ));
}
fn filter644_order_program() -> Program {
    let mut p = filter644_program(SourceOwner::Primary);
    p.expressions.extend([
        node(
            7,
            Expression::Call {
                function: ScalarFunction::Equal,
                args: vec![2, 3],
            },
        ),
        node(
            8,
            Expression::Const {
                value: Value::Bool(true),
            },
        ),
        node(
            9,
            Expression::If {
                condition: 7,
                then: 8,
                else_: 42,
            },
        ),
        node(
            40,
            Expression::Const {
                value: Value::String("target-first".into()),
            },
        ),
        node(41, Expression::Raise { message: Some(40) }),
        node(42, Expression::Raise { message: Some(43) }),
        node(
            43,
            Expression::Const {
                value: Value::String("predicate-later".into()),
            },
        ),
    ]);
    p.root.bindings[0].expression = 41;
    filter644_predicate(&mut p, 9);
    p
}
fn filter644_nonbool_program(id: u32) -> Program {
    let mut p = filter644_program(SourceOwner::Primary);
    p.expressions.push(node(
        id,
        Expression::Const {
            value: Value::Int(7),
        },
    ));
    filter644_predicate(&mut p, id);
    p
}
fn filter644_lazy_program(condition: bool) -> Program {
    let mut p = filter644_program(SourceOwner::Primary);
    p.expressions.extend([
        node(0, owned(named("Ref"), &["Rows", "Flag"], Some(&["Rows"]))),
        node(
            6,
            Expression::Const {
                value: Value::Bool(condition),
            },
        ),
        node(
            7,
            Expression::Const {
                value: Value::Bool(true),
            },
        ),
        node(
            8,
            Expression::Const {
                value: Value::Bool(false),
            },
        ),
        node(
            9,
            Expression::If {
                condition: 6,
                then: 7,
                else_: 8,
            },
        ),
        node(
            10,
            Expression::If {
                condition: 6,
                then: 9,
                else_: 0,
            },
        ),
        node(
            11,
            Expression::If {
                condition: 8,
                then: 0,
                else_: 8,
            },
        ),
    ]);
    filter644_predicate(&mut p, if condition { 10 } else { 11 });
    p
}
fn filter644_nested_program() -> Program {
    let mut p = filter644_program(SourceOwner::Primary);
    p.expressions.extend([
        node(
            21,
            owned(
                SourceOwner::Primary,
                &["Rows", "Rows", "Value"],
                Some(&["Rows", "Rows"]),
            ),
        ),
        node(22, position(SourceOwner::Primary, Some(&["Rows", "Rows"]))),
        node(
            23,
            Expression::Call {
                function: ScalarFunction::GreaterThan,
                args: vec![22, 3],
            },
        ),
    ]);
    let mut child = scope(&[
        ("Value", 21, ScalarType::Int),
        ("Position", 22, ScalarType::Int),
        ("ParentPosition", 2, ScalarType::Int),
    ]);
    child.target_field = "Child".into();
    child.repeating = true;
    child.iteration = Some(IterationPlan::new(
        codegen::OwnedSourceIteration::new(OwnedSourceRef {
            owner: SourceOwner::Primary,
            path: path(&["Rows", "Rows"]),
        }),
        Some(23),
        None,
        Vec::new(),
        IterationOutput::Repeated,
    ));
    p.root.children.push(child);
    p.target = SchemaNode::group(
        "Output",
        vec![
            SchemaNode::scalar("Value", ScalarType::Int),
            SchemaNode::scalar("Position", ScalarType::Int),
            SchemaNode::scalar("Root", ScalarType::String),
            SchemaNode::group(
                "Child",
                vec![
                    SchemaNode::scalar("Value", ScalarType::Int),
                    SchemaNode::scalar("Position", ScalarType::Int),
                    SchemaNode::scalar("ParentPosition", ScalarType::Int),
                ],
            )
            .repeating(),
        ],
    )
    .repeating();
    p
}
fn filter644_plain_program(identity: bool) -> Program {
    let mut p = filter644_program(SourceOwner::Primary);
    p.expressions
        .iter_mut()
        .find(|node| node.id == 4)
        .unwrap()
        .expression = Expression::Const {
        value: Value::Bool(true),
    };
    if identity {
        p.expressions[0].expression = Expression::Const {
            value: Value::Int(7),
        };
        p.expressions[1].expression = position(SourceOwner::Primary, None);
        p.root.iteration = Some(IterationPlan::new(
            codegen::OwnedSourceIteration::new(OwnedSourceRef {
                owner: SourceOwner::Primary,
                path: Vec::new(),
            }),
            Some(4),
            None,
            Vec::new(),
            IterationOutput::Repeated,
        ));
    } else {
        p.expressions[0].expression = owned(SourceOwner::Primary, &["Rows", "Value"], None);
    }
    p
}
fn filter644_failure_program() -> Program {
    let mut p = filter644_order_program();
    p.expressions.extend([
        node(
            60,
            Expression::Const {
                value: Value::Bool(true),
            },
        ),
        node(
            61,
            Expression::Const {
                value: Value::String("rule-first".into()),
            },
        ),
    ]);
    p.failure_rules.push(codegen::FailureRule {
        iteration: codegen::FailureIteration::Source(codegen::SourceIteration::new(path(&[
            "Rows",
        ]))),
        selection: codegen::FailureSelection::WhenTrue(60),
        message: Some(61),
    });
    p
}
fn filter644_modes_program() -> Program {
    let mut p = filter644_program(SourceOwner::Primary);
    let named_program = filter644_program(named("Ref"));
    let offset = |id| id + 10;
    p.expressions
        .extend(named_program.expressions.into_iter().map(|mut node| {
            node.id = offset(node.id);
            if let Expression::Call { args, .. } = &mut node.expression {
                for arg in args {
                    *arg = offset(*arg);
                }
            }
            node
        }));
    let mut target = named_program.root;
    for binding in &mut target.bindings {
        binding.expression = offset(binding.expression);
    }
    target.iteration = Some(IterationPlan::new(
        codegen::OwnedSourceIteration::new(OwnedSourceRef {
            owner: named("Ref"),
            path: path(&["Rows"]),
        }),
        Some(14),
        None,
        Vec::new(),
        IterationOutput::Repeated,
    ));
    p.extra_targets.push(NamedTargetProgram {
        name: "Named".into(),
        target: named_program.target,
        root: target,
    });
    p
}
pub(super) fn filter644_host_programs() -> Vec<(&'static str, Program, &'static str)> {
    let mut never = filter644_order_program();
    never
        .expressions
        .iter_mut()
        .find(|node| node.id == 8)
        .unwrap()
        .expression = Expression::Const {
        value: Value::Bool(false),
    };
    filter644_predicate(&mut never, 8);
    let mut predicate = filter644_order_program();
    filter644_predicate(&mut predicate, 42);
    vec![
        (
            "primary",
            filter644_program(SourceOwner::Primary),
            FILTER644_PRIMARY,
        ),
        ("named", filter644_program(named("Ref")), FILTER644_NAMED),
        ("order", filter644_order_program(), FILTER644_ORDER),
        ("never", never, FILTER644_NEVER),
        ("predicate", predicate, FILTER644_PREDICATE),
        ("zero", filter644_nonbool_program(0), FILTER644_ZERO),
        (
            "maximum",
            filter644_nonbool_program(u32::MAX),
            FILTER644_MAXIMUM,
        ),
        (
            "lazy_true",
            filter644_lazy_program(true),
            FILTER644_LAZY_TRUE,
        ),
        (
            "lazy_false",
            filter644_lazy_program(false),
            FILTER644_LAZY_FALSE,
        ),
        ("nested", filter644_nested_program(), FILTER644_NESTED),
        ("plain", filter644_plain_program(false), FILTER644_PLAIN),
        (
            "identity",
            filter644_plain_program(true),
            FILTER644_IDENTITY,
        ),
        ("failure", filter644_failure_program(), FILTER644_FAILURE),
        ("modes", filter644_modes_program(), FILTER644_MODES),
    ]
}
pub(super) const FILTER644_HOST_COMMON: &str = r#"
use codegen_runtime::{Value, SourceOwnerRef, OwnedSourceError, OwnedSourceIdentity, field, group, repeated, scalar};
use std::error::Error;
fn fields(items:impl IntoIterator<Item=(&'static str,Instance)>)->Instance { group(items.into_iter().map(|(name,value)|field(name,value))) }
fn text(value:&str)->Instance { scalar(Value::String(value.into())) }
fn int(value:i64)->Instance { scalar(Value::Int(value)) }
fn row(value:i64)->Instance {
 let nested=repeated([
  fields([("Value",int(value*10+1)),("Flag",scalar(Value::Bool(true)))]),
  fields([("Value",int(value*10+2)),("Flag",scalar(Value::Bool(true)))]),
 ]);
 fields([("Value",int(value)),("Flag",scalar(Value::Bool(true))),("Rows",nested)])
}
fn input(root:&str,values:&[i64])->Instance { fields([("Value",text(root)),("Ref",fields([("Value",text("primary-collision"))])),("Rows",repeated(values.iter().copied().map(row)))]) }
fn limits()->OwnedSourceLimits { OwnedSourceLimits { max_roots:16,max_active_frames:8,max_path_segments:16,max_metadata_bytes:1048576,max_walk_contexts:64 } }
fn output(root:&str,values:&[i64])->Instance { repeated(values.iter().enumerate().map(|(index,value)|fields([("Value",int(*value)),("Position",int((index+1)as i64)),("Root",text(root))]))) }
fn capture(id:&str,p:&Instance,supplied:&[BasicNamedSourceInput<'_, '_>],selection:Option<TargetSelection<'_>>,policy:OwnedSourceLimits) { eprintln!("INPUT {id} primary={p:#?} supplied={supplied:#?} selection={selection:?} policy={policy:?}"); }
fn raw<T:std::fmt::Debug>(id:&str,actual:Result<T,RuntimeError>)->Result<T,RuntimeError> {
 eprintln!("RAW {id} FULL_RETURN={actual:#?}");
 if let Err(error)=&actual {
  eprintln!("RAW {id} DISPLAY={} CAUSE={:?} NEXT_CAUSE={:?}",error,error.source().map(ToString::to_string),error.source().and_then(|e|e.source()).map(ToString::to_string));
  match error {
   RuntimeError::OwnedSourceAtNode{node,source} => { eprintln!("RAW {id} ACTUAL_NODE={node} COMPLETE_TYPED_CAUSE={source:#?}"); assert!(std::ptr::eq(error.source().unwrap().downcast_ref::<OwnedSourceError>().unwrap(),source)); },
   RuntimeError::NotABool{node,..}|RuntimeError::MappingException{node,..} => { eprintln!("RAW {id} ACTUAL_NODE={node}"); assert!(error.source().is_none()); },
   _=>{}
  }
 }
 actual
}
fn all(id:&str,p:&Instance,supplied:&[BasicNamedSourceInput<'_, '_>])->Result<ExecutionOutputs,RuntimeError> { capture(id,p,supplied,None,limits());raw(id,execute_with_basic_owned_sources(p,supplied,None,limits())) }
fn selected(id:&str,p:&Instance,supplied:&[BasicNamedSourceInput<'_, '_>],selection:TargetSelection<'_>,policy:OwnedSourceLimits)->Result<SelectedTargetOutput,RuntimeError> { capture(id,p,supplied,Some(selection),policy);raw(id,execute_selected_with_basic_owned_sources(p,supplied,None,policy,selection)) }
"#;
const FILTER644_PRIMARY: &str = r#"
#[test] fn run() {
 let p=input("P.root",&[30,10,20]);let a=all("F644-G01",&p,&[]);
 assert_eq!(a,Ok(ExecutionOutputs{primary:output("P.root",&[10,20]),extras:vec![]}));
 let p=fields([("Value",text("P.root")),("Rows",repeated([row(30),scalar(Value::Null),row(10),row(20)]))]);let a=all("F644-G02",&p,&[]);
 assert_eq!(a,Ok(ExecutionOutputs{primary:output("P.root",&[10,20]),extras:vec![]}));
 let p=input("P.root",&[]);let a=all("F644-G03",&p,&[]);assert_eq!(a,Ok(ExecutionOutputs{primary:repeated([]),extras:vec![]}));
 let p=input("P.root",&[30]);let a=all("F644-G04",&p,&[]);assert_eq!(a,Ok(ExecutionOutputs{primary:repeated([]),extras:vec![]}));
 let p=fields([("Value",text("P.root"))]);let a=all("F644-G05",&p,&[]);assert_eq!(a,Ok(ExecutionOutputs{primary:repeated([]),extras:vec![]}));
 let mut zero=limits();zero.max_roots=0;let a=selected("F644-G06",&p,&[],TargetSelection::Named("unknown"),zero);
 assert_eq!(a,Err(RuntimeError::UnknownTarget{name:"unknown".into()}));
}
"#;
const FILTER644_NAMED: &str = r#"
#[test] fn run() {
 let p=input("P.root",&[30,10,20]);let n=input("N.root",&[3,1,2]);let supplied=[BasicNamedSourceInput{name:"Ref",instance:&n}];
 let a=all("F644-G07",&p,&supplied);assert_eq!(a,Ok(ExecutionOutputs{primary:output("N.root",&[1,2]),extras:vec![]}));
 let a=all("F644-G08",&p,&[]);assert_eq!(a,Err(RuntimeError::OwnedSource(OwnedSourceError::SourceUnavailable{name:"Ref".into()})));
}
"#;
const FILTER644_ORDER: &str = r#"
#[test] fn run() { let p=input("P.root",&[30,10]);let a=all("F644-G09",&p,&[]);assert_eq!(a,Err(RuntimeError::MappingException{node:41,message:Some("target-first".into())})); }
"#;
const FILTER644_NEVER: &str = r#"
#[test] fn run() { let p=input("P.root",&[30,10]);let a=all("F644-G10",&p,&[]);assert_eq!(a,Ok(ExecutionOutputs{primary:repeated([]),extras:vec![]})); }
"#;
const FILTER644_PREDICATE: &str = r#"
#[test] fn run() { let p=input("P.root",&[30,10]);let a=all("F644-G11",&p,&[]);assert_eq!(a,Err(RuntimeError::MappingException{node:42,message:Some("predicate-later".into())})); }
"#;
const FILTER644_ZERO: &str = r#"
#[test] fn run() { let p=input("P.root",&[30]);let a=all("F644-G12",&p,&[]);assert_eq!(a,Err(RuntimeError::NotABool{node:0,found:"int"})); }
"#;
const FILTER644_MAXIMUM: &str = r#"
#[test] fn run() { let p=input("P.root",&[30]);let a=all("F644-G13",&p,&[]);assert_eq!(a,Err(RuntimeError::NotABool{node:u32::MAX,found:"int"})); }
"#;
const FILTER644_LAZY_TRUE: &str = r#"
#[test] fn run() { let p=input("P.root",&[30]);let a=all("F644-G14",&p,&[]);assert_eq!(a,Ok(ExecutionOutputs{primary:output("P.root",&[30]),extras:vec![]})); }
"#;
const FILTER644_LAZY_FALSE: &str = r#"
#[test] fn run() { let p=input("P.root",&[30]);let a=all("F644-G15",&p,&[]);assert_eq!(a,Ok(ExecutionOutputs{primary:repeated([]),extras:vec![]})); }
"#;
const FILTER644_NESTED: &str = r#"
#[test] fn run() {
 let p=input("P.root",&[30,10,20]);let a=all("F644-G16",&p,&[]);
 let expected=repeated([10,20].into_iter().enumerate().map(|(i,value)|{
  let child=repeated([fields([("Value",int(value*10+2)),("Position",int(1)),("ParentPosition",int((i+1)as i64))])]);
  fields([("Value",int(value)),("Position",int((i+1)as i64)),("Root",text("P.root")),("Child",child)])
 }));
 assert_eq!(a,Ok(ExecutionOutputs{primary:expected,extras:vec![]}));
}
"#;
const FILTER644_PLAIN: &str = r#"
#[test] fn run() { let p=fields([("Value",text("P.root")),("Rows",row(50))]);let a=all("F644-G17",&p,&[]);assert_eq!(a,Err(RuntimeError::OwnedSourceAtNode{node:2,source:OwnedSourceError::InactiveSourceFrame{owner:OwnedSourceIdentity::Primary,collection:Some(vec!["Rows".into()])}})); }
"#;
const FILTER644_IDENTITY: &str = r#"
#[test] fn run() { let p=input("P.root",&[30]);let a=all("F644-G18",&p,&[]);assert_eq!(a,Err(RuntimeError::OwnedSourceAtNode{node:2,source:OwnedSourceError::InactiveSourceFrame{owner:OwnedSourceIdentity::Primary,collection:None}})); }
"#;
const FILTER644_FAILURE: &str = r#"
#[test] fn run() { let p=input("P.root",&[30,10]);let a=all("F644-G19",&p,&[]);assert_eq!(a,Err(RuntimeError::MappingFailure{rule:1,message:Some("rule-first".into())})); }
"#;
const FILTER644_MODES: &str = r#"
#[test] fn run() {
 let p=input("P.root",&[30,10,20]);let n=input("N.root",&[3,1,2]);let supplied=[BasicNamedSourceInput{name:"Ref",instance:&n}];
 let a=all("F644-G20",&p,&supplied);assert_eq!(a,Ok(ExecutionOutputs{primary:output("P.root",&[10,20]),extras:vec![NamedOutput{name:"Named".into(),instance:output("N.root",&[1,2])}]}));
 let a=selected("F644-G21",&p,&[],TargetSelection::Primary,limits());assert_eq!(a,Ok(SelectedTargetOutput::Primary(output("P.root",&[10,20]))));
 let a=selected("F644-G22",&p,&supplied,TargetSelection::Named("Named"),limits());assert_eq!(a,Ok(SelectedTargetOutput::Named(NamedOutput{name:"Named".into(),instance:output("N.root",&[1,2])})));
 let a=selected("F644-G23",&p,&[],TargetSelection::Named("Named"),limits());assert_eq!(a,Err(RuntimeError::OwnedSource(OwnedSourceError::SourceUnavailable{name:"Ref".into()})));
 let a=all("F644-G24",&p,&[]);assert_eq!(a,Err(RuntimeError::OwnedSource(OwnedSourceError::SourceUnavailable{name:"Ref".into()})));
}
"#;

#[test]
#[ignore = "builds and runs generated Rust consumers with cargo"]
fn public_basic_filter_generated_fourteen_programs_retain_all_twenty_four_results() {
    let run = HostRun::new(
        "FERRULE_PUBLIC_RUST_FILTER644_EVIDENCE_ROOT",
        "public_rust_filter644",
    );
    let (root, runtime) = (run.root.clone(), run.runtime.clone());
    assert!(root.is_absolute() && runtime.is_absolute());
    fs::create_dir(&root).expect("fresh namespace; retain failures");
    fs::create_dir(root.join("src")).unwrap();
    fs::create_dir(root.join("tests")).unwrap();
    let mut libraries = String::new();
    let mut checks = String::new();
    for (name, program, check) in filter644_host_programs() {
        let options = public519_options("ferrule_public_filters_644");
        retained_file(
            &root.join(format!("{name}.Program.original")),
            format!("{program:#?}\n").as_bytes(),
        );
        retained_file(
            &root.join(format!("{name}.Options.original")),
            format!("{options:#?}\n").as_bytes(),
        );
        let actual = filter644_result(name, &program, &options);
        retained_file(
            &root.join(format!("{name}.renderer-result.original")),
            format!("{actual:#?}\n").as_bytes(),
        );
        let files = actual.expect("full inputs/artifacts/error retained before qualification");
        let source = public519_source(&files);
        retained_file(
            &root.join(format!("{name}.generated.original.rs")),
            source.as_bytes(),
        );
        libraries.push_str(&format!("pub mod {name} {{\n{source}\n}}\n"));
        checks.push_str(&format!("mod {name} {{\nuse ferrule_public_filters_644::{name}::*;\n{FILTER644_HOST_COMMON}\n{check}\n}}\n"));
    }
    retained_file(&root.join("src/lib.rs"), libraries.as_bytes());
    retained_file(&root.join("tests/filters.rs"), checks.as_bytes());
    let options = Options {
        package_name: "ferrule_public_filters_644".into(),
        runtime_dependency: RuntimeDependency::Path(
            runtime
                .to_str()
                .expect("Unicode current runtime path")
                .into(),
        ),
    };
    retained_file(
        &root.join("Cargo.toml"),
        render_manifest(&options).as_bytes(),
    );
    retained_file(
        &root.join("COMMAND.original"),
        b"cargo test --offline --jobs 1 --test filters -- --nocapture --test-threads=1\n",
    );
    let actual = Command::new("cargo")
        .args([
            "test",
            "--offline",
            "--jobs",
            "1",
            "--test",
            "filters",
            "--",
            "--nocapture",
            "--test-threads=1",
        ])
        .current_dir(&root)
        .generated_host_output(&root);
    match &actual {
        Ok(output) => {
            retained_file(&root.join("stdout.complete.original"), &output.stdout);
            retained_file(&root.join("stderr.complete.original"), &output.stderr);
            retained_file(
                &root.join("status.complete.original"),
                format!("{:?}\n", output.status).as_bytes(),
            );
        }
        Err(error) => retained_file(
            &root.join("call-error.complete.original"),
            format!("{error:#?}\n").as_bytes(),
        ),
    }
    eprintln!("RAW F644 complete compiler/host return={actual:#?}");
    assert!(
        actual
            .expect("complete reached output or call error retained")
            .status
            .success()
    );
}

// Separate explicit sorting source and finite generated-consumer fixtures.
