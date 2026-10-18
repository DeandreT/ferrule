// Genuine saved-project fixtures with hand-authored complete output oracles.
// These tests retain public consumers; emitting a host is not executing it.
fn group_441_endpoint() -> SchemaNode {
    SchemaNode::group(
        "Endpoint",
        vec![
            SchemaNode::scalar("Marker", ScalarType::String),
            SchemaNode::scalar("Limit", ScalarType::Int),
            SchemaNode::group(
                "Rows",
                vec![
                    SchemaNode::scalar("Key", ScalarType::String),
                    SchemaNode::scalar("Label", ScalarType::String),
                    SchemaNode::scalar("Keep", ScalarType::Bool),
                    SchemaNode::scalar("Rank", ScalarType::Int),
                    SchemaNode::scalar("Limit", ScalarType::Int),
                ],
            )
            .repeating(),
        ],
    )
}

fn group_441_node(owner: &SourceOwner, parts: &[&str], frame: Option<&[&str]>) -> ::mapping::Node {
    ::mapping::Node::OwnedSourceField(OwnedSourceField {
        source: OwnedSourceRef {
            owner: owner.clone(),
            path: path(parts),
        },
        frame: frame.map(path),
    })
}

fn group_441_project(owner: SourceOwner, post: bool) -> ::mapping::Project {
    use ::mapping::{Graph, NamedSource, NamedTarget, Node, Project, Scope, ScopeIteration};
    let select = || ScopeIteration::OwnedSource {
        source: OwnedSourceRef {
            owner: owner.clone(),
            path: path(&["Rows"]),
        },
        output_path: None,
    };
    let bind = |name: &str, node| ::mapping::Binding {
        target_field: name.into(),
        node,
    };
    let members = Scope {
        target_field: "Members".into(),
        iteration: select(),
        bindings: vec![
            bind("Label", 3),
            bind("MemberPosition", 4),
            bind("RootMarker", 5),
        ],
        ..Scope::default()
    };
    let groups = Scope {
        target_field: "Groups".into(),
        iteration: select(),
        group_by: Some(1),
        post_group_filter: post.then_some(2),
        bindings: vec![
            bind("First", 3),
            bind("GroupPosition", 4),
            bind("RootMarker", 5),
        ],
        children: vec![members],
        ..Scope::default()
    };
    let target = SchemaNode::group(
        "Output",
        vec![
            SchemaNode::group(
                "Groups",
                vec![
                    SchemaNode::scalar("First", ScalarType::String),
                    SchemaNode::scalar("GroupPosition", ScalarType::Int),
                    SchemaNode::scalar("RootMarker", ScalarType::String),
                    SchemaNode::group(
                        "Members",
                        vec![
                            SchemaNode::scalar("Label", ScalarType::String),
                            SchemaNode::scalar("MemberPosition", ScalarType::Int),
                            SchemaNode::scalar("RootMarker", ScalarType::String),
                        ],
                    )
                    .repeating(),
                ],
            )
            .repeating(),
        ],
    );
    let root = Scope {
        children: vec![groups],
        ..Scope::default()
    };
    Project {
        source: group_441_endpoint(),
        target: target.clone(),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: ["Rows", " Røws💠 "]
            .iter()
            .map(|name| NamedSource {
                name: (*name).into(),
                path: String::new(),
                schema: group_441_endpoint(),
                options: Default::default(),
                dynamic_path: None,
            })
            .collect(),
        extra_targets: vec![NamedTarget {
            name: " Exact 雪 ".into(),
            path: None,
            schema: target,
            options: Default::default(),
            root: root.clone(),
        }],
        failure_rules: vec![],
        user_functions: Default::default(),
        root,
        graph: Graph {
            nodes: std::collections::BTreeMap::from([
                (1, group_441_node(&owner, &["Rows", "Key"], None)),
                (2, group_441_node(&owner, &["Rows", "Keep"], None)),
                (
                    3,
                    group_441_node(&owner, &["Rows", "Label"], Some(&["Rows"])),
                ),
                (
                    4,
                    Node::OwnedPosition(OwnedPosition {
                        owner: owner.clone(),
                        collection: Some(path(&["Rows"])),
                    }),
                ),
                (5, group_441_node(&owner, &["Marker"], Some(&[]))),
                (6, group_441_node(&owner, &["Rows", "Rank"], None)),
                (
                    7,
                    Node::SourceField {
                        path: path(&["Limit"]),
                        frame: None,
                    },
                ),
                (
                    8,
                    Node::Const {
                        value: Value::Int(1),
                    },
                ),
                (
                    9,
                    Node::Call {
                        function: "greater_than".into(),
                        args: vec![4, 8],
                    },
                ),
                (
                    10,
                    Node::SourceField {
                        path: path(&["Key"]),
                        frame: None,
                    },
                ),
                (
                    40,
                    Node::Const {
                        value: Value::String("group control".into()),
                    },
                ),
            ]),
        },
    }
}

