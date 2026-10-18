//! Public AdjacentBy consumers with independently literal products and errors.
//! Reuse the genuine By fixture, not its bucket oracle or actual engine output.

use super::owned_basic_group_by::{configure_groups, generated_source};
use super::*;
use mapping::SourceOwner;

fn project(owner: SourceOwner) -> mapping::Project {
    adjacent(super::owned_basic_group_by::project(owner))
}

fn adjacent(mut project: mapping::Project) -> mapping::Project {
    configure_groups(&mut project, |scope| {
        scope.group_adjacent_by = scope.group_by.take();
    });
    project
}

fn lower(project: &mapping::Project) -> Program {
    let saved = serde_json::to_vec_pretty(project).unwrap();
    let restored: mapping::Project = serde_json::from_slice(&saved).unwrap();
    assert_eq!(serde_json::to_vec_pretty(&restored).unwrap(), saved);
    codegen::lower_with_basic_owned_group_adjacent_by(&restored).unwrap()
}

fn options() -> Options {
    let mut options = super::owned_basic_group_by::options();
    options.package_name = "owned_adjacent_host".into();
    options
}

fn grouping_refusal() -> ProgramValidationError {
    use codegen::{
        OwnedSourceCapabilityOrigin as Origin, OwnedSourceCapabilityReason as Reason,
        OwnedSourceDiagnostic as Diagnostic, OwnedSourceDiagnosticLocation as Location,
        OwnedSourceDiagnosticRequest as Request,
    };
    let route = mapping::SourceScopeLocation::root(mapping::SourceScopeTarget::Primary)
        .descendant(mapping::SourceScopeStep::Child(0));
    ProgramValidationError::OwnedSource {
        diagnostic: Box::new(Diagnostic::Capability {
            location: Location::Iteration {
                scope: route.clone(),
            },
            request: Some(Request::Iteration {
                owner: (&SourceOwner::Primary).into(),
                path: vec!["Rows".into()],
            }),
            reason: Reason::Grouping,
            origin: Origin::Scope { scope: route },
        }),
    }
}

#[test]
fn adjacent_companion_keeps_old_profiles_closed_and_structural_errors_first() {
    let program = lower(&project(SourceOwner::Primary));
    for old in [
        emit,
        emit_with_basic_owned_sources,
        emit_with_basic_owned_filters,
        emit_with_basic_owned_sort,
        emit_with_basic_owned_windows,
        emit_with_basic_owned_row_failures,
        emit_with_basic_owned_group_by,
        emit_with_basic_owned_sequence_item_at,
    ] {
        let Err(EmitError::InvalidProgram(error)) = old(&program, &options()) else {
            panic!("old profile admitted owned AdjacentBy");
        };
        assert_eq!(error, grouping_refusal());
    }
    let files = emit_with_basic_owned_group_adjacent_by(&program, &options()).unwrap();
    let source = generated_source(&files);
    assert_eq!(files.len(), 2);
    assert_eq!(
        source
            .matches("context.map_group_adjacent_by_source_cohort(")
            .count(),
        2
    );
    assert!(!source.contains("context.map_group_by_source_cohort("));
    assert!(source.contains("pub fn execute_with_basic_owned_sources<'name, 'src>("));
    assert!(source.contains("pub fn execute_selected_with_basic_owned_sources<'name, 'src>("));
    assert!(
        source.contains("execution: Option<&ExecutionContext<'src>>, limits: OwnedSourceLimits,")
    );
    assert!(!source.contains("pub fn execute_json"));
    assert!(!source.contains("OwnedSourceLimits::default"));
    let mut malformed = program.clone();
    malformed.root.children[0].iteration = Some(
        IterationPlan::owned_source(mapping::OwnedSourceRef {
            owner: SourceOwner::Primary,
            path: vec!["Rows".into()],
        })
        .with_grouping(GroupingPlan::AdjacentBy { key: 1000 }),
    );
    let before = format!("{malformed:#?}");
    let Err(EmitError::InvalidProgram(error)) =
        emit_with_basic_owned_group_adjacent_by(&malformed, &options())
    else {
        panic!("missing key");
    };
    assert_eq!(
        error,
        ProgramValidationError::MissingGroupingExpression {
            target_path: vec!["Groups".into()],
            expression: 1000,
            role: codegen::GroupingExpressionRole::AdjacentKey,
        }
    );
    assert_eq!(format!("{malformed:#?}"), before);
}

