// Starting fixtures reuse genuine Projects and the existing typed public host.
// Literal outputs below are independent of native/generated execution.
fn starting_443_project(owner: SourceOwner) -> ::mapping::Project {
    let mut p = group_441_project(owner.clone(), false);
    fn add_start(schema: &mut SchemaNode) {
        let ir::SchemaKind::Group { children, .. } = &mut schema.kind else {
            unreachable!()
        };
        let rows = children
            .iter_mut()
            .find(|child| child.name == "Rows")
            .unwrap();
        let ir::SchemaKind::Group { children, .. } = &mut rows.kind else {
            unreachable!()
        };
        children.push(SchemaNode::scalar("Start", ScalarType::Bool));
    }
    add_start(&mut p.source);
    for source in &mut p.extra_sources {
        add_start(&mut source.schema);
    }
    p.root.children[0].group_by = None;
    p.root.children[0].group_starting_with = Some(101);
    p.graph
        .nodes
        .insert(101, group_441_node(&owner, &["Rows", "Start"], None));
    starting_443_sync(&mut p);
    p
}

fn starting_443_sync(project: &mut ::mapping::Project) {
    for target in &mut project.extra_targets {
        target.root = project.root.clone();
        target.schema = project.target.clone();
    }
}

fn starting_443_program(project: &::mapping::Project) -> Program {
    let saved = serde_json::to_vec_pretty(project).unwrap();
    let restored: ::mapping::Project = serde_json::from_slice(&saved).unwrap();
    assert_eq!(serde_json::to_vec_pretty(&restored).unwrap(), saved);
    codegen::lower_with_basic_owned_group_starting_with(&restored).unwrap()
}

fn starting_443_source(artifacts: &codegen::ArtifactSet) -> &str {
    std::str::from_utf8(
        &artifacts
            .files()
            .iter()
            .find(|file| file.path.as_str() == "GeneratedMapping.cs")
            .unwrap()
            .contents,
    )
    .unwrap()
}

