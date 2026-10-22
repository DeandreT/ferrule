//! Starting companion fixtures and literal public products; execution belongs to
//! the parent qualification lane, not source preparation.

use super::owned_basic_group_by::{configure_groups, generated_source};
use super::*;
use mapping::{Node, OwnedSourceField, OwnedSourceRef, SourceOwner};

// #444 Ending reuses this module's Starting fixture and refusal.
#[path = "owned_basic_group_ending_with.rs"]
mod ending_with;
// #445 IntoBlocks reuses the same fixture and refusal.
#[path = "owned_basic_group_into_blocks.rs"]
mod into_blocks;

fn path(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|part| (*part).into()).collect()
}

fn project(owner: SourceOwner) -> mapping::Project {
    let mut p = super::owned_basic_group_by::project(owner.clone());
    fn start_field(schema: &mut SchemaNode) {
        let SchemaKind::Group { children, .. } = &mut schema.kind else {
            unreachable!()
        };
        let rows = children
            .iter_mut()
            .find(|child| child.name == "Rows")
            .unwrap();
        let SchemaKind::Group { children, .. } = &mut rows.kind else {
            unreachable!()
        };
        children.push(SchemaNode::scalar("Start", ScalarType::Bool));
    }
    start_field(&mut p.source);
    for source in &mut p.extra_sources {
        start_field(&mut source.schema);
    }
    p.graph.nodes.insert(
        101,
        Node::OwnedSourceField(OwnedSourceField {
            source: OwnedSourceRef {
                owner,
                path: path(&["Rows", "Start"]),
            },
            frame: None,
        }),
    );
    configure_groups(&mut p, |scope| {
        scope.group_by = None;
        scope.group_starting_with = Some(101);
    });
    p
}

fn lower(project: &mapping::Project) -> Program {
    let saved = serde_json::to_vec_pretty(project).unwrap();
    let restored: mapping::Project = serde_json::from_slice(&saved).unwrap();
    assert_eq!(serde_json::to_vec_pretty(&restored).unwrap(), saved);
    codegen::lower_with_basic_owned_group_starting_with(&restored).unwrap()
}

fn options() -> Options {
    let mut options = super::owned_basic_group_by::options();
    options.package_name = "owned_starting_host".into();
    options
}

fn artifacts(project: &mapping::Project) -> ArtifactSet {
    emit_with_basic_owned_group_starting_with(&lower(project), &options()).unwrap()
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
                path: path(&["Rows"]),
            }),
            reason: Reason::Grouping,
            origin: Origin::Scope { scope: route },
        }),
    }
}

#[test]
fn starting_has_only_the_new_companion_and_original_structural_precedence() {
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
        emit_with_basic_owned_group_adjacent_by,
    ] {
        let Err(EmitError::InvalidProgram(error)) = old(&program, &options()) else {
            panic!("older profile admitted owned StartingWith");
        };
        assert_eq!(error, grouping_refusal());
    }
    let mut invalid = options();
    invalid.package_name = "0invalid".into();
    assert!(matches!(
        emit_with_basic_owned_group_starting_with(&program, &invalid),
        Err(EmitError::InvalidPackageName(_))
    ));
    let mut missing = program.clone();
    missing.root.children[0].iteration = Some(
        missing.root.children[0]
            .iteration
            .clone()
            .unwrap()
            .with_grouping(GroupingPlan::StartingWith { predicate: 1000 }),
    );
    let before = format!("{missing:#?}");
    let Err(EmitError::InvalidProgram(error)) =
        emit_with_basic_owned_group_starting_with(&missing, &invalid)
    else {
        panic!("missing predicate");
    };
    assert_eq!(
        error,
        ProgramValidationError::MissingGroupingExpression {
            target_path: path(&["Groups"]),
            expression: 1000,
            role: codegen::GroupingExpressionRole::StartingPredicate,
        }
    );
    assert_eq!(format!("{missing:#?}"), before);
    for mode in [
        GroupingPlan::EndingWith { predicate: 101 },
        GroupingPlan::IntoBlocks { size: 103 },
    ] {
        let mut closed = program.clone();
        closed.expressions.push(codegen::ExpressionNode {
            id: 103,
            expression: Expression::Const {
                value: Value::Int(2),
            },
        });
        closed.root.children[0].iteration = Some(
            closed.root.children[0]
                .iteration
                .clone()
                .unwrap()
                .with_grouping(mode),
        );
        let Err(EmitError::InvalidProgram(error)) =
            emit_with_basic_owned_group_starting_with(&closed, &options())
        else {
            panic!("closed mode");
        };
        assert_eq!(error, grouping_refusal());
    }
}