#[test]
fn adjacent_pair_preserves_by_itemat_bytes_and_dispatches_only_permitted_groups() {
    let by = super::owned_basic_group_by::project(SourceOwner::Primary);
    let program = lower(&by);
    assert_eq!(
        emit_with_basic_owned_group_adjacent_by(&program, &options()).unwrap(),
        emit_with_basic_owned_sequence_item_at(&program, &options()).unwrap()
    );
    for mode in 0..3 {
        let mut closed = project(SourceOwner::Primary);
        configure_groups(&mut closed, |scope| {
            scope.group_adjacent_by = None;
            match mode {
                0 => scope.group_starting_with = Some(5),
                1 => scope.group_ending_with = Some(5),
                _ => scope.group_into_blocks = Some(8),
            }
        });
        assert!(codegen::lower_with_basic_owned_group_adjacent_by(&closed).is_err());
        let mut direct = lower(&project(SourceOwner::Primary));
        // Lowering removes unused controls. Retain valid direct-Program
        // expressions so these probes reach the grouping capability gate.
        direct.expressions.extend([
            codegen::ExpressionNode {
                id: 5,
                expression: codegen::Expression::Const {
                    value: Value::Bool(true),
                },
            },
            codegen::ExpressionNode {
                id: 8,
                expression: codegen::Expression::Const {
                    value: Value::Int(2),
                },
            },
        ]);
        direct.root.children[0].iteration = Some(
            direct.root.children[0]
                .iteration
                .clone()
                .unwrap()
                .with_grouping(match mode {
                    0 => GroupingPlan::StartingWith { predicate: 5 },
                    1 => GroupingPlan::EndingWith { predicate: 5 },
                    _ => GroupingPlan::IntoBlocks { size: 8 },
                }),
        );
        let Err(EmitError::InvalidProgram(error)) =
            emit_with_basic_owned_group_adjacent_by(&direct, &options())
        else {
            panic!("closed mode");
        };
        assert_eq!(error, grouping_refusal());
    }
    for post_only in [false, true] {
        let p = adjacent(super::owned_basic_group_by::legacy_control_project(
            true, post_only,
        ));
        let files = emit_with_basic_owned_group_adjacent_by(&lower(&p), &options()).unwrap();
        let source = generated_source(&files);
        assert!(source.contains("fn scope_root_0_0(context: &BasicScopeContext<'_>)"));
        assert!(!source.contains("fn legacy_scope_root_0_0("));
        assert!(source.contains("context.map_group_adjacent_by_source_cohort("));
    }
    for mode in 0..5 {
        let mut ordinary = super::owned_basic_group_by::legacy_control_project(false, false);
        configure_groups(&mut ordinary, |scope| {
            scope.group_by = None;
            match mode {
                0 => scope.group_by = Some(1),
                1 => scope.group_adjacent_by = Some(1),
                2 => scope.group_starting_with = Some(5),
                3 => scope.group_ending_with = Some(5),
                _ => scope.group_into_blocks = Some(8),
            }
        });
        let program = lower(&ordinary);
        assert_eq!(
            emit_with_basic_owned_group_adjacent_by(&program, &options()).unwrap(),
            emit_with_basic_owned_sequence_item_at(&program, &options()).unwrap()
        );
    }
}

