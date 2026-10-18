// Public callers are retained for the coordinator; emission is not execution.
// Every expected scalar/tree below is authored independently of engine/backend output.
fn itemat_473_endpoint() -> SchemaNode {
    SchemaNode::group(
        "Endpoint",
        vec![SchemaNode::group(
            "Cfg",
            vec![
                SchemaNode::scalar("Index", ScalarType::Int),
                SchemaNode::scalar("Text", ScalarType::String),
            ],
        )],
    )
}

fn itemat_473_project(sequence: ::mapping::SequenceExpr, result: ScalarType) -> ::mapping::Project {
    use ::mapping::{Graph, NamedSource, NamedTarget, Node, Project, Scope};
    let target = SchemaNode::group("Output", vec![SchemaNode::scalar("Result", result)]);
    let root = Scope {
        bindings: vec![::mapping::Binding {
            target_field: "Result".into(),
            node: 40,
        }],
        ..Scope::default()
    };
    Project {
        source: itemat_473_endpoint(),
        target: target.clone(),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: ["Cfg", " Cfg💠 "]
            .iter()
            .map(|name| NamedSource {
                name: (*name).into(),
                path: String::new(),
                schema: itemat_473_endpoint(),
                options: Default::default(),
                dynamic_path: None,
            })
            .collect(),
        extra_targets: vec![NamedTarget {
            name: " Exact💠 ".into(),
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
                (
                    10,
                    Node::Const {
                        value: Value::String("a|β|終".into()),
                    },
                ),
                (
                    11,
                    Node::Const {
                        value: Value::String("|".into()),
                    },
                ),
                (
                    12,
                    Node::Const {
                        value: Value::Int(2),
                    },
                ),
                (
                    13,
                    Node::Const {
                        value: Value::String("[,;]".into()),
                    },
                ),
                (
                    14,
                    Node::Const {
                        value: Value::String(String::new()),
                    },
                ),
                (
                    15,
                    Node::Const {
                        value: Value::Int(10),
                    },
                ),
                (
                    16,
                    Node::Const {
                        value: Value::Int(12),
                    },
                ),
                (
                    30,
                    group_441_node(&named("Cfg"), &["Cfg", "Index"], Some(&[])),
                ),
                (
                    40,
                    Node::SequenceItemAt {
                        sequence,
                        index: 30,
                    },
                ),
                (
                    80,
                    Node::Const {
                        value: Value::String("INDEX".into()),
                    },
                ),
                (
                    81,
                    Node::Const {
                        value: Value::String("PRODUCER".into()),
                    },
                ),
                (
                    90,
                    Node::SourceField {
                        path: vec![],
                        frame: None,
                    },
                ),
            ]),
        },
    }
}

fn itemat_473_tokens() -> ::mapping::SequenceExpr {
    ::mapping::SequenceExpr::Tokenize {
        input: 10,
        delimiter: 11,
        item: 90,
    }
}

fn itemat_473_program(project: &::mapping::Project) -> Program {
    eprintln!(
        "ITEMAT473 complete Project={}",
        ::mapping::project_file::encode_pretty(project).unwrap()
    );
    let result = codegen::lower_with_basic_owned_sequence_item_at(project);
    eprintln!("ITEMAT473 complete lowering={result:#?}");
    result.expect("genuine Project lowers through explicit ItemAt companion")
}

fn retain_public_itemat_473_consumer(
    label: &str,
    project: &::mapping::Project,
    body: &str,
) -> String {
    retain_public_itemat_473_consumer_with_probe(label, project, body, None)
}

fn retain_public_itemat_473_consumer_with_probe(
    label: &str,
    project: &::mapping::Project,
    body: &str,
    probe: Option<&str>,
) -> String {
    let program = itemat_473_program(project);
    let result = crate::emit_with_basic_owned_sequence_item_at(&program);
    eprintln!("ITEMAT473 {label} complete emission={result:#?}");
    let artifacts = result.expect("complete ItemAt artifacts emit");
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
    eprintln!("ITEMAT473 {label} complete independently expected public caller={host}");
    if let Some(probe) = probe {
        eprintln!(
            "ITEMAT473 {label} complete test-only existing partial observer implementation={probe}"
        );
    }
    if let Some(base) = std::env::var_os("FERRULE_PUBLIC_BASIC_CSHARP_HOST_DIR") {
        let base = std::path::PathBuf::from(base);
        assert!(base.is_absolute());
        let root = base.join(label);
        std::fs::create_dir(&root).expect("fresh ItemAt consumer namespace");
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
        if let Some(probe) = probe {
            // The emitted artifact bytes stay untouched. The existing Runtime
            // wildcard includes this separate private test-only partial file.
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(root.join("generated/Runtime/ItemAt473TestProbe.cs"))
                .unwrap();
            std::io::Write::write_all(&mut file, probe.as_bytes()).unwrap();
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
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(root.join(name))
                .unwrap();
            std::io::Write::write_all(&mut file, contents.as_bytes()).unwrap();
        }
    }
    source
}

