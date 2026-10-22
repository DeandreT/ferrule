// #442 public consumers reuse the By fixture and mandatory-policy API host.
// Literal run products below are independent of actual emission/execution.
fn adjacent_442_project(owner: SourceOwner) -> ::mapping::Project {
    let mut p = group_441_project(owner, false);
    p.root.children[0].group_adjacent_by = p.root.children[0].group_by.take();
    adjacent_442_sync(&mut p);
    p
}

fn adjacent_442_sync(project: &mut ::mapping::Project) {
    for target in &mut project.extra_targets {
        target.root = project.root.clone();
        target.schema = project.target.clone();
    }
}

fn adjacent_442_program(project: &::mapping::Project) -> Program {
    let saved = serde_json::to_vec_pretty(project).unwrap();
    let restored: ::mapping::Project = serde_json::from_slice(&saved).unwrap();
    assert_eq!(serde_json::to_vec_pretty(&restored).unwrap(), saved);
    codegen::lower_with_basic_owned_group_adjacent_by(&restored).unwrap()
}

fn adjacent_442_assert_item_binding_is_declaration_only(source: &str) {
    // The established C# retained inventory renders the synthetic item node's
    // private overloads. No invocation or other matching occurrence is allowed.
    let lines = source.lines().collect::<Vec<_>>();
    let occurrences = lines
        .iter()
        .enumerate()
        .filter(|(_, line)| line.contains("Node_90("))
        .map(|(index, line)| (*line, lines.get(index + 1).copied()))
        .collect::<Vec<_>>();
    let declaration = "    private static global::Ferrule.Runtime.FerruleValue Node_90(";
    assert_eq!(
        occurrences,
        vec![
            (
                declaration,
                Some("        global::Ferrule.Runtime.ScopeContext context) =>"),
            ),
            (
                declaration,
                Some("        global::Ferrule.Runtime.FerruleBasicScopeContext context)"),
            ),
        ],
        "the synthetic item binding may have only the two established private declarations, never an invocation",
    );
}

fn retain_public_adjacent_442_consumer(
    label: &str,
    project: &::mapping::Project,
    body: &str,
) -> String {
    retain_public_adjacent_442_consumer_with_probe(label, project, body, None)
}