fn starting_443_refusal() -> ProgramValidationError {
    use codegen::{
        OwnedSourceCapabilityOrigin as Origin, OwnedSourceCapabilityReason as Reason,
        OwnedSourceDiagnostic as Diagnostic, OwnedSourceDiagnosticLocation as Location,
        OwnedSourceDiagnosticRequest as Request,
    };
    let route = ::mapping::SourceScopeLocation::root(::mapping::SourceScopeTarget::Primary)
        .descendant(::mapping::SourceScopeStep::Child(0));
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
fn public_basic_group_starting_with_443_keeps_old_profiles_and_structural_errors() {
    let program = starting_443_program(&starting_443_project(SourceOwner::Primary));
    for old in [
        crate::emit,
        crate::emit_with_basic_owned_sources,
        crate::emit_with_basic_owned_filters,
        crate::emit_with_basic_owned_sort,
        crate::emit_with_basic_owned_windows,
        crate::emit_with_basic_owned_row_failures,
        crate::emit_with_basic_owned_group_by,
        crate::emit_with_basic_owned_sequence_item_at,
        crate::emit_with_basic_owned_group_adjacent_by,
    ] {
        assert_eq!(
            old(&program),
            Err(EmitError::ProgramValidation(starting_443_refusal()))
        );
    }
    let mut missing = program.clone();
    missing.root.children[0].iteration = Some(
        missing.root.children[0]
            .iteration
            .clone()
            .unwrap()
            .with_grouping(codegen::GroupingPlan::StartingWith { predicate: 1000 }),
    );
    let before = format!("{missing:#?}");
    assert_eq!(
        crate::emit_with_basic_owned_group_starting_with(&missing),
        Err(EmitError::ProgramValidation(
            ProgramValidationError::MissingGroupingExpression {
                expression: 1000,
                role: codegen::GroupingExpressionRole::StartingPredicate,
                target_path: path(&["Groups"])
            }
        ))
    );
    assert_eq!(format!("{missing:#?}"), before);
    for grouping in [
        codegen::GroupingPlan::EndingWith { predicate: 101 },
        codegen::GroupingPlan::IntoBlocks { size: 103 },
    ] {
        let mut closed = program.clone();
        closed.expressions.push(codegen::ExpressionNode {
            id: 103,
            expression: codegen::Expression::Const {
                value: Value::Int(2),
            },
        });
        closed.root.children[0].iteration = Some(
            closed.root.children[0]
                .iteration
                .clone()
                .unwrap()
                .with_grouping(grouping),
        );
        assert_eq!(
            crate::emit_with_basic_owned_group_starting_with(&closed),
            Err(EmitError::ProgramValidation(starting_443_refusal()))
        );
    }
}

#[test]
fn public_basic_group_starting_with_443_bool_control_and_existing_callback_roles() {
    let mut p = starting_443_project(SourceOwner::Primary);
    let scope = &mut p.root.children[0];
    scope.sort_by = Some(6);
    scope.filter = Some(9);
    scope.post_group_filter = Some(2);
    scope.sort_filter_order = ::mapping::SortFilterOrder::FilterThenSort;
    scope.windows = vec![::mapping::SequenceWindow::FromTo { first: 8, last: 7 }];
    starting_443_sync(&mut p);
    let artifacts =
        crate::emit_with_basic_owned_group_starting_with(&starting_443_program(&p)).unwrap();
    let source = starting_443_source(&artifacts);
    assert_eq!(
        source.matches(".MapGroupStartingWithSourceCohort(").count(),
        2
    );
    assert!(source.contains("class GroupStartingWithControls_1 : global::Ferrule.Runtime.IFerruleBasicGroupStartingWithControls"));
    assert!(source.contains("public bool StartsGroup(global::Ferrule.Runtime.FerruleBasicScopeContext candidate)\n            => global::Ferrule.Runtime.FerruleFunctions.RequireBoolean(Node_101(candidate), 101U);"));
    assert!(!source.contains(" GroupKey("));
    assert!(source.contains("RequireBoolean(Node_2(candidate), 2U)"));
    assert!(source.contains("RequireBoolean(Node_9(candidate), 9U)"));
    assert!(source.contains("0 => Node_6(candidate)"));
    assert!(
        source.find("ItemCount(8U, Node_8(parent))").unwrap()
            < source.find("ItemCount(7U, Node_7(parent))").unwrap()
    );
    assert!(source.contains("public static ExecutionOutputs ExecuteWithBasicOwnedSources("));
    assert!(
        source.contains("public static SelectedTargetOutput ExecuteSelectedWithBasicOwnedSources(")
    );
    assert!(!source.contains("ExecuteJson"));
}

fn starting_443_isolated_legacy(post_only: bool) -> ::mapping::Project {
    use ::mapping::{Node, ScopeIteration};
    let mut p = starting_443_project(SourceOwner::Primary);
    p.extra_targets.clear();
    let scope = &mut p.root.children[0];
    scope.iteration = ScopeIteration::Source(path(&["Rows"]));
    scope.children[0].iteration = ScopeIteration::Source(vec![]);
    scope
        .bindings
        .retain(|binding| binding.target_field != "RootMarker");
    scope.children[0]
        .bindings
        .retain(|binding| binding.target_field != "RootMarker");
    fn remove_marker(schema: &mut SchemaNode) {
        if let ir::SchemaKind::Group { children, .. } = &mut schema.kind {
            children.retain(|child| child.name != "RootMarker");
            for child in children {
                remove_marker(child);
            }
        }
    }
    remove_marker(&mut p.target);
    p.graph.nodes.insert(
        3,
        Node::SourceField {
            path: path(&["Label"]),
            frame: Some(path(&["Rows"])),
        },
    );
    p.graph.nodes.insert(
        4,
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
                args: vec![2, 2],
            },
        );
        p.root.children[0].post_group_filter = Some(102);
    }
    p
}