const ITEMAT473_INPUT: &str = r#"
var input=G(("Cfg",G(("Index",I(1)),("Text",S("P-a|P-b|P-c")))));
var reference=G(("Cfg",G(("Index",I(3)),("Text",S("N-a|N-b|N-c")))));
var exact=G(("Cfg",G(("Index",I(2)),("Text",S("U-a|U-b|U-c")))));
var supplied=new[]{new NamedInput("Cfg",reference),new NamedInput(" Cfg💠 ",exact)};var limits=Policy();
"#;

const ITEMAT473_SCALAR: &str = r#"
void Scalar(Actual actual,FerruleValueKind kind,string? text=null,long? number=null)
{
    Check(actual.Error is null&&actual.Result is not null,"complete successful ItemAt result");
    void One(FerruleInstance instance)
    {
        Check(instance is FerruleGroup,"whole scalar result group");var group=(FerruleGroup)instance;
        Check(group.Fields.Count==1&&group.Fields[0].Name=="Result"&&group.Fields[0].Value is FerruleScalar,"exact one ordered scalar result field");
        var value=((FerruleScalar)group.Fields[0].Value).Value;Check(value.Kind==kind,"exact selected scalar tag");
        if(kind==FerruleValueKind.String)Check(value.StringValue==text,"exact selected scalar text");
        if(kind==FerruleValueKind.Int64)Check(number.HasValue&&value.Int64Value==number.Value,"exact selected scalar integer");
    }
    switch(actual.Result)
    {
        case ExecutionOutputs all:One(all.Primary);Check(all.Extras.Count==1&&all.Extras[0].Name==" Exact💠 ","complete exact named target");One(all.Extras[0].Instance);break;
        case SelectedTargetOutput.Primary primary:One(primary.Instance);break;
        case SelectedTargetOutput.Named named:Check(named.Output.Name==" Exact💠 ","exact selected named target");One(named.Output.Instance);break;
        default:throw new InvalidOperationException("unexpected public ItemAt result");
    }
}
"#;

fn itemat_473_success_body(kind: &str, text: Option<&str>, number: Option<i64>) -> String {
    let text = text
        .map(crate::literal::string)
        .unwrap_or_else(|| "null".into());
    let number = number
        .map(|value| value.to_string())
        .unwrap_or_else(|| "null".into());
    format!(
        r#"{ITEMAT473_INPUT}{ITEMAT473_SCALAR}
foreach(var selection in new FerruleTargetSelection?[]{{null,FerruleTargetSelection.Primary(),FerruleTargetSelection.Named(" Exact💠 ")}})
{{
var probe=new Probe(supplied);var actual=Reach("ITEMAT473.scalar",input,probe,limits,null,selection,()=>selection is null
    ? (object)GeneratedMapping.ExecuteWithBasicOwnedSources(input,probe,limits,null)
    : GeneratedMapping.ExecuteSelectedWithBasicOwnedSources(input,selection,probe,limits,null));
Scalar(actual,FerruleValueKind.{kind},{text},{number});Reads(actual,1,2);
}}
"#
    )
}

