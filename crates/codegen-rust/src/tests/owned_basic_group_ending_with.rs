//! Ending companion fixtures over the Starting scaffold, plus one compiled
//! public consumer with literal expected products.

use super::*;

fn ending(owner: SourceOwner) -> mapping::Project {
    let mut p = project(owner);
    configure_groups(&mut p, |scope| {
        scope.group_starting_with = None;
        scope.group_ending_with = Some(101);
    });
    p
}

fn ending_lower(project: &mapping::Project) -> Program {
    codegen::lower_with_basic_owned_group_ending_with(project).unwrap()
}

fn ending_options() -> Options {
    let mut options = super::options();
    options.package_name = "owned_ending_host".into();
    options
}

fn ending_artifacts(project: &mapping::Project) -> ArtifactSet {
    emit_with_basic_owned_group_ending_with(&ending_lower(project), &ending_options()).unwrap()
}

#[test]
fn ending_has_only_the_new_companion_and_original_structural_precedence() {
    let program = ending_lower(&ending(SourceOwner::Primary));
    for old in [
        emit,
        emit_with_basic_owned_sources,
        emit_with_basic_owned_filters,
        emit_with_basic_owned_sort,
        emit_with_basic_owned_windows,
        emit_with_basic_owned_row_failures,
        emit_with_basic_owned_group_by,
        emit_with_basic_owned_sequence_item_at,
        emit_with_basic_owned_group_adjacent_by,
        emit_with_basic_owned_group_starting_with,
    ] {
        let Err(EmitError::InvalidProgram(error)) = old(&program, &ending_options()) else {
            panic!("older profile admitted owned EndingWith");
        };
        assert_eq!(error, grouping_refusal());
    }
    let mut invalid = ending_options();
    invalid.package_name = "0invalid".into();
    assert!(matches!(
        emit_with_basic_owned_group_ending_with(&program, &invalid),
        Err(EmitError::InvalidPackageName(_))
    ));
    let mut missing = program.clone();
    missing.root.children[0].iteration = Some(
        missing.root.children[0]
            .iteration
            .clone()
            .unwrap()
            .with_grouping(GroupingPlan::EndingWith { predicate: 1000 }),
    );
    let Err(EmitError::InvalidProgram(error)) =
        emit_with_basic_owned_group_ending_with(&missing, &invalid)
    else {
        panic!("missing predicate");
    };
    assert_eq!(
        error,
        ProgramValidationError::MissingGroupingExpression {
            target_path: path(&["Groups"]),
            expression: 1000,
            role: codegen::GroupingExpressionRole::EndingPredicate,
        }
    );
    let mut blocks = program.clone();
    blocks.expressions.push(codegen::ExpressionNode {
        id: 103,
        expression: Expression::Const {
            value: Value::Int(2),
        },
    });
    blocks.root.children[0].iteration = Some(
        blocks.root.children[0]
            .iteration
            .clone()
            .unwrap()
            .with_grouping(GroupingPlan::IntoBlocks { size: 103 }),
    );
    let Err(EmitError::InvalidProgram(error)) =
        emit_with_basic_owned_group_ending_with(&blocks, &ending_options())
    else {
        panic!("closed fixed blocks");
    };
    assert_eq!(error, grouping_refusal());
}

#[test]
fn ending_boolean_controls_keep_predicate_parent_and_member_roles() {
    let mut p = ending(SourceOwner::Primary);
    configure_groups(&mut p, |scope| {
        scope.sort_by = Some(6);
        scope.filter = Some(12);
        scope.post_group_filter = Some(5);
        scope.windows = vec![mapping::SequenceWindow::FromTo { first: 8, last: 7 }];
    });
    let files = ending_artifacts(&p);
    let source = generated_source(&files);
    assert_eq!(files.len(), 2);
    assert_eq!(
        source
            .matches(
                "impl codegen_runtime::BasicGroupEndingWithControls<RuntimeError> for Controls"
            )
            .count(),
        2
    );
    assert!(source.contains("fn ends_group(&mut self, boundary_context: &BasicScopeContext<'_>) -> Result<bool, RuntimeError> {\n            let value = expression_101(boundary_context)?;\n            require_bool(101, value)\n        }"));
    assert_eq!(
        source
            .matches("context.map_group_ending_with_source_cohort(")
            .count(),
        2
    );
    assert!(!source.contains("fn group_key("));
    assert!(!source.contains("fn starts_group("));
    assert!(source.contains("expression_5(member_context)?"));
    assert!(source.contains("expression_12(predicate_context)?"));
    assert!(source.contains("0 => expression_6(key_context)"));
    assert!(
        source.find("expression_8(parent_context)?").unwrap()
            < source.find("expression_7(parent_context)?").unwrap()
    );
}

#[test]
fn ending_composition_conserves_prior_by_adjacent_and_starting_artifacts() {
    for mode in 0..3 {
        let mut p = super::super::owned_basic_group_by::project(SourceOwner::Primary);
        if mode == 1 {
            configure_groups(&mut p, |scope| {
                scope.group_adjacent_by = scope.group_by.take()
            });
        }
        if mode == 2 {
            p = project(SourceOwner::Primary);
        }
        let program = ending_lower(&p);
        assert_eq!(
            emit_with_basic_owned_group_ending_with(&program, &ending_options()).unwrap(),
            emit_with_basic_owned_group_starting_with(&program, &ending_options()).unwrap()
        );
    }
}

