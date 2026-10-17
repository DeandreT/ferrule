use super::*;

fn row_program() -> Program {
    let mut p = filter644_program(SourceOwner::Primary);
    p.expressions.extend([
        node(
            10,
            Expression::Const {
                value: Value::String("ok".into()),
            },
        ),
        node(11, owned(named("Ref"), &["Value"], Some(&[]))),
        node(
            12,
            Expression::Const {
                value: Value::Bool(true),
            },
        ),
        node(13, Expression::Raise { message: None }),
        node(
            14,
            Expression::If {
                condition: 12,
                then: 11,
                else_: 13,
            },
        ),
        node(
            15,
            Expression::Const {
                value: Value::String("other".into()),
            },
        ),
    ]);
    p.root = scope(&[("Value", 10, ScalarType::String)]);
    p.target = SchemaNode::group(
        "Output",
        vec![SchemaNode::scalar("Value", ScalarType::String)],
    );
    p.extra_targets.push(NamedTargetProgram {
        name: "Other".into(),
        target: p.target.clone(),
        root: scope(&[("Value", 15, ScalarType::String)]),
    });
    p.failure_rules = vec![codegen::FailureRule {
        iteration: codegen::FailureIteration::Source(codegen::SourceIteration::new(path(&[
            "Rows",
        ]))),
        selection: codegen::FailureSelection::WhenTrue(4),
        message: Some(1),
    }];
    p
}

fn row_artifacts(p: &Program) -> ArtifactSet {
    emit_with_basic_owned_row_failures(p, &public519_options("primary_row_failures")).unwrap()
}

#[test]
fn primary_row_failures_emission_keeps_old_gates_structure_and_public_entry_shapes() {
    let p = row_program();
    let options = public519_options("primary_row_failures");
    let expected = codegen::validate_program(&p).unwrap_err();
    for emit in [
        super::emit,
        emit_with_basic_owned_sources,
        emit_with_basic_owned_filters,
        emit_with_basic_owned_sort,
        emit_with_basic_owned_windows,
    ] {
        filter644_invalid(emit(&p, &options), expected.clone());
    }
    let files = row_artifacts(&p);
    let source = public519_source(&files);
    assert!(source.contains(
        "context.visit_primary_failure_rows(&[\"Rows\"], |item_context| -> Result<(), RuntimeError>"
    ));
    assert!(source.contains("let predicate = expression_4(item_context)?;"));
    assert!(source.contains("let message = Some(expression_1(item_context)?);"));
    assert!(source.contains(".with_owned_source_node(1)"));
    assert!(source.contains("pub fn execute_with_basic_owned_sources<'name, 'src>"));
    assert!(source.contains("pub fn execute_selected_with_basic_owned_sources<'name, 'src>"));
    let mut bad = p;
    bad.expressions.push(node(
        1,
        Expression::Const {
            value: Value::Int(9),
        },
    ));
    let mut options = options;
    options.package_name = "0bad".into();
    filter644_invalid(
        emit_with_basic_owned_row_failures(&bad, &options),
        ProgramValidationError::DuplicateExpression { node: 1 },
    );
}

#[test]
fn primary_row_failures_companion_preserves_full_window_artifacts_for_old_admitted_models() {
    for (_, p, _) in filter644_host_programs()
        .into_iter()
        .chain(sort668_host_programs())
    {
        let options = public519_options("unchanged_profiles");
        assert_eq!(
            emit_with_basic_owned_row_failures(&p, &options).unwrap(),
            emit_with_basic_owned_windows(&p, &options).unwrap()
        );
    }
    let mut p = filter644_program(SourceOwner::Primary);
    p.failure_rules.push(codegen::FailureRule {
        iteration: codegen::FailureIteration::Source(codegen::SourceIteration::new(path(&[
            "Rows",
        ]))),
        selection: codegen::FailureSelection::WhenTrue(12),
        message: Some(13),
    });
    p.expressions.extend([
        node(
            12,
            Expression::Const {
                value: Value::Bool(false),
            },
        ),
        node(
            13,
            Expression::Const {
                value: Value::String("unused".into()),
            },
        ),
    ]);
    let options = public519_options("unchanged_ordinary_failure");
    assert_eq!(
        emit_with_basic_owned_row_failures(&p, &options).unwrap(),
        emit_with_basic_owned_windows(&p, &options).unwrap()
    );
}