fn group_441_program(project: &::mapping::Project) -> Program {
    let original = ::mapping::project_file::encode_pretty(project).unwrap();
    eprintln!("GROUP441 complete saved Project before lowering={original}");
    let result = codegen::lower_with_basic_owned_group_by(project);
    eprintln!("GROUP441 complete lowering result={result:#?}");
    result.expect("genuine By Project lowers through the explicit companion")
}

fn retain_public_group_441_consumer(
    label: &str,
    project: &::mapping::Project,
    body: &str,
) -> String {
    let program = group_441_program(project);
    let result = crate::emit_with_basic_owned_group_by(&program);
    eprintln!("GROUP441 {label} complete artifacts/refusal={result:#?}");
    let artifacts = result.expect("explicit By artifacts emit");
    let source = String::from_utf8(
        artifacts
            .files()
            .iter()
            .find(|file| file.path.as_str() == "GeneratedMapping.cs")
            .unwrap()
            .contents
            .clone(),
    )
    .unwrap();
    let host = public_basic_consumer(body, false);
    eprintln!("GROUP441 {label} complete public caller and independent comparisons={host}");
    if let Some(base) = std::env::var_os("FERRULE_PUBLIC_BASIC_CSHARP_HOST_DIR") {
        let base = std::path::PathBuf::from(base);
        assert!(base.is_absolute(), "public consumer root must be absolute");
        let root = base.join(label);
        std::fs::create_dir(&root).expect("fresh group consumer namespace");
        for entry in artifacts.files() {
            let destination = root.join("generated").join(entry.path.as_str());
            std::fs::create_dir_all(destination.parent().unwrap()).unwrap();
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(destination)
                .unwrap();
            std::io::Write::write_all(&mut file, &entry.contents).unwrap();
        }
        std::fs::create_dir(root.join("consumer")).unwrap();
        for (name, contents) in [
            (
                "consumer/PublicConsumer.csproj",
                PUBLIC_BASIC_CONSUMER_PROJECT.to_string(),
            ),
            ("consumer/Program.cs", host),
            (
                "Project.original.json",
                ::mapping::project_file::encode_pretty(project).unwrap(),
            ),
            ("Program.ir.original.txt", format!("{program:#?}\n")),
        ] {
            let mut destination = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(root.join(name))
                .unwrap();
            std::io::Write::write_all(&mut destination, contents.as_bytes()).unwrap();
        }
    }
    source
}

const GROUP441_INPUT: &str = r#"
FerruleInstance Row(string key,string label,bool keep,long rank)=>G(("Key",S(key)),("Label",S(label)),("Keep",new FerruleScalar(FerruleValue.FromBoolean(keep))),("Rank",I(rank)),("Limit",I(0)));
var input=G(("Marker",S("P.root")),("Limit",I(1)),("Rows",R(Row("A","P-a1",false,30),Row("B","P-b1",true,10),Row("A","P-a2",true,20),Row("C","P-c1",false,40),Row("B","P-b2",false,15))));
var reference=G(("Marker",S("N.root")),("Limit",I(1)),("Rows",R(Row("A","N-a1",false,300),Row("B","N-b1",false,100),Row("A","N-a2",true,200),Row("C","N-c1",false,400),Row("B","N-b2",true,150))));
var limits=Policy();var supplied=new[]{new NamedInput("Rows",reference)};
"#;
const GROUP441_P_ALL: &str = r#"[{"First":"P-a1","GroupPosition":1,"RootMarker":"P.root","Members":[{"Label":"P-a1","MemberPosition":1,"RootMarker":"P.root"},{"Label":"P-a2","MemberPosition":2,"RootMarker":"P.root"}]},{"First":"P-b1","GroupPosition":2,"RootMarker":"P.root","Members":[{"Label":"P-b1","MemberPosition":1,"RootMarker":"P.root"},{"Label":"P-b2","MemberPosition":2,"RootMarker":"P.root"}]},{"First":"P-c1","GroupPosition":3,"RootMarker":"P.root","Members":[{"Label":"P-c1","MemberPosition":1,"RootMarker":"P.root"}]}]"#;
const GROUP441_P_POST: &str = r#"[{"First":"P-a1","GroupPosition":1,"RootMarker":"P.root","Members":[{"Label":"P-a1","MemberPosition":1,"RootMarker":"P.root"},{"Label":"P-a2","MemberPosition":2,"RootMarker":"P.root"}]},{"First":"P-b1","GroupPosition":2,"RootMarker":"P.root","Members":[{"Label":"P-b1","MemberPosition":1,"RootMarker":"P.root"},{"Label":"P-b2","MemberPosition":2,"RootMarker":"P.root"}]}]"#;
const GROUP441_N_POST: &str = r#"[{"First":"N-a1","GroupPosition":1,"RootMarker":"N.root","Members":[{"Label":"N-a1","MemberPosition":1,"RootMarker":"N.root"},{"Label":"N-a2","MemberPosition":2,"RootMarker":"N.root"}]},{"First":"N-b1","GroupPosition":2,"RootMarker":"N.root","Members":[{"Label":"N-b1","MemberPosition":1,"RootMarker":"N.root"},{"Label":"N-b2","MemberPosition":2,"RootMarker":"N.root"}]}]"#;