fn retain_public_adjacent_442_consumer_with_probe(
    label: &str,
    project: &::mapping::Project,
    body: &str,
    probe: Option<&str>,
) -> String {
    let program = adjacent_442_program(project);
    let artifacts = crate::emit_with_basic_owned_group_adjacent_by(&program).unwrap();
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
    eprintln!(
        "ADJACENT442 {label} complete Project={}\nProgram={program:#?}\nindependent consumer={host}",
        ::mapping::project_file::encode_pretty(project).unwrap()
    );
    for file in artifacts.files() {
        // Retain every artifact body before comparison without expanding each
        // byte onto its own Debug line. JSON preserves valid UTF-8 exactly;
        // a non-UTF-8 body retains its complete byte array instead.
        let body = match std::str::from_utf8(&file.contents) {
            Ok(text) => serde_json::json!({ "utf8": text }),
            Err(_) => serde_json::json!({ "bytes": file.contents }),
        };
        eprintln!(
            "ADJACENT442_ARTIFACT {}",
            serde_json::json!({
                "case": label,
                "path": file.path.as_str(),
                "byte_length": file.contents.len(),
                "body": body,
            })
        );
    }
    if let Some(probe) = probe {
        eprintln!("ADJACENT442 {label} complete existing private node probe={probe}");
    }
    if let Some(base) = std::env::var_os("FERRULE_PUBLIC_BASIC_CSHARP_HOST_DIR") {
        let base = std::path::PathBuf::from(base);
        assert!(base.is_absolute());
        let root = base.join(label);
        std::fs::create_dir(&root).expect("fresh adjacent consumer namespace");
        let mut files = artifacts
            .files()
            .iter()
            .map(|entry| {
                (
                    format!("generated/{}", entry.path.as_str()),
                    entry.contents.clone(),
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
        if let Some(probe) = probe {
            // Match #473's maintained private partial-source seam. Emitted
            // artifact bytes and GeneratedMapping's public API stay untouched.
            files.push((
                "generated/Runtime/Adjacent442TestProbe.cs".into(),
                probe.as_bytes().to_vec(),
            ));
        }
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
    source
}

fn adjacent_442_refusal() -> ProgramValidationError {
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
fn public_basic_group_adjacent_by_442_preserves_old_gates_bytes_and_structural_precedence() {
    let p = adjacent_442_program(&adjacent_442_project(SourceOwner::Primary));
    for old in [
        crate::emit,
        crate::emit_with_basic_owned_sources,
        crate::emit_with_basic_owned_filters,
        crate::emit_with_basic_owned_sort,
        crate::emit_with_basic_owned_windows,
        crate::emit_with_basic_owned_row_failures,
        crate::emit_with_basic_owned_group_by,
        crate::emit_with_basic_owned_sequence_item_at,
    ] {
        assert_eq!(
            old(&p),
            Err(EmitError::ProgramValidation(adjacent_442_refusal()))
        );
    }
    let by = adjacent_442_program(&group_441_project(SourceOwner::Primary, true));
    assert_eq!(
        crate::emit_with_basic_owned_group_adjacent_by(&by),
        crate::emit_with_basic_owned_sequence_item_at(&by)
    );
    for grouping in [
        codegen::GroupingPlan::StartingWith { predicate: 2 },
        codegen::GroupingPlan::EndingWith { predicate: 2 },
        codegen::GroupingPlan::IntoBlocks { size: 8 },
    ] {
        let mut direct = p.clone();
        // These otherwise unused controls were pruned during lowering. Supply
        // valid expressions before asserting the later capability refusal.
        direct.expressions.extend([
            codegen::ExpressionNode {
                id: 2,
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
                .with_grouping(grouping),
        );
        let before = format!("{direct:#?}");
        assert_eq!(
            crate::emit_with_basic_owned_group_adjacent_by(&direct),
            Err(EmitError::ProgramValidation(adjacent_442_refusal()))
        );
        assert_eq!(format!("{direct:#?}"), before);
    }
    let mut missing = p;
    missing.root.children[0].iteration = Some(
        missing.root.children[0]
            .iteration
            .clone()
            .unwrap()
            .with_grouping(codegen::GroupingPlan::AdjacentBy { key: 1000 }),
    );
    assert_eq!(
        crate::emit_with_basic_owned_group_adjacent_by(&missing),
        Err(EmitError::ProgramValidation(
            ProgramValidationError::MissingGroupingExpression {
                target_path: path(&["Groups"]),
                expression: 1000,
                role: codegen::GroupingExpressionRole::AdjacentKey
            }
        ))
    );
}

// All fields, scalar tags, group origins, ordering and exact target products are compared.
const ADJACENT442_COMPARE: &str = r#"
void SameTree(FerruleInstance actual,FerruleInstance expected)
{
    if(expected is FerruleScalar es)
    {Check(actual is FerruleScalar a&&a.Value.Kind==es.Value.Kind&&a.Value.Equals(es.Value),"exact scalar tag/value");return;}
    if(expected is FerruleRepeated er)
    {Check(actual is FerruleRepeated,"exact repetition");var a=(FerruleRepeated)actual;Check(a.Items.Count==er.Items.Count,"complete member count");for(var i=0;i<er.Items.Count;i++)SameTree(a.Items[i],er.Items[i]);return;}
    Check(actual is FerruleGroup&&expected is FerruleGroup,"exact group");var ag=(FerruleGroup)actual;var eg=(FerruleGroup)expected;
    Check(ag.XmlTypeOrigin.Kind==FerruleXmlTypeOriginKind.Unknown&&eg.XmlTypeOrigin.Kind==FerruleXmlTypeOriginKind.Unknown,"unknown XML group origin");
    Check(ag.Fields.Count==eg.Fields.Count,"complete ordered fields");for(var i=0;i<eg.Fields.Count;i++){Check(ag.Fields[i].Name==eg.Fields[i].Name,"exact field order/name");SameTree(ag.Fields[i].Value,eg.Fields[i].Value);}
}
void Complete(Actual actual,FerruleInstance expected)
{
    Check(actual.Error is null&&actual.Result is not null,"complete successful result");
    switch(actual.Result)
    {
        case ExecutionOutputs all:SameTree(all.Primary,expected);Check(all.Extras.Count==1&&all.Extras[0].Name==" Exact 雪 ","exact complete extras");SameTree(all.Extras[0].Instance,expected);break;
        case SelectedTargetOutput.Primary p:SameTree(p.Instance,expected);break;
        case SelectedTargetOutput.Named n:Check(n.Output.Name==" Exact 雪 ","exact selected target");SameTree(n.Output.Instance,expected);break;
        default:throw new InvalidOperationException("unknown public product");
    }
}
"#;

const ADJACENT442_ITEM_PROBE: &str = r#"
var itemProbeField=typeof(GeneratedMapping).GetField("ItemAt473TestNodes",BindingFlags.NonPublic|BindingFlags.Static)??throw new InvalidOperationException("private test probe missing");
var itemNodes=(List<uint>)(itemProbeField.GetValue(null)??throw new InvalidOperationException("private test probe is null"));
void ItemAtEdges(int expected)
{
    var reached=itemNodes.ToArray();
    Console.WriteLine(Json(new {classification="actual_reached_nodes_before_comparison",nodes=reached}));
    Check(reached.Count(node=>node==70U)==expected&&reached.Count(node=>node==71U)==expected&&reached.Count(node=>node==72U)==expected&&!reached.Contains(90U),"actual ItemAt/producer edges reached, synthetic item binding never invoked");
    if(expected==0)Check(reached.Length==0,"empty cohort has no expression callbacks");
}
"#;

const ADJACENT442_RUN: &str = r#"
FerruleInstance Run(long ordinal,string first,string marker,params (string Label,long Position)[] members)=>G(
    ("First",S(first)),("GroupPosition",I(ordinal)),("RootMarker",S(marker)),
    ("Members",R(members.Select(m=>G(("Label",S(m.Label)),("MemberPosition",I(m.Position)),("RootMarker",S(marker)))).ToArray())));
"#;

const ADJACENT442_INPUT: &str = r#"
FerruleInstance Row(string key,string label,bool keep,long rank)=>G(("Key",S(key)),("Label",S(label)),("Keep",new FerruleScalar(FerruleValue.FromBoolean(keep))),("Rank",I(rank)),("Limit",I(0)));
var input=G(("Marker",S("P.root")),("Limit",I(2)),("Rows",R(Row("A","P-a1",false,30),Row("B","P-b1",true,10),Row("A","P-a2",false,20),Row("A","P-a3",true,25),Row("C","P-c1",false,40),Row("A","P-a4",true,50))));
var reference=G(("Marker",S("N.root")),("Limit",I(1)),("Rows",R(Row("A","N-a1",true,300),Row("B","N-b1",false,100),Row("A","N-a2",true,200),Row("A","N-a3",false,250),Row("C","N-c1",true,400),Row("A","N-a4",false,500))));
var exact=G(("Marker",S("U.root")),("Limit",I(2)),("Rows",R(Row("A","U-only",true,7))));
var supplied=new[]{new NamedInput("Rows",reference),new NamedInput(" Røws💠 ",exact)};var limits=Policy();
"#;

const ADJACENT442_P5: &str = r#"R(Run(1,"P-a1","P.root",("P-a1",1)),Run(2,"P-b1","P.root",("P-b1",1)),Run(3,"P-a2","P.root",("P-a2",1),("P-a3",2)),Run(4,"P-c1","P.root",("P-c1",1)),Run(5,"P-a4","P.root",("P-a4",1)))"#;
const ADJACENT442_N5: &str = r#"R(Run(1,"N-a1","N.root",("N-a1",1)),Run(2,"N-b1","N.root",("N-b1",1)),Run(3,"N-a2","N.root",("N-a2",1),("N-a3",2)),Run(4,"N-c1","N.root",("N-c1",1)),Run(5,"N-a4","N.root",("N-a4",1)))"#;

fn adjacent_442_success(groups: &str, supplied: bool) -> String {
    let inputs = if supplied {
        "supplied"
    } else {
        "Array.Empty<NamedInput>()"
    };
    format!(
        r#"{ADJACENT442_COMPARE}{ADJACENT442_RUN}{ADJACENT442_INPUT}
var expected=G(("Groups",{groups}));
foreach(var selection in new FerruleTargetSelection?[]{{null,FerruleTargetSelection.Primary(),FerruleTargetSelection.Named(" Exact 雪 ")}})
{{
    var probe=new Probe({inputs});var actual=Reach("ADJACENT442.complete",input,probe,limits,null,selection,()=>selection is null
        ? (object)GeneratedMapping.ExecuteWithBasicOwnedSources(input,probe,limits,null)
        : GeneratedMapping.ExecuteSelectedWithBasicOwnedSources(input,selection,probe,limits,null));
    Complete(actual,expected);Reads(actual,1,{count});
}}
"#,
        count = if supplied { 2 } else { 0 }
    )
}

#[test]
fn public_basic_group_adjacent_by_442_literal_runs_exact_owners_retention_and_parent_bounds() {
    for (label, owner, groups) in [
        ("primary", SourceOwner::Primary, ADJACENT442_P5),
        ("named", named("Rows"), ADJACENT442_N5),
        (
            "exact",
            named(" Røws💠 "),
            r#"R(Run(1,"U-only","U.root",("U-only",1)))"#,
        ),
    ] {
        let source = retain_public_adjacent_442_consumer(
            &format!("public-adjacent-442-{label}"),
            &adjacent_442_project(owner),
            &adjacent_442_success(groups, true),
        );
        assert_eq!(
            source.matches(".MapGroupAdjacentBySourceCohort(").count(),
            2
        );
        assert!(!source.contains(".MapGroupBySourceCohort("));
        assert!(!source.contains("ExecuteJson"));
    }
    let mut post = adjacent_442_project(SourceOwner::Primary);
    post.root.children[0].post_group_filter = Some(2);
    adjacent_442_sync(&mut post);
    retain_public_adjacent_442_consumer(
        "public-adjacent-442-post",
        &post,
        &adjacent_442_success(
            r#"R(Run(1,"P-b1","P.root",("P-b1",1)),Run(2,"P-a2","P.root",("P-a2",1),("P-a3",2)),Run(3,"P-a4","P.root",("P-a4",1)))"#,
            false,
        ),
    );
    let mut sorted = adjacent_442_project(SourceOwner::Primary);
    sorted.root.children[0].sort_by = Some(6);
    adjacent_442_sync(&mut sorted);
    retain_public_adjacent_442_consumer(
        "public-adjacent-442-sorted",
        &sorted,
        &adjacent_442_success(
            r#"R(Run(1,"P-b1","P.root",("P-b1",1)),Run(2,"P-a2","P.root",("P-a2",1),("P-a3",2),("P-a1",3)),Run(3,"P-c1","P.root",("P-c1",1)),Run(4,"P-a4","P.root",("P-a4",1)))"#,
            false,
        ),
    );
    sorted.root.children[0].windows = vec![::mapping::SequenceWindow::First { count: 7 }];
    adjacent_442_sync(&mut sorted);
    let source = retain_public_adjacent_442_consumer(
        "public-adjacent-442-parent-bound",
        &sorted,
        &adjacent_442_success(
            r#"R(Run(1,"P-b1","P.root",("P-b1",1)),Run(2,"P-a2","P.root",("P-a2",1),("P-a3",2),("P-a1",3)))"#,
            false,
        ),
    );
    assert!(source.contains("ItemCount(7U, Node_7(parent))"));
}

#[test]
fn public_basic_group_adjacent_by_442_itemat_generate_uses_group_and_member_ordinals_empty_catalogue()
 {
    use ::mapping::{Node, SequenceExpr};
    let mut p = adjacent_442_project(SourceOwner::Primary);
    p.extra_sources.clear();
    p.graph.nodes.extend([
        (
            70,
            Node::SequenceItemAt {
                sequence: SequenceExpr::Generate {
                    from: Some(71),
                    to: 72,
                    item: 90,
                },
                index: 4,
            },
        ),
        (
            71,
            Node::Const {
                value: Value::Int(10),
            },
        ),
        (
            72,
            Node::Const {
                value: Value::Int(12),
            },
        ),
        (
            90,
            Node::SourceField {
                path: vec![],
                frame: None,
            },
        ),
    ]);
    let scope = &mut p.root.children[0];
    scope.bindings.push(::mapping::Binding {
        target_field: "Picked".into(),
        node: 70,
    });
    scope.children[0].bindings.push(::mapping::Binding {
        target_field: "Picked".into(),
        node: 70,
    });
    let ir::SchemaKind::Group { children, .. } = &mut p.target.kind else {
        unreachable!()
    };
    let ir::SchemaKind::Group { children, .. } = &mut children[0].kind else {
        unreachable!()
    };
    let member = children
        .iter_mut()
        .find(|child| child.name == "Members")
        .unwrap();
    let ir::SchemaKind::Group {
        children: members, ..
    } = &mut member.kind
    else {
        unreachable!()
    };
    members.push(SchemaNode::scalar("Picked", ScalarType::Int));
    children.insert(3, SchemaNode::scalar("Picked", ScalarType::Int));
    adjacent_442_sync(&mut p);
    let body = format!(
        r#"{ADJACENT442_COMPARE}{ADJACENT442_ITEM_PROBE}{ADJACENT442_INPUT}
FerruleInstance PickedRun(long ordinal,string first,FerruleInstance picked,params (string Label,long Position,long Picked)[] members)=>G(
    ("First",S(first)),("GroupPosition",I(ordinal)),("RootMarker",S("P.root")),("Picked",picked),
    ("Members",R(members.Select(m=>G(("Label",S(m.Label)),("MemberPosition",I(m.Position)),("RootMarker",S("P.root")),("Picked",I(m.Picked)))).ToArray())));
var expected=G(("Groups",R(PickedRun(1,"P-a1",I(10),("P-a1",1,10)),PickedRun(2,"P-b1",I(11),("P-b1",1,10)),
    PickedRun(3,"P-a2",I(12),("P-a2",1,10),("P-a3",2,11)),PickedRun(4,"P-c1",new FerruleScalar(FerruleValue.Null),("P-c1",1,10)),
    PickedRun(5,"P-a4",new FerruleScalar(FerruleValue.Null),("P-a4",1,10)))));
foreach(var selection in new FerruleTargetSelection?[]{{null,FerruleTargetSelection.Primary(),FerruleTargetSelection.Named(" Exact 雪 ")}})
{{
    itemNodes.Clear();var probe=new Probe(Array.Empty<NamedInput>());var actual=Reach("ADJACENT442.generate",input,probe,limits,null,selection,()=>selection is null
        ? (object)GeneratedMapping.ExecuteWithBasicOwnedSources(input,probe,limits,null)
        : GeneratedMapping.ExecuteSelectedWithBasicOwnedSources(input,selection,probe,limits,null));
    ItemAtEdges(selection is null?22:11);Complete(actual,expected);Reads(actual,1,0);
}}
var empty=G(("Marker",S("P.root")),("Limit",I(2)),("Rows",R()));var emptyProbe=new Probe(Array.Empty<NamedInput>());
itemNodes.Clear();var emptyActual=Reach("ADJACENT442.empty",empty,emptyProbe,limits,null,null,()=>GeneratedMapping.ExecuteWithBasicOwnedSources(empty,emptyProbe,limits,null));
ItemAtEdges(0);Complete(emptyActual,G(("Groups",R())));
"#
    );
    let source = retain_public_adjacent_442_consumer_with_probe(
        "public-adjacent-442-generate-empty-catalogue",
        &p,
        &body,
        Some(ITEMAT473_PRIVATE_PROBE),
    );
    assert!(source.contains(".MapGroupAdjacentBySourceCohort("));
    assert!(source.contains("FerruleAggregateOperation.ItemAt"));
    adjacent_442_assert_item_binding_is_declaration_only(&source);
}

#[test]
fn public_basic_group_adjacent_by_442_typed_errors_and_selection_precedence() {
    use ::mapping::Node;
    for (label, node, message) in [("key", 1, "key"), ("post", 2, "post")] {
        let mut p = adjacent_442_project(SourceOwner::Primary);
        p.root.children[0].post_group_filter = Some(2);
        p.root.children[0].windows = vec![::mapping::SequenceWindow::First { count: 50 }];
        p.graph.nodes.extend([
            (
                50,
                Node::Const {
                    value: Value::Int(0),
                },
            ),
            (
                51,
                Node::Const {
                    value: Value::String(message.into()),
                },
            ),
            (node, Node::Raise { message: Some(51) }),
        ]);
        adjacent_442_sync(&mut p);
        let body = format!(
            r#"{ADJACENT442_INPUT}
foreach(var selection in new FerruleTargetSelection?[]{{null,FerruleTargetSelection.Primary(),FerruleTargetSelection.Named(" Exact 雪 ")}})
{{
    var probe=new Probe(Array.Empty<NamedInput>());var actual=Reach("ADJACENT442.{label}",input,probe,limits,null,selection,()=>selection is null
        ? (object)GeneratedMapping.ExecuteWithBasicOwnedSources(input,probe,limits,null)
        : GeneratedMapping.ExecuteSelectedWithBasicOwnedSources(input,selection,probe,limits,null));
    var e=Runtime(actual,FerruleRuntimeError.MappingException);Check(e.Node=={node}U&&e.MappingExceptionMessage=="{message}"&&e.Message=="node {node}: mapping exception: {message}"&&e.InnerException is null,"exact raised node/message");
    NullMetadata(e,"Node","MappingExceptionMessage");Reads(actual,1,0);
}}
var forbidden=new Probe(Array.Empty<NamedInput>(),throwCount:true,throwIndex:true);var zero=new FerruleOwnedSourceLimits(0,0,0,0,0);
var unknown=Reach("ADJACENT442.unknown",input,forbidden,zero,null,FerruleTargetSelection.Named("Exact 雪"),()=>GeneratedMapping.ExecuteSelectedWithBasicOwnedSources(input,FerruleTargetSelection.Named("Exact 雪"),forbidden,zero,null));
InputError(unknown,FerruleRuntimeError.UnknownTarget,"Exact 雪","project has no named target `Exact 雪`");Reads(unknown,0,0);
"#
        );
        retain_public_adjacent_442_consumer(
            &format!("public-adjacent-442-{label}-before-zero"),
            &p,
            &body,
        );
    }
    let mut p = adjacent_442_project(SourceOwner::Primary);
    p.root.children[0].post_group_filter = Some(2);
    p.graph.nodes.insert(
        2,
        Node::Const {
            value: Value::String("not bool".into()),
        },
    );
    adjacent_442_sync(&mut p);
    retain_public_adjacent_442_consumer(
        "public-adjacent-442-post-not-bool",
        &p,
        &format!(
            r#"{ADJACENT442_INPUT}
var probe=new Probe(Array.Empty<NamedInput>());var actual=Reach("ADJACENT442.bool",input,probe,limits,null,null,()=>GeneratedMapping.ExecuteWithBasicOwnedSources(input,probe,limits,null));
var error=Runtime(actual,FerruleRuntimeError.NotABool);Check(error.Node==2U&&error.FoundKind==FerruleValueKind.String&&error.Message=="Graph node 2 expected a bool, found string."&&error.InnerException is null,"exact bool tag/node");NullMetadata(error,"Node","FoundKind");Reads(actual,1,0);
"#
        ),
    );
    let mut bound = adjacent_442_project(SourceOwner::Primary);
    bound.graph.nodes.insert(
        7,
        Node::Const {
            value: Value::Bool(true),
        },
    );
    bound.root.children[0].windows = vec![::mapping::SequenceWindow::First { count: 7 }];
    adjacent_442_sync(&mut bound);
    retain_public_adjacent_442_consumer(
        "public-adjacent-442-empty-invalid-bound",
        &bound,
        &format!(
            r#"{ADJACENT442_INPUT}
var empty=G(("Marker",S("P.root")),("Limit",I(2)),("Rows",R()));
foreach(var selection in new FerruleTargetSelection?[]{{null,FerruleTargetSelection.Primary(),FerruleTargetSelection.Named(" Exact 雪 ")}})
{{
    var probe=new Probe(Array.Empty<NamedInput>());var actual=Reach("ADJACENT442.empty-bound",empty,probe,limits,null,selection,()=>selection is null
        ? (object)GeneratedMapping.ExecuteWithBasicOwnedSources(empty,probe,limits,null)
        : GeneratedMapping.ExecuteSelectedWithBasicOwnedSources(empty,selection,probe,limits,null));
    var error=Runtime(actual,FerruleRuntimeError.NotAnItemCount);Check(error.Node==7U&&error.FoundKind==FerruleValueKind.Bool&&error.Message=="Node 7: expected an item count, found Bool."&&error.InnerException is null,"exact empty parent count error");NullMetadata(error,"Node","FoundKind");Reads(actual,1,0);
}}
"#
        ),
    );
    let mut failure = adjacent_442_project(SourceOwner::Primary);
    failure.graph.nodes.insert(1, Node::Raise { message: None });
    failure.failure_rules.push(::mapping::FailureRule {
        iteration: ::mapping::FailureIteration::Source {
            collection: path(&["Rows"]),
        },
        selection: ::mapping::FailureSelection::WhenTrue { predicate: 2 },
        message: Some(3),
    });
    retain_public_adjacent_442_consumer(
        "public-adjacent-442-global-before-key",
        &failure,
        &format!(
            r#"{ADJACENT442_INPUT}
foreach(var selection in new FerruleTargetSelection?[]{{null,FerruleTargetSelection.Primary(),FerruleTargetSelection.Named(" Exact 雪 ")}})
{{
    var probe=new Probe(Array.Empty<NamedInput>());var actual=Reach("ADJACENT442.global-first",input,probe,limits,null,selection,()=>selection is null
        ? (object)GeneratedMapping.ExecuteWithBasicOwnedSources(input,probe,limits,null)
        : GeneratedMapping.ExecuteSelectedWithBasicOwnedSources(input,selection,probe,limits,null));Failure(actual,1,"P-b1");Reads(actual,1,0);
}}
"#
        ),
    );
}

#[test]
fn public_basic_group_adjacent_by_442_complete_catalogues_and_global_dependencies() {
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
    let mut p = adjacent_442_project(SourceOwner::Primary);
    let mut unused = p.extra_sources[0].clone();
    unused.name = "unusedStatic".into();
    p.extra_sources.insert(0, unused.clone());
    unused.name = "unavailableDynamic".into();
    unused.dynamic_path = Some(::mapping::DynamicSourcePath::legacy(40, vec![]));
    p.extra_sources.push(unused);
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
    for global in [false, true] {
        if global {
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
        let program = adjacent_442_program(&p);
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
            vec![(0, " Exact 雪 ", if global { required } else { optional })]
        );
        let artifacts = crate::emit_with_basic_owned_group_adjacent_by(&program).unwrap();
        let source = std::str::from_utf8(
            &artifacts
                .files()
                .iter()
                .find(|file| file.path.as_str() == "GeneratedMapping.cs")
                .unwrap()
                .contents,
        )
        .unwrap();
        assert!(source.contains("CheckedNamedInputs_0"));
        assert!(source.contains("// declaration 3"));
    }
}

#[test]
fn public_basic_group_adjacent_by_442_key_post_ancestry_and_all_ordinary_islands_remain_distinct() {
    use ::mapping::{Node, Scope, ScopeIteration};
    for only_post in [false, true] {
        let mut p = adjacent_442_project(SourceOwner::Primary);
        let groups = &mut p.root.children[0];
        groups.iteration = ScopeIteration::Source(path(&["Rows"]));
        groups.children[0].iteration = ScopeIteration::Source(vec![]);
        groups.post_group_filter = Some(2);
        p.graph.nodes.extend([
            (
                3,
                Node::SourceField {
                    path: path(&["Label"]),
                    frame: None,
                },
            ),
            (
                4,
                Node::Position {
                    collection: path(&["Rows"]),
                },
            ),
            (
                5,
                Node::Const {
                    value: Value::String("P.root".into()),
                },
            ),
            (
                if only_post { 1 } else { 2 },
                Node::SourceField {
                    path: path(&[if only_post { "Key" } else { "Keep" }]),
                    frame: None,
                },
            ),
        ]);
        adjacent_442_sync(&mut p);
        let source = retain_public_adjacent_442_consumer(
            if only_post {
                "public-adjacent-442-legacy-post"
            } else {
                "public-adjacent-442-legacy-key"
            },
            &p,
            &adjacent_442_success(
                r#"R(Run(1,"P-b1","P.root",("P-b1",1)),Run(2,"P-a2","P.root",("P-a2",1),("P-a3",2)),Run(3,"P-a4","P.root",("P-a4",1)))"#,
                true,
            ),
        );
        assert!(!source.contains("Scope_2(global::Ferrule.Runtime.ScopeContext context)"));
        assert!(source.contains(".MapGroupAdjacentBySourceCohort("));
    }
    for mode in 0..5 {
        let mut p = adjacent_442_project(SourceOwner::Primary);
        p.graph.nodes.insert(
            91,
            Node::Const {
                value: Value::Bool(true),
            },
        );
        let mut ordinary = Scope {
            target_field: "Ordinary".into(),
            iteration: ScopeIteration::Source(path(&["Rows"])),
            bindings: vec![::mapping::Binding {
                target_field: "Value".into(),
                node: 40,
            }],
            ..Default::default()
        };
        match mode {
            0 => ordinary.group_by = Some(40),
            1 => ordinary.group_adjacent_by = Some(40),
            2 => ordinary.group_starting_with = Some(91),
            3 => ordinary.group_ending_with = Some(91),
            _ => ordinary.group_into_blocks = Some(8),
        }
        p.root.children.push(ordinary);
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
        adjacent_442_sync(&mut p);
        let ordinary = if mode < 2 {
            r#"R(G(("Value",S("group control"))))"#
        } else {
            r#"R(G(("Value",S("group control"))),G(("Value",S("group control"))),G(("Value",S("group control"))),G(("Value",S("group control"))),G(("Value",S("group control"))),G(("Value",S("group control"))))"#
        };
        let body = format!(
            r#"{ADJACENT442_COMPARE}{ADJACENT442_RUN}{ADJACENT442_INPUT}
var expected=G(("Groups",{ADJACENT442_P5}),("Ordinary",{ordinary}));
foreach(var selection in new FerruleTargetSelection?[]{{null,FerruleTargetSelection.Primary(),FerruleTargetSelection.Named(" Exact 雪 ")}})
{{
    var probe=new Probe(supplied);var actual=Reach("ADJACENT442.island",input,probe,limits,null,selection,()=>selection is null
        ? (object)GeneratedMapping.ExecuteWithBasicOwnedSources(input,probe,limits,null)
        : GeneratedMapping.ExecuteSelectedWithBasicOwnedSources(input,selection,probe,limits,null));Complete(actual,expected);Reads(actual,1,2);
}}
"#
        );
        let source = retain_public_adjacent_442_consumer(
            &format!("public-adjacent-442-ordinary-{mode}"),
            &p,
            &body,
        );
        assert!(source.contains("Scope_3(global::Ferrule.Runtime.ScopeContext context)"));
        assert_eq!(
            source.matches(".MapGroupAdjacentBySourceCohort(").count(),
            2
        );
    }
}

#[test]
fn public_basic_group_adjacent_by_442_itemat_key_stays_candidate_bound_and_runs_do_not_coalesce() {
    use ::mapping::{Node, SequenceExpr};
    let mut p = adjacent_442_project(SourceOwner::Primary);
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
                value: Value::String("A|B|A".into()),
            },
        ),
        (
            72,
            Node::Const {
                value: Value::String("|".into()),
            },
        ),
        (
            90,
            Node::SourceField {
                path: vec![],
                frame: None,
            },
        ),
    ]);
    p.root.children[0].group_adjacent_by = Some(70);
    adjacent_442_sync(&mut p);
    let body = format!(
        r#"{ADJACENT442_COMPARE}{ADJACENT442_ITEM_PROBE}{ADJACENT442_RUN}{ADJACENT442_INPUT}
input=G(("Marker",S("P.root")),("Limit",I(2)),("Rows",R(Row("A","P-a1",false,30),Row("B","P-b1",true,10),Row("A","P-a2",false,20))));
var expected=G(("Groups",R(Run(1,"P-a1","P.root",("P-a1",1)),Run(2,"P-b1","P.root",("P-b1",1)),Run(3,"P-a2","P.root",("P-a2",1)))));
foreach(var selection in new FerruleTargetSelection?[]{{null,FerruleTargetSelection.Primary(),FerruleTargetSelection.Named(" Exact 雪 ")}})
{{
    itemNodes.Clear();var probe=new Probe(Array.Empty<NamedInput>());var actual=Reach("ADJACENT442.itemat-key",input,probe,limits,null,selection,()=>selection is null
        ? (object)GeneratedMapping.ExecuteWithBasicOwnedSources(input,probe,limits,null)
        : GeneratedMapping.ExecuteSelectedWithBasicOwnedSources(input,selection,probe,limits,null));ItemAtEdges(selection is null?6:3);Complete(actual,expected);Reads(actual,1,0);
}}
"#
    );
    let source = retain_public_adjacent_442_consumer_with_probe(
        "public-adjacent-442-itemat-key",
        &p,
        &body,
        Some(ITEMAT473_PRIVATE_PROBE),
    );
    assert!(source.contains("=> Node_70(candidate);"));
    adjacent_442_assert_item_binding_is_declaration_only(&source);
}