fn legacy_target_program() -> Program {
    let mut p = row_program();
    p.expressions.extend([
        node(
            30,
            Expression::SourceField {
                path: path(&["Value"]),
                frame: None,
            },
        ),
        node(31, Expression::Position { collection: vec![] }),
        node(
            32,
            Expression::Const {
                value: Value::Bool(false),
            },
        ),
    ]);
    p.failure_rules[0].selection = codegen::FailureSelection::WhenTrue(32);
    let mut child = scope(&[("Position", 31, ScalarType::Int)]);
    child.target_field = "Legacy".into();
    child.iteration = Some(IterationPlan::new(
        codegen::SourceIteration::new(vec![]),
        None,
        None,
        vec![],
        IterationOutput::First,
    ));
    p.root = scope(&[
        ("Value", 30, ScalarType::Int),
        ("Position", 31, ScalarType::Int),
    ]);
    p.root.repeating = true;
    p.root.children.push(child);
    p.root.iteration = Some(IterationPlan::source(path(&["Rows"])));
    p.target = SchemaNode::group(
        "Output",
        vec![
            SchemaNode::scalar("Value", ScalarType::Int),
            SchemaNode::scalar("Position", ScalarType::Int),
            SchemaNode::group(
                "Legacy",
                vec![SchemaNode::scalar("Position", ScalarType::Int)],
            ),
        ],
    )
    .repeating();
    p
}

#[test]
fn primary_row_failures_do_not_contaminate_unrelated_legacy_target_demand() {
    let files = row_artifacts(&legacy_target_program());
    let source = public519_source(&files);
    assert!(source.contains("fn legacy_scope_root(context: &ScopeContext<'_>)"));
    assert!(source.contains("fn legacy_scope_root_0(context: &ScopeContext<'_>)"));
    assert!(source.contains("fn expression_1(context: &BasicScopeContext<'_>)"));
    assert!(!source.contains("fn legacy_expression_1("));
}

fn window_target_program(order: SortFilterOrder) -> Program {
    let mut p = row_program();
    let target = filter644_program(SourceOwner::Primary);
    p.root = target.root;
    p.target = target.target;
    p.expressions.extend([
        node(
            31,
            Expression::Const {
                value: Value::Int(2),
            },
        ),
        node(
            33,
            owned(SourceOwner::Primary, &["Rows", "Flag"], Some(&["Rows"])),
        ),
    ]);
    p.failure_rules[0].selection = codegen::FailureSelection::WhenTrue(33);
    let input = p.root.iteration.as_ref().unwrap().input().clone();
    p.root.iteration = Some(IterationPlan::new(
        input,
        Some(4),
        Some(codegen::SortPlan::new(
            codegen::SortKey {
                expression: 1,
                descending: false,
            },
            vec![],
            order,
        )),
        vec![
            SequenceWindow::SkipFirst { count: 3 },
            SequenceWindow::Last { count: 31 },
            SequenceWindow::First { count: 3 },
        ],
        IterationOutput::Repeated,
    ));
    p
}