#[test]
fn public_basic_group_starting_with_443_isolated_demand_ancestry_and_ordinary_islands() {
    use ::mapping::{Node, Scope, ScopeIteration};
    for post_only in [false, true] {
        let program = starting_443_program(&starting_443_isolated_legacy(post_only));
        assert_eq!(
            program
                .expressions
                .iter()
                .filter(|node| matches!(
                    node.expression,
                    codegen::Expression::OwnedSourceField(_)
                        | codegen::Expression::OwnedPosition(_)
                ))
                .count(),
            1
        );
        let artifacts = crate::emit_with_basic_owned_group_starting_with(&program).unwrap();
        let source = starting_443_source(&artifacts);
        assert!(!source.contains("Scope_2(global::Ferrule.Runtime.ScopeContext context)"));
        assert!(source.contains(".MapGroupStartingWithSourceCohort("));
        assert!(source.contains(
            "\"Rows\", global::Ferrule.Runtime.FerruleBasicNamedSourceMode.RequiredLegacy"
        ));
    }
    for (mode, helper) in [
        (0, "GroupBy"),
        (1, "GroupAdjacentBy"),
        (2, "GroupStartingWith"),
        (3, "GroupEndingWith"),
        (4, "GroupIntoBlocks"),
    ] {
        let mut p = starting_443_project(SourceOwner::Primary);
        p.extra_targets.clear();
        p.graph.nodes.insert(
            104,
            Node::Const {
                value: Value::Bool(true),
            },
        );
        let mut island = Scope {
            target_field: "Ordinary".into(),
            iteration: ScopeIteration::Source(path(&["Rows"])),
            bindings: vec![::mapping::Binding {
                target_field: "Value".into(),
                node: 40,
            }],
            ..Default::default()
        };
        match mode {
            0 => island.group_by = Some(40),
            1 => island.group_adjacent_by = Some(40),
            2 => island.group_starting_with = Some(104),
            3 => island.group_ending_with = Some(104),
            _ => island.group_into_blocks = Some(8),
        }
        p.root.children.push(island);
        let ir::SchemaKind::Group { children, .. } = &mut p.target.kind else {
            unreachable!()
        };
        children.push(
            SchemaNode::group(
                "Ordinary",
                vec![SchemaNode::scalar("Value", ScalarType::String)],
            )
            .repeating(),
        );
        let artifacts =
            crate::emit_with_basic_owned_group_starting_with(&starting_443_program(&p)).unwrap();
        let source = starting_443_source(&artifacts);
        assert!(source.contains("Scope_3(global::Ferrule.Runtime.ScopeContext context)"));
        assert!(source.contains(&format!("{helper}(candidates_3,")));
        assert_eq!(
            source.matches(".MapGroupStartingWithSourceCohort(").count(),
            1
        );
    }
}