fn group_441_success_body(expected_groups: &str) -> String {
    let expected = serde_json::to_string(&format!("{{\"Groups\":{expected_groups}}}")).unwrap();
    format!(
        r#"{GROUP441_INPUT}
var expected={expected};
foreach(var selection in new FerruleTargetSelection?[]{{null,FerruleTargetSelection.Primary(),FerruleTargetSelection.Named(" Exact 雪 ")}})
{{
    var probe=new Probe(supplied);var actual=Reach("GROUP441.complete",input,probe,limits,null,selection,()=>selection is null
        ? (object)GeneratedMapping.ExecuteWithBasicOwnedSources(input,probe,limits,null)
        : GeneratedMapping.ExecuteSelectedWithBasicOwnedSources(input,selection,probe,limits,null));
    Tree(actual,expected);Reads(actual,1,1);
    if(actual.Result is ExecutionOutputs all)Check(all.Extras.Count==1&&all.Extras[0].Name==" Exact 雪 "&&Json(Plain(all.Extras[0].Instance))==expected,"complete exact named output");
}}
"#
    )
}

#[test]
fn public_basic_group_by_441_preserves_old_gates_and_two_typed_entry_points() {
    let project = group_441_project(SourceOwner::Primary, true);
    let program = group_441_program(&project);
    for result in [
        crate::emit(&program),
        crate::emit_with_basic_owned_sources(&program),
        crate::emit_with_basic_owned_filters(&program),
        crate::emit_with_basic_owned_sort(&program),
        crate::emit_with_basic_owned_windows(&program),
        crate::emit_with_basic_owned_row_failures(&program),
    ] {
        assert!(matches!(result, Err(EmitError::ProgramValidation(_))));
    }
    let source = retain_public_group_441_consumer(
        "public-group-441-primary-post",
        &project,
        &group_441_success_body(GROUP441_P_POST),
    );
    assert_eq!(
        source
            .matches("public static ExecutionOutputs ExecuteWithBasicOwnedSources(")
            .count(),
        1
    );
    assert_eq!(
        source
            .matches("public static SelectedTargetOutput ExecuteSelectedWithBasicOwnedSources(")
            .count(),
        1
    );
    assert!(!source.contains("NamedJsonInput"));
    assert!(!source.contains("ExecuteJson"));
    assert_eq!(source.matches(".MapGroupBySourceCohort(").count(), 2);
    assert!(source.contains(
        "class GroupByControls_1 : global::Ferrule.Runtime.IFerruleBasicGroupByControls"
    ));
    assert!(source.contains("=> new(Descending, false, 0, false, false, true);"));
    assert!(source.contains("RequireBoolean(Node_2(candidate), 2U)"));
    let plain = basic(
        vec![expression(1, field(SourceOwner::Primary, &["Value"], None))],
        group(vec![binding("Value", 1, ScalarType::String)]),
    );
    assert_eq!(
        crate::emit_with_basic_owned_group_by(&plain),
        crate::emit_with_basic_owned_row_failures(&plain)
    );
}