#[test]
fn starting_boolean_controls_keep_original_predicate_parent_and_member_roles() {
    let mut p = project(SourceOwner::Primary);
    configure_groups(&mut p, |scope| {
        scope.sort_by = Some(6);
        scope.filter = Some(12);
        scope.post_group_filter = Some(5);
        scope.sort_filter_order = mapping::SortFilterOrder::FilterThenSort;
        scope.windows = vec![mapping::SequenceWindow::FromTo { first: 8, last: 7 }];
    });
    let files = artifacts(&p);
    let source = generated_source(&files);
    assert_eq!(files.len(), 2);
    assert_eq!(
        source
            .matches(
                "impl codegen_runtime::BasicGroupStartingWithControls<RuntimeError> for Controls"
            )
            .count(),
        2
    );
    assert!(source.contains("fn starts_group(&mut self, boundary_context: &BasicScopeContext<'_>) -> Result<bool, RuntimeError> {\n            let value = expression_101(boundary_context)?;\n            require_bool(101, value)\n        }"));
    assert_eq!(
        source
            .matches("context.map_group_starting_with_source_cohort(")
            .count(),
        2
    );
    assert!(!source.contains("fn group_key("));
    assert!(source.contains("expression_5(member_context)?"));
    assert!(source.contains("expression_12(predicate_context)?"));
    assert!(source.contains("0 => expression_6(key_context)"));
    assert!(
        source.find("expression_8(parent_context)?").unwrap()
            < source.find("expression_7(parent_context)?").unwrap()
    );
    assert!(source.contains("pub fn execute_with_basic_owned_sources<'name, 'src>("));
    assert!(source.contains("pub fn execute_selected_with_basic_owned_sources<'name, 'src>("));
    assert!(
        source.contains("execution: Option<&ExecutionContext<'src>>, limits: OwnedSourceLimits,")
    );
    assert!(!source.contains("pub fn execute_json"));
}

fn isolated_legacy(post_only: bool) -> mapping::Project {
    let mut p = project(SourceOwner::Primary);
    p.extra_targets.clear();
    let scope = &mut p.root.children[0];
    scope.iteration = mapping::ScopeIteration::Source(path(&["Rows"]));
    scope.children[0].iteration = mapping::ScopeIteration::Source(vec![]);
    scope
        .bindings
        .retain(|binding| binding.target_field != "RootMarker");
    scope.children[0]
        .bindings
        .retain(|binding| binding.target_field != "RootMarker");
    fn remove_marker(schema: &mut SchemaNode) {
        if let SchemaKind::Group { children, .. } = &mut schema.kind {
            children.retain(|child| child.name != "RootMarker");
            for child in children {
                remove_marker(child);
            }
        }
    }
    remove_marker(&mut p.target);
    p.graph.nodes.insert(
        2,
        Node::SourceField {
            path: path(&["Label"]),
            frame: Some(path(&["Rows"])),
        },
    );
    p.graph.nodes.insert(
        3,
        Node::Position {
            collection: path(&["Rows"]),
        },
    );
    if post_only {
        p.graph.nodes.insert(
            101,
            Node::SourceField {
                path: path(&["Start"]),
                frame: Some(path(&["Rows"])),
            },
        );
        p.graph.nodes.insert(
            102,
            Node::Call {
                function: "equal".into(),
                args: vec![5, 5],
            },
        );
        p.root.children[0].post_group_filter = Some(102);
    }
    p
}