#[test]
fn public_basic_itemat_473_four_pure_producers_keep_exact_tags_and_parent_owner() {
    use ::mapping::{Node, SequenceExpr};
    for (id, sequence, ty, expected_text, expected_int) in [
        (
            "tokens",
            itemat_473_tokens(),
            ScalarType::String,
            Some("終"),
            None,
        ),
        (
            "range",
            SequenceExpr::Generate {
                from: Some(15),
                to: 16,
                item: 90,
            },
            ScalarType::Int,
            None,
            Some(12),
        ),
        (
            "length",
            SequenceExpr::TokenizeByLength {
                input: 10,
                length: 12,
                item: 90,
            },
            ScalarType::String,
            Some("βz"),
            None,
        ),
        (
            "regex",
            SequenceExpr::TokenizeRegex {
                input: 10,
                pattern: 13,
                flags: None,
                item: 90,
            },
            ScalarType::String,
            Some("bb"),
            None,
        ),
        (
            "regex-flags",
            SequenceExpr::TokenizeRegex {
                input: 10,
                pattern: 13,
                flags: Some(14),
                item: 90,
            },
            ScalarType::String,
            Some("bb"),
            None,
        ),
    ] {
        let mut project = itemat_473_project(sequence, ty);
        if id == "length" || id.starts_with("regex") {
            project.graph.nodes.insert(
                30,
                group_441_node(&named(" Cfg💠 "), &["Cfg", "Index"], Some(&[])),
            );
            project.graph.nodes.insert(
                10,
                Node::Const {
                    value: Value::String(
                        if id == "length" {
                            "a💠βzQ"
                        } else {
                            "a,bb;終"
                        }
                        .into(),
                    ),
                },
            );
        }
        let body = itemat_473_success_body(
            if ty == ScalarType::Int {
                "Int64"
            } else {
                "String"
            },
            expected_text,
            expected_int,
        );
        let source =
            retain_public_itemat_473_consumer(&format!("public-itemat-473-{id}"), &project, &body);
        let method = &source[source.find("FerruleValue NodeBody_40(").unwrap()..];
        let method = &method[..method.find("\n    }\n").unwrap()];
        assert!(
            method.find("sequence_values_node_40").unwrap()
                < method
                    .find("var sequence_index_node_40 = Node_30(context);")
                    .unwrap()
        );
        assert_eq!(method.matches("Node_30(context)").count(), 1);
        assert!(method.contains(
            "FerruleAggregateOperation.ItemAt, sequence_values_node_40, sequence_index_node_40"
        ));
        assert!(!method.contains("EnumerateGenerated"));
        assert!(!method.contains("ItemCount("));
        assert!(!source.contains("ExecuteJson"));
    }
}

#[test]
fn public_basic_itemat_473_owned_producer_arguments_and_nested_consumers_stay_parent_bound() {
    use ::mapping::{Node, SequenceExpr};
    let mut project = itemat_473_project(itemat_473_tokens(), ScalarType::String);
    project.graph.nodes.insert(
        10,
        group_441_node(&named("Cfg"), &["Cfg", "Text"], Some(&[])),
    );
    retain_public_itemat_473_consumer(
        "public-itemat-473-owned-producer",
        &project,
        &itemat_473_success_body("String", Some("N-c"), None),
    );
    project.graph.nodes.insert(
        31,
        Node::SequenceItemAt {
            sequence: SequenceExpr::Generate {
                from: None,
                to: 30,
                item: 91,
            },
            index: 30,
        },
    );
    project.graph.nodes.insert(
        91,
        Node::SourceField {
            path: vec![],
            frame: None,
        },
    );
    if let Some(Node::SequenceItemAt { index, .. }) = project.graph.nodes.get_mut(&40) {
        *index = 31;
    }
    let source = retain_public_itemat_473_consumer(
        "public-itemat-473-nested",
        &project,
        &itemat_473_success_body("String", Some("N-c"), None),
    );
    let method = &source[source.find("FerruleValue NodeBody_31(").unwrap()..];
    let method = &method[..method.find("\n    }\n").unwrap()];
    assert_eq!(
        method.matches("Node_30(context)").count(),
        2,
        "producer and index are separate semantic edges"
    );
    assert!(!method.contains("EnumerateGenerated"));
}

#[test]
fn public_basic_itemat_473_empty_null_and_producer_failures_keep_index_order() {
    use ::mapping::{Node, SequenceExpr};
    for id in ["empty", "null", "json-null", "producer"] {
        let mut project = itemat_473_project(
            if id == "empty" {
                SequenceExpr::Generate {
                    from: Some(15),
                    to: 16,
                    item: 90,
                }
            } else {
                itemat_473_tokens()
            },
            ScalarType::String,
        );
        project
            .graph
            .nodes
            .insert(30, Node::Raise { message: Some(80) });
        let mut captured_input = String::new();
        let (node, message) = match id {
            "empty" => {
                project.target = SchemaNode::group(
                    "Output",
                    vec![SchemaNode::scalar("Result", ScalarType::Int)],
                );
                project.extra_targets[0].schema = project.target.clone();
                project.graph.nodes.insert(
                    15,
                    Node::Const {
                        value: Value::Int(4),
                    },
                );
                project.graph.nodes.insert(
                    16,
                    Node::Const {
                        value: Value::Int(3),
                    },
                );
                (30, "INDEX")
            }
            "null" | "json-null" => {
                project.graph.nodes.insert(
                    10,
                    group_441_node(&named("Cfg"), &["Cfg", "Text"], Some(&[])),
                );
                if let ir::SchemaKind::Group { children, .. } =
                    &mut project.extra_sources[0].schema.kind
                    && let ir::SchemaKind::Group { children, .. } = &mut children[0].kind
                {
                    children[1] = SchemaNode::scalar("Text", ScalarType::String)
                        .nullable()
                        .unwrap();
                }
                let null = if id == "null" { "Null" } else { "JsonNull" };
                captured_input = format!(
                    "reference=G((\"Cfg\",G((\"Index\",I(3)),(\"Text\",new FerruleScalar(FerruleValue.{null})))));supplied=new[]{{new NamedInput(\"Cfg\",reference),new NamedInput(\" Cfg💠 \",exact)}};\n"
                );
                project
                    .graph
                    .nodes
                    .insert(11, Node::Raise { message: Some(81) });
                (30, "INDEX")
            }
            _ => {
                project
                    .graph
                    .nodes
                    .insert(10, Node::Raise { message: Some(81) });
                (10, "PRODUCER")
            }
        };
        let body = format!(
            r#"{ITEMAT473_INPUT}{captured_input}{PUBLIC703_CLR}{PUBLIC703_RAISE}
var probe=new Probe(supplied);var actual=Reach("ITEMAT473.order",input,probe,limits,null,null,()=>GeneratedMapping.ExecuteWithBasicOwnedSources(input,probe,limits,null));MappingRaise(actual,{node}U,"{message}");Reads(actual,1,2);
"#
        );
        retain_public_itemat_473_consumer(
            &format!("public-itemat-473-order-{id}"),
            &project,
            &body,
        );
    }
}