#[test]
fn public_basic_group_by_441_represents_first_member_complete_bucket_and_exact_named_owner() {
    let primary = group_441_project(SourceOwner::Primary, false);
    let source = retain_public_group_441_consumer(
        "public-group-441-primary-all",
        &primary,
        &group_441_success_body(GROUP441_P_ALL),
    );
    assert!(source.contains("WalkOwnedSourceCohort(global::Ferrule.Runtime.FerruleOwnedSourceOwner.Primary, new string[] { \"Rows\" })"));
    assert!(source.contains("error.WithOwnedSourceNode(1U)"));
    assert!(source.contains("error.WithOwnedSourceNode(4U)"));
    let named_project = group_441_project(named("Rows"), true);
    let named_source = retain_public_group_441_consumer(
        "public-group-441-named-post",
        &named_project,
        &group_441_success_body(GROUP441_N_POST),
    );
    assert!(named_source.contains("WalkOwnedSourceCohort(global::Ferrule.Runtime.FerruleOwnedSourceOwner.Named(\"Rows\"), new string[] { \"Rows\" })"));
    assert!(named_source.contains("OwnedRootIsDeclaredNonrepeating("));
}

#[test]
fn public_basic_group_by_441_parent_windows_follow_sorted_candidate_key_and_post_roles() {
    let mut project = group_441_project(SourceOwner::Primary, true);
    let grouped = &mut project.root.children[0];
    grouped.sort_by = Some(6);
    grouped.windows = vec![::mapping::SequenceWindow::First { count: 7 }];
    project.extra_targets[0].root = project.root.clone();
    let expected = r#"[{"First":"P-b1","GroupPosition":1,"RootMarker":"P.root","Members":[{"Label":"P-b1","MemberPosition":1,"RootMarker":"P.root"},{"Label":"P-b2","MemberPosition":2,"RootMarker":"P.root"}]}]"#;
    let source = retain_public_group_441_consumer(
        "public-group-441-sort-parent-first",
        &project,
        &group_441_success_body(expected),
    );
    assert!(source.contains("0 => Node_6(candidate)"));
    assert!(source.contains("ItemCount(7U, Node_7(parent))"));
    assert!(source.contains("=> Node_1(candidate);"));
    assert!(source.contains("RequireBoolean(Node_2(candidate), 2U)"));
}

#[test]
fn public_basic_group_by_441_key_post_only_demand_keeps_members_paired_and_ordinary_by_island() {
    for only_post in [false, true] {
        let mut project = group_441_project(SourceOwner::Primary, true);
        project.extra_targets.clear();
        project.root.children[0].iteration = ::mapping::ScopeIteration::Source(path(&["Rows"]));
        project.root.children[0].children[0].iteration = ::mapping::ScopeIteration::Source(vec![]);
        for (id, field) in [(3, "Label"), (5, "Marker")] {
            project.graph.nodes.insert(
                id,
                ::mapping::Node::SourceField {
                    path: path(&[field]),
                    frame: None,
                },
            );
        }
        project
            .graph
            .nodes
            .insert(4, ::mapping::Node::Position { collection: vec![] });
        if only_post {
            project.graph.nodes.insert(
                1,
                ::mapping::Node::SourceField {
                    path: path(&["Key"]),
                    frame: None,
                },
            );
        } else {
            project.graph.nodes.insert(
                2,
                ::mapping::Node::SourceField {
                    path: path(&["Keep"]),
                    frame: None,
                },
            );
        }
        let mut ordinary = project.root.children[0].clone();
        ordinary.target_field = "Ordinary".into();
        ordinary.group_by = Some(10);
        ordinary.post_group_filter = None;
        project.root.children.push(ordinary);
        if let ir::SchemaKind::Group { children, .. } = &mut project.target.kind {
            let mut ordinary = children[0].clone();
            ordinary.name = "Ordinary".into();
            children.push(ordinary);
        }
        let expected = serde_json::to_string(&format!(
            "{{\"Groups\":{GROUP441_P_POST},\"Ordinary\":{GROUP441_P_ALL}}}"
        ))
        .unwrap();
        let body = format!(
            r#"{GROUP441_INPUT}
var probe=new Probe(supplied);var actual=Reach("GROUP441.key-post-only",input,probe,limits,null,null,()=>GeneratedMapping.ExecuteWithBasicOwnedSources(input,probe,limits,null));Tree(actual,{expected});Reads(actual,1,1);
"#
        );
        let source = retain_public_group_441_consumer(
            if only_post {
                "public-group-441-post-only"
            } else {
                "public-group-441-key-only"
            },
            &project,
            &body,
        );
        assert!(
            source.contains("var cohort_1 = context.WalkSourceCohort(new string[] { \"Rows\" })")
        );
        assert!(source.contains(
            "var cohort_2 = context.WalkSourceCohort(global::System.Array.Empty<string>())"
        ));
        assert!(!source.contains("return Scope_2(context.LegacyView)"));
        assert!(source.contains("return Scope_3(context.LegacyView)"));
        assert!(source.contains("context.GroupBy(candidates_3"));
    }
}