#[test]
fn starting_isolated_boundary_or_post_demand_inherits_members_not_ordinary_siblings() {
    for post_only in [false, true] {
        let program = lower(&isolated_legacy(post_only));
        assert_eq!(
            program
                .expressions
                .iter()
                .filter(|node| matches!(
                    node.expression,
                    Expression::OwnedSourceField(_) | Expression::OwnedPosition(_)
                ))
                .count(),
            1
        );
        let files = emit_with_basic_owned_group_starting_with(&program, &options()).unwrap();
        let source = generated_source(&files);
        assert!(source.contains("fn scope_root_0_0(context: &BasicScopeContext<'_>)"));
        assert!(!source.contains("fn legacy_scope_root_0_0("));
        assert!(source.contains("context.walk_source_cohort(&[\"Rows\"])?"));
        assert!(source.contains("name: \"Rows\", mode: BasicNamedSourceMode::RequiredLegacy"));
    }
    for (mode, helper) in [
        (0, "by"),
        (1, "adjacent_by"),
        (2, "starting_with"),
        (3, "ending_with"),
        (4, "into_blocks"),
    ] {
        let mut p = project(SourceOwner::Primary);
        p.extra_targets.clear();
        let mut island = mapping::Scope {
            target_field: "Ordinary".into(),
            iteration: mapping::ScopeIteration::Source(path(&["Rows"])),
            bindings: vec![mapping::Binding {
                target_field: "Value".into(),
                node: 99,
            }],
            ..Default::default()
        };
        p.graph.nodes.insert(
            104,
            Node::Const {
                value: Value::Bool(true),
            },
        );
        match mode {
            0 => island.group_by = Some(99),
            1 => island.group_adjacent_by = Some(99),
            2 => island.group_starting_with = Some(104),
            3 => island.group_ending_with = Some(104),
            _ => island.group_into_blocks = Some(8),
        }
        p.root.children.push(island);
        let SchemaKind::Group { children, .. } = &mut p.target.kind else {
            unreachable!()
        };
        children.push(
            SchemaNode::group(
                "Ordinary",
                vec![SchemaNode::scalar("Value", ScalarType::String)],
            )
            .repeating(),
        );
        let files = artifacts(&p);
        let source = generated_source(&files);
        assert!(source.contains("fn legacy_scope_root_1(context: &ScopeContext<'_>)"));
        assert!(source.contains(&format!("GroupedItems::{helper}(")));
        assert_eq!(
            source
                .matches("context.map_group_starting_with_source_cohort(")
                .count(),
            1
        );
        assert!(source.contains("unused expression_ literal"));
    }
}

#[test]
fn starting_composition_conserves_prior_by_and_adjacent_artifacts_and_failure_permissions() {
    for adjacent in [false, true] {
        let mut p = super::owned_basic_group_by::project(SourceOwner::Primary);
        if adjacent {
            configure_groups(&mut p, |scope| {
                scope.group_adjacent_by = scope.group_by.take()
            });
        }
        let program = lower(&p);
        assert_eq!(
            emit_with_basic_owned_group_starting_with(&program, &options()).unwrap(),
            emit_with_basic_owned_group_adjacent_by(&program, &options()).unwrap()
        );
    }
    let mut p = project(SourceOwner::Primary);
    p.graph.nodes.extend([
        (
            70,
            Node::SequenceItemAt {
                sequence: mapping::SequenceExpr::Tokenize {
                    input: 71,
                    delimiter: 72,
                    item: 90,
                },
                index: 3,
            },
        ),
        (
            71,
            Node::Const {
                value: Value::String("F|F|T|T|F|T|F".into()),
            },
        ),
        (
            72,
            Node::Const {
                value: Value::String("|".into()),
            },
        ),
        (
            73,
            Node::Const {
                value: Value::String("T".into()),
            },
        ),
        (
            90,
            Node::SourceField {
                path: vec![],
                frame: None,
            },
        ),
        (
            101,
            Node::Call {
                function: "equal".into(),
                args: vec![70, 73],
            },
        ),
    ]);
    let program = lower(&p);
    let files = emit_with_basic_owned_group_starting_with(&program, &options()).unwrap();
    let source = generated_source(&files);
    assert!(source.contains("expression_70(context)?"));
    assert!(source.contains("let sequence_values ="));
    assert!(!source.contains("fn expression_90("));
    for message in [false, true] {
        let mut direct = program.clone();
        direct.failure_rules.push(FailureRule {
            iteration: FailureIteration::Source(SourceIteration::new(path(&["Rows"]))),
            selection: if message {
                FailureSelection::All
            } else {
                FailureSelection::WhenTrue(101)
            },
            message: message.then_some(101),
        });
        assert!(matches!(
            emit_with_basic_owned_group_starting_with(&direct, &options()),
            Err(EmitError::InvalidProgram(
                ProgramValidationError::OwnedSource { .. }
            ))
        ));
    }
}