fn host_programs() -> Vec<(&'static str, Program, &'static str)> {
    let base = row_program();
    let mut named = base.clone();
    named.failure_rules[0].message = Some(14);
    let mut ordered = base.clone();
    ordered.failure_rules.push(codegen::FailureRule {
        iteration: codegen::FailureIteration::Source(codegen::SourceIteration::new(vec![])),
        selection: codegen::FailureSelection::All,
        message: Some(10),
    });
    ordered.root.bindings[0].expression = 13;
    ordered.extra_targets[0].root.bindings[0].expression = 13;
    let mut fallback = base.clone();
    fallback.extra_sources.push(NamedSourceProgram {
        name: "Rows".into(),
        source: filter644_schema(),
        dynamic: None,
    });
    vec![
        (
            "rows",
            base,
            r#"
#[test] fn run() {
 let p=input("root",&[30,10,20]);
 let expected=RuntimeError::MappingFailure{rule:1,message:Some("10".into())};
 assert_eq!(all("rows-all",&p,&[]),Err(expected));
 assert_eq!(selected("rows-primary",&p,&[],TargetSelection::Primary,limits()),Err(RuntimeError::MappingFailure{rule:1,message:Some("10".into())}));
 assert_eq!(selected("rows-named",&p,&[],TargetSelection::Named("Other"),limits()),Err(RuntimeError::MappingFailure{rule:1,message:Some("10".into())}));
 for values in [&[][..],&[30][..]] {
  let p=input("root",values);
  assert_eq!(all("success",&p,&[]),Ok(ExecutionOutputs{primary:fields([("Value",text("ok"))]),extras:vec![NamedOutput{name:"Other".into(),instance:fields([("Value",text("other"))])}]}));
 }
 let p=fields([("Rows",repeated([row(30),row(10),Instance::DocumentSet(vec![])]))]);
 assert_eq!(all("later-specialized",&p,&[]),Err(RuntimeError::MappingFailure{rule:1,message:Some("10".into())}));
 let p=fields([("Rows",repeated([Instance::DocumentSet(vec![]),row(10)]))]);
 assert_eq!(all("reached-specialized",&p,&[]),Err(RuntimeError::BasicSourceBridge(codegen_runtime::BasicSourceBridgeError::UnsupportedBasicContext{kind:codegen_runtime::InstanceKind::DocumentSet})));
 let p=fields([("Rows",repeated([scalar(Value::Null),row(10)]))]);
 assert_eq!(all("null-slot",&p,&[]),Err(RuntimeError::MappingFailure{rule:1,message:Some("10".into())}));
 let p=fields([("Rows",row(10))]);
 assert_eq!(all("singleton-no-position",&p,&[]),Err(RuntimeError::OwnedSourceAtNode{node:2,source:OwnedSourceError::InactiveSourceFrame{owner:OwnedSourceIdentity::Primary,collection:Some(vec!["Rows".into()])}}));
 let p=input("root",&[30]); let mut zero=limits(); zero.max_roots=0;
 assert_eq!(selected("unknown-first",&p,&[],TargetSelection::Named("Absent"),zero),Err(RuntimeError::UnknownTarget{name:"Absent".into()}));
}
"#,
        ),
        (
            "named_root",
            named,
            r#"
#[test] fn run() {
 let p=input("primary shadow",&[30]);
 assert_eq!(all("lazy-message",&p,&[]),Ok(ExecutionOutputs{primary:fields([("Value",text("ok"))]),extras:vec![NamedOutput{name:"Other".into(),instance:fields([("Value",text("other"))])}]}));
 let p=input("primary shadow",&[30,10]); let n=input("named immutable",&[]); let supplied=[BasicNamedSourceInput{name:"Ref",instance:&n}];
 assert_eq!(all("exact-owner",&p,&supplied),Err(RuntimeError::MappingFailure{rule:1,message:Some("named immutable".into())}));
 assert_eq!(all("missing-owner",&p,&[]),Err(RuntimeError::OwnedSourceAtNode{node:11,source:OwnedSourceError::SourceUnavailable{name:"Ref".into()}}));
 let n=text("malformed named root"); let supplied=[BasicNamedSourceInput{name:"Ref",instance:&n}];
  assert_eq!(all("typed-cause",&p,&supplied),Err(RuntimeError::OwnedSourceAtNode{node:11,source:OwnedSourceError::Traversal{owner:OwnedSourceIdentity::Named("Ref".into()),source:codegen_runtime::SourcePathError::CannotTraverse{path:vec!["Value".into()],segment:0,found:codegen_runtime::InstanceKind::Scalar}}}));
}
"#,
        ),
        (
            "ordered",
            ordered,
            r#"
#[test] fn run() {
 let p=input("root",&[30,10]); assert_eq!(all("first-stored",&p,&[]),Err(RuntimeError::MappingFailure{rule:1,message:Some("10".into())}));
 let p=input("root",&[30]); assert_eq!(all("ordinary-next",&p,&[]),Err(RuntimeError::MappingFailure{rule:2,message:Some("ok".into())}));
}
"#,
        ),
        (
            "fallback",
            fallback,
            r#"
#[test] fn run() {
 let p=fields([("Value",text("no rows"))]); let supplied=[BasicNamedSourceInput{name:"Rows",instance:&p}];
 assert_eq!(all("aliased-named-fallback",&p,&supplied),Err(RuntimeError::BasicSourceBridge(codegen_runtime::BasicSourceBridgeError::UnsupportedPrimaryFailureContext)));
}
"#,
        ),
        (
            "legacy_target",
            legacy_target_program(),
            r#"
#[test] fn run() {
 let p=input("root",&[30,10]);
 let expected=repeated([(30,1),(10,2)].into_iter().map(|(value,position)|fields([("Value",int(value)),("Position",int(position)),("Legacy",fields([("Position",int(1))]))])));
 assert_eq!(all("unrelated-legacy",&p,&[]),Ok(ExecutionOutputs{primary:expected.clone(),extras:vec![NamedOutput{name:"Other".into(),instance:fields([("Value",text("other"))])}]}));
 assert_eq!(selected("unrelated-legacy-primary",&p,&[],TargetSelection::Primary,limits()),Ok(SelectedTargetOutput::Primary(expected)));
}
"#,
        ),
        (
            "window_target_filter_sort",
            window_target_program(SortFilterOrder::FilterThenSort),
            WINDOW_TARGET_FILTER_SORT,
        ),
        (
            "window_target_sort_filter",
            window_target_program(SortFilterOrder::SortThenFilter),
            WINDOW_TARGET_SORT_FILTER,
        ),
    ]
}