#[test]
fn ending_literal_public_consumer_keeps_trailing_groups_and_typed_errors() {
    let directory = TempDir::new("owned_ending_consumer");
    let mut plain = ending(SourceOwner::Primary);
    plain.extra_sources.clear();
    let mut post = plain.clone();
    configure_groups(&mut post, |scope| scope.post_group_filter = Some(5));
    let mut nonbool = plain.clone();
    nonbool.graph.nodes.insert(
        101,
        Node::Const {
            value: Value::String("true".into()),
        },
    );
    let mut library = String::from("#![deny(warnings)]\n");
    for (name, p) in [("plain", plain), ("post", post), ("nonbool", nonbool)] {
        library.push_str(&format!(
            "pub mod {name} {{\n{}\n}}\n",
            generated_source(&ending_artifacts(&p))
        ));
    }
    fs::create_dir_all(directory.path().join("src")).unwrap();
    fs::create_dir_all(directory.path().join("tests")).unwrap();
    fs::write(
        directory.path().join("Cargo.toml"),
        render_manifest(&ending_options()),
    )
    .unwrap();
    fs::write(directory.path().join("src/lib.rs"), library).unwrap();
    fs::write(directory.path().join("tests/consumer.rs"), ENDING_CONSUMER).unwrap();
    let output = Command::new("cargo")
        .args(["test", "--offline", "--test", "consumer"])
        .env_remove("CARGO_ENCODED_RUSTFLAGS")
        .env("RUSTFLAGS", "-D warnings")
        .current_dir(directory.path())
        .generated_host_output(directory.path())
        .unwrap();
    assert!(
        output.status.success(),
        "Ending consumer failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

const ENDING_CONSUMER: &str = r#"
use owned_ending_host::*;
use codegen_runtime::{Instance,OwnedSourceLimits,RuntimeError,TargetSelection,Value,field,group,repeated,scalar};
fn text(v:&str)->Instance {scalar(Value::String(v.into()))}
fn fields(v:impl IntoIterator<Item=(&'static str,Instance)>)->Instance {group(v.into_iter().map(|(n,v)|field(n,v)))}
fn row(label:&str,start:bool,keep:bool)->Instance {fields([("Key",text("A")),("Label",text(label)),("Keep",scalar(Value::Bool(keep))),("Rank",scalar(Value::Int(0))),("Start",scalar(Value::Bool(start)))])}
fn input()->Instance {fields([("Marker",text("P.root")),("Limit",scalar(Value::Int(2))),("Rows",repeated([
    row("p0",false,false),row("p1",false,true),row("p2",true,false),row("p3",true,false),
    row("p4",false,true),row("p5",true,false),row("p6",false,true),
]))])}
fn g(first:&str,pos:i64,members:&[&str])->Instance {fields([
    ("First",text(first)),("GroupPosition",scalar(Value::Int(pos))),("RootMarker",text("P.root")),
    ("Members",repeated(members.iter().enumerate().map(|(i,label)|fields([("Label",text(label)),("MemberPosition",scalar(Value::Int(i as i64+1))),("RootMarker",text("P.root"))])))),
])}
fn out(v:impl IntoIterator<Item=Instance>)->Instance {fields([("Groups",repeated(v))])}
fn limits()->OwnedSourceLimits {OwnedSourceLimits{max_roots:8,max_active_frames:16,max_path_segments:64,max_metadata_bytes:1048576,max_walk_contexts:128}}
const AUDIT:&str=" Exact audit 雪 ";
macro_rules! check {($m:ident,$source:expr,$wanted:expr)=>{{let source=$source;let wanted=$wanted;
    assert_eq!($m::execute_with_basic_owned_sources(source,&[],None,limits()),
        Ok($m::ExecutionOutputs{primary:wanted.clone(),extras:vec![$m::NamedOutput{name:AUDIT.into(),instance:wanted.clone()}]}));
    assert_eq!($m::execute_selected_with_basic_owned_sources(source,&[],None,limits(),TargetSelection::Primary),
        Ok($m::SelectedTargetOutput::Primary(wanted.clone())));
    assert_eq!($m::execute_selected_with_basic_owned_sources(source,&[],None,limits(),TargetSelection::Named(AUDIT)),
        Ok($m::SelectedTargetOutput::Named($m::NamedOutput{name:AUDIT.into(),instance:wanted})));
}};}
#[test] fn trailing_group_and_member_retention() {let p=input();
    check!(plain,&p,out([g("p0",1,&["p0","p1","p2"]),g("p3",2,&["p3"]),g("p4",3,&["p4","p5"]),g("p6",4,&["p6"])]));
    check!(post,&p,out([g("p0",1,&["p0","p1","p2"]),g("p4",2,&["p4","p5"]),g("p6",3,&["p6"])]));
    let empty=fields([("Marker",text("P.root")),("Limit",scalar(Value::Int(2))),("Rows",repeated([]))]);check!(plain,&empty,out([]));
}
#[test] fn non_boolean_boundary_is_a_typed_error() {
    assert_eq!(nonbool::execute_with_basic_owned_sources(&input(),&[],None,limits()),
        Err(RuntimeError::NotABool{node:101,found:"string"}));
}
"#;