#[test]
fn public_basic_itemat_473_missing_named_index_never_uses_matching_primary_even_when_empty() {
    use ::mapping::Node;
    for empty in [false, true] {
        let mut project = itemat_473_project(itemat_473_tokens(), ScalarType::String);
        if empty {
            project
                .graph
                .nodes
                .insert(10, Node::Const { value: Value::Null });
        }
        let body = format!(
            r#"{ITEMAT473_INPUT}
var probe=new Probe(Array.Empty<NamedInput>());var actual=Reach("ITEMAT473.named-absent",input,probe,limits,null,null,()=>GeneratedMapping.ExecuteWithBasicOwnedSources(input,probe,limits,null));OwnedAbsent(actual,"Cfg",30U);Reads(actual,1,0);
"#
        );
        retain_public_itemat_473_consumer(
            if empty {
                "public-itemat-473-empty-named-absent"
            } else {
                "public-itemat-473-named-absent"
            },
            &project,
            &body,
        );
    }
}

#[test]
fn public_basic_itemat_473_by_representatives_and_members_keep_source_positions() {
    use ::mapping::{Node, SequenceExpr};
    let mut project = group_441_project(named("Rows"), false);
    for schema in std::iter::once(&mut project.source).chain(
        project
            .extra_sources
            .iter_mut()
            .map(|source| &mut source.schema),
    ) {
        if let ir::SchemaKind::Group { children, .. } = &mut schema.kind
            && let ir::SchemaKind::Group { children, .. } = &mut children[2].kind
        {
            children.push(SchemaNode::scalar("Index", ScalarType::Int));
        }
    }
    project.graph.nodes.extend([
        (
            10,
            Node::Const {
                value: Value::String("x|y|z".into()),
            },
        ),
        (
            11,
            Node::Const {
                value: Value::String("|".into()),
            },
        ),
        (
            30,
            group_441_node(&named("Rows"), &["Rows", "Index"], Some(&["Rows"])),
        ),
        (
            40,
            Node::SequenceItemAt {
                sequence: itemat_473_tokens(),
                index: 30,
            },
        ),
        (
            41,
            Node::SequenceItemAt {
                sequence: SequenceExpr::Tokenize {
                    input: 10,
                    delimiter: 11,
                    item: 91,
                },
                index: 4,
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
            91,
            Node::SourceField {
                path: vec![],
                frame: None,
            },
        ),
    ]);
    let bindings = vec![
        ::mapping::Binding {
            target_field: "Result".into(),
            node: 40,
        },
        ::mapping::Binding {
            target_field: "ByPosition".into(),
            node: 41,
        },
    ];
    project.root.children[0].bindings = bindings.clone();
    project.root.children[0].children[0].bindings = bindings;
    let fields = || {
        vec![
            SchemaNode::scalar("Result", ScalarType::String),
            SchemaNode::scalar("ByPosition", ScalarType::String),
        ]
    };
    let mut grouped = fields();
    grouped.push(SchemaNode::group("Members", fields()).repeating());
    project.target = SchemaNode::group(
        "Output",
        vec![SchemaNode::group("Groups", grouped).repeating()],
    );
    project.extra_targets[0].schema = project.target.clone();
    project.extra_targets[0].root = project.root.clone();
    let source = retain_public_itemat_473_consumer(
        "public-itemat-473-by-members",
        &project,
        r#"
var input=G(("Rows",R(G(("Key",S("A")),("Index",I(1))))));
var reference=G(("Rows",R(G(("Key",S("A")),("Index",I(3))),G(("Key",S("A")),("Index",I(1))),G(("Key",S("B")),("Index",I(2))))));
var limits=Policy();var supplied=new[]{new NamedInput("Rows",reference)};
foreach(var selection in new FerruleTargetSelection?[]{null,FerruleTargetSelection.Primary(),FerruleTargetSelection.Named(" Exact 雪 ")})
{
var probe=new Probe(supplied);var actual=Reach("ITEMAT473.by-members",input,probe,limits,null,selection,()=>selection is null?(object)GeneratedMapping.ExecuteWithBasicOwnedSources(input,probe,limits,null):GeneratedMapping.ExecuteSelectedWithBasicOwnedSources(input,selection,probe,limits,null));
Tree(actual,"{\"Groups\":[{\"Result\":\"z\",\"ByPosition\":\"x\",\"Members\":[{\"Result\":\"z\",\"ByPosition\":\"x\"},{\"Result\":\"x\",\"ByPosition\":\"y\"}]},{\"Result\":\"y\",\"ByPosition\":\"y\",\"Members\":[{\"Result\":\"y\",\"ByPosition\":\"x\"}]}]}");Reads(actual,1,1);
}
"#,
    );
    assert!(source.contains(".MapGroupBySourceCohort("));
    assert!(source.contains("class GroupByControls_1"));
}

#[test]
fn public_basic_itemat_473_old_profiles_and_failure_consumers_remain_closed() {
    let project = itemat_473_project(itemat_473_tokens(), ScalarType::String);
    let program = itemat_473_program(&project);
    for result in [
        crate::emit(&program),
        crate::emit_with_basic_owned_sources(&program),
        crate::emit_with_basic_owned_filters(&program),
        crate::emit_with_basic_owned_sort(&program),
        crate::emit_with_basic_owned_windows(&program),
        crate::emit_with_basic_owned_row_failures(&program),
        crate::emit_with_basic_owned_group_by(&program),
    ] {
        assert!(matches!(result, Err(EmitError::ProgramValidation(_))));
    }
    let mut closed = project;
    closed.failure_rules.push(::mapping::FailureRule {
        iteration: ::mapping::FailureIteration::Source { collection: vec![] },
        selection: ::mapping::FailureSelection::All,
        message: Some(40),
    });
    let failure = codegen::lower_with_basic_owned_sequence_item_at(&closed);
    eprintln!("ITEMAT473 closed failure complete typed result={failure:#?}");
    assert!(failure.is_err());
    let plain = basic(
        vec![expression(1, field(SourceOwner::Primary, &["Value"], None))],
        group(vec![binding("Value", 1, ScalarType::String)]),
    );
    assert_eq!(
        crate::emit_with_basic_owned_sequence_item_at(&plain),
        crate::emit_with_basic_owned_group_by(&plain)
    );
}

#[test]
fn public_basic_itemat_473_direct_program_rejects_nonpure_producers() {
    let mut program =
        itemat_473_program(&itemat_473_project(itemat_473_tokens(), ScalarType::String));
    // A real recursive anchor is required before producer capability can be
    // tested. Empty children on the old nonrepeating endpoint was structurally
    // invalid and correctly failed before reaching the owned capability gate.
    program.source = SchemaNode::group(
        "Directory",
        vec![
            SchemaNode::scalar("name", ScalarType::String),
            SchemaNode::group(
                "files",
                vec![SchemaNode::scalar("name", ScalarType::String)],
            )
            .repeating(),
            SchemaNode::recursive_group("children", "Directory").repeating(),
        ],
    );
    let consumer = program
        .expressions
        .iter_mut()
        .find(|node| node.id == 40)
        .unwrap();
    consumer.expression = Expression::SequenceItemAt {
        sequence: codegen::GeneratedSequence::RecursiveCollect {
            collection: vec![],
            children: path(&["children"]),
            descent_value: path(&["name"]),
            values: path(&["files"]),
            value: path(&["name"]),
            prefix: 10,
            separator: 11,
            item: 90,
        },
        index: 30,
    };
    let mut ordinary = program.clone();
    ordinary
        .expressions
        .iter_mut()
        .find(|node| node.id == 30)
        .unwrap()
        .expression = Expression::Const {
        value: Value::Int(3),
    };
    assert_eq!(
        codegen::validate_program(&ordinary),
        Ok(()),
        "recursive producer is structurally valid on its existing ordinary route"
    );

    let mut malformed = program.clone();
    if let Expression::SequenceItemAt {
        sequence: codegen::GeneratedSequence::RecursiveCollect { children, .. },
        ..
    } = &mut malformed
        .expressions
        .iter_mut()
        .find(|node| node.id == 40)
        .unwrap()
        .expression
    {
        *children = path(&["MissingChildren473"]);
    }
    let malformed = crate::emit_with_basic_owned_sequence_item_at(&malformed);
    eprintln!("ITEMAT473 malformed producer complete structural-first result={malformed:#?}");
    assert_eq!(
        malformed,
        Err(EmitError::ProgramValidation(
            ProgramValidationError::InvalidRecursiveSequencePath {
                owner: codegen::SequenceOwner::Expression(40),
                role: codegen::RecursiveSequencePathRole::Children,
                path: path(&["MissingChildren473"]),
            }
        ))
    );
    let result = crate::emit_with_basic_owned_sequence_item_at(&program);
    eprintln!("ITEMAT473 closed producer complete typed result={result:#?}");
    assert!(matches!(
        result,
        Err(EmitError::ProgramValidation(ProgramValidationError::OwnedSource { diagnostic }))
            if matches!(*diagnostic, codegen::OwnedSourceDiagnostic::Capability { .. })
    ));
}

#[test]
fn public_basic_itemat_473_index_conversion_uses_itemat_not_window_counts() {
    let mut project = itemat_473_project(itemat_473_tokens(), ScalarType::String);
    let types = ir::ScalarTypeSet::new([
        ScalarType::String,
        ScalarType::Int,
        ScalarType::Float,
        ScalarType::Bool,
    ])
    .unwrap();
    for endpoint in std::iter::once(&mut project.source).chain(
        project
            .extra_sources
            .iter_mut()
            .map(|source| &mut source.schema),
    ) {
        if let ir::SchemaKind::Group { children, .. } = &mut endpoint.kind
            && let ir::SchemaKind::Group { children, .. } = &mut children[0].kind
        {
            children[0] = SchemaNode::scalar_union("Index", types).nullable().unwrap();
        }
    }
    let body = format!(
        r#"{ITEMAT473_SCALAR}
var input=G(("Cfg",G(("Index",I(1)),("Text",S("P-a|P-b|P-c")))));var limits=Policy();
var cases=new (FerruleValue Index,string? Text)[]{{
    (FerruleValue.FromInt64(0),null),(FerruleValue.FromInt64(-1),null),(FerruleValue.FromInt64(4),null),(FerruleValue.FromInt64(long.MaxValue),null),
    (FerruleValue.FromBoolean(true),null),(FerruleValue.Null,null),(FerruleValue.JsonNull,null),(FerruleValue.XmlNil,null),
    (FerruleValue.FromString("2.0"),null),(FerruleValue.FromString("2e0"),null),(FerruleValue.FromString(" 2 "),"β"),
    (FerruleValue.FromDouble(2.49),"β"),(FerruleValue.FromDouble(2.5),"終"),(FerruleValue.FromDouble(0.5),"a"),(FerruleValue.FromDouble(-0.5),null),
    (FerruleValue.FromDouble(double.NaN),null),(FerruleValue.FromDouble(double.PositiveInfinity),null),(FerruleValue.FromDouble(double.NegativeInfinity),null),
}};
foreach(var item in cases)
{{
var reference=G(("Cfg",G(("Index",new FerruleScalar(item.Index)),("Text",S("N-a|N-b|N-c")))));
var probe=new Probe(new[]{{new NamedInput("Cfg",reference)}});var actual=Reach("ITEMAT473.conversion",input,probe,limits,null,null,()=>GeneratedMapping.ExecuteWithBasicOwnedSources(input,probe,limits,null));
Scalar(actual,item.Text is null?FerruleValueKind.Null:FerruleValueKind.String,item.Text);Reads(actual,1,1);
}}
"#
    );
    let source = retain_public_itemat_473_consumer("public-itemat-473-conversion", &project, &body);
    let method = &source[source.find("FerruleValue NodeBody_40(").unwrap()..];
    let method = &method[..method.find("\n    }\n").unwrap()];
    assert!(!method.contains("ItemCount("));
}

#[test]
fn public_basic_itemat_473_selected_catalogues_separate_owned_optional_from_legacy_required() {
    use ::mapping::{NamedSource, NamedTarget, Node, Scope};
    use codegen::NamedInputAvailability::{Optional, RequiredLegacy};
    let mut project = itemat_473_project(itemat_473_tokens(), ScalarType::String);
    project.extra_sources.push(NamedSource {
        name: "Unused".into(),
        path: String::new(),
        schema: itemat_473_endpoint(),
        options: Default::default(),
        dynamic_path: None,
    });
    project.graph.nodes.insert(
        50,
        Node::SourceField {
            path: path(&["Cfg", "Cfg", "Index"]),
            frame: None,
        },
    );
    project.extra_targets.push(NamedTarget {
        name: "legacyCfg".into(),
        path: None,
        schema: SchemaNode::group(
            "Legacy",
            vec![SchemaNode::scalar("Result", ScalarType::Int)],
        ),
        options: Default::default(),
        root: Scope {
            bindings: vec![::mapping::Binding {
                target_field: "Result".into(),
                node: 50,
            }],
            ..Scope::default()
        },
    });
    let program = itemat_473_program(&project);
    let plan =
        codegen::legacy_named_input_admission_plan_with_basic_owned_sequence_item_at(&program)
            .unwrap();
    let optional = vec![
        (0, "Cfg", Optional),
        (1, " Cfg💠 ", Optional),
        (2, "Unused", Optional),
    ];
    let required = vec![
        (0, "Cfg", RequiredLegacy),
        (1, " Cfg💠 ", Optional),
        (2, "Unused", Optional),
    ];
    assert_eq!(checked_rows(&plan.primary), optional);
    assert_eq!(checked_rows(&plan.named_targets[0].catalogue), optional);
    assert_eq!(checked_rows(&plan.named_targets[1].catalogue), required);
    assert_eq!(checked_rows(&plan.all_outputs), required);
    let body = format!(
        r#"{ITEMAT473_INPUT}{ITEMAT473_SCALAR}
foreach(var selection in new[]{{FerruleTargetSelection.Primary(),FerruleTargetSelection.Named(" Exact💠 ")}})
{{
var missing=new Probe(Array.Empty<NamedInput>());var absent=Reach("ITEMAT473.optional-owned",input,missing,limits,null,selection,()=>GeneratedMapping.ExecuteSelectedWithBasicOwnedSources(input,selection,missing,limits,null));OwnedAbsent(absent,"Cfg",30U);Reads(absent,1,0);
var foreign=new Probe(new[]{{new NamedInput(" Cfg💠 ",exact)}});var wrong=Reach("ITEMAT473.exact-owned",input,foreign,limits,null,selection,()=>GeneratedMapping.ExecuteSelectedWithBasicOwnedSources(input,selection,foreign,limits,null));OwnedAbsent(wrong,"Cfg",30U);Reads(wrong,1,1);
var present=new Probe(supplied);var actual=Reach("ITEMAT473.selected-owned",input,present,limits,null,selection,()=>GeneratedMapping.ExecuteSelectedWithBasicOwnedSources(input,selection,present,limits,null));Scalar(actual,FerruleValueKind.String,"終");Reads(actual,1,2);
}}
var legacy=FerruleTargetSelection.Named("legacyCfg");
var p0=new Probe(Array.Empty<NamedInput>());var a=Reach("ITEMAT473.required-legacy",input,p0,limits,null,legacy,()=>GeneratedMapping.ExecuteSelectedWithBasicOwnedSources(input,legacy,p0,limits,null));InputError(a,FerruleRuntimeError.MissingNamedSource,"Cfg","named source is required by this mapping");Reads(a,1,0);
var p1=new Probe(new[]{{new NamedInput(" Cfg💠 ",exact)}});var b=Reach("ITEMAT473.required-exact-legacy",input,p1,limits,null,legacy,()=>GeneratedMapping.ExecuteSelectedWithBasicOwnedSources(input,legacy,p1,limits,null));InputError(b,FerruleRuntimeError.MissingNamedSource,"Cfg","named source is required by this mapping");Reads(b,1,1);
var p2=new Probe(supplied);var c=Reach("ITEMAT473.legacy-present",input,p2,limits,null,legacy,()=>GeneratedMapping.ExecuteSelectedWithBasicOwnedSources(input,legacy,p2,limits,null));Tree(c,"{{\"Result\":3}}");Check(c.Result is SelectedTargetOutput.Named selected&&selected.Output.Name=="legacyCfg","exact legacy selected target");Reads(c,1,2);
var p3=new Probe(Array.Empty<NamedInput>());var d=Reach("ITEMAT473.all-required-before-owned",input,p3,limits,null,null,()=>GeneratedMapping.ExecuteWithBasicOwnedSources(input,p3,limits,null));InputError(d,FerruleRuntimeError.MissingNamedSource,"Cfg","named source is required by this mapping");Reads(d,1,0);
"#
    );
    let source =
        retain_public_itemat_473_consumer("public-itemat-473-selected-catalogues", &project, &body);
    assert!(
        !source.contains("FerruleBasicNamedSourceMode.ReservedUnavailable"),
        "unused static declarations are Optional, not reserved dynamic slots"
    );
}

const ITEMAT473_PRIVATE_PROBE: &str = r#"namespace Ferrule.Generated;
public static partial class GeneratedMapping
{
    private static readonly global::System.Collections.Generic.List<uint> ItemAt473TestNodes = new();
    static partial void ObserveOwnedBasicNode(uint node, global::Ferrule.Runtime.FerruleBasicScopeContext context)
        => ItemAt473TestNodes.Add(node);
}
"#;

#[test]
fn public_basic_itemat_473_private_node_probe_counts_reached_edges_and_empty_index_once() {
    use ::mapping::{Node, SequenceExpr};
    for id in ["tokens", "empty", "shared-node"] {
        let sequence = match id {
            "tokens" => itemat_473_tokens(),
            "empty" => SequenceExpr::Generate {
                from: Some(15),
                to: 16,
                item: 90,
            },
            _ => SequenceExpr::Generate {
                from: None,
                to: 30,
                item: 90,
            },
        };
        let mut project = itemat_473_project(
            sequence,
            if id == "tokens" {
                ScalarType::String
            } else {
                ScalarType::Int
            },
        );
        if id == "empty" {
            project.graph.nodes.insert(
                15,
                Node::Const {
                    value: Value::Int(4),
                },
            );
            project.graph.nodes.insert(
                16,
                Node::Const {
                    value: Value::Int(3),
                },
            );
        }
        let (kind, text, number, expected) = match id {
            "tokens" => ("String", "\"終\"", "null", "40U,10U,11U,30U"),
            "empty" => ("Null", "null", "null", "40U,15U,16U,30U"),
            _ => ("Int64", "null", "3", "40U,30U,30U"),
        };
        let body = format!(
            r#"{ITEMAT473_INPUT}{ITEMAT473_SCALAR}
var field=typeof(GeneratedMapping).GetField("ItemAt473TestNodes",BindingFlags.NonPublic|BindingFlags.Static)??throw new InvalidOperationException("private test probe missing");
var nodes=(List<uint>)(field.GetValue(null)??throw new InvalidOperationException("private test probe is null"));
var expected=new uint[]{{{expected}}};
foreach(var selection in new FerruleTargetSelection?[]{{null,FerruleTargetSelection.Primary(),FerruleTargetSelection.Named(" Exact💠 ")}})
{{
nodes.Clear();var probe=new Probe(supplied);var actual=Reach("ITEMAT473.actual-node-order",input,probe,limits,null,selection,()=>selection is null?(object)GeneratedMapping.ExecuteWithBasicOwnedSources(input,probe,limits,null):GeneratedMapping.ExecuteSelectedWithBasicOwnedSources(input,selection,probe,limits,null));
Console.WriteLine(Json(new{{classification="actual_reached_nodes_before_comparison",nodes=nodes.ToArray()}}));
Scalar(actual,FerruleValueKind.{kind},{text},{number});Reads(actual,1,2);Check(nodes.SequenceEqual(selection is null?expected.Concat(expected):expected),"complete reached order; producer values before once-per-invocation index; no synthetic item calls");
}}
nodes.Clear();var missing=FerruleTargetSelection.Named("missing");var zero=new FerruleOwnedSourceLimits(0,0,0,0,0);var p0=new Probe(supplied,throwCount:true);var a=Reach("ITEMAT473.no-callbacks-unknown",input,p0,zero,null,missing,()=>GeneratedMapping.ExecuteSelectedWithBasicOwnedSources(input,missing,p0,zero,null));InputError(a,FerruleRuntimeError.UnknownTarget,"missing","project has no named target `missing`");Reads(a,0,0);Check(nodes.Count==0,"unknown selection has no producer/index callbacks");
nodes.Clear();var p1=new Probe(new[]{{new NamedInput("X",reference)}});var b=Reach("ITEMAT473.no-callbacks-admission",input,p1,limits,null,null,()=>GeneratedMapping.ExecuteWithBasicOwnedSources(input,p1,limits,null));InputError(b,FerruleRuntimeError.UnexpectedNamedSource,"X","named source is not declared by this mapping");Reads(b,1,1);Check(nodes.Count==0,"name admission has no producer/index callbacks");
"#
        );
        let source = retain_public_itemat_473_consumer_with_probe(
            &format!("public-itemat-473-node-probe-{id}"),
            &project,
            &body,
            Some(ITEMAT473_PRIVATE_PROBE),
        );
        assert!(source.contains("static partial void ObserveOwnedBasicNode(uint node"));
        assert!(
            !source.contains("ItemAt473TestNodes"),
            "test probe stays a separate private source, not a generated public API"
        );
    }
}