const WINDOW_TARGET_FILTER_SORT: &str = r#"
fn window_input(fail:bool)->Instance {
 fields([("Value",text("root")),("Rows",repeated([(50,fail),(30,false),(10,false),(20,false),(40,false)].into_iter().map(|(value,flag)|fields([("Value",int(value)),("Flag",scalar(Value::Bool(flag)))]))))])
}
#[test] fn run() {
 let p=window_input(false); let expected=output("root",&[30]);
 assert_eq!(all("window-filter-sort-all",&p,&[]),Ok(ExecutionOutputs{primary:expected.clone(),extras:vec![NamedOutput{name:"Other".into(),instance:fields([("Value",text("other"))])}]}));
 assert_eq!(selected("window-filter-sort-primary",&p,&[],TargetSelection::Primary,limits()),Ok(SelectedTargetOutput::Primary(expected)));
 let p=window_input(true);
 assert_eq!(all("window-filter-sort-preflight",&p,&[]),Err(RuntimeError::MappingFailure{rule:1,message:Some("50".into())}));
}
"#;

const WINDOW_TARGET_SORT_FILTER: &str = r#"
fn window_input(fail:bool)->Instance {
 fields([("Value",text("root")),("Rows",repeated([(50,fail),(30,false),(10,false),(20,false),(40,false)].into_iter().map(|(value,flag)|fields([("Value",int(value)),("Flag",scalar(Value::Bool(flag)))]))))])
}
#[test] fn run() {
 let p=window_input(false); let expected=output("root",&[40]);
 assert_eq!(all("window-sort-filter-all",&p,&[]),Ok(ExecutionOutputs{primary:expected.clone(),extras:vec![NamedOutput{name:"Other".into(),instance:fields([("Value",text("other"))])}]}));
 assert_eq!(selected("window-sort-filter-primary",&p,&[],TargetSelection::Primary,limits()),Ok(SelectedTargetOutput::Primary(expected)));
 let p=window_input(true);
 assert_eq!(all("window-sort-filter-preflight",&p,&[]),Err(RuntimeError::MappingFailure{rule:1,message:Some("50".into())}));
}
"#;

#[test]
fn primary_row_failures_generated_models_are_independently_admitted() {
    assert_eq!(host_programs().len(), 7);
    for (_, p, _) in host_programs() {
        row_artifacts(&p);
    }
}

#[test]
fn primary_row_failures_generated_public_consumers_preserve_full_contracts() {
    let output = TempDir::new("primary_row_failures_809");
    let runtime = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("codegen-runtime");
    let options = Options {
        package_name: "ferrule_primary_row_failures".into(),
        runtime_dependency: RuntimeDependency::Path(runtime.to_str().unwrap().into()),
    };
    let mut libraries = String::from("#![deny(warnings)]\n");
    let mut checks = String::new();
    for (name, p, check) in host_programs() {
        let files = emit_with_basic_owned_row_failures(&p, &options).unwrap();
        libraries.push_str(&format!(
            "pub mod {name} {{\n{}\n}}\n",
            public519_source(&files)
        ));
        checks.push_str(&format!("mod {name} {{\nuse ferrule_primary_row_failures::{name}::*;\n{FILTER644_HOST_COMMON}\n{check}\n}}\n"));
    }
    fs::create_dir_all(output.path().join("src")).unwrap();
    fs::create_dir_all(output.path().join("tests")).unwrap();
    fs::write(output.path().join("Cargo.toml"), render_manifest(&options)).unwrap();
    fs::write(output.path().join("src/lib.rs"), libraries).unwrap();
    fs::write(output.path().join("tests/rows.rs"), checks).unwrap();
    let result = Command::new("cargo")
        .args([
            "test",
            "--offline",
            "--jobs",
            "1",
            "--test",
            "rows",
            "--",
            "--test-threads=1",
        ])
        .current_dir(output.path())
        .generated_host_output(output.path())
        .unwrap();
    assert!(
        result.status.success(),
        "generated primary row failures failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
}