#[test]
fn adjacent_catalogues_keep_all_original_ordinals_and_global_dependencies() {
    use codegen::NamedInputAvailability::{
        Optional as O, RequiredLegacy as R, ReservedUnavailable as U,
    };
    fn rows<'a>(
        catalogue: &codegen::NamedInputCatalogue<'a>,
    ) -> Vec<(usize, &'a str, codegen::NamedInputAvailability)> {
        catalogue
            .entries
            .iter()
            .map(|entry| (entry.declaration_index, entry.name, entry.availability))
            .collect()
    }
    let mut p = project(SourceOwner::Primary);
    let mut unused = p.extra_sources[0].clone();
    unused.name = "unusedStatic".into();
    p.extra_sources.insert(0, unused.clone());
    unused.name = "unavailableDynamic".into();
    unused.dynamic_path = Some(mapping::DynamicSourcePath {
        node: 99,
        iteration: vec![],
    });
    p.extra_sources.push(unused);
    p.graph.nodes.insert(
        80,
        mapping::Node::SourceField {
            path: vec!["Rows".into(), "Marker".into()],
            frame: None,
        },
    );
    p.root.bindings.push(mapping::Binding {
        target_field: "Legacy".into(),
        node: 80,
    });
    let SchemaKind::Group { children, .. } = &mut p.target.kind else {
        unreachable!()
    };
    children.push(SchemaNode::scalar("Legacy", ScalarType::String));
    for global in [false, true] {
        if global {
            p.graph.nodes.insert(
                81,
                mapping::Node::Const {
                    value: Value::Bool(false),
                },
            );
            p.failure_rules.push(mapping::FailureRule {
                iteration: mapping::FailureIteration::Source { collection: vec![] },
                selection: mapping::FailureSelection::WhenTrue { predicate: 81 },
                message: Some(80),
            });
        }
        let program = lower(&p);
        let plan =
            codegen::legacy_named_input_admission_plan_with_basic_owned_group_adjacent_by(&program)
                .unwrap();
        let optional = vec![
            (0, "unusedStatic", O),
            (1, "Rows", O),
            (2, " Røws💠 ", O),
            (3, "unavailableDynamic", U),
        ];
        let required = vec![
            (0, "unusedStatic", O),
            (1, "Rows", R),
            (2, " Røws💠 ", O),
            (3, "unavailableDynamic", U),
        ];
        assert_eq!(rows(&plan.all_outputs), required);
        assert_eq!(rows(&plan.primary), required);
        assert_eq!(
            plan.named_targets
                .iter()
                .map(|target| (target.target_index, target.name, rows(&target.catalogue)))
                .collect::<Vec<_>>(),
            vec![(
                0,
                " Exact audit 雪 ",
                if global { required } else { optional }
            )]
        );
        let files = emit_with_basic_owned_group_adjacent_by(&program, &options()).unwrap();
        assert!(generated_source(&files).contains("PRIVATE_CHECKED_NAMED_INPUTS_0"));
    }
}