#[test]
fn public_basic_group_by_441_nonrepeating_owned_root_is_one_group_with_one_member() {
    let mut project = group_441_project(SourceOwner::Primary, false);
    project.extra_targets.clear();
    project.extra_sources.clear();
    project.source = SchemaNode::group(
        "Root",
        vec![
            SchemaNode::scalar("Key", ScalarType::String),
            SchemaNode::scalar("Label", ScalarType::String),
            SchemaNode::scalar("Marker", ScalarType::String),
        ],
    );
    {
        let scope = &mut project.root.children[0];
        scope.iteration = ::mapping::ScopeIteration::OwnedSource {
            source: OwnedSourceRef {
                owner: SourceOwner::Primary,
                path: vec![],
            },
            output_path: None,
        };
        scope.children[0].iteration = scope.iteration.clone();
    }
    project.graph.nodes = std::collections::BTreeMap::from([
        (1, group_441_node(&SourceOwner::Primary, &["Key"], None)),
        (3, group_441_node(&SourceOwner::Primary, &["Label"], None)),
        (
            4,
            ::mapping::Node::OwnedPosition(OwnedPosition {
                owner: SourceOwner::Primary,
                collection: Some(vec![]),
            }),
        ),
        (
            5,
            group_441_node(&SourceOwner::Primary, &["Marker"], Some(&[])),
        ),
    ]);
    let source = retain_public_group_441_consumer(
        "public-group-441-singleton-root",
        &project,
        r#"
var input=G(("Key",S("A")),("Label",S("one")),("Marker",S("root")));var probe=new Probe(Array.Empty<NamedInput>());var limits=Policy();
var actual=Reach("GROUP441.singleton",input,probe,limits,null,null,()=>GeneratedMapping.ExecuteWithBasicOwnedSources(input,probe,limits,null));
Tree(actual,"{\"Groups\":[{\"First\":\"one\",\"GroupPosition\":1,\"RootMarker\":\"root\",\"Members\":[{\"Label\":\"one\",\"MemberPosition\":1,\"RootMarker\":\"root\"}]}]}");Reads(actual,1,0);
"#,
    );
    assert!(source.contains("WalkOwnedSourceCohort(global::Ferrule.Runtime.FerruleOwnedSourceOwner.Primary, global::System.Array.Empty<string>())"));
}

#[test]
fn public_basic_group_by_441_primary_row_failures_precede_all_group_callbacks() {
    let mut project = group_441_project(SourceOwner::Primary, true);
    project
        .graph
        .nodes
        .insert(1, ::mapping::Node::Raise { message: Some(40) });
    project.failure_rules = vec![::mapping::FailureRule {
        iteration: ::mapping::FailureIteration::Source {
            collection: path(&["Rows"]),
        },
        selection: ::mapping::FailureSelection::All,
        message: Some(3),
    }];
    let body = format!(
        r#"{GROUP441_INPUT}
foreach(var selection in new FerruleTargetSelection?[]{{null,FerruleTargetSelection.Primary(),FerruleTargetSelection.Named(" Exact 雪 ")}})
{{
var probe=new Probe(supplied);var actual=Reach("GROUP441.global-before-key",input,probe,limits,null,selection,()=>selection is null
    ? (object)GeneratedMapping.ExecuteWithBasicOwnedSources(input,probe,limits,null)
    : GeneratedMapping.ExecuteSelectedWithBasicOwnedSources(input,selection,probe,limits,null));
Failure(actual,1,"P-a1");Reads(actual,1,1);
}}
"#
    );
    let source =
        retain_public_group_441_consumer("public-group-441-global-before-key", &project, &body);
    assert_eq!(source.matches(".VisitPrimaryFailureRows(").count(), 1);
    let evaluator = &source[source
        .find("private static object ExecuteCheckedBasicContext(")
        .unwrap()..];
    assert!(
        evaluator
            .find("EvaluateBasicFailureRules(context)")
            .unwrap()
            < evaluator.find("if (selected.HasValue)").unwrap()
    );
    assert!(
        evaluator
            .find("EvaluateBasicFailureRules(context)")
            .unwrap()
            < evaluator.find("Scope_0(context)").unwrap()
    );
}