#[test]
fn public_basic_group_starting_with_443_conserves_keyed_bytes_and_closes_itemat_failures() {
    use ::mapping::{Node, SequenceExpr};
    for adjacent in [false, true] {
        let mut p = group_441_project(SourceOwner::Primary, true);
        if adjacent {
            p.root.children[0].group_adjacent_by = p.root.children[0].group_by.take();
        }
        starting_443_sync(&mut p);
        let program = starting_443_program(&p);
        assert_eq!(
            crate::emit_with_basic_owned_group_starting_with(&program),
            crate::emit_with_basic_owned_group_adjacent_by(&program)
        );
    }
    let mut p = starting_443_project(SourceOwner::Primary);
    p.graph.nodes.extend([
        (
            70,
            Node::SequenceItemAt {
                sequence: SequenceExpr::Tokenize {
                    input: 71,
                    delimiter: 72,
                    item: 90,
                },
                index: 4,
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
    let program = starting_443_program(&p);
    let artifacts = crate::emit_with_basic_owned_group_starting_with(&program).unwrap();
    let source = starting_443_source(&artifacts);
    assert!(source.contains("FerruleAggregateOperation.ItemAt"));
    assert!(source.contains("RequireBoolean(Node_101(candidate), 101U)"));
    adjacent_442_assert_item_binding_is_declaration_only(source);
    for message in [false, true] {
        let mut direct = program.clone();
        direct.failure_rules.push(codegen::FailureRule {
            iteration: codegen::FailureIteration::Source(codegen::SourceIteration::new(path(&[
                "Rows",
            ]))),
            selection: if message {
                codegen::FailureSelection::All
            } else {
                codegen::FailureSelection::WhenTrue(101)
            },
            message: message.then_some(101),
        });
        assert!(matches!(
            crate::emit_with_basic_owned_group_starting_with(&direct),
            Err(EmitError::ProgramValidation(
                ProgramValidationError::OwnedSource { .. }
            ))
        ));
    }
}

#[test]
fn public_basic_group_starting_with_443_complete_catalogues_preserve_global_dependencies() {
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
    let mut p = starting_443_project(SourceOwner::Primary);
    let mut unused = p.extra_sources[0].clone();
    unused.name = "unusedStatic".into();
    p.extra_sources.insert(0, unused.clone());
    unused.name = "unavailableDynamic".into();
    unused.dynamic_path = Some(::mapping::DynamicSourcePath {
        node: 40,
        iteration: vec![],
    });
    p.extra_sources.push(unused);
    for mode in 0..3 {
        if mode == 1 {
            p.graph.nodes.insert(
                80,
                ::mapping::Node::SourceField {
                    path: path(&["Rows", "Marker"]),
                    frame: None,
                },
            );
            p.root.bindings.push(::mapping::Binding {
                target_field: "Legacy".into(),
                node: 80,
            });
            let ir::SchemaKind::Group { children, .. } = &mut p.target.kind else {
                unreachable!()
            };
            children.push(SchemaNode::scalar("Legacy", ScalarType::String));
        }
        if mode == 2 {
            p.graph.nodes.insert(
                80,
                ::mapping::Node::SourceField {
                    path: path(&["Rows", "Marker"]),
                    frame: None,
                },
            );
            p.graph.nodes.insert(
                81,
                ::mapping::Node::Const {
                    value: Value::Bool(false),
                },
            );
            p.failure_rules.push(::mapping::FailureRule {
                iteration: ::mapping::FailureIteration::Source { collection: vec![] },
                selection: ::mapping::FailureSelection::WhenTrue { predicate: 81 },
                message: Some(80),
            });
        }
        let program = starting_443_program(&p);
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
            vec![(0, " Exact 雪 ", named)]
        );
        let artifacts = crate::emit_with_basic_owned_group_starting_with(&program).unwrap();
        let source = starting_443_source(&artifacts);
        assert!(source.contains("CheckedNamedInputs_0"));
        assert!(source.contains("// declaration 3"));
    }
}

fn retain_public_starting_443_consumer(
    label: &str,
    project: &::mapping::Project,
    body: &str,
) -> String {
    let program = starting_443_program(project);
    let artifacts = crate::emit_with_basic_owned_group_starting_with(&program).unwrap();
    let host = public_basic_consumer(&format!("{STARTING443_RAW_CAPTURE}\n{body}"), false);
    eprintln!(
        "STARTING443 {label} complete Project={}\nProgram={program:#?}\nindependent consumer={host}",
        ::mapping::project_file::encode_pretty(project).unwrap()
    );
    for file in artifacts.files() {
        let body = match std::str::from_utf8(&file.contents) {
            Ok(text) => serde_json::json!({"utf8":text}),
            Err(_) => serde_json::json!({"bytes":file.contents}),
        };
        eprintln!(
            "STARTING443_ARTIFACT {}",
            serde_json::json!({"case":label,"path":file.path.as_str(),"byte_length":file.contents.len(),"body":body})
        );
    }
    if let Some(base) = std::env::var_os("FERRULE_PUBLIC_BASIC_CSHARP_HOST_DIR") {
        let base = std::path::PathBuf::from(base);
        assert!(base.is_absolute());
        let root = base.join(label);
        std::fs::create_dir(&root).expect("fresh Starting consumer namespace");
        let mut files = artifacts
            .files()
            .iter()
            .map(|file| {
                (
                    format!("generated/{}", file.path.as_str()),
                    file.contents.clone(),
                )
            })
            .collect::<Vec<_>>();
        files.extend([
            (
                "consumer/PublicConsumer.csproj".into(),
                PUBLIC_BASIC_CONSUMER_PROJECT.as_bytes().to_vec(),
            ),
            ("consumer/Program.cs".into(), host.into_bytes()),
            (
                "Project.original.json".into(),
                ::mapping::project_file::encode_pretty(project)
                    .unwrap()
                    .into_bytes(),
            ),
            (
                "Program.ir.original.txt".into(),
                format!("{program:#?}\n").into_bytes(),
            ),
        ]);
        for (name, contents) in files {
            let destination = root.join(name);
            std::fs::create_dir_all(destination.parent().unwrap()).unwrap();
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(destination)
                .unwrap();
            std::io::Write::write_all(&mut file, &contents).unwrap();
        }
    }
    starting_443_source(&artifacts).to_owned()
}

// Capture inside the subject delegate, before the shared Reach helper performs
// its older InstanceSnapshot projection. Do not alter the common host or oracle.
const STARTING443_RAW_CAPTURE: &str = r#"
object? Starting443Instance(FerruleInstance? instance)=>instance switch
{
    null=>null,
    FerruleScalar scalar=>new {kind="Scalar",value=ValueSnapshot(scalar.Value)},
    FerruleGroup group=>new {kind="Group",origin=new {kind=group.XmlTypeOrigin.Kind.ToString(),identity=group.XmlTypeOrigin.Identity,literal=group.XmlTypeOrigin.Literal},
        fields=group.Fields.Select(field=>new {field.Name,value=Starting443Instance(field.Value)}).ToArray()},
    FerruleRepeated repeated=>new {kind="Repeated",items=repeated.Items.Select(Starting443Instance).ToArray()},
    FerruleMappedSequence mapped=>new {kind="MappedSequence",items=mapped.Items.Select(Starting443Instance).ToArray()},
    FerruleDocumentSet documents=>new {kind="DocumentSet",documents=documents.Documents.Select(document=>new {document.Path,document.ResolvedSourcePath,value=Starting443Instance(document.Value)}).ToArray()},
    _=>throw new InvalidOperationException("Unrecorded raw instance type: "+instance.GetType().FullName),
};
object? Starting443Product(object? result)=>result switch
{
    null=>null,
    ExecutionOutputs all=>new {kind="ExecutionOutputs",primary=Starting443Instance(all.Primary),extras=all.Extras.Select(named=>new {named.Name,instance=Starting443Instance(named.Instance)}).ToArray()},
    SelectedTargetOutput.Primary primary=>new {kind="Primary",instance=Starting443Instance(primary.Instance)},
    SelectedTargetOutput.Named named=>new {kind="Named",named.Output.Name,instance=Starting443Instance(named.Output.Instance)},
    _=>throw new InvalidOperationException("Unrecorded raw product type: "+result.GetType().FullName),
};
object? RetainStarting443(Func<object?> subject)
{
    var result=subject();
    Console.WriteLine(Json(new {classification="complete_origin_aware_actual_before_host_snapshot",result=Starting443Product(result)}));
    return result;
}
"#;

const STARTING443_INPUT: &str = r#"
FerruleInstance Row(string label,bool start,bool keep)=>G(("Key",S("A")),("Label",S(label)),("Keep",new FerruleScalar(FerruleValue.FromBoolean(keep))),("Rank",I(0)),("Limit",I(0)),("Start",new FerruleScalar(FerruleValue.FromBoolean(start))));
var input=G(("Marker",S("P.root")),("Limit",I(2)),("Rows",R(Row("p0",false,false),Row("p1",false,true),Row("p2",true,false),Row("p3",true,false),Row("p4",false,true),Row("p5",true,false),Row("p6",false,true))));
var limits=Policy();
"#;

#[test]
fn public_basic_group_starting_with_443_literal_public_products_and_errors() {
    for (label, post, groups) in [
        (
            "prefix",
            false,
            r#"R(Run(1,"p0","P.root",("p0",1),("p1",2)),Run(2,"p2","P.root",("p2",1)),Run(3,"p3","P.root",("p3",1),("p4",2)),Run(4,"p5","P.root",("p5",1),("p6",2)))"#,
        ),
        (
            "post",
            true,
            r#"R(Run(1,"p0","P.root",("p0",1),("p1",2)),Run(2,"p3","P.root",("p3",1),("p4",2)),Run(3,"p5","P.root",("p5",1),("p6",2)))"#,
        ),
    ] {
        let mut p = starting_443_project(SourceOwner::Primary);
        p.extra_sources.clear();
        p.root.children[0].post_group_filter = post.then_some(2);
        starting_443_sync(&mut p);
        let body = format!(
            r#"{ADJACENT442_COMPARE}{ADJACENT442_RUN}{STARTING443_INPUT}
var expected=G(("Groups",{groups}));
foreach(var selection in new FerruleTargetSelection?[]{{null,FerruleTargetSelection.Primary(),FerruleTargetSelection.Named(" Exact 雪 ")}})
{{
    var probe=new Probe(Array.Empty<NamedInput>());var actual=Reach("STARTING443.complete",input,probe,limits,null,selection,()=>RetainStarting443(()=>selection is null
        ? (object)GeneratedMapping.ExecuteWithBasicOwnedSources(input,probe,limits,null)
        : GeneratedMapping.ExecuteSelectedWithBasicOwnedSources(input,selection,probe,limits,null)));Complete(actual,expected);Reads(actual,1,0);
}}
var empty=G(("Marker",S("P.root")),("Limit",I(2)),("Rows",R()));var p0=new Probe(Array.Empty<NamedInput>());
Complete(Reach("STARTING443.empty",empty,p0,limits,null,null,()=>RetainStarting443(()=>GeneratedMapping.ExecuteWithBasicOwnedSources(empty,p0,limits,null))),G(("Groups",R())));
"#
        );
        retain_public_starting_443_consumer(&format!("public-starting-443-{label}"), &p, &body);
    }
    for raised in [false, true] {
        let mut p = starting_443_project(SourceOwner::Primary);
        p.extra_sources.clear();
        p.graph.nodes.insert(
            101,
            if raised {
                ::mapping::Node::Raise { message: Some(40) }
            } else {
                ::mapping::Node::Const {
                    value: Value::String("true".into()),
                }
            },
        );
        p.graph.nodes.insert(
            103,
            ::mapping::Node::Const {
                value: Value::Int(0),
            },
        );
        p.root.children[0].windows = vec![::mapping::SequenceWindow::First { count: 103 }];
        starting_443_sync(&mut p);
        let comparison = if raised {
            r#"var error=Runtime(actual,FerruleRuntimeError.MappingException);Check(error.Node==101U&&error.MappingExceptionMessage=="group control"&&error.Message=="node 101: mapping exception: group control"&&error.InnerException is null,"exact first boundary Raise");NullMetadata(error,"Node","MappingExceptionMessage");"#
        } else {
            r#"var error=Runtime(actual,FerruleRuntimeError.NotABool);Check(error.Node==101U&&error.FoundKind==FerruleValueKind.String&&error.Message=="Graph node 101 expected a bool, found string."&&error.InnerException is null,"exact boundary boolean tag/node");NullMetadata(error,"Node","FoundKind");"#
        };
        let body = format!(
            r#"{STARTING443_INPUT}
foreach(var selection in new FerruleTargetSelection?[]{{null,FerruleTargetSelection.Primary(),FerruleTargetSelection.Named(" Exact 雪 ")}})
{{
    var probe=new Probe(Array.Empty<NamedInput>());var actual=Reach("STARTING443.error",input,probe,limits,null,selection,()=>RetainStarting443(()=>selection is null
        ? (object)GeneratedMapping.ExecuteWithBasicOwnedSources(input,probe,limits,null)
        : GeneratedMapping.ExecuteSelectedWithBasicOwnedSources(input,selection,probe,limits,null)));{comparison}Reads(actual,1,0);
}}
var forbidden=new Probe(Array.Empty<NamedInput>(),throwCount:true,throwIndex:true);var zero=new FerruleOwnedSourceLimits(0,0,0,0,0);
var unknown=Reach("STARTING443.unknown",input,forbidden,zero,null,FerruleTargetSelection.Named("Exact 雪"),()=>RetainStarting443(()=>GeneratedMapping.ExecuteSelectedWithBasicOwnedSources(input,FerruleTargetSelection.Named("Exact 雪"),forbidden,zero,null)));
InputError(unknown,FerruleRuntimeError.UnknownTarget,"Exact 雪","project has no named target `Exact 雪`");Reads(unknown,0,0);
"#
        );
        retain_public_starting_443_consumer(
            if raised {
                "public-starting-443-raised"
            } else {
                "public-starting-443-not-bool"
            },
            &p,
            &body,
        );
    }
}