fn consumer_projects() -> Vec<(&'static str, mapping::Project)> {
    let primary = project(SourceOwner::Primary);
    let mut result = vec![
        ("primary", primary.clone()),
        (
            "named",
            project(SourceOwner::Named {
                name: "Rows".into(),
            }),
        ),
        (
            "exact",
            project(SourceOwner::Named {
                name: " Røws💠 ".into(),
            }),
        ),
    ];
    for (name, post_only) in [("legacy_key", false), ("legacy_post", true)] {
        let mut p = adjacent(super::owned_basic_group_by::legacy_control_project(
            true, post_only,
        ));
        // Both the all-output and exact selected API exercise inherited members.
        p.extra_targets.push(mapping::NamedTarget {
            name: " Exact audit 雪 ".into(),
            path: None,
            options: Default::default(),
            schema: p.target.clone(),
            root: p.root.clone(),
        });
        if !post_only {
            configure_groups(&mut p, |scope| scope.post_group_filter = Some(5));
        }
        result.push((name, p));
    }
    let mut post = primary.clone();
    configure_groups(&mut post, |scope| scope.post_group_filter = Some(5));
    result.push(("post", post.clone()));
    let mut sorted = primary.clone();
    configure_groups(&mut sorted, |scope| scope.sort_by = Some(6));
    result.push(("sorted", sorted.clone()));
    configure_groups(&mut sorted, |scope| {
        scope.windows = vec![mapping::SequenceWindow::First { count: 7 }]
    });
    result.push(("parent_bound", sorted));
    for (name, order) in [
        ("filter_before", mapping::SortFilterOrder::FilterThenSort),
        ("filter_after", mapping::SortFilterOrder::SortThenFilter),
    ] {
        let mut p = primary.clone();
        configure_groups(&mut p, |scope| {
            scope.sort_by = Some(6);
            scope.filter = Some(11);
            scope.sort_filter_order = order;
        });
        p.graph.nodes.insert(
            10,
            mapping::Node::Const {
                value: Value::Int(2),
            },
        );
        result.push((name, p));
    }
    let mut picked = primary.clone();
    picked.extra_sources.clear(); // Exercise an actual adjacent empty catalogue under -D warnings.
    picked.graph.nodes.extend([
        (
            40,
            mapping::Node::SequenceItemAt {
                sequence: mapping::SequenceExpr::Generate {
                    from: Some(41),
                    to: 42,
                    item: 90,
                },
                index: 3,
            },
        ),
        (
            41,
            mapping::Node::Const {
                value: Value::Int(10),
            },
        ),
        (
            42,
            mapping::Node::Const {
                value: Value::Int(12),
            },
        ),
        (
            90,
            mapping::Node::SourceField {
                path: vec![],
                frame: None,
            },
        ),
    ]);
    configure_groups(&mut picked, |scope| {
        scope.bindings.push(mapping::Binding {
            target_field: "Picked".into(),
            node: 40,
        });
        scope.children[0].bindings.push(mapping::Binding {
            target_field: "Picked".into(),
            node: 40,
        });
    });
    fn add_picked(schema: &mut SchemaNode) {
        let SchemaKind::Group { children, .. } = &mut schema.kind else {
            unreachable!()
        };
        let SchemaKind::Group { children, .. } = &mut children[0].kind else {
            unreachable!()
        };
        let member = children
            .iter_mut()
            .find(|child| child.name == "Members")
            .unwrap();
        let SchemaKind::Group {
            children: members, ..
        } = &mut member.kind
        else {
            unreachable!()
        };
        members.push(SchemaNode::scalar("Picked", ScalarType::Int));
        children.insert(3, SchemaNode::scalar("Picked", ScalarType::Int));
    }
    add_picked(&mut picked.target);
    for target in &mut picked.extra_targets {
        add_picked(&mut target.schema);
    }
    result.push(("picked", picked));
    let mut token_key = primary.clone();
    token_key.graph.nodes.extend([
        (
            40,
            mapping::Node::SequenceItemAt {
                sequence: mapping::SequenceExpr::Tokenize {
                    input: 41,
                    delimiter: 42,
                    item: 90,
                },
                index: 3,
            },
        ),
        (
            41,
            mapping::Node::Const {
                value: Value::String("A|B|A".into()),
            },
        ),
        (
            42,
            mapping::Node::Const {
                value: Value::String("|".into()),
            },
        ),
        (
            90,
            mapping::Node::SourceField {
                path: vec![],
                frame: None,
            },
        ),
    ]);
    configure_groups(&mut token_key, |scope| scope.group_adjacent_by = Some(40));
    result.push(("token_key", token_key));
    for (name, node, message) in [("key_error", 1, "key"), ("post_error", 5, "post")] {
        let mut p = post.clone();
        p.graph.nodes.insert(
            101,
            mapping::Node::Const {
                value: Value::String(message.into()),
            },
        );
        p.graph
            .nodes
            .insert(node, mapping::Node::Raise { message: Some(101) });
        configure_groups(&mut p, |scope| {
            scope.windows = vec![mapping::SequenceWindow::First { count: 9 }]
        });
        result.push((name, p));
    }
    let mut nonbool = post;
    nonbool.graph.nodes.insert(
        5,
        mapping::Node::Const {
            value: Value::String("not bool".into()),
        },
    );
    result.push(("nonbool", nonbool));
    let mut invalid_bound = primary.clone();
    invalid_bound.graph.nodes.insert(
        7,
        mapping::Node::Const {
            value: Value::Bool(true),
        },
    );
    configure_groups(&mut invalid_bound, |scope| {
        scope.windows = vec![mapping::SequenceWindow::First { count: 7 }]
    });
    result.push(("invalid_bound", invalid_bound));
    let mut failure = primary;
    failure
        .graph
        .nodes
        .insert(1, mapping::Node::Raise { message: None });
    failure.failure_rules.push(mapping::FailureRule {
        iteration: mapping::FailureIteration::Source {
            collection: vec!["Rows".into()],
        },
        selection: mapping::FailureSelection::WhenTrue { predicate: 5 },
        message: Some(2),
    });
    result.push(("failure", failure));
    result
}