#[test]
fn public_basic_group_by_441_fused_order_preserves_candidate_post_before_later_filter() {
    use ::mapping::Node;
    for prefilter in [false, true] {
        let mut project = group_441_project(SourceOwner::Primary, true);
        project.graph.nodes.extend([
            (2, Node::Raise { message: Some(40) }),
            (
                11,
                Node::Call {
                    function: "greater_than".into(),
                    args: vec![4, 8],
                },
            ),
            (12, Node::Raise { message: Some(40) }),
            (
                13,
                Node::Const {
                    value: Value::Bool(true),
                },
            ),
            (
                14,
                Node::If {
                    condition: 11,
                    then: 12,
                    else_: 13,
                },
            ),
        ]);
        let grouped = &mut project.root.children[0];
        grouped.filter = Some(14);
        if prefilter {
            grouped.sort_by = Some(6);
            grouped.sort_filter_order = ::mapping::SortFilterOrder::FilterThenSort;
        }
        project.extra_targets.clear();
        let expected = if prefilter { 12 } else { 2 };
        let body = format!(
            r#"{GROUP441_INPUT}
{PUBLIC703_CLR}{PUBLIC703_RAISE}
var probe=new Probe(supplied);var actual=Reach("GROUP441.filter-key-post-order",input,probe,limits,null,null,()=>GeneratedMapping.ExecuteWithBasicOwnedSources(input,probe,limits,null));MappingRaise(actual,{expected}U,"group control");Reads(actual,1,1);
"#
        );
        let source = retain_public_group_441_consumer(
            if prefilter {
                "public-group-441-prefilter-order"
            } else {
                "public-group-441-candidate-order"
            },
            &project,
            &body,
        );
        assert!(source.contains("RequireBoolean(Node_14(candidate), 14U)"));
        assert!(source.contains("RequireBoolean(Node_2(candidate), 2U)"));
        assert!(!source.contains("context.BeginRetainedFilterPhase(cohort_1)"));
    }
}

#[test]
fn public_basic_group_by_441_sort_bounds_and_first_zero_precede_group_output_only() {
    use ::mapping::Node;
    for stage in ["sort", "bound", "key-after-zero"] {
        let mut project = group_441_project(SourceOwner::Primary, true);
        project.extra_targets.clear();
        let grouped = &mut project.root.children[0];
        grouped.windows = vec![::mapping::SequenceWindow::First { count: 7 }];
        project
            .graph
            .nodes
            .insert(1, Node::Raise { message: Some(40) });
        let expected = match stage {
            "sort" => {
                grouped.sort_by = Some(6);
                project
                    .graph
                    .nodes
                    .insert(6, Node::Raise { message: Some(40) });
                project
                    .graph
                    .nodes
                    .insert(7, Node::Raise { message: Some(40) });
                6
            }
            "bound" => {
                grouped.sort_by = Some(6);
                project
                    .graph
                    .nodes
                    .insert(7, Node::Raise { message: Some(40) });
                7
            }
            _ => {
                project.graph.nodes.insert(
                    7,
                    Node::Const {
                        value: Value::Int(0),
                    },
                );
                1
            }
        };
        let body = format!(
            r#"{GROUP441_INPUT}
{PUBLIC703_CLR}{PUBLIC703_RAISE}
var probe=new Probe(supplied);var actual=Reach("GROUP441.sort-bound-key",input,probe,limits,null,null,()=>GeneratedMapping.ExecuteWithBasicOwnedSources(input,probe,limits,null));MappingRaise(actual,{expected}U,"group control");Reads(actual,1,1);
"#
        );
        retain_public_group_441_consumer(&format!("public-group-441-{stage}"), &project, &body);
    }
}