#[test]
fn starting_catalogues_are_complete_exact_and_phase_aware() {
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
    unused.dynamic_path = Some(mapping::DynamicSourcePath::legacy(99, vec![]));
    p.extra_sources.push(unused);
    for mode in 0..3 {
        if mode == 1 {
            p.graph.nodes.insert(
                80,
                Node::SourceField {
                    path: path(&["Rows", "Marker"]),
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
        }
        if mode == 2 {
            p.graph.nodes.insert(
                80,
                Node::SourceField {
                    path: path(&["Rows", "Marker"]),
                    frame: None,
                },
            );
            p.graph.nodes.insert(
                81,
                Node::Const {
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
        let plan = codegen::legacy_named_input_admission_plan_with_basic_owned_group_starting_with(
            &program,
        )
        .unwrap();
        let expected = vec![
            (0, "unusedStatic", O),
            (1, "Rows", if mode > 0 { R } else { O }),
            (2, " Røws💠 ", O),
            (3, "unavailableDynamic", U),
        ];
        let named = vec![
            (0, "unusedStatic", O),
            (1, "Rows", if mode == 2 { R } else { O }),
            (2, " Røws💠 ", O),
            (3, "unavailableDynamic", U),
        ];
        assert_eq!(rows(&plan.all_outputs), expected);
        assert_eq!(rows(&plan.primary), expected);
        assert_eq!(
            plan.named_targets
                .iter()
                .map(|target| (target.target_index, target.name, rows(&target.catalogue)))
                .collect::<Vec<_>>(),
            vec![(0, " Exact audit 雪 ", named)]
        );
        let files = emit_with_basic_owned_group_starting_with(&program, &options()).unwrap();
        assert!(generated_source(&files).contains("PRIVATE_CHECKED_NAMED_INPUTS_0"));
    }
}

#[test]
fn starting_literal_public_consumer_keeps_prefix_consecutive_starts_and_typed_errors() {
    let directory = TempDir::new("owned_starting_consumer");
    let mut plain = project(SourceOwner::Primary);
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
    let mut raised = plain.clone();
    raised
        .graph
        .nodes
        .insert(101, Node::Raise { message: Some(99) });
    configure_groups(&mut raised, |scope| {
        scope.windows = vec![mapping::SequenceWindow::First { count: 9 }]
    });
    let mut library = String::from("#![deny(warnings)]\n");
    for (name, p) in [
        ("plain", plain),
        ("post", post),
        ("nonbool", nonbool),
        ("raised", raised),
    ] {
        fs::write(
            directory.path().join(format!("{name}.Project.json")),
            serde_json::to_vec_pretty(&p).unwrap(),
        )
        .unwrap();
        library.push_str(&format!(
            "pub mod {name} {{\n{}\n}}\n",
            generated_source(&artifacts(&p))
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
        .args([
            "test",
            "--offline",
            "--test",
            "consumer",
            "--",
            "--nocapture",
        ])
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
            "Starting consumer retained at {}:\n{}\n{}",
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
use owned_starting_host::*;
use codegen_runtime::{Instance,OwnedSourceLimits,RuntimeError,TargetSelection,Value,XmlTypeOrigin,field,group,repeated,scalar};
// Borrow the original tree: InstanceGroup::Debug/serde omit its XML origin.
// Ordered path records supplement, rather than replace, the complete typed log.
fn retain_instance(path:&str,instance:&Instance) {
    match instance {
        Instance::Scalar(value)=>eprintln!("raw instance path={path:?} kind=Scalar value={value:?} binary64={:?}",
            match value {Value::Float(value)=>Some(value.to_bits()),_=>None}),
        Instance::Group(fields)=>{
            let (kind,identity,literal)=match fields.xml_type_origin() {
                XmlTypeOrigin::Unknown=>("Unknown",None,None),
                XmlTypeOrigin::Absent=>("Absent",None,None),
                XmlTypeOrigin::Explicit(identity)=>("Explicit",Some(identity),None),
                XmlTypeOrigin::ExplicitPadded{literal,resolved_identity}=>("ExplicitPadded",Some(resolved_identity),Some(literal)),
            };
            eprintln!("raw instance path={path:?} kind=Group xml_type_origin.kind={kind} identity={identity:?} literal={literal:?} fields={}",fields.len());
            for (index,(name,value)) in fields.iter().enumerate() {
                eprintln!("raw field path={path:?} index={index} name={name:?}");
                retain_instance(&format!("{path}/field-{index}"),value);
            }
        }
        Instance::Repeated(items)|Instance::MappedSequence(items)=>{
            let kind=if matches!(instance,Instance::Repeated(_)){"Repeated"}else{"MappedSequence"};
            eprintln!("raw instance path={path:?} kind={kind} items={}",items.len());
            for (index,value) in items.iter().enumerate(){retain_instance(&format!("{path}/item-{index}"),value);}
        }
        Instance::DocumentSet(documents)=>{
            eprintln!("raw instance path={path:?} kind=DocumentSet documents={}",documents.len());
            for (index,document) in documents.iter().enumerate(){
                eprintln!("raw document path={path:?} index={index} actual={document:#?}");
                retain_instance(&format!("{path}/document-{index}"),document.value());
            }
        }
    }
}
macro_rules! retain_all {($actual:expr)=>{if let Ok(all)=&$actual {
    eprintln!("raw product kind=ExecutionOutputs extras={}",all.extras.len());
    retain_instance("All.primary",&all.primary);
    for (index,named) in all.extras.iter().enumerate(){
        eprintln!("raw named output index={index} name={:?}",named.name);
        retain_instance(&format!("All.extra-{index}"),&named.instance);
    }
}};}
macro_rules! retain_selected {($m:ident,$actual:expr)=>{if let Ok(selected)=&$actual {match selected {
    $m::SelectedTargetOutput::Primary(instance)=>{eprintln!("raw product kind=Primary");retain_instance("Primary",instance);}
    $m::SelectedTargetOutput::Named(named)=>{eprintln!("raw product kind=Named name={:?}",named.name);retain_instance("Named",&named.instance);}
}}};}
fn text(v:&str)->Instance {scalar(Value::String(v.into()))}
fn fields(v:impl IntoIterator<Item=(&'static str,Instance)>)->Instance {group(v.into_iter().map(|(n,v)|field(n,v)))}
fn row(label:&str,start:bool,keep:bool)->Instance {fields([("Key",text("A")),("Label",text(label)),("Keep",scalar(Value::Bool(keep))),("Rank",scalar(Value::Int(0))),("Start",scalar(Value::Bool(start)))])}
fn input()->Instance {fields([("Marker",text("P.root")),("Limit",scalar(Value::Int(2))),("Rows",repeated([
    row("p0",false,false),row("p1",false,true),row("p2",true,false),row("p3",true,false),
    row("p4",false,true),row("p5",true,false),row("p6",false,true),
]))])}
fn g(first:&str,pos:i64,members:&[(&str,i64)])->Instance {fields([
    ("First",text(first)),("GroupPosition",scalar(Value::Int(pos))),("RootMarker",text("P.root")),
    ("Members",repeated(members.iter().map(|(label,pos)|fields([("Label",text(label)),("MemberPosition",scalar(Value::Int(*pos))),("RootMarker",text("P.root"))])))),
])}
fn out(v:impl IntoIterator<Item=Instance>)->Instance {fields([("Groups",repeated(v))])}
fn limits()->OwnedSourceLimits {OwnedSourceLimits{max_roots:8,max_active_frames:16,max_path_segments:64,max_metadata_bytes:1048576,max_walk_contexts:128}}
macro_rules! check {($m:ident,$source:expr,$wanted:expr)=>{{let source=$source;let wanted=$wanted;
    eprintln!("complete input before subject: module={} source={source:#?} expected={wanted:#?}",stringify!($m));
    let all=$m::execute_with_basic_owned_sources(source,&[],None,limits());retain_all!(all);eprintln!("complete All actual before comparison: {all:#?}");
    assert_eq!(all,Ok($m::ExecutionOutputs{primary:wanted.clone(),extras:vec![$m::NamedOutput{name:" Exact audit 雪 ".into(),instance:wanted.clone()}]}));
    let primary=$m::execute_selected_with_basic_owned_sources(source,&[],None,limits(),TargetSelection::Primary);retain_selected!($m,primary);eprintln!("complete Primary actual before comparison: {primary:#?}");
    assert_eq!(primary,Ok($m::SelectedTargetOutput::Primary(wanted.clone())));
    let named=$m::execute_selected_with_basic_owned_sources(source,&[],None,limits(),TargetSelection::Named(" Exact audit 雪 "));retain_selected!($m,named);eprintln!("complete named actual before comparison: {named:#?}");
    assert_eq!(named,Ok($m::SelectedTargetOutput::Named($m::NamedOutput{name:" Exact audit 雪 ".into(),instance:wanted})));
}};}
macro_rules! fails {($m:ident,$p:expr,$wanted:expr)=>{{
    eprintln!("complete error input before subject: module={} source={:#?} expected={:#?}",stringify!($m),$p,$wanted);
    let all=$m::execute_with_basic_owned_sources($p,&[],None,limits());retain_all!(all);eprintln!("complete All error actual before comparison: {all:#?}");assert_eq!(all,Err($wanted));
    let primary=$m::execute_selected_with_basic_owned_sources($p,&[],None,limits(),TargetSelection::Primary);retain_selected!($m,primary);eprintln!("complete Primary error actual before comparison: {primary:#?}");assert_eq!(primary,Err($wanted));
    let named=$m::execute_selected_with_basic_owned_sources($p,&[],None,limits(),TargetSelection::Named(" Exact audit 雪 "));retain_selected!($m,named);eprintln!("complete named error actual before comparison: {named:#?}");assert_eq!(named,Err($wanted));
}};}
#[test] fn literal_prefix_and_member_retention() {let p=input();
    check!(plain,&p,out([g("p0",1,&[("p0",1),("p1",2)]),g("p2",2,&[("p2",1)]),g("p3",3,&[("p3",1),("p4",2)]),g("p5",4,&[("p5",1),("p6",2)])]));
    check!(post,&p,out([g("p0",1,&[("p0",1),("p1",2)]),g("p3",2,&[("p3",1),("p4",2)]),g("p5",3,&[("p5",1),("p6",2)])]));
    let empty=fields([("Marker",text("P.root")),("Limit",scalar(Value::Int(2))),("Rows",repeated([]))]);check!(plain,&empty,out([]));
}
#[test] fn exact_predicate_error_and_unknown_target_precedence() {let p=input();
    fails!(nonbool,&p,RuntimeError::NotABool{node:101,found:"string"});
    fails!(raised,&p,RuntimeError::MappingException{node:101,message:Some("unused expression_ literal 雪".into())});
    let mut zero=limits();zero.max_roots=0;
    let unknown=plain::execute_selected_with_basic_owned_sources(&p,&[],None,zero,TargetSelection::Named("Exact audit 雪"));
    retain_selected!(plain,unknown);eprintln!("complete unknown-target actual before comparison: {unknown:#?}");assert_eq!(unknown,Err(RuntimeError::UnknownTarget{name:"Exact audit 雪".into()}));
}
"#;