#[test]
fn adjacent_public_consumers_compare_complete_literal_products_errors_and_empty_catalogues() {
    let directory = TempDir::new("owned_adjacent_consumer");
    let mut library = String::new();
    for (name, project) in consumer_projects() {
        fs::write(
            directory.path().join(format!("{name}.Project.json")),
            serde_json::to_vec_pretty(&project).unwrap(),
        )
        .unwrap();
        let program = lower(&project);
        let files = emit_with_basic_owned_group_adjacent_by(&program, &options()).unwrap();
        library.push_str(&format!(
            "pub mod {name} {{\n{}\n}}\n",
            generated_source(&files)
        ));
    }
    fs::create_dir_all(directory.path().join("src")).unwrap();
    fs::create_dir_all(directory.path().join("tests")).unwrap();
    fs::write(
        directory.path().join("Cargo.toml"),
        render_manifest(&options()),
    )
    .unwrap();
    fs::write(directory.path().join("src/lib.rs"), library).unwrap();
    fs::write(directory.path().join("tests/consumer.rs"), CONSUMER).unwrap();
    let output = Command::new("cargo")
        .args(["test", "--offline", "--test", "consumer"])
        .env_remove("CARGO_ENCODED_RUSTFLAGS")
        .env("RUSTFLAGS", "-D warnings")
        .current_dir(directory.path())
        .generated_host_output(directory.path())
        .unwrap();
    fs::write(directory.path().join("stdout.complete"), &output.stdout).unwrap();
    fs::write(directory.path().join("stderr.complete"), &output.stderr).unwrap();
    if !output.status.success() {
        let retained = directory.path().to_owned();
        std::mem::forget(directory);
        panic!(
            "adjacent consumer retained at {}:\n{}\n{}",
            retained.display(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    if std::env::var_os("FERRULE_CODEGEN_KEEP_ARTIFACTS").is_some() {
        std::mem::forget(directory);
    }
}

const CONSUMER: &str = r#"
use owned_adjacent_host::*;
use codegen_runtime::{BasicNamedSourceInput,Instance,OwnedSourceLimits,RuntimeError,TargetSelection,
    Value,field,group,repeated,scalar};
fn text(value:&str)->Instance {scalar(Value::String(value.into()))}
fn fields(values:impl IntoIterator<Item=(&'static str,Instance)>)->Instance {group(values.into_iter().map(|(name,value)|field(name,value)))}
fn row(key:&str,label:&str,keep:bool,rank:i64)->Instance {fields([("Key",text(key)),("Label",text(label)),("Keep",scalar(Value::Bool(keep))),("Rank",scalar(Value::Int(rank)))])}
fn input(marker:&str,prefix:&str)->Instance {fields([("Marker",text(marker)),("Limit",scalar(Value::Int(2))),("Rows",repeated([
    row("A",&format!("{prefix}-a1"),false,30),row("B",&format!("{prefix}-b1"),true,10),
    row("A",&format!("{prefix}-a2"),false,20),row("A",&format!("{prefix}-a3"),true,25),
    row("C",&format!("{prefix}-c1"),false,40),row("A",&format!("{prefix}-a4"),true,50),
]))])}
fn empty()->Instance {fields([("Marker",text("P.root")),("Limit",scalar(Value::Int(2))),("Rows",repeated([]))])}
// Only expand the listed complete fields and explicit ordinals: never regroup inputs.
fn g(first:&str,ordinal:i64,marker:&str,members:&[(&str,i64)])->Instance {fields([
    ("First",text(first)),("GroupPosition",scalar(Value::Int(ordinal))),("RootMarker",text(marker)),
    ("Members",repeated(members.iter().map(|(label,position)|fields([
        ("Label",text(label)),("MemberPosition",scalar(Value::Int(*position))),("RootMarker",text(marker)),
    ])))),
])}
fn out(groups:impl IntoIterator<Item=Instance>)->Instance {fields([("Groups",repeated(groups))])}
fn limits()->OwnedSourceLimits {OwnedSourceLimits {max_roots:8,max_active_frames:16,max_path_segments:64,max_metadata_bytes:1048576,max_walk_contexts:128}}
macro_rules! check {($module:ident,$p:expr,$supplied:expr,$wanted:expr)=>{{
    let p=$p;let supplied=$supplied;let wanted=$wanted;
    assert_eq!($module::execute_with_basic_owned_sources(p,supplied,None,limits()),Ok($module::ExecutionOutputs {primary:wanted.clone(),extras:vec![$module::NamedOutput {name:" Exact audit 雪 ".into(),instance:wanted.clone()}]}));
    assert_eq!($module::execute_selected_with_basic_owned_sources(p,supplied,None,limits(),TargetSelection::Primary),Ok($module::SelectedTargetOutput::Primary(wanted.clone())));
    assert_eq!($module::execute_selected_with_basic_owned_sources(p,supplied,None,limits(),TargetSelection::Named(" Exact audit 雪 ")),Ok($module::SelectedTargetOutput::Named($module::NamedOutput {name:" Exact audit 雪 ".into(),instance:wanted})));
}};}
macro_rules! failed {($module:ident,$p:expr,$wanted:expr)=>{{
    assert_eq!($module::execute_with_basic_owned_sources($p,&[],None,limits()),Err($wanted));
    assert_eq!($module::execute_selected_with_basic_owned_sources($p,&[],None,limits(),TargetSelection::Primary),Err($wanted));
    assert_eq!($module::execute_selected_with_basic_owned_sources($p,&[],None,limits(),TargetSelection::Named(" Exact audit 雪 ")),Err($wanted));
}};}
#[test] fn three_exact_owners_keep_later_equal_runs_separate() {
    let p=input("P.root","P");let n=input("N.root","N");let u=input("U.root","U");
    let supplied=[BasicNamedSourceInput{name:"Rows",instance:&n},BasicNamedSourceInput{name:" Røws💠 ",instance:&u}];
    check!(primary,&p,&supplied,out([
        g("P-a1",1,"P.root",&[("P-a1",1)]),g("P-b1",2,"P.root",&[("P-b1",1)]),
        g("P-a2",3,"P.root",&[("P-a2",1),("P-a3",2)]),g("P-c1",4,"P.root",&[("P-c1",1)]),g("P-a4",5,"P.root",&[("P-a4",1)]),
    ]));
    check!(named,&p,&supplied,out([
        g("N-a1",1,"N.root",&[("N-a1",1)]),g("N-b1",2,"N.root",&[("N-b1",1)]),
        g("N-a2",3,"N.root",&[("N-a2",1),("N-a3",2)]),g("N-c1",4,"N.root",&[("N-c1",1)]),g("N-a4",5,"N.root",&[("N-a4",1)]),
    ]));
    check!(exact,&p,&supplied,out([
        g("U-a1",1,"U.root",&[("U-a1",1)]),g("U-b1",2,"U.root",&[("U-b1",1)]),
        g("U-a2",3,"U.root",&[("U-a2",1),("U-a3",2)]),g("U-c1",4,"U.root",&[("U-c1",1)]),g("U-a4",5,"U.root",&[("U-a4",1)]),
    ]));
    let mut zero=limits();zero.max_roots=0;
    assert_eq!(named::execute_selected_with_basic_owned_sources(&p,&[],None,zero,TargetSelection::Named("Exact audit 雪")),Err(RuntimeError::UnknownTarget{name:"Exact audit 雪".into()}));
}
#[test] fn member_retention_sorting_parent_bounds_and_filter_orders_are_distinct() {
    let p=input("P.root","P");
    check!(post,&p,&[],out([g("P-b1",1,"P.root",&[("P-b1",1)]),g("P-a2",2,"P.root",&[("P-a2",1),("P-a3",2)]),g("P-a4",3,"P.root",&[("P-a4",1)])]));
    check!(sorted,&p,&[],out([g("P-b1",1,"P.root",&[("P-b1",1)]),g("P-a2",2,"P.root",&[("P-a2",1),("P-a3",2),("P-a1",3)]),g("P-c1",3,"P.root",&[("P-c1",1)]),g("P-a4",4,"P.root",&[("P-a4",1)])]));
    check!(parent_bound,&p,&[],out([g("P-b1",1,"P.root",&[("P-b1",1)]),g("P-a2",2,"P.root",&[("P-a2",1),("P-a3",2),("P-a1",3)])]));
    check!(filter_before,&p,&[],out([g("P-a2",1,"P.root",&[("P-a2",1),("P-a3",2)]),g("P-c1",2,"P.root",&[("P-c1",1)]),g("P-a4",3,"P.root",&[("P-a4",1)])]));
    check!(filter_after,&p,&[],out([g("P-a3",1,"P.root",&[("P-a3",1),("P-a1",2)]),g("P-c1",2,"P.root",&[("P-c1",1)]),g("P-a4",3,"P.root",&[("P-a4",1)])]));
    check!(primary,&empty(),&[],out([]));
    let n=input("N.root","N");let required=[BasicNamedSourceInput{name:"Rows",instance:&n}];
    let retained=out([g("P-b1",1,"P.root",&[("P-b1",1)]),g("P-a2",2,"P.root",&[("P-a2",1),("P-a3",2)]),g("P-a4",3,"P.root",&[("P-a4",1)])]);
    check!(legacy_key,&p,&required,retained.clone());check!(legacy_post,&p,&required,retained);
}
#[test] fn errors_remain_typed_complete_and_cannot_hide_behind_zero_windows() {
    let p=input("P.root","P");
    failed!(key_error,&p,RuntimeError::MappingException{node:1,message:Some("key".into())});
    failed!(post_error,&p,RuntimeError::MappingException{node:5,message:Some("post".into())});
    failed!(nonbool,&p,RuntimeError::NotABool{node:5,found:"string"});
    failed!(invalid_bound,&empty(),RuntimeError::NotAnItemCount{node:7,found:"bool"});
    failed!(failure,&p,RuntimeError::MappingFailure{rule:1,message:Some("P-b1".into())});
}
fn picked_group(first:&str,ordinal:i64,value:Value,members:&[(&str,i64,i64)])->Instance {fields([
    ("First",text(first)),("GroupPosition",scalar(Value::Int(ordinal))),("RootMarker",text("P.root")),("Picked",scalar(value)),
    ("Members",repeated(members.iter().map(|(label,position,picked)|fields([
        ("Label",text(label)),("MemberPosition",scalar(Value::Int(*position))),("RootMarker",text("P.root")),("Picked",scalar(Value::Int(*picked))),
    ])))),
])}
#[test] fn inclusive_generate_10_to_12_receives_group_and_member_positions_without_new_api() {
    let p=input("P.root","P");
    check!(picked,&p,&[],out([
        picked_group("P-a1",1,Value::Int(10),&[("P-a1",1,10)]),
        picked_group("P-b1",2,Value::Int(11),&[("P-b1",1,10)]),
        picked_group("P-a2",3,Value::Int(12),&[("P-a2",1,10),("P-a3",2,11)]),
        picked_group("P-c1",4,Value::Null,&[("P-c1",1,10)]),
        picked_group("P-a4",5,Value::Null,&[("P-a4",1,10)]),
    ]));
    check!(picked,&empty(),&[],out([]));
    let three=fields([("Marker",text("P.root")),("Limit",scalar(Value::Int(2))),("Rows",repeated([
        row("A","P-a1",false,30),row("B","P-b1",true,10),row("A","P-a2",false,20),
    ]))]);
    check!(token_key,&three,&[],out([
        g("P-a1",1,"P.root",&[("P-a1",1)]),g("P-b1",2,"P.root",&[("P-b1",1)]),g("P-a2",3,"P.root",&[("P-a2",1)]),
    ]));
}
"#;