#[test]
fn public_basic_group_by_441_literals_never_become_scope_or_node_identifiers() {
    let mut project = group_441_project(SourceOwner::Primary, false);
    project.extra_targets.clear();
    let literal = "Node_1 Scope_0 ScopeItem_1";
    if let ir::SchemaKind::Group { children, .. } = &mut project.source.kind
        && let ir::SchemaKind::Group { children, .. } = &mut children[2].kind
    {
        children[0].name = literal.into();
    }
    project.graph.nodes.insert(
        1,
        group_441_node(&SourceOwner::Primary, &["Rows", literal], None),
    );
    let input = GROUP441_INPUT.replace("(\"Key\",S(key))", &format!("(\"{literal}\",S(key))"));
    let expected = serde_json::to_string(&format!("{{\"Groups\":{GROUP441_P_ALL}}}")).unwrap();
    let body = format!(
        r#"{input}
var probe=new Probe(supplied);var actual=Reach("GROUP441.literal-path",input,probe,limits,null,null,()=>GeneratedMapping.ExecuteWithBasicOwnedSources(input,probe,limits,null));Tree(actual,{expected});Reads(actual,1,1);Check(((ExecutionOutputs)actual.Result!).Extras.Count==0,"no named output");
"#
    );
    let source = retain_public_group_441_consumer("public-group-441-literals", &project, &body);
    assert!(source.contains("new string[] { \"Rows\", \"Node_1 Scope_0 ScopeItem_1\" }"));
    assert!(!source.contains("Node_1 Scope_0 ScopeItem_1_"));
}

#[test]
fn public_basic_group_by_441_empty_groups_still_evaluate_parent_bounds() {
    let mut project = group_441_project(SourceOwner::Primary, true);
    project.extra_targets.clear();
    project.root.children[0].windows = vec![::mapping::SequenceWindow::First { count: 7 }];
    project.graph.nodes.insert(
        7,
        ::mapping::Node::Const {
            value: Value::Bool(false),
        },
    );
    let source = retain_public_group_441_consumer(
        "public-group-441-empty-invalid-bound",
        &project,
        r#"
var input=G(("Marker",S("P.root")),("Limit",I(1)),("Rows",R()));var probe=new Probe(Array.Empty<NamedInput>());var limits=Policy();
var actual=Reach("GROUP441.empty-bound",input,probe,limits,null,null,()=>GeneratedMapping.ExecuteWithBasicOwnedSources(input,probe,limits,null));
var error=Runtime(actual,FerruleRuntimeError.NotAnItemCount);Check(error.Node==7U&&error.FoundKind==FerruleValueKind.Bool&&error.InnerException is null,"empty cohort still evaluates invalid parent bound before keys");NullMetadata(error,"Node","FoundKind");Reads(actual,1,0);
"#,
    );
    assert!(source.contains("ItemCount(7U, Node_7(parent))"));
}

#[test]
fn public_basic_group_by_441_rejects_other_grouping_modes_without_changing_saved_wire() {
    for mode in ["adjacent", "starting", "ending", "blocks"] {
        let mut project = group_441_project(SourceOwner::Primary, true);
        let scope = &mut project.root.children[0];
        scope.group_by = None;
        match mode {
            "adjacent" => scope.group_adjacent_by = Some(1),
            "starting" => scope.group_starting_with = Some(2),
            "ending" => scope.group_ending_with = Some(2),
            _ => scope.group_into_blocks = Some(8),
        }
        let saved = ::mapping::project_file::encode_pretty(&project).unwrap();
        let result = codegen::lower_with_basic_owned_group_by(&project);
        eprintln!(
            "GROUP441 closed {mode} complete Project={saved} complete typed refusal={result:#?}"
        );
        assert!(result.is_err(), "only By opens in the explicit profile");
        assert_eq!(
            saved,
            ::mapping::project_file::encode_pretty(&project).unwrap()
        );
    }
}

#[test]
fn public_basic_group_by_441_any_member_retention_never_short_circuits_later_predicates() {
    use ::mapping::Node;
    let mut project = group_441_project(SourceOwner::Primary, true);
    project.extra_targets.clear();
    project.graph.nodes.extend([
        (
            2,
            Node::If {
                condition: 15,
                then: 13,
                else_: 12,
            },
        ),
        (12, Node::Raise { message: Some(40) }),
        (
            13,
            Node::Const {
                value: Value::Bool(true),
            },
        ),
        (
            15,
            group_441_node(&SourceOwner::Primary, &["Rows", "Keep"], None),
        ),
    ]);
    let body = format!(
        r#"
{PUBLIC703_CLR}{PUBLIC703_RAISE}
var input=G(("Marker",S("P.root")),("Limit",I(1)),("Rows",R(
G(("Key",S("A")),("Label",S("first")),("Keep",new FerruleScalar(FerruleValue.FromBoolean(true))),("Rank",I(1)),("Limit",I(0))),
G(("Key",S("A")),("Label",S("later")),("Keep",new FerruleScalar(FerruleValue.FromBoolean(false))),("Rank",I(2)),("Limit",I(0))))));
var probe=new Probe(Array.Empty<NamedInput>());var limits=Policy();var actual=Reach("GROUP441.no-post-short-circuit",input,probe,limits,null,null,()=>GeneratedMapping.ExecuteWithBasicOwnedSources(input,probe,limits,null));MappingRaise(actual,12U,"group control");Reads(actual,1,0);
"#
    );
    retain_public_group_441_consumer("public-group-441-no-post-short-circuit", &project, &body);
}

#[test]
fn public_basic_group_by_441_all_rejected_buckets_never_construct_target_items() {
    let mut project = group_441_project(SourceOwner::Primary, true);
    project.graph.nodes.insert(
        2,
        ::mapping::Node::Const {
            value: Value::Bool(false),
        },
    );
    project
        .graph
        .nodes
        .insert(3, ::mapping::Node::Raise { message: Some(40) });
    retain_public_group_441_consumer(
        "public-group-441-all-post-false",
        &project,
        &group_441_success_body("[]"),
    );
}

#[test]
fn public_basic_group_by_441_unrelated_ordinary_grouping_modes_keep_legacy_dispatch() {
    use ::mapping::{Node, ScopeIteration};
    for (mode, call) in [
        ("by", "GroupBy"),
        ("adjacent", "GroupAdjacentBy"),
        ("starting", "GroupStartingWith"),
        ("ending", "GroupEndingWith"),
        ("blocks", "GroupIntoBlocks"),
    ] {
        let mut project = group_441_project(SourceOwner::Primary, true);
        project.extra_targets.clear();
        let grouped = &mut project.root.children[0];
        grouped.iteration = ScopeIteration::Source(path(&["Rows"]));
        grouped.children[0].iteration = ScopeIteration::Source(vec![]);
        grouped.group_by = None;
        match mode {
            "by" => grouped.group_by = Some(1),
            "adjacent" => grouped.group_adjacent_by = Some(1),
            "starting" => grouped.group_starting_with = Some(2),
            "ending" => grouped.group_ending_with = Some(2),
            _ => grouped.group_into_blocks = Some(8),
        }
        for (id, field) in [
            (1, "Key"),
            (2, "Keep"),
            (3, "Label"),
            (5, "Marker"),
            (6, "Rank"),
        ] {
            project.graph.nodes.insert(
                id,
                Node::SourceField {
                    path: path(&[field]),
                    frame: None,
                },
            );
        }
        project
            .graph
            .nodes
            .insert(4, Node::Position { collection: vec![] });
        let program = group_441_program(&project);
        let artifacts = crate::emit_with_basic_owned_group_by(&program)
            .expect("ordinary static grouping islands remain admitted");
        let source = std::str::from_utf8(
            &artifacts
                .files()
                .iter()
                .find(|file| file.path.as_str() == "GeneratedMapping.cs")
                .unwrap()
                .contents,
        )
        .unwrap();
        assert!(source.contains("return Scope_1(context.LegacyView)"));
        assert!(source.contains(&format!("context.{call}(candidates_1")));
        assert!(!source.contains(".MapGroupBySourceCohort("));
        assert!(!source.contains("class GroupByControls_"));
    }
}

#[test]
fn public_basic_group_by_441_implicit_first_selects_groups_and_keeps_complete_members() {
    let mut project = group_441_project(SourceOwner::Primary, true);
    project.extra_targets.clear();
    project.root.children[0].iteration_output = ::mapping::IterationOutput::First;
    if let ir::SchemaKind::Group { children, .. } = &mut project.target.kind {
        children[0].repeating = false;
    }
    let expected=serde_json::to_string(r#"{"Groups":{"First":"P-a1","GroupPosition":1,"RootMarker":"P.root","Members":[{"Label":"P-a1","MemberPosition":1,"RootMarker":"P.root"},{"Label":"P-a2","MemberPosition":2,"RootMarker":"P.root"}]}}"#).unwrap();
    let body = format!(
        r#"{GROUP441_INPUT}
var probe=new Probe(supplied);var actual=Reach("GROUP441.implicit-first",input,probe,limits,null,null,()=>GeneratedMapping.ExecuteWithBasicOwnedSources(input,probe,limits,null));Tree(actual,{expected});Reads(actual,1,1);
"#
    );
    let source =
        retain_public_group_441_consumer("public-group-441-implicit-first", &project, &body);
    assert!(source.contains("=> new(Descending, false, 0, true, false, true);"));
    assert!(
        source.contains("return items_1.Count == 0 ? new global::Ferrule.Runtime.FerruleGroup")
    );
}
