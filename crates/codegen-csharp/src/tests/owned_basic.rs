//! Prospective private generator controls. No public capability is enabled.
//! Set FERRULE_PRIVATE_OWNED_CSHARP_HOST_DIR only in the protected root test
//! actor to retain the generated Mapping, TargetBuilder, and complete typed host.
//! Hosts must be compiled against the separately qualified #489 C#204 runtime;
//! ordinary embedded runtime/artifacts and all public emitter routes stay exact.
use super::*;
use crate::owned_basic::render_basic_owned_mapping;
use ::mapping::{OwnedPosition, OwnedSourceField, OwnedSourceRef, SourceOwner};
use codegen::{NamedSourceProgram, NamedTargetProgram, ScalarTargetDomain, TargetConstruction};

fn path(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|part| (*part).into()).collect()
}
fn named(name: &str) -> SourceOwner {
    SourceOwner::Named { name: name.into() }
}
fn field(owner: SourceOwner, parts: &[&str], frame: Option<&[&str]>) -> Expression {
    Expression::OwnedSourceField(OwnedSourceField {
        source: OwnedSourceRef {
            owner,
            path: path(parts),
        },
        frame: frame.map(path),
    })
}
fn position(owner: SourceOwner, collection: Option<&[&str]>) -> Expression {
    Expression::OwnedPosition(OwnedPosition {
        owner,
        collection: collection.map(path),
    })
}
fn expression(id: u32, expression: Expression) -> ExpressionNode {
    ExpressionNode { id, expression }
}
fn binding(name: &str, id: u32, ty: ScalarType) -> Binding {
    Binding {
        target_field: name.into(),
        expression: id,
        target_domain: ty.into(),
        repeating: false,
    }
}
fn group(bindings: Vec<Binding>) -> TargetScope {
    TargetScope {
        target_field: String::new(),
        repeating: false,
        iteration: None,
        construction: TargetConstruction::Group,
        bindings,
        children: Vec::new(),
    }
}
fn owned_iteration(owner: SourceOwner, parts: &[&str], bindings: Vec<Binding>) -> TargetScope {
    let mut scope = group(bindings);
    scope.repeating = true;
    scope.iteration = Some(IterationPlan::owned_source(OwnedSourceRef {
        owner,
        path: path(parts),
    }));
    scope
}
fn plain_iteration(parts: &[&str], bindings: Vec<Binding>) -> TargetScope {
    let mut scope = group(bindings);
    scope.repeating = true;
    scope.iteration = Some(IterationPlan::source(path(parts)));
    scope
}
fn source_schema(name: &str) -> SchemaNode {
    SchemaNode::group(
        name,
        vec![
            SchemaNode::scalar("Value", ScalarType::String),
            SchemaNode::scalar("Score", ScalarType::Int),
            SchemaNode::group(
                "Details",
                vec![SchemaNode::scalar("Counter", ScalarType::Int)],
            ),
            SchemaNode::group("Ref", vec![SchemaNode::scalar("Value", ScalarType::String)]),
            SchemaNode::group(
                "Rows",
                vec![
                    SchemaNode::scalar("Value", ScalarType::String),
                    SchemaNode::scalar("Score", ScalarType::Int),
                    SchemaNode::group(
                        "Details",
                        vec![SchemaNode::scalar("Counter", ScalarType::Int)],
                    ),
                    SchemaNode::group(
                        "Rows",
                        vec![SchemaNode::scalar("Value", ScalarType::String)],
                    )
                    .repeating(),
                ],
            )
            .repeating(),
        ],
    )
}
fn target_schema(scope: &TargetScope, name: &str) -> SchemaNode {
    let mut node = match scope.construction {
        TargetConstruction::Scalar {
            target_domain: ScalarTargetDomain::Single(ty),
            ..
        } => SchemaNode::scalar(name, ty),
        TargetConstruction::Group => {
            let mut children = scope
                .bindings
                .iter()
                .map(|binding| {
                    let ScalarTargetDomain::Single(ty) = binding.target_domain else {
                        panic!("literal fixture domain")
                    };
                    let node = SchemaNode::scalar(&binding.target_field, ty);
                    if binding.repeating {
                        node.repeating()
                    } else {
                        node
                    }
                })
                .collect::<Vec<_>>();
            children.extend(
                scope
                    .children
                    .iter()
                    .map(|child| target_schema(child, &child.target_field)),
            );
            SchemaNode::group(name, children)
        }
        _ => panic!("private fixture construction"),
    };
    if scope.repeating {
        node = node.repeating();
    }
    node
}
fn basic(expressions: Vec<ExpressionNode>, root: TargetScope) -> Program {
    Program {
        xml_boundary: None,
        source: source_schema("Primary"),
        extra_sources: ["Unused", "Ref", " Ref 雪 ", "ref", "primary"]
            .iter()
            .map(|name| NamedSourceProgram {
                name: (*name).into(),
                source: source_schema("Static"),
                dynamic: None,
            })
            .collect(),
        target: target_schema(&root, "Output"),
        root,
        expressions,
        user_functions: vec![],
        failure_rules: vec![],
        extra_targets: vec![],
    }
}
fn add_target(program: &mut Program, name: &str, root: TargetScope) {
    program.extra_targets.push(NamedTargetProgram {
        name: name.into(),
        target: target_schema(&root, "NamedOutput"),
        root,
    });
}
fn complete_host(body: &str) -> String {
    let mut host = HOST_PREFIX.to_string();
    host.push_str(HOST_ERROR_SNAPSHOT);
    host.push_str("\n    public static void Main()\n    {\n");
    host.push_str(body);
    host.push_str(HOST_SUFFIX);
    host
}
fn retain_host(id: &str, program: &Program, body: &str) -> String {
    let result = render_basic_owned_mapping(program);
    eprintln!(
        "{id} complete typed Program={program:#?} private renderer Result={result:#?} complete host body={body}"
    );
    let mapping = result.expect("valid frozen Basic fixture renders privately");
    let host = complete_host(body);
    eprintln!("{id} complete generated typed host={host}");
    if let Some(directory) = std::env::var_os("FERRULE_PRIVATE_OWNED_CSHARP_HOST_DIR") {
        let directory = std::path::PathBuf::from(directory).join(id);
        std::fs::create_dir_all(&directory).expect("protected actor supplied output directory");
        std::fs::write(directory.join("GeneratedMapping.cs"), &mapping)
            .expect("retain private mapping");
        std::fs::write(
            directory.join("GeneratedTargetBuilder.cs"),
            crate::runtime::TARGET_BUILDER,
        )
        .expect("retain unchanged target helper");
        std::fs::write(directory.join("Program.cs"), host).expect("retain complete typed host");
        std::fs::write(directory.join("Program.ir.txt"), format!("{program:#?}\n"))
            .expect("retain complete typed input");
    }
    mapping
}

#[test]
fn private_exact_owner_identities_and_legacy_collision_hosts() {
    let p = basic(
        vec![expression(1, field(SourceOwner::Primary, &["Value"], None))],
        group(vec![binding("Value", 1, ScalarType::String)]),
    );
    let s = retain_host(
        "G439-P01",
        &p,
        r#"var a=Reach("P01",()=>ExecuteOwnedBasic(Input,Declared,Limits)); ExpectTree(a,"{\"Value\":\"P.root\"}");"#,
    );
    assert!(s.contains("FerruleOwnedSourceOwner.Primary, new string[] { \"Value\" }, null"));
    let p = basic(
        vec![
            expression(1, field(named("Ref"), &["Value"], None)),
            expression(
                2,
                Expression::SourceField {
                    frame: None,
                    path: path(&["Ref", "Value"]),
                },
            ),
        ],
        group(vec![
            binding("Named", 1, ScalarType::String),
            binding("Legacy", 2, ScalarType::String),
        ]),
    );
    let s = retain_host(
        "G439-P02",
        &p,
        r#"var a=Reach("P02",()=>ExecuteOwnedBasic(Input,Declared,Limits)); ExpectTree(a,"{\"Named\":\"N.root\",\"Legacy\":\"P.collision\"}");"#,
    );
    assert!(s.contains("FerruleOwnedSourceOwner.Named(\"Ref\")"));
    assert!(s.contains("ResolveLegacyField(new string[] { \"Ref\", \"Value\" }, null)"));
    let p = basic(
        vec![
            expression(1, field(named(" Ref 雪 "), &["Value"], None)),
            expression(2, field(named("ref"), &["Value"], None)),
            expression(3, field(named("primary"), &["Value"], None)),
        ],
        group(vec![
            binding("Exact", 1, ScalarType::String),
            binding("Lower", 2, ScalarType::String),
            binding("LiteralPrimary", 3, ScalarType::String),
        ]),
    );
    let s = retain_host(
        "G439-P03",
        &p,
        r#"var a=Reach("P03",()=>ExecuteOwnedBasic(Input,Declared,Limits)); ExpectTree(a,"{\"Exact\":\"N.exact\",\"Lower\":\"N.lower\",\"LiteralPrimary\":\"N.literal.primary\"}");"#,
    );
    assert!(s.is_ascii());
    assert!(s.contains("\\u96EA"));
    assert!(s.contains("Named(\"ref\")"));
    assert!(s.contains("Named(\"primary\")"));
}
#[test]
fn private_named_walk_and_legacy_nested_frame_hosts() {
    let p = basic(
        vec![
            expression(1, field(named("Ref"), &["Rows", "Value"], None)),
            expression(2, position(named("Ref"), Some(&["Rows"]))),
            expression(
                3,
                Expression::Position {
                    collection: path(&["Ref", "Rows"]),
                },
            ),
        ],
        owned_iteration(
            named("Ref"),
            &["Rows"],
            vec![
                binding("Value", 1, ScalarType::String),
                binding("OwnedPosition", 2, ScalarType::Int),
                binding("LegacyPosition", 3, ScalarType::Int),
            ],
        ),
    );
    let s = retain_host(
        "G439-P04",
        &p,
        r#"var a=Reach("P04",()=>ExecuteOwnedBasic(Input,Declared,Limits)); ExpectTree(a,"[{\"Value\":\"N1\",\"OwnedPosition\":1,\"LegacyPosition\":1},{\"Value\":\"N2\",\"OwnedPosition\":2,\"LegacyPosition\":2}]");"#,
    );
    assert!(s.contains("WalkOwnedSourceCohort(global::Ferrule.Runtime.FerruleOwnedSourceOwner.Named(\"Ref\"), new string[] { \"Rows\" })"));
    assert_eq!(s.matches("WalkOwnedSourceCohort(").count(), 1);
    assert!(s.contains("GC.KeepAlive(cohort_0)"));
    let mut root = group(vec![
        binding("Outer", 1, ScalarType::String),
        binding("Inner", 2, ScalarType::String),
        binding("ParentPosition", 3, ScalarType::Int),
        binding("Position", 4, ScalarType::Int),
    ]);
    root.repeating = true;
    root.iteration = Some(IterationPlan::source(path(&["Rows", "Rows"])));
    let p = basic(
        vec![
            expression(
                1,
                field(SourceOwner::Primary, &["Rows", "Value"], Some(&["Rows"])),
            ),
            expression(
                2,
                field(SourceOwner::Primary, &["Rows", "Rows", "Value"], None),
            ),
            expression(3, position(SourceOwner::Primary, Some(&["Rows"]))),
            expression(4, position(SourceOwner::Primary, Some(&["Rows", "Rows"]))),
        ],
        root,
    );
    let s = retain_host(
        "G439-P05",
        &p,
        r#"var a=Reach("P05",()=>ExecuteOwnedBasic(Input,Declared,Limits)); ExpectTree(a,"[{\"Outer\":\"P1\",\"Inner\":\"P1.1\",\"ParentPosition\":1,\"Position\":1},{\"Outer\":\"P1\",\"Inner\":\"P1.2\",\"ParentPosition\":1,\"Position\":2}]");"#,
    );
    assert_eq!(s.matches("WalkSourceCohort(").count(), 1);
    assert!(!s.contains(".WalkSource("));
}
#[test]
fn private_nested_owned_identity_preserves_descendant_context_host() {
    let mut identity = owned_iteration(
        SourceOwner::Primary,
        &["Rows"],
        vec![
            binding("Outer", 1, ScalarType::String),
            binding("Inner", 2, ScalarType::String),
            binding("ParentPosition", 3, ScalarType::Int),
            binding("Position", 4, ScalarType::Int),
        ],
    );
    identity.target_field = "Identity".into();
    let mut inner = owned_iteration(SourceOwner::Primary, &["Rows", "Rows"], vec![]);
    inner.target_field = "Inner".into();
    inner.children.push(identity);
    let mut root = owned_iteration(SourceOwner::Primary, &["Rows"], vec![]);
    root.children.push(inner);
    let p = basic(
        vec![
            expression(
                1,
                field(SourceOwner::Primary, &["Rows", "Value"], Some(&["Rows"])),
            ),
            expression(
                2,
                field(SourceOwner::Primary, &["Rows", "Rows", "Value"], None),
            ),
            expression(3, position(SourceOwner::Primary, Some(&["Rows"]))),
            expression(4, position(SourceOwner::Primary, None)),
        ],
        root,
    );
    let s = retain_host(
        "G439-P06-identity",
        &p,
        r#"var a=Reach("P06.identity",()=>ExecuteOwnedBasic(Input,Declared,Limits)); ExpectTree(a,"[{\"Inner\":[{\"Identity\":[{\"Outer\":\"P1\",\"Inner\":\"P1.1\",\"ParentPosition\":1,\"Position\":1}]},{\"Identity\":[{\"Outer\":\"P1\",\"Inner\":\"P1.2\",\"ParentPosition\":1,\"Position\":2}]}]},{\"Inner\":[]},{\"Inner\":[]}]");"#,
    );
    assert_eq!(s.matches("WalkOwnedSourceCohort(").count(), 3);
    assert!(!s.contains("Reindex"));
    assert!(!s.contains("CloneView"));
    // The P06 later compact1 operation is outside control-free #518; its frozen
    // full expectation remains preserved for #440/#519, never rewritten here.
}
#[test]
fn private_owned_absence_plain_scope_and_malformed_error_hosts() {
    // P08 generated traversal is legacy plain inside the paired context. The
    // direct owned plain walk below separately observes the frozen runtime case.
    // #516 requires repeating owned DTO terminals; this test does not widen it.
    let p = basic(
        vec![
            expression(
                1,
                field(named("Ref"), &["Rows", "Details", "Counter"], None),
            ),
            expression(
                2,
                field(
                    named("Ref"),
                    &["Rows", "Details", "Counter"],
                    Some(&["Rows"]),
                ),
            ),
            expression(
                3,
                Expression::SourceField {
                    frame: None,
                    path: path(&["Details", "Counter"]),
                },
            ),
        ],
        owned_iteration(
            named("Ref"),
            &["Rows"],
            vec![
                binding("Owned", 1, ScalarType::Int),
                binding("Pinned", 2, ScalarType::Int),
                binding("Legacy", 3, ScalarType::Int),
            ],
        ),
    );
    retain_host(
        "G439-P07",
        &p,
        r#"var a=Reach("P07",()=>ExecuteOwnedBasic(Input,Declared,Limits)); ExpectTree(a,"[{\"Owned\":\"Null\",\"Pinned\":\"Null\",\"Legacy\":99},{\"Owned\":7,\"Pinned\":7,\"Legacy\":7}]");"#,
    );
    let p = basic(
        vec![
            expression(
                1,
                field(SourceOwner::Primary, &["Details", "Counter"], None),
            ),
            expression(2, Expression::Position { collection: vec![] }),
            expression(3, position(SourceOwner::Primary, None)),
        ],
        plain_iteration(
            &["Details"],
            vec![
                binding("Counter", 1, ScalarType::Int),
                binding("LegacyPosition", 2, ScalarType::Int),
            ],
        ),
    );
    retain_host(
        "G439-P08",
        &p,
        r#"var a=Reach("P08.result",()=>ExecuteOwnedBasic(Input,Declared,Limits)); var b=Reach("P08.position",()=>{var roots=FerruleBasicSourceRoots.Create(Input,Declared,Limits);var rows=roots.RootContext().WalkOwnedSourceCohort(FerruleOwnedSourceOwner.Primary,new[]{"Details"});return Node_3(rows[0].Context);}); var c=Reach("P08.direct.owned.plain",()=>{var roots=FerruleBasicSourceRoots.Create(Input,Declared,Limits);var rows=roots.RootContext().WalkOwnedSourceCohort(FerruleOwnedSourceOwner.Primary,new[]{"Details"});var candidate=rows[0];var tree=G(("Counter",new FerruleScalar(Node_1(candidate.Context))),("LegacyPosition",new FerruleScalar(Node_2(candidate.Context))));var result=new DirectPlainResult(tree,candidate);GC.KeepAlive(rows);return result;}); ExpectTree(a,"[{\"Counter\":99,\"LegacyPosition\":1}]"); ExpectOwned(b,FerruleOwnedSourceErrorKind.InactiveSourceFrame,true,null,null);ExpectTree(c,"{\"Counter\":99,\"LegacyPosition\":1}");Check(!((DirectPlainResult)c.Result!).Candidate.HasNewCursor,"P08 direct plain no new occurrence");"#,
    );
    let p = basic(
        vec![expression(
            1,
            field(SourceOwner::Primary, &["Details", "Counter"], None),
        )],
        group(vec![binding("Counter", 1, ScalarType::Int)]),
    );
    retain_host(
        "G439-P11",
        &p,
        r#"Input=G(("Details",I(7))); var a=Reach("P11",()=>ExecuteOwnedBasic(Input,Declared,Limits)); var e=ExpectOwned(a,FerruleOwnedSourceErrorKind.Traversal,true,null,null); var c=e.InnerException as FerruleOwnedSourceTraversalException; Check(c is not null && c.Error==FerruleOwnedSourceTraversalKind.CannotTraverse && c.Segment==1 && c.Found==FerruleOwnedSourceInstanceKind.Scalar && c.Path.SequenceEqual(new[]{"Details","Counter"}),"P11 exact typed cause"); Check(ReferenceEquals(e.InnerException,c) && e.Path is not null && e.Path.SequenceEqual(new[]{"Details","Counter"}) && e.Segment==1 && e.FoundInstance==FerruleOwnedSourceInstanceKind.Scalar && e.LimitKind is null && e.Used is null && e.Requested is null && e.Maximum is null,"P11 complete bare traversal cause");"#,
    );
}
#[test]
fn private_lazy_if_and_absent_unused_owner_hosts() {
    for condition in [true, false] {
        let p = basic(
            vec![
                expression(
                    1,
                    Expression::Const {
                        value: Value::Bool(condition),
                    },
                ),
                expression(
                    2,
                    Expression::Const {
                        value: Value::String("chosen".into()),
                    },
                ),
                expression(3, position(SourceOwner::Primary, Some(&["Rows"]))),
                expression(
                    4,
                    Expression::If {
                        condition: 1,
                        then: 2,
                        else_: 3,
                    },
                ),
            ],
            group(vec![binding("Value", 4, ScalarType::String)]),
        );
        let body = if condition {
            r#"var a=Reach("P09.true",()=>ExecuteOwnedBasic(Input,Declared,Limits)); ExpectTree(a,"{\"Value\":\"chosen\"}"); Check(a.Nodes.SequenceEqual(new uint[]{4,1,2}),"P09 true full order"); Check(a.Nodes.Count(n=>n==3)==0,"P09 true else never reached");"#
        } else {
            r#"var a=Reach("P09.false",()=>ExecuteOwnedBasic(Input,Declared,Limits)); ExpectOwned(a,FerruleOwnedSourceErrorKind.InactiveSourceFrame,true,null,new[]{"Rows"}); Check(a.Nodes.SequenceEqual(new uint[]{4,1,3}),"P09 false full order"); Check(a.Nodes.Count(n=>n==3)==1,"P09 false else exactly once");"#
        };
        let s = retain_host(
            if condition {
                "G439-P09-true"
            } else {
                "G439-P09-false"
            },
            &p,
            body,
        );
        assert!(s.contains("return Node_2(context);"));
        assert!(s.contains("return Node_3(context);"));
        assert!(!s.contains("FerruleValue[] { Node_2"));
    }
    let p = basic(
        vec![expression(1, field(SourceOwner::Primary, &["Value"], None))],
        group(vec![binding("Value", 1, ScalarType::String)]),
    );
    retain_host(
        "G439-P10-primary",
        &p,
        r#"var a=Reach("P10.primary",()=>ExecuteOwnedBasic(Input,Declared,Limits)); ExpectTree(a,"{\"Value\":\"P.root\"}"); Check(Declared[0].Instance is null,"P10 explicit unavailable Unused declaration remains unused");"#,
    );
    let p = basic(
        vec![expression(1, field(named("Unused"), &["Value"], None))],
        group(vec![binding("Value", 1, ScalarType::String)]),
    );
    retain_host(
        "G439-P10-demand",
        &p,
        r#"var a=Reach("P10.demand",()=>ExecuteOwnedBasic(Input,Declared,Limits)); ExpectOwned(a,FerruleOwnedSourceErrorKind.SourceUnavailable,false,"Unused",null);"#,
    );
}
#[test]
fn private_flat_root_empty_pin_and_shared_physical_owner_hosts() {
    let p = basic(
        vec![
            expression(1, field(SourceOwner::Primary, &["Value"], Some(&[]))),
            expression(2, position(SourceOwner::Primary, Some(&[]))),
        ],
        owned_iteration(
            SourceOwner::Primary,
            &[],
            vec![
                binding("Value", 1, ScalarType::String),
                binding("Position", 2, ScalarType::Int),
            ],
        ),
    );
    let s = retain_host(
        "G439-P12",
        &p,
        r#"Input=R(G(("Value",S("flat1"))),G(("Value",S("flat2")))); var a=Reach("P12",()=>ExecuteOwnedBasic(Input,Declared,Limits)); ExpectTree(a,"[{\"Value\":\"flat1\",\"Position\":1},{\"Value\":\"flat2\",\"Position\":2}]");"#,
    );
    assert!(!p.source.repeating);
    assert!(s.contains("OwnedPosition(global::Ferrule.Runtime.FerruleOwnedSourceOwner.Primary, global::System.Array.Empty<string>()"));
    assert!(
        s.contains("Primary, new string[] { \"Value\" }, global::System.Array.Empty<string>()")
            || s.contains("Primary, new string[] { \"Value\" }, Array.Empty<string>()")
    );
    let mut child = owned_iteration(
        named("Ref"),
        &["Rows"],
        vec![
            binding("PrimaryPosition", 1, ScalarType::Int),
            binding("NamedPosition", 2, ScalarType::Int),
        ],
    );
    child.target_field = "Named".into();
    let mut root = owned_iteration(SourceOwner::Primary, &["Rows"], vec![]);
    root.children.push(child);
    let p = basic(
        vec![
            expression(1, position(SourceOwner::Primary, Some(&["Rows"]))),
            expression(2, position(named("Ref"), Some(&["Rows"]))),
        ],
        root,
    );
    retain_host(
        "G439-P13",
        &p,
        r#"Declared[1]=new FerruleDeclaredNamedSourceRoot("Ref",Input); var a=Reach("P13",()=>ExecuteOwnedBasic(Input,Declared,Limits)); ExpectTree(a,"[{\"Named\":[{\"PrimaryPosition\":1,\"NamedPosition\":1},{\"PrimaryPosition\":1,\"NamedPosition\":2},{\"PrimaryPosition\":1,\"NamedPosition\":3}]},{\"Named\":[{\"PrimaryPosition\":2,\"NamedPosition\":1},{\"PrimaryPosition\":2,\"NamedPosition\":2},{\"PrimaryPosition\":2,\"NamedPosition\":3}]},{\"Named\":[{\"PrimaryPosition\":3,\"NamedPosition\":1},{\"PrimaryPosition\":3,\"NamedPosition\":2},{\"PrimaryPosition\":3,\"NamedPosition\":3}]}]"); Check(ReferenceEquals(Input,Declared[1].Instance),"P13 same physical object, distinct source owner contexts");"#,
    );
}
#[test]
fn private_plain_child_and_null_legacy_owned_cardinality_hosts() {
    // P17 generated child is legacy plain under a genuine owned Rows parent.
    // Compact/reindex and generated owned-plain DTO support remain outside #518.
    let mut child = plain_iteration(
        &["Details"],
        vec![
            binding("Counter", 1, ScalarType::Int),
            binding("ParentPosition", 2, ScalarType::Int),
        ],
    );
    child.target_field = "Details".into();
    let mut root = owned_iteration(SourceOwner::Primary, &["Rows"], vec![]);
    root.children.push(child);
    let p = basic(
        vec![
            expression(
                1,
                field(SourceOwner::Primary, &["Rows", "Details", "Counter"], None),
            ),
            expression(2, position(SourceOwner::Primary, None)),
        ],
        root,
    );
    retain_host(
        "G439-P17-plain",
        &p,
        r#"Input=G(("Rows",R(G(("Value",S("r1")),("Details",G(("Counter",I(1))))),G(("Value",S("r2")),("Details",G(("Counter",I(2))))))));var a=Reach("P17.plain",()=>ExecuteOwnedBasic(Input,Declared,Limits));var b=Reach("P17.direct.owned.plain",()=>{var roots=FerruleBasicSourceRoots.Create(Input,Declared,Limits);var parents=roots.RootContext().WalkOwnedSourceCohort(FerruleOwnedSourceOwner.Primary,new[]{"Rows"});var children=parents[1].Context.WalkOwnedSourceCohort(FerruleOwnedSourceOwner.Primary,new[]{"Rows","Details"});var candidate=children[0];var tree=G(("Counter",new FerruleScalar(Node_1(candidate.Context))),("ParentPosition",new FerruleScalar(Node_2(candidate.Context))));var result=new DirectPlainResult(tree,candidate);GC.KeepAlive(parents);GC.KeepAlive(children);return result;});ExpectTree(a,"[{\"Details\":[{\"Counter\":1,\"ParentPosition\":1}]},{\"Details\":[{\"Counter\":2,\"ParentPosition\":2}]}]");ExpectTree(b,"{\"Counter\":2,\"ParentPosition\":2}");Check(!((DirectPlainResult)b.Result!).Candidate.HasNewCursor,"P17 direct child no new cursor or invented position");"#,
    );
    for owned in [true, false] {
        let root = if owned {
            owned_iteration(
                SourceOwner::Primary,
                &[],
                vec![
                    binding("Value", 1, ScalarType::String),
                    binding("Position", 2, ScalarType::Int),
                ],
            )
        } else {
            let mut root = group(vec![
                binding("Value", 1, ScalarType::String),
                binding("Position", 2, ScalarType::Int),
            ]);
            root.repeating = true;
            root.iteration = Some(IterationPlan::source(vec![]));
            root
        };
        let p = basic(
            vec![
                expression(
                    1,
                    if owned {
                        field(SourceOwner::Primary, &["Value"], Some(&[]))
                    } else {
                        Expression::SourceField {
                            frame: None,
                            path: path(&["Value"]),
                        }
                    },
                ),
                expression(
                    2,
                    if owned {
                        position(SourceOwner::Primary, Some(&[]))
                    } else {
                        Expression::Position { collection: vec![] }
                    },
                ),
            ],
            root,
        );
        let body = if owned {
            r#"Input=R(new FerruleScalar(FerruleValue.Null),G(("Value",S("kept"))),new FerruleScalar(FerruleValue.JsonNull));var a=Reach("P18.owned",()=>ExecuteOwnedBasic(Input,Declared,Limits));ExpectTree(a,"[{\"Value\":\"kept\",\"Position\":2}]");"#
        } else {
            r#"Input=R(new FerruleScalar(FerruleValue.Null),G(("Value",S("kept"))),new FerruleScalar(FerruleValue.JsonNull));var a=Reach("P18.legacy",()=>ExecuteOwnedBasic(Input,Declared,Limits));ExpectTree(a,"[{\"Value\":\"Null\",\"Position\":1},{\"Value\":\"kept\",\"Position\":2},{\"Value\":\"Null\",\"Position\":3}]");"#
        };
        retain_host(
            if owned {
                "G439-P18-owned"
            } else {
                "G439-P18-legacy"
            },
            &p,
            body,
        );
    }
}
#[test]
fn private_selected_target_raise_and_unknown_order_hosts() {
    let mut p = basic(
        vec![
            expression(1, field(SourceOwner::Primary, &["Value"], None)),
            expression(2, Expression::Raise { message: None }),
        ],
        group(vec![binding("Value", 1, ScalarType::String)]),
    );
    add_target(
        &mut p,
        "RaiseTarget",
        group(vec![binding("Value", 2, ScalarType::String)]),
    );
    let s = retain_host(
        "G439-P19",
        &p,
        r#"var a=ReachSelected("P19.selected",FerruleTargetSelection.Primary());var b=Reach("P19.all",()=>ExecuteOwnedBasic(Input,Declared,Limits));ExpectTree(a,"{\"Value\":\"P.root\"}");Check(a.Nodes.SequenceEqual(new uint[]{1}),"P19 unselected Raise not reached");ExpectRuntime(b,FerruleRuntimeError.MappingException);Check(b.Result is null,"P19 no partial ExecutionOutputs returned");Check(b.Nodes.SequenceEqual(new uint[]{1,2}),"P19 all target order and Raise");"#,
    );
    assert!(s.contains("FerruleFailures.MappingException(2U, null)"));
    let mut p = basic(
        vec![
            expression(1, field(SourceOwner::Primary, &["Value"], None)),
            expression(2, field(named("Ref"), &["Value"], None)),
        ],
        group(vec![binding("Value", 1, ScalarType::String)]),
    );
    add_target(
        &mut p,
        "named",
        group(vec![binding("Value", 2, ScalarType::String)]),
    );
    let s = retain_host(
        "G439-P20",
        &p,
        r#"var a=Reach("P20.all",()=>ExecuteOwnedBasic(Input,Declared,Limits));var b=ReachSelected("P20.named",FerruleTargetSelection.Named("named"));Limits=new FerruleOwnedSourceLimits(0,0,0,0,0);var c=ReachSelected("P20.unknown",FerruleTargetSelection.Named("missing"));ExpectTree(a,"{\"Value\":\"P.root\"}");var all=(ExecutionOutputs)a.Result!;Check(all.Extras.Count==1 && all.Extras[0].Name=="named" && Json(Plain(all.Extras[0].Instance))=="{\"Value\":\"N.root\"}","P20 exact ordered named output");ExpectTree(b,"{\"Value\":\"N.root\"}");var e=ExpectRuntime(c,FerruleRuntimeError.UnknownTarget);Check(e.Detail=="missing" && c.Nodes.Length==0,"P20 unknown before impossible roots budget");"#,
    );
    assert!(
        s.find("var selected = ResolveOwnedBasicTarget(selection);")
            .unwrap()
            < s.rfind("var roots = global::Ferrule.Runtime.FerruleBasicSourceRoots.Create")
                .unwrap()
    );
}
#[test]
fn private_zero_byte_scalar_and_forwarded_five_limit_hosts() {
    let mut p = basic(
        vec![expression(1, field(SourceOwner::Primary, &[], None))],
        group(vec![binding("Value", 1, ScalarType::String)]),
    );
    p.source = SchemaNode::scalar("Input", ScalarType::String);
    p.extra_sources.clear();
    let s = retain_host(
        "G439-R02",
        &p,
        r#"Input=S("P.root");Declared=Array.Empty<FerruleDeclaredNamedSourceRoot>();Limits=new FerruleOwnedSourceLimits(1,0,0,0,0);var a=Reach("R02",()=>ExecuteOwnedBasic(Input,Declared,Limits));ExpectTree(a,"{\"Value\":\"P.root\"}");"#,
    );
    assert!(
        s.contains("Primary, global::System.Array.Empty<string>(), null")
            || s.contains("Primary, Array.Empty<string>(), null")
    );
    assert!(!s.contains("WalkSourceCohort("));
    assert!(!s.contains("WalkOwnedSourceCohort("));
    p.extra_sources.push(NamedSourceProgram {
        name: "Unused".into(),
        source: source_schema("Static"),
        dynamic: None,
    });
    retain_host(
        "G439-R01-R04-registry",
        &p,
        r#"Input=S("P.root");Declared=new[]{new FerruleDeclaredNamedSourceRoot("Unused",null)};var a=ReachWithLimits("R04.roots.exact",new FerruleOwnedSourceLimits(2,0,0,6,0));var b=ReachWithLimits("R04.roots.refuse",new FerruleOwnedSourceLimits(1,0,0,6,0));var c=ReachWithLimits("R04.bytes.refuse",new FerruleOwnedSourceLimits(2,0,0,5,0));var d=ReachWithLimits("R01.bytes.zero",new FerruleOwnedSourceLimits(2,0,0,0,0));ExpectTree(a,"{\"Value\":\"P.root\"}");ExpectLimit(b,FerruleOwnedSourceLimitKind.Roots,1);ExpectLimit(c,FerruleOwnedSourceLimitKind.MetadataBytes,5);ExpectLimit(d,FerruleOwnedSourceLimitKind.MetadataBytes,0);"#,
    );
    let mut p = basic(
        vec![expression(1, field(SourceOwner::Primary, &["Value"], None))],
        group(vec![binding("Value", 1, ScalarType::String)]),
    );
    p.extra_sources.clear();
    retain_host(
        "G439-R04-path",
        &p,
        r#"Declared=Array.Empty<FerruleDeclaredNamedSourceRoot>();var a=ReachWithLimits("R04.path.exact",new FerruleOwnedSourceLimits(1,0,1,4096,0));var b=ReachWithLimits("R04.path.refuse",new FerruleOwnedSourceLimits(1,0,0,4096,0));ExpectTree(a,"{\"Value\":\"P.root\"}");ExpectLimit(b,FerruleOwnedSourceLimitKind.PathSegments,0);"#,
    );
    let mut p = basic(
        vec![
            expression(1, field(SourceOwner::Primary, &["Value"], Some(&[]))),
            expression(2, position(SourceOwner::Primary, Some(&[]))),
        ],
        owned_iteration(
            SourceOwner::Primary,
            &[],
            vec![
                binding("Value", 1, ScalarType::String),
                binding("Position", 2, ScalarType::Int),
            ],
        ),
    );
    p.extra_sources.clear();
    retain_host(
        "G439-R03-R04-walk",
        &p,
        r#"Declared=Array.Empty<FerruleDeclaredNamedSourceRoot>();Input=R(G(("Value",S("flat1"))),G(("Value",S("flat2"))));var a=ReachWithLimits("R04.frames.exact.walk.exact",new FerruleOwnedSourceLimits(1,1,1,4096,2));var b=ReachWithLimits("R04.frames.refuse",new FerruleOwnedSourceLimits(1,0,1,4096,2));var c=ReachWithLimits("R04.walk.refuse",new FerruleOwnedSourceLimits(1,1,1,4096,1));Input=R(G(("Value",S("a"))),G(("Value",S("b"))),G(("Value",S("c"))));var d=ReachWithLimits("R03.walk.refuse",new FerruleOwnedSourceLimits(1,1,1,4096,2));ExpectTree(a,"[{\"Value\":\"flat1\",\"Position\":1},{\"Value\":\"flat2\",\"Position\":2}]");ExpectLimit(b,FerruleOwnedSourceLimitKind.Frames,0);static void ExpectPairedWalkLimit(Reached actual,nuint requested,nuint maximum)
{
    var outer=ExpectRuntime(actual,FerruleRuntimeError.BasicSourceBridge);
    var bridge=outer.BasicSourceBridgeFailure;
    Check(bridge is not null && ReferenceEquals(bridge,outer.InnerException),"exact bare paired bridge cause");
    var cause=bridge!;
    Check(cause.Error==FerruleBasicSourceBridgeErrorKind.PairedMetadataLimit && cause.Component==FerruleBasicSourceBridgeComponent.WalkContexts && cause.Used==0 && cause.Requested==requested && cause.Maximum==maximum,"exact paired walk used/requested/maximum");
    Check(cause.InputOrdinal is null && cause.CandidateOrdinal is null && cause.DeclarationIndex is null && cause.DeclaredCount is null && cause.Found is null && cause.InnerException is null,"unset unrelated paired cause payloads");
    Check(outer.Node is null && outer.Function is null && outer.ExpectedArity is null && outer.ActualArity is null && outer.FoundKind is null && outer.AggregateOperation is null && outer.Detail is null && outer.RequestedItems is null && outer.MaximumItems is null && outer.MaximumDepth is null && outer.RuntimeValue is null && outer.FailureRule is null && outer.MappingFailureMessage is null && outer.MappingExceptionMessage is null && outer.Join is null && outer.UserFunction is null && outer.FunctionParameter is null && outer.ExpectedScalarType is null && outer.RuntimeParameter is null && outer.SourceField is null && outer.FoundInstance is null && outer.PrimaryRoot is null && outer.OwnedSourceFailure is null,"unset unrelated outer runtime payloads");
    Check(outer.Message=="basic source bridge failed" && cause.Message=="basic source bridge refused","exact unchanged bridge factory messages");
    Check(actual.Nodes.Length==0 && actual.Frames.Length==0 && actual.Result is null,"no expression/frame trace or partial returned outcome");
}
ExpectPairedWalkLimit(c,2,1);ExpectPairedWalkLimit(d,3,2);Check(c.Result is null && d.Result is null,"R03/R04 no partial output");"#,
    );
    retain_host(
        "G439-R05",
        &p,
        r#"var a=ReachWithLimits("R05",new FerruleOwnedSourceLimits(nuint.MaxValue,nuint.MaxValue,nuint.MaxValue,nuint.MaxValue,nuint.MaxValue));var e=ExpectOwned(a,FerruleOwnedSourceErrorKind.MetadataArithmeticOverflow,null,null,null);Check(e.LimitKind==FerruleOwnedSourceLimitKind.MetadataSlots && a.Nodes.Length==0,"R05 checked derivation before expression entry");"#,
    );
}
#[test]
fn private_ordinary_context_scalar_call_if_parameter_and_construction_seams() {
    let mut p = basic(
        vec![
            expression(1, field(SourceOwner::Primary, &["Value"], None)),
            expression(
                2,
                Expression::Const {
                    value: Value::String("!".into()),
                },
            ),
            expression(
                3,
                Expression::Call {
                    function: ScalarFunction::Concat,
                    args: vec![1, 2],
                },
            ),
            expression(
                4,
                Expression::RuntimeValue {
                    value: RuntimeValue::MappingFilePath,
                },
            ),
            expression(
                5,
                Expression::RuntimeParameter {
                    name: "provided".into(),
                    ty: ScalarType::String,
                },
            ),
            expression(
                6,
                Expression::RuntimeParameterDefault {
                    name: "absent".into(),
                    ty: ScalarType::String,
                    default: 2,
                },
            ),
            expression(
                7,
                Expression::Const {
                    value: Value::Bool(true),
                },
            ),
            expression(
                8,
                Expression::If {
                    condition: 7,
                    then: 3,
                    else_: 6,
                },
            ),
        ],
        group(vec![
            binding("Call", 3, ScalarType::String),
            binding("Runtime", 4, ScalarType::String),
            binding("Parameter", 5, ScalarType::String),
            binding("Default", 6, ScalarType::String),
            binding("If", 8, ScalarType::String),
        ]),
    );
    retain_host(
        "518-ordinary-context",
        &p,
        r#"ParameterValues=new[]{new KeyValuePair<string,FerruleValue>("provided",FerruleValue.FromString("parameter"))};Execution=FerruleExecutionContext.WithParameters("mapping.test",new FerruleRuntimeParameters(ParameterValues));var a=Reach("ordinary.context",()=>ExecuteOwnedBasic(Input,Declared,Limits,Execution));ExpectTree(a,"{\"Call\":\"P.root!\",\"Runtime\":\"mapping.test\",\"Parameter\":\"parameter\",\"Default\":\"!\",\"If\":\"P.root!\"}");Check(a.Nodes.SequenceEqual(new uint[]{3,1,2,4,5,6,2,8,7,3,1,2}),"ordinary argument/default/If order");"#,
    );
    p.root = TargetScope {
        target_field: String::new(),
        repeating: false,
        iteration: None,
        construction: TargetConstruction::Scalar {
            expression: 1,
            target_domain: ScalarType::String.into(),
        },
        bindings: vec![],
        children: vec![],
    };
    p.target = target_schema(&p.root, "Output");
    let refused = render_basic_owned_mapping(&p);
    eprintln!("518 owned scalar construction complete Program={p:#?} Result={refused:#?}");
    let Err(EmitError::ProgramValidation(ProgramValidationError::OwnedSource { diagnostic })) =
        refused
    else {
        panic!("complete owned scalar construction refusal captured");
    };
    let route = ::mapping::SourceScopeLocation::root(::mapping::SourceScopeTarget::Primary);
    assert_eq!(
        *diagnostic,
        codegen::OwnedSourceDiagnostic::Capability {
            location: codegen::OwnedSourceDiagnosticLocation::Expression {
                namespace: codegen::OwnedSourceNamespace::Main,
                node: 1,
                scope: Some(route.clone()),
            },
            request: Some(codegen::OwnedSourceDiagnosticRequest::Field {
                owner: codegen::OwnedSourceDiagnosticOwner::Primary,
                path: path(&["Value"]),
                frame: None,
            }),
            reason: codegen::OwnedSourceCapabilityReason::SpecialConstruction,
            origin: codegen::OwnedSourceCapabilityOrigin::Scope { scope: route },
        }
    );
    // This is the ordinary scalar construction control in the paired context.
    // Owned scalar construction stays explicitly closed by shared validation.
    p.expressions[0].expression = Expression::SourceField {
        path: path(&["Value"]),
        frame: None,
    };
    retain_host(
        "518-scalar-construction",
        &p,
        r#"var a=Reach("scalar.target",()=>ExecuteOwnedBasic(Input,Declared,Limits));ExpectTree(a,"\"P.root\"");"#,
    );
}
#[test]
fn private_denials_do_not_open_public_emitters_or_controlled_contexts() {
    let p = basic(
        vec![expression(1, field(SourceOwner::Primary, &["Value"], None))],
        group(vec![binding("Value", 1, ScalarType::String)]),
    );
    let ordinary = emit(&p);
    let typed = emit_typed(&p);
    let private = render_basic_owned_mapping(&p);
    eprintln!(
        "518 public guards complete Program={p:#?} ordinary={ordinary:#?} typed={typed:#?} private={private:#?}"
    );
    assert!(matches!(
        ordinary,
        Err(EmitError::ProgramValidation(
            ProgramValidationError::OwnedSource { .. }
        ))
    ));
    assert!(matches!(
        typed,
        Err(EmitError::ProgramValidation(
            ProgramValidationError::OwnedSource { .. }
        ))
    ));
    assert!(private.is_ok());
    let mut p = basic(
        vec![
            expression(1, field(SourceOwner::Primary, &["Rows", "Value"], None)),
            expression(
                2,
                Expression::Const {
                    value: Value::Bool(true),
                },
            ),
        ],
        owned_iteration(
            SourceOwner::Primary,
            &["Rows"],
            vec![binding("Value", 1, ScalarType::String)],
        ),
    );
    p.root.iteration = Some(IterationPlan::new(
        codegen::OwnedSourceIteration::new(OwnedSourceRef {
            owner: SourceOwner::Primary,
            path: path(&["Rows"]),
        }),
        Some(2),
        None,
        vec![],
        IterationOutput::Repeated,
    ));
    let result = render_basic_owned_mapping(&p);
    eprintln!("518 closed control complete Program={p:#?} Result={result:#?}");
    assert!(
        matches!(result,Err(EmitError::ProgramValidation(ProgramValidationError::OwnedSource{diagnostic})) if matches!(*diagnostic,codegen::OwnedSourceDiagnostic::Capability{reason:codegen::OwnedSourceCapabilityReason::ScopeControl,..}))
    );
}

#[test]
fn private_owned_literal_and_none_empty_seams_preserve_exact_spelling() {
    let source = SchemaNode::group(
        "Primary",
        vec![SchemaNode::scalar("Value", ScalarType::String)],
    );
    for frame in [None, Some(&[][..])] {
        let mut p = basic(
            vec![expression(
                1,
                field(SourceOwner::Primary, &["Value"], frame),
            )],
            owned_iteration(
                SourceOwner::Primary,
                &[],
                vec![binding("Value", 1, ScalarType::String)],
            ),
        );
        p.source = source.clone();
        let result = render_basic_owned_mapping(&p);
        eprintln!("518 None/Some[] complete Program={p:#?} Result={result:#?}");
        let rendered = result.unwrap();
        let expected = if frame.is_some() {
            "Primary, new string[] { \"Value\" }, global::System.Array.Empty<string>()"
        } else {
            "Primary, new string[] { \"Value\" }, null"
        };
        assert!(rendered.contains(expected));
    }
    let literal_cases = [
        ("🙂", "\"\\uD83D\\uDE42\""),
        (
            "quote\" backslash\\ newline\n tab\t",
            "\"quote\\\" backslash\\\\ newline\\n tab\\t\"",
        ),
    ];
    for (input, expected) in literal_cases {
        let p = basic(
            vec![expression(
                1,
                Expression::Const {
                    value: Value::String(input.into()),
                },
            )],
            group(vec![binding("Value", 1, ScalarType::String)]),
        );
        let result = render_basic_owned_mapping(&p);
        eprintln!(
            "518 full literal input={input:?} expected_constant={expected:?} complete Program={p:#?} Result={result:#?}"
        );
        let rendered = result.unwrap();
        assert!(rendered.is_ascii());
        assert!(rendered.contains(expected));
    }
}

#[test]
fn private_uniterated_nonempty_and_empty_repetitions_keep_typed_refusal() {
    let p = basic(
        vec![expression(
            1,
            field(SourceOwner::Primary, &["Rows", "Value"], None),
        )],
        group(vec![binding("Value", 1, ScalarType::String)]),
    );
    for empty in [false, true] {
        let body = if empty {
            r#"Input=G(("Rows",R()));var a=Reach("518.uniterated.empty",()=>ExecuteOwnedBasic(Input,Declared,Limits));var e=ExpectOwned(a,FerruleOwnedSourceErrorKind.UniteratedRepetition,true,null,null);Check(e.Path is not null && e.Path.SequenceEqual(new[]{"Rows","Value"}) && e.Segment==1 && e.FoundInstance is null && e.InnerException is null,"strict empty uniterated repetition");"#
        } else {
            r#"var a=Reach("518.uniterated.nonempty",()=>ExecuteOwnedBasic(Input,Declared,Limits));var e=ExpectOwned(a,FerruleOwnedSourceErrorKind.UniteratedRepetition,true,null,null);Check(e.Path is not null && e.Path.SequenceEqual(new[]{"Rows","Value"}) && e.Segment==1 && e.FoundInstance is null && e.InnerException is null,"strict nonempty uniterated repetition");"#
        };
        retain_host(
            if empty {
                "518-uniterated-empty"
            } else {
                "518-uniterated-nonempty"
            },
            &p,
            body,
        );
    }
}

const HOST_PREFIX: &str = r#"using System;
using System.Linq;
using System.Collections.Generic;
using System.Text.Json;
using Ferrule.Runtime;
namespace Ferrule.Generated;
public static partial class GeneratedMapping
{
    private static readonly List<uint> NodeTrace=new();
    private static readonly List<object> FrameTrace=new();
    static partial void ObserveOwnedBasicNode(uint node,FerruleBasicScopeContext context)
    {
        NodeTrace.Add(node);
        FrameTrace.Add(new { node, legacyFrames=context.LegacyView.Frames.Select(InstanceSnapshot).ToArray() });
    }
    private sealed record DirectPlainResult(FerruleInstance Tree,FerruleBasicSourceCandidate Candidate);
    private sealed record Reached(object? Result,Exception? Error,uint[] Nodes,object[] Frames,string CompleteInput,string CompleteActual);
    private static FerruleInstance Input=PrimaryFixture();
    private static FerruleDeclaredNamedSourceRoot[] Declared=Declarations();
    private static FerruleOwnedSourceLimits Limits=new(16,8,16,1048576,64);
    private static FerruleExecutionContext? Execution=null;
    private static KeyValuePair<string,FerruleValue>[] ParameterValues=Array.Empty<KeyValuePair<string,FerruleValue>>();
    private static string Json(object? value)=>JsonSerializer.Serialize(value);
    private static Reached ReachWithLimits(string id,FerruleOwnedSourceLimits limits)
    { Limits=limits;return Reach(id,()=>ExecuteOwnedBasic(Input,Declared,Limits)); }
    private static Reached ReachSelected(string id,FerruleTargetSelection selection)
    { return Reach(id,()=>ExecuteSelectedOwnedBasic(Input,selection,Declared,Limits,Execution),selection); }
    private static Reached Reach(string id,Func<object?> subject,FerruleTargetSelection? selection=null)
    {
        NodeTrace.Clear();FrameTrace.Clear();
        var input=Json(new { classification="complete_typed_input", selection=selection is null?null:new {selection.IsPrimary,selection.Name},source=InstanceSnapshot(Input),
            declarations=Declared.Select((d,i)=>new { index=i,d.Name,source=InstanceSnapshot(d.Instance) }).ToArray(),
            limits=new { roots=(ulong)Limits.MaxRoots,frames=(ulong)Limits.MaxActiveFrames,segments=(ulong)Limits.MaxPathSegments,bytes=(ulong)Limits.MaxMetadataBytes,contexts=(ulong)Limits.MaxWalkContexts },
            execution=Execution is null?null:new { Execution.MappingFilePath,Execution.MainMappingFilePath,Execution.CurrentDateTime,parameterCount=Execution.RuntimeParameters.Count,parameters=ParameterValues.Select(p=>new {p.Key,value=ValueSnapshot(p.Value)}).ToArray() } });
        Console.WriteLine(Json(new {id,classification="before_subject",completeInput=input}));
        object? result=null;Exception? error=null;
        try { result=subject(); } catch(Exception reached) { error=reached; }
        var nodes=NodeTrace.ToArray();var frames=FrameTrace.ToArray();
        var complete=Json(new { classification="actual_reached_result", result=ResultSnapshot(result),error=error is null?null:ErrorSnapshot(error),nodes,frames });
        Console.WriteLine(Json(new {id,classification="before_comparisons",completeInput=input,completeActual=complete}));
        return new Reached(result,error,nodes,frames,input,complete);
    }
    private static object? ResultSnapshot(object? result)=>result switch
    {
        ExecutionOutputs all=>new {kind="ExecutionOutputs",primary=InstanceSnapshot(all.Primary),extras=all.Extras.Select(e=>new {e.Name,instance=InstanceSnapshot(e.Instance)}).ToArray()},
        SelectedTargetOutput.Primary primary=>new {kind="Primary",instance=InstanceSnapshot(primary.Instance)},
        SelectedTargetOutput.Named named=>new {kind="Named",named.Output.Name,instance=InstanceSnapshot(named.Output.Instance)},
        DirectPlainResult plain=>new {kind="direct_owned_plain_runtime_control",tree=InstanceSnapshot(plain.Tree),sourceId=(ulong)plain.Candidate.SourceId,raw=plain.Candidate.RawSourceOrdinal.HasValue?(ulong?)plain.Candidate.RawSourceOrdinal.Value:null,newCursor=plain.Candidate.HasNewCursor,legacyFrames=plain.Candidate.Context.LegacyView.Frames.Select(InstanceSnapshot).ToArray()},
        FerruleInstance instance=>InstanceSnapshot(instance),
        FerruleValue value=>ValueSnapshot(value),
        Exception error=>ErrorSnapshot(error),
        null=>null,
        _=>throw new InvalidOperationException("Unrecorded returned result type")
    };
    private static object ValueSnapshot(FerruleValue value)=>new {kind=value.Kind.ToString(),value=value.Kind switch
    {FerruleValueKind.String=>(object?)value.StringValue,FerruleValueKind.Int64=>value.Int64Value,FerruleValueKind.Bool=>value.BooleanValue,FerruleValueKind.Double=>BitConverter.DoubleToInt64Bits(value.DoubleValue),_=>null}};
    private static object? InstanceSnapshot(FerruleInstance? value)=>value switch
    {
        null=>null,FerruleScalar scalar=>new {kind="Scalar",value=ValueSnapshot(scalar.Value)},
        FerruleGroup group=>new {kind="Group",fields=group.Fields.Select(field=>new {field.Name,value=InstanceSnapshot(field.Value)}).ToArray()},
        FerruleRepeated repeated=>new {kind="Repeated",items=repeated.Items.Select(InstanceSnapshot).ToArray()},
        _=>throw new InvalidOperationException("Unrecorded input instance kind")
    };
    private static object? Plain(FerruleInstance value)=>value switch
    {
        FerruleScalar scalar=>scalar.Value.Kind switch {FerruleValueKind.String=>(object?)scalar.Value.StringValue,FerruleValueKind.Int64=>scalar.Value.Int64Value,FerruleValueKind.Bool=>scalar.Value.BooleanValue,FerruleValueKind.Null=>"Null",FerruleValueKind.JsonNull=>"JsonNull",FerruleValueKind.XmlNil=>"XmlNil",FerruleValueKind.Double=>scalar.Value.DoubleValue,_=>throw new InvalidOperationException()},
        FerruleGroup group=>group.Fields.ToDictionary(field=>field.Name,field=>Plain(field.Value),StringComparer.Ordinal),
        FerruleRepeated repeated=>repeated.Items.Select(Plain).ToArray(),_=>throw new InvalidOperationException()
    };
    private static void Check(bool condition,string expected) { if(!condition)throw new InvalidOperationException("Frozen expected: "+expected); }
    private static void ExpectTree(Reached actual,string expected)
    {
        Check(actual.Error is null,"successful typed return");
        var instance=actual.Result switch {ExecutionOutputs all=>all.Primary,SelectedTargetOutput.Primary primary=>primary.Instance,SelectedTargetOutput.Named named=>named.Output.Instance,DirectPlainResult plain=>plain.Tree,FerruleInstance root=>root,_=>throw new InvalidOperationException("Missing tree")};
        var reached=Json(Plain(instance));Console.WriteLine(Json(new {classification="comparison",actual=reached,expected}));Check(reached==expected,expected);
    }
    private static FerruleRuntimeException ExpectRuntime(Reached actual,FerruleRuntimeError expected)
    {var error=actual.Error as FerruleRuntimeException;Check(error is not null && error.Error==expected,"outer "+expected);return error!;}
    private static FerruleOwnedSourceException ExpectOwned(Reached actual,FerruleOwnedSourceErrorKind expected,bool? primary,string? name,string[]? collection)
    {
        var outer=ExpectRuntime(actual,FerruleRuntimeError.OwnedSource);var error=outer.OwnedSourceFailure;
        Check(error is not null && ReferenceEquals(error,outer.InnerException) && error.Error==expected,"bare owned cause "+expected);
        var found=error!;
        if(primary.HasValue)Check(found.Owner?.IsPrimary==primary && found.Owner?.Name==name && found.RequestedName==name,"exact owner/name");
        Check(collection is null?found.Collection is null:found.Collection is not null && found.Collection.SequenceEqual(collection),"exact None or Some collection");
        Check(found.DeclarationIndices is null && found.Frame is null && found.RequestedIndex is null,"unchanged absent declaration/frame/index fields");
        if(expected!=FerruleOwnedSourceErrorKind.Traversal && expected!=FerruleOwnedSourceErrorKind.UniteratedRepetition)
            Check(found.Path is null && found.Segment is null && found.FoundInstance is null && found.InnerException is null,"no invented traversal payload");
        if(expected!=FerruleOwnedSourceErrorKind.MetadataLimit && expected!=FerruleOwnedSourceErrorKind.MetadataArithmeticOverflow)
            Check(found.LimitKind is null && found.Used is null && found.Requested is null && found.Maximum is null,"no invented resource payload");
        Check(outer.Node is null && outer.Function is null && outer.ExpectedArity is null && outer.ActualArity is null && outer.FoundKind is null && outer.AggregateOperation is null && outer.Detail is null && outer.RequestedItems is null && outer.MaximumItems is null && outer.MaximumDepth is null && outer.RuntimeValue is null && outer.FailureRule is null && outer.MappingFailureMessage is null && outer.MappingExceptionMessage is null && outer.Join is null && outer.UserFunction is null && outer.FunctionParameter is null && outer.ExpectedScalarType is null && outer.RuntimeParameter is null && outer.SourceField is null && outer.FoundInstance is null && outer.PrimaryRoot is null && outer.BasicSourceBridgeFailure is null,"complete untouched runtime cause envelope");
        Check(actual.Result is null,"no partial returned outcome");return found;
    }
    private static void ExpectLimit(Reached actual,FerruleOwnedSourceLimitKind expected,nuint maximum)
    {var error=ExpectOwned(actual,FerruleOwnedSourceErrorKind.MetadataLimit,null,null,null);Check(error.LimitKind==expected && error.Maximum==maximum,"exact limit kind and maximum");}
    private static FerruleInstance S(string value)=>new FerruleScalar(FerruleValue.FromString(value));
    private static FerruleInstance I(long value)=>new FerruleScalar(FerruleValue.FromInt64(value));
    private static FerruleInstance G(params (string Name,FerruleInstance Value)[] fields)=>new FerruleGroup(fields.Select(field=>new FerruleField(field.Name,field.Value)));
    private static FerruleInstance R(params FerruleInstance[] values)=>new FerruleRepeated(values);
    private static FerruleInstance PrimaryFixture()=>G(("Value",S("P.root")),("Ref",G(("Value",S("P.collision")))),("Details",G(("Counter",I(99)))),("Rows",R(G(("Value",S("P1")),("Score",I(30)),("Rows",R(G(("Value",S("P1.1"))),G(("Value",S("P1.2")))))),G(("Value",S("P2")),("Score",I(10))),G(("Value",S("P3")),("Score",I(20)),("Details",G())))));
    private static FerruleDeclaredNamedSourceRoot[] Declarations()=>new[]{new FerruleDeclaredNamedSourceRoot("Unused",null),new FerruleDeclaredNamedSourceRoot("Ref",G(("Value",S("N.root")),("Rows",R(G(("Value",S("N1")),("Score",I(30)),("Details",new FerruleScalar(FerruleValue.JsonNull))),G(("Value",S("N2")),("Score",I(10)),("Details",G(("Counter",I(7))))))))),new FerruleDeclaredNamedSourceRoot(" Ref 雪 ",G(("Value",S("N.exact")))),new FerruleDeclaredNamedSourceRoot("ref",G(("Value",S("N.lower")))),new FerruleDeclaredNamedSourceRoot("primary",G(("Value",S("N.literal.primary"))))};
"#;
const HOST_SUFFIX: &str = r#"
    }
}
"#;

const HOST_ERROR_SNAPSHOT: &str = r#"    private static object ErrorSnapshot(Exception error)
    {
        var outer = error as FerruleRuntimeException;
        var owned = error as FerruleOwnedSourceException;
        var bridge = error as FerruleBasicSourceBridgeException;
        var traversal = error as FerruleOwnedSourceTraversalException;
        return new
        {
            type = error.GetType().FullName, error.Message, error.HResult, fullNative = error.ToString(),
            argument = (error as ArgumentException)?.ParamName,
            runtime = outer is null ? null : new
            {
                category = outer.Error.ToString(), outer.Node, outer.Function, outer.ExpectedArity, outer.ActualArity,
                foundKind = outer.FoundKind?.ToString(), aggregate = outer.AggregateOperation?.ToString(), outer.Detail,
                requestedItems = outer.RequestedItems?.ToString(), maximumItems = outer.MaximumItems?.ToString(), outer.MaximumDepth,
                runtimeValue = outer.RuntimeValue?.ToString(), outer.FailureRule, outer.MappingFailureMessage,
                outer.MappingExceptionMessage, outer.Join, outer.UserFunction, outer.FunctionParameter,
                expectedScalar = outer.ExpectedScalarType?.ToString(), outer.RuntimeParameter, outer.SourceField,
                outer.FoundInstance, outer.PrimaryRoot,
                ownedSameInner = outer.OwnedSourceFailure is not null && ReferenceEquals(outer.OwnedSourceFailure, outer.InnerException),
                bridgeSameInner = outer.BasicSourceBridgeFailure is not null && ReferenceEquals(outer.BasicSourceBridgeFailure, outer.InnerException),
                owned = outer.OwnedSourceFailure is null ? null : ErrorSnapshot(outer.OwnedSourceFailure),
                bridge = outer.BasicSourceBridgeFailure is null ? null : ErrorSnapshot(outer.BasicSourceBridgeFailure),
            },
            owned = owned is null ? null : new
            {
                kind = owned.Error.ToString(), owner = owned.Owner?.Name, primary = owned.Owner?.IsPrimary,
                name = owned.RequestedName, declarations = owned.DeclarationIndices?.ToArray(),
                path = owned.Path?.ToArray(), frame = owned.Frame?.ToArray(), collection = owned.Collection?.ToArray(),
                segment = owned.Segment.HasValue ? (ulong?)owned.Segment.Value : null,
                found = owned.FoundInstance?.ToString(), index = owned.RequestedIndex.HasValue ? (ulong?)owned.RequestedIndex.Value : null,
                limit = owned.LimitKind?.ToString(), used = owned.Used.HasValue ? (ulong?)owned.Used.Value : null,
                requested = owned.Requested.HasValue ? (ulong?)owned.Requested.Value : null,
                maximum = owned.Maximum.HasValue ? (ulong?)owned.Maximum.Value : null,
            },
            bridge = bridge is null ? null : new
            {
                kind = bridge.Error.ToString(), component = bridge.Component?.ToString(),
                used = bridge.Used.HasValue ? (ulong?)bridge.Used.Value : null,
                requested = bridge.Requested.HasValue ? (ulong?)bridge.Requested.Value : null,
                maximum = bridge.Maximum.HasValue ? (ulong?)bridge.Maximum.Value : null,
                input = bridge.InputOrdinal.HasValue ? (ulong?)bridge.InputOrdinal.Value : null,
                candidate = bridge.CandidateOrdinal.HasValue ? (ulong?)bridge.CandidateOrdinal.Value : null,
                declaration = bridge.DeclarationIndex.HasValue ? (ulong?)bridge.DeclarationIndex.Value : null,
                declared = bridge.DeclaredCount.HasValue ? (ulong?)bridge.DeclaredCount.Value : null,
                found = bridge.Found?.ToString(),
            },
            traversal = traversal is null ? null : new
            { kind = traversal.Error.ToString(), path = traversal.Path.ToArray(),
              segment = traversal.Segment.HasValue ? (ulong?)traversal.Segment.Value : null, found = traversal.Found.ToString() },
            inner = error.InnerException is null ? null : ErrorSnapshot(error.InnerException),
        };
    }
"#;

#[test]
fn private_legacy_dynamic_inventory_closes_only_potential_demands() {
    use codegen::{
        OwnedSourceCapabilityOrigin as Origin, OwnedSourceCapabilityReason as Reason,
        OwnedSourceDiagnostic as Diagnostic, OwnedSourceDiagnosticLocation as Location,
        OwnedSourceNamespace as Namespace,
    };
    use std::error::Error as _;
    fn leaf_expected() -> ProgramValidationError {
        ProgramValidationError::OwnedSource {
            diagnostic: Box::new(Diagnostic::Capability {
                location: Location::Expression {
                    namespace: Namespace::Main,
                    node: 2,
                    scope: None,
                },
                request: None,
                reason: Reason::UnsupportedBasicContext,
                origin: Origin::RetainedExpression {
                    namespace: Namespace::Main,
                    node: 2,
                },
            }),
        }
    }
    fn scope_expected(target: ::mapping::SourceScopeTarget) -> ProgramValidationError {
        let scope = ::mapping::SourceScopeLocation::root(target);
        ProgramValidationError::OwnedSource {
            diagnostic: Box::new(Diagnostic::Capability {
                location: Location::Iteration {
                    scope: scope.clone(),
                },
                request: None,
                reason: Reason::DynamicLoader,
                origin: Origin::Scope { scope },
            }),
        }
    }
    let mut base = basic(
        vec![
            expression(1, field(SourceOwner::Primary, &["Value"], None)),
            expression(
                2,
                Expression::SourceField {
                    path: path(&["D", "Value"]),
                    frame: None,
                },
            ),
            expression(
                10,
                Expression::Const {
                    value: Value::String("d.json".into()),
                },
            ),
        ],
        group(vec![binding("Own", 1, ScalarType::String)]),
    );
    base.source = SchemaNode::group(
        "Primary",
        vec![
            SchemaNode::scalar("Value", ScalarType::String),
            SchemaNode::group(
                "Rows",
                vec![SchemaNode::scalar("Value", ScalarType::String)],
            )
            .repeating(),
            SchemaNode::group("D", vec![SchemaNode::scalar("Value", ScalarType::String)]),
            SchemaNode::group("d", vec![SchemaNode::scalar("Value", ScalarType::String)]),
        ],
    );
    base.extra_sources.push(NamedSourceProgram {
        name: "D".into(),
        source: source_schema("Dynamic"),
        dynamic: Some(codegen::DynamicSourceProgram {
            path: 10,
            driver: codegen::SourceIteration::new(path(&["Rows"])),
        }),
    });
    let mut cases: Vec<(&str, Program, Option<ProgramValidationError>)> = Vec::new();
    let mut unused = base.clone();
    unused.expressions.remove(1);
    cases.push(("L518-01 unused metadata", unused, None));
    cases.push(("L518-02 dead valid legacy leaf", base.clone(), None));
    let mut path_demand = base.clone();
    path_demand
        .root
        .bindings
        .push(binding("Dynamic", 2, ScalarType::String));
    path_demand.target = target_schema(&path_demand.root, "Output");
    cases.push((
        "L518-03 retained path",
        path_demand.clone(),
        Some(leaf_expected()),
    ));
    let mut frame_demand = path_demand.clone();
    frame_demand.expressions[1].expression = Expression::SourceField {
        path: path(&["Value"]),
        frame: Some(path(&["D", "Rows"])),
    };
    cases.push((
        "L518-04 retained frame",
        frame_demand,
        Some(leaf_expected()),
    ));
    let mut empty_frame = path_demand.clone();
    empty_frame.expressions[1].expression = Expression::SourceField {
        path: path(&["D", "Value"]),
        frame: Some(Vec::new()),
    };
    cases.push((
        "L518-05 present empty frame replaces path",
        empty_frame,
        None,
    ));
    let mut exact_case = path_demand.clone();
    exact_case.expressions[1].expression = Expression::SourceField {
        path: path(&["d", "Value"]),
        frame: None,
    };
    cases.push(("L518-06 exact case-sensitive match", exact_case, None));
    let mut primary_iteration = base.clone();
    primary_iteration.root =
        plain_iteration(&["D", "Rows"], vec![binding("Own", 1, ScalarType::String)]);
    primary_iteration.target = target_schema(&primary_iteration.root, "Output");
    cases.push((
        "L518-07 primary iteration",
        primary_iteration,
        Some(scope_expected(::mapping::SourceScopeTarget::Primary)),
    ));
    let mut named_iteration = base.clone();
    add_target(
        &mut named_iteration,
        "Iterate",
        plain_iteration(&["D", "Rows"], vec![binding("Own", 1, ScalarType::String)]),
    );
    cases.push((
        "L518-08 named iteration",
        named_iteration,
        Some(scope_expected(::mapping::SourceScopeTarget::Named {
            index: 0,
            name: "Iterate".into(),
        })),
    ));
    let mut lazy = base.clone();
    lazy.expressions.extend([
        expression(
            3,
            Expression::Const {
                value: Value::Bool(true),
            },
        ),
        expression(
            4,
            Expression::If {
                condition: 3,
                then: 1,
                else_: 2,
            },
        ),
    ]);
    lazy.root = group(vec![binding("Value", 4, ScalarType::String)]);
    lazy.target = target_schema(&lazy.root, "Output");
    cases.push((
        "L518-09 potential unchosen dependency",
        lazy,
        Some(leaf_expected()),
    ));
    let mut static_source = path_demand;
    static_source
        .extra_sources
        .last_mut()
        .expect("literal D declaration")
        .dynamic = None;
    cases.push(("L518-10 static same-name metadata", static_source, None));
    for (id, program, expected) in cases {
        eprintln!("INPUT {id} complete Program={program:#?}");
        let actual = render_basic_owned_mapping(&program);
        eprintln!("RAW {id} complete Result={actual:#?}");
        if let Err(error) = &actual {
            eprintln!(
                "RAW {id} complete source cause={:?}",
                error.source().map(|source| format!("{source:#?}"))
            );
        }
        eprintln!("EXPECTED {id} independent diagnostic={expected:#?}");
        match expected {
            Some(expected) => match actual {
                Err(EmitError::ProgramValidation(actual)) => assert_eq!(actual, expected, "{id}"),
                other => panic!("{id}: captured full refusal differs: {other:#?}"),
            },
            None => assert!(actual.is_ok(), "{id}: complete result captured above"),
        }
    }
}

fn checked_program() -> Program {
    let mut p = basic(
        vec![
            expression(1, field(SourceOwner::Primary, &["Value"], None)),
            expression(
                2,
                Expression::SourceField {
                    path: path(&["Ref", "Value"]),
                    frame: None,
                },
            ),
            expression(3, field(named("ref"), &["Value"], None)),
            expression(
                99,
                Expression::Const {
                    value: Value::String("unused.json".into()),
                },
            ),
        ],
        group(vec![binding("Value", 1, ScalarType::String)]),
    );
    p.extra_sources[0].dynamic = Some(codegen::DynamicSourceProgram {
        path: 99,
        driver: codegen::SourceIteration::new(Vec::new()),
    });
    add_target(
        &mut p,
        "legacy",
        group(vec![binding("Value", 2, ScalarType::String)]),
    );
    add_target(
        &mut p,
        "owned",
        group(vec![binding("Value", 3, ScalarType::String)]),
    );
    p
}
fn checked_rows<'a>(
    catalogue: &codegen::NamedInputCatalogue<'a>,
) -> Vec<(usize, &'a str, codegen::NamedInputAvailability)> {
    catalogue
        .entries
        .iter()
        .map(|entry| (entry.declaration_index, entry.name, entry.availability))
        .collect()
}
fn checked_result(label: &str, program: &Program) -> Result<String, EmitError> {
    let actual = crate::render_checked_private_owned_basic_mapping(program);
    eprintln!(
        "{label} complete typed Program={program:#?} complete checked source Result={actual:#?}"
    );
    actual
}
fn complete_checked_host(program: &Program, body: &str) -> String {
    let named=program.extra_targets.iter().enumerate().map(|(index,target)|format!("new {{ index={index},name={},entries=CheckedCatalogueSnapshot(CheckedNamedInputs_{index}) }}",crate::literal::string(&target.name))).collect::<Vec<_>>().join(",");
    let extension = CHECKED_HOST_EXTENSION.replace(
        "__NAMED_CATALOGUES__",
        &format!("new object[] {{ {named} }}"),
    );
    let mut host = HOST_PREFIX.to_string();
    host.push_str(HOST_ERROR_SNAPSHOT);
    host.push_str(&extension);
    host.push_str("\n    public static void Main()\n    {\n");
    host.push_str(body);
    host.push_str(HOST_SUFFIX);
    host
}
fn retain_checked_host(id: &str, program: &Program, body: &str) -> String {
    let source = checked_result(id, program).expect("full checked source result captured");
    let host = complete_checked_host(program, body);
    eprintln!("{id} complete prospective checked host={host}");
    if let Some(directory) = std::env::var_os("FERRULE_PRIVATE_CHECKED_CSHARP_HOST_DIR") {
        let directory = std::path::PathBuf::from(directory).join(id);
        std::fs::create_dir_all(&directory).expect("root-supplied fresh checked host directory");
        std::fs::write(directory.join("GeneratedMapping.cs"), &source)
            .expect("retain checked mapping");
        std::fs::write(
            directory.join("GeneratedTargetBuilder.cs"),
            crate::runtime::TARGET_BUILDER,
        )
        .expect("unchanged target helper");
        std::fs::write(directory.join("Program.cs"), host).expect("full host retained");
        std::fs::write(directory.join("Program.ir.txt"), format!("{program:#?}\n"))
            .expect("full typed Program retained");
    }
    source
}

#[test]
fn checked_catalogues_source_and_prepared_entries_are_separate_and_complete() {
    use codegen::NamedInputAvailability::{
        Optional as O, RequiredLegacy as R, ReservedUnavailable as U,
    };
    let p = checked_program();
    let plan = codegen::legacy_named_input_admission_plan(&p);
    eprintln!("S01 complete Program={p:#?} complete plan={plan:#?}");
    let plan = plan.expect("raw plan captured");
    let all = checked_rows(&plan.all_outputs);
    let primary = checked_rows(&plan.primary);
    let named = plan
        .named_targets
        .iter()
        .map(|target| {
            (
                target.target_index,
                target.name,
                checked_rows(&target.catalogue),
            )
        })
        .collect::<Vec<_>>();
    eprintln!("S01 all={all:#?} primary={primary:#?} named={named:#?}");
    let optional = vec![
        (0, "Unused", U),
        (1, "Ref", O),
        (2, " Ref 雪 ", O),
        (3, "ref", O),
        (4, "primary", O),
    ];
    let required = vec![
        (0, "Unused", U),
        (1, "Ref", R),
        (2, " Ref 雪 ", O),
        (3, "ref", O),
        (4, "primary", O),
    ];
    assert_eq!(all, required);
    assert_eq!(primary, optional);
    assert_eq!(named, [(0, "legacy", required), (1, "owned", optional)]);
    let old = render_basic_owned_mapping(&p);
    eprintln!("S02 prepared full source={old:#?}");
    let old = old.expect("prepared captured");
    let checked = checked_result("S02_S03", &p).expect("checked captured");
    assert!(checked.starts_with(&old));
    let tail = &checked[old.len()..];
    assert!(tail.contains("new(\" Ref \\u96EA \", global::Ferrule.Runtime.FerruleBasicNamedSourceMode.Optional), // declaration 2"));
    assert!(tail.contains("private readonly struct CheckedNamedInputAccessor : global::Ferrule.Runtime.IFerruleNamedSourceInputAccessor<NamedInput>"));
    assert!(tail.contains("return item is null ? default : new(item.Name, item.Instance);"));
    assert!(tail.contains("source, catalogue, accessor.Supplied, accessor, limits"));
    assert!(!tail.contains(".ToArray("));
    assert!(!tail.contains(".ToList("));
    assert!(!tail.contains("new global::Ferrule.Runtime.FerruleOwnedSourceLimits"));
    let selected = tail
        .find("private static SelectedTargetOutput ExecuteSelectedCheckedOwnedBasic")
        .unwrap();
    let selected = &tail[selected..];
    assert!(
        selected.find("ResolveOwnedBasicTarget(selection)").unwrap()
            < selected.find("ObserveCheckedInputBoundary(true)").unwrap()
    );
    assert_eq!(
        tail.matches("ExecuteCheckedBasicContext(context,").count(),
        2
    );
}

#[test]
fn checked_selected_catalogues_host_keeps_unrelated_legacy_requirements_lazy() {
    retain_checked_host(
        "C538-selection",
        &checked_program(),
        r#"
var supplied=new[]{new NamedInput("ref",Declared[3].Instance!),new NamedInput("Ref",Declared[1].Instance!)};
var p=new CheckedProbe(supplied);var a=CheckedReach("H01",p,()=>ExecuteCheckedOwnedBasic(Input,p,Limits));ExpectTree(a.Value,"{\"Value\":\"P.root\"}");var all=(ExecutionOutputs)a.Value.Result!;Check(all.Extras.Count==2,"two exact target outputs");Check(all.Extras[0].Name=="legacy" && Json(Plain(all.Extras[0].Instance))=="{\"Value\":\"P.collision\"}" && all.Extras[1].Name=="owned" && Json(Plain(all.Extras[1].Instance))=="{\"Value\":\"N.lower\"}","all targets/order");ExpectReads(a,1,2,1,2);
p=new CheckedProbe(Array.Empty<NamedInput>());var b=CheckedReach("H02",p,()=>ExecuteSelectedCheckedOwnedBasic(Input,FerruleTargetSelection.Primary(),p,Limits),FerruleTargetSelection.Primary());ExpectTree(b.Value,"{\"Value\":\"P.root\"}");ExpectReads(b,1,0,1,0);
p=new CheckedProbe(Array.Empty<NamedInput>());var c=CheckedReach("H03 absent",p,()=>ExecuteSelectedCheckedOwnedBasic(Input,FerruleTargetSelection.Named("legacy"),p,Limits),FerruleTargetSelection.Named("legacy"));ExpectInputError(c.Value,FerruleRuntimeError.MissingNamedSource,"Ref","named source is required by this mapping");ExpectReads(c,1,0,1,0);
p=new CheckedProbe(new[]{supplied[1]});var d=CheckedReach("H03 present",p,()=>ExecuteSelectedCheckedOwnedBasic(Input,FerruleTargetSelection.Named("legacy"),p,Limits),FerruleTargetSelection.Named("legacy"));ExpectTree(d.Value,"{\"Value\":\"P.collision\"}");ExpectReads(d,1,1,1,1);
p=new CheckedProbe(new[]{supplied[0]});var e=CheckedReach("H04",p,()=>ExecuteSelectedCheckedOwnedBasic(Input,FerruleTargetSelection.Named("owned"),p,Limits),FerruleTargetSelection.Named("owned"));ExpectTree(e.Value,"{\"Value\":\"N.lower\"}");ExpectReads(e,1,1,1,1);
Limits=new(0,0,0,0,0);p=new CheckedProbe(new[]{new NamedInput("X",Input)});var f=CheckedReach("H05",p,()=>ExecuteSelectedCheckedOwnedBasic(Input,FerruleTargetSelection.Named("missing"),p,Limits),FerruleTargetSelection.Named("missing"));ExpectInputError(f.Value,FerruleRuntimeError.UnknownTarget,"missing","project has no named target `missing`");ExpectReads(f,0,0,0,0);
Limits=new(16,8,16,1048576,64);p=new CheckedProbe(Array.Empty<NamedInput>());var g=CheckedReach("H08 absent",p,()=>ExecuteSelectedCheckedOwnedBasic(Input,FerruleTargetSelection.Primary(),p,Limits),FerruleTargetSelection.Primary());ExpectTree(g.Value,"{\"Value\":\"P.root\"}");ExpectReads(g,1,0,1,0);
p=new CheckedProbe(new[]{new NamedInput("Unused",Input)});var h=CheckedReach("H08 supplied",p,()=>ExecuteSelectedCheckedOwnedBasic(Input,FerruleTargetSelection.Primary(),p,Limits),FerruleTargetSelection.Primary());ExpectInputError(h.Value,FerruleRuntimeError.UnexpectedNamedSource,"Unused","named source is not declared by this mapping");ExpectReads(h,1,1,1,1);
"#,
    );
}

#[test]
fn checked_optional_and_required_input_hosts_keep_exact_error_classes() {
    let owned = basic(
        vec![expression(1, field(named("Ref"), &["Value"], None))],
        group(vec![binding("Value", 1, ScalarType::String)]),
    );
    retain_checked_host(
        "C538-optional",
        &owned,
        r#"var p=new CheckedProbe(Array.Empty<NamedInput>());var a=CheckedReach("H06",p,()=>ExecuteCheckedOwnedBasic(Input,p,Limits));ExpectOwned(a.Value,FerruleOwnedSourceErrorKind.SourceUnavailable,false,"Ref",null);ExpectReads(a,1,0,1,0);p=new CheckedProbe(new[]{new NamedInput("Ref",Declared[1].Instance!)});var b=CheckedReach("H07",p,()=>ExecuteCheckedOwnedBasic(Input,p,Limits));ExpectTree(b.Value,"{\"Value\":\"N.root\"}");ExpectReads(b,1,1,1,1);"#,
    );
    let legacy = basic(
        vec![expression(
            1,
            Expression::SourceField {
                path: path(&["Ref", "Value"]),
                frame: None,
            },
        )],
        group(vec![binding("Value", 1, ScalarType::String)]),
    );
    retain_checked_host(
        "C538-required",
        &legacy,
        r#"var p=new CheckedProbe(Array.Empty<NamedInput>());var a=CheckedReach("H03 required",p,()=>ExecuteCheckedOwnedBasic(Input,p,Limits));ExpectInputError(a.Value,FerruleRuntimeError.MissingNamedSource,"Ref","named source is required by this mapping");ExpectReads(a,1,0,1,0);p=new CheckedProbe(new[]{new NamedInput("Ref",Declared[1].Instance!)});var b=CheckedReach("H03 supplied",p,()=>ExecuteCheckedOwnedBasic(Input,p,Limits));ExpectTree(b.Value,"{\"Value\":\"P.collision\"}");ExpectReads(b,1,1,1,1);"#,
    );
}

#[test]
fn checked_supplied_records_host_captures_order_null_and_caller_failures() {
    let p = basic(
        vec![expression(1, field(SourceOwner::Primary, &["Value"], None))],
        group(vec![binding("Value", 1, ScalarType::String)]),
    );
    retain_checked_host(
        "C538-supplied",
        &p,
        r#"
var r=Declared[1].Instance!;var different=G(("Value",S("different")));
var scenarios=new[]{
("H09",new[]{new NamedInput("X",r),new NamedInput("Ref",r)},FerruleRuntimeError.UnexpectedNamedSource,"X","named source is not declared by this mapping",1),
("H10",new[]{new NamedInput("Ref",r),new NamedInput("Ref",r)},FerruleRuntimeError.DuplicateNamedSource,"Ref","named source was supplied more than once",2),
("H11",new[]{new NamedInput("Ref",r),new NamedInput("Ref",different)},FerruleRuntimeError.DuplicateNamedSource,"Ref","named source was supplied more than once",2),
("H12",new[]{new NamedInput("",r)},FerruleRuntimeError.UnexpectedNamedSource,"","named source is not declared by this mapping",1),
("H13 unknown",new[]{new NamedInput("X",r),new NamedInput("Ref",r),new NamedInput("Ref",r)},FerruleRuntimeError.UnexpectedNamedSource,"X","named source is not declared by this mapping",1),
("H13 duplicate",new[]{new NamedInput("Ref",r),new NamedInput("Ref",r),new NamedInput("X",r)},FerruleRuntimeError.DuplicateNamedSource,"Ref","named source was supplied more than once",2)};
foreach(var row in scenarios){var probe=new CheckedProbe(row.Item2);var actual=CheckedReach(row.Item1,probe,()=>ExecuteCheckedOwnedBasic(Input,probe,Limits));ExpectInputError(actual.Value,row.Item3,row.Item4,row.Item5);ExpectReads(actual,1,row.Item6,1,row.Item6);}
foreach(var row in new[]{("H26",new NamedInput[]{null!}),("H29",new[]{new NamedInput(null!,r)}),("H30",new[]{new NamedInput("Ref",null!)})}){var probe=new CheckedProbe(row.Item2);var actual=CheckedReach(row.Item1,probe,()=>ExecuteCheckedOwnedBasic(Input,probe,Limits));Check(actual.Value.Error is ArgumentNullException e && e.ParamName=="supplied" && e.InnerException is null,"exact null argument, no NullReferenceException");ExpectReads(actual,1,1,1,1);}
var invalid=new CheckedProbe(new[]{new NamedInput("\uD800",r)});var bad=CheckedReach("H31",invalid,()=>ExecuteCheckedOwnedBasic(Input,invalid,Limits));Check(bad.Value.Error is ArgumentException unicode && unicode.ParamName=="supplied" && unicode.InnerException is null && bad.Value.Error is not ArgumentNullException,"well-formed UTF16 refusal");ExpectReads(bad,1,1,1,1);
var count=new CheckedProbe(new[]{new NamedInput("Ref",r)},throwCount:true);var ca=CheckedReach("H27",count,()=>ExecuteCheckedOwnedBasic(Input,count,Limits));Check(ReferenceEquals(ca.Value.Error,count.Failure),"caller Count exception exact reference");ExpectReads(ca,1,0,1,0);
var index=new CheckedProbe(new[]{new NamedInput("Ref",r)},throwIndex:true);var ix=CheckedReach("H28",index,()=>ExecuteCheckedOwnedBasic(Input,index,Limits));Check(ReferenceEquals(ix.Value.Error,index.Failure),"caller item exception exact reference");ExpectReads(ix,1,0,1,1);
"#,
    );
}

#[test]
fn checked_exact_name_host_preserves_catalogue_order_and_unicode() {
    let p = basic(
        vec![
            expression(1, field(named(" Ref 雪 "), &["Value"], None)),
            expression(2, field(named("ref"), &["Value"], None)),
            expression(3, field(named("primary"), &["Value"], None)),
        ],
        group(vec![
            binding("Exact", 1, ScalarType::String),
            binding("Lower", 2, ScalarType::String),
            binding("LiteralPrimary", 3, ScalarType::String),
        ]),
    );
    retain_checked_host(
        "C538-exact",
        &p,
        r#"var p=new CheckedProbe(new[]{new NamedInput("primary",Declared[4].Instance!),new NamedInput("ref",Declared[3].Instance!),new NamedInput(" Ref 雪 ",Declared[2].Instance!)});var a=CheckedReach("H14",p,()=>ExecuteCheckedOwnedBasic(Input,p,Limits));ExpectTree(a.Value,"{\"Exact\":\"N.exact\",\"Lower\":\"N.lower\",\"LiteralPrimary\":\"N.literal.primary\"}");ExpectReads(a,1,3,1,3);Check(CheckedAllInputs.Select(d=>d.Name).SequenceEqual(new[]{"Unused","Ref"," Ref 雪 ","ref","primary"}),"complete original catalogue/order/ordinals");"#,
    );
}

#[test]
fn checked_five_limit_hosts_preserve_policy_and_typed_causes() {
    let p = basic(
        vec![expression(1, field(SourceOwner::Primary, &["Value"], None))],
        group(vec![binding("Value", 1, ScalarType::String)]),
    );
    retain_checked_host(
        "C538-limits",
        &p,
        r#"
var probe=new CheckedProbe(new[]{new NamedInput("Ref",Declared[1].Instance!)});Limits=new(6,8,16,1048576,64);var a=CheckedReach("H15",probe,()=>ExecuteCheckedOwnedBasic(Input,probe,Limits));ExpectTree(a.Value,"{\"Value\":\"P.root\"}");ExpectReads(a,1,1,1,1);
probe=new CheckedProbe(new[]{new NamedInput("Ref",Declared[1].Instance!)});Limits=new(5,8,16,1048576,64);var b=CheckedReach("H16",probe,()=>ExecuteCheckedOwnedBasic(Input,probe,Limits));ExpectExactLimit(b.Value,FerruleOwnedSourceLimitKind.Roots,0,6,5);ExpectReads(b,1,0,0,0);
probe=new CheckedProbe(new[]{new NamedInput("Ref",Declared[1].Instance!)});Limits=new(16,8,16,0,64);var c=CheckedReach("H17",probe,()=>ExecuteCheckedOwnedBasic(Input,probe,Limits));ExpectExactLimit(c.Value,FerruleOwnedSourceLimitKind.MetadataBytes,0,6,0);ExpectReads(c,1,0,0,0);
probe=new CheckedProbe(Array.Empty<NamedInput>());Limits=new(16,8,0,1048576,64);var d=CheckedReach("H18",probe,()=>ExecuteCheckedOwnedBasic(Input,probe,Limits));ExpectExactLimit(d.Value,FerruleOwnedSourceLimitKind.PathSegments,0,1,0);ExpectReads(d,1,0,1,0);
"#,
    );
    let mut walk = basic(
        vec![
            expression(1, field(SourceOwner::Primary, &["Value"], Some(&[]))),
            expression(2, position(SourceOwner::Primary, Some(&[]))),
        ],
        owned_iteration(
            SourceOwner::Primary,
            &[],
            vec![
                binding("Value", 1, ScalarType::String),
                binding("Position", 2, ScalarType::Int),
            ],
        ),
    );
    walk.extra_sources.clear();
    retain_checked_host(
        "C538-walk",
        &walk,
        r#"Input=R(G(("Value",S("flat1"))),G(("Value",S("flat2"))));var p=new CheckedProbe(Array.Empty<NamedInput>());Limits=new(1,1,1,4096,2);var a=CheckedReach("H20 inclusive",p,()=>ExecuteCheckedOwnedBasic(Input,p,Limits));ExpectTree(a.Value,"[{\"Value\":\"flat1\",\"Position\":1},{\"Value\":\"flat2\",\"Position\":2}]");ExpectReads(a,1,0,1,0);p=new CheckedProbe(Array.Empty<NamedInput>());Limits=new(1,0,1,4096,2);var b=CheckedReach("H19",p,()=>ExecuteCheckedOwnedBasic(Input,p,Limits));ExpectExactLimit(b.Value,FerruleOwnedSourceLimitKind.Frames,0,1,0);ExpectReads(b,1,0,1,0);p=new CheckedProbe(Array.Empty<NamedInput>());Limits=new(1,1,1,4096,1);var c=CheckedReach("H20 refused",p,()=>ExecuteCheckedOwnedBasic(Input,p,Limits));ExpectPairedWalkLimit(c.Value);ExpectReads(c,1,0,1,0);Check(b.Value.Result is null && c.Value.Result is null,"no partial output");"#,
    );
}

#[test]
fn checked_lazy_and_raise_hosts_keep_legacy_eager_admission_and_selected_targets() {
    for (name, flag, body) in [
        (
            "C538-lazy-true",
            true,
            r#"var p=new CheckedProbe(Array.Empty<NamedInput>());var a=CheckedReach("H21",p,()=>ExecuteCheckedOwnedBasic(Input,p,Limits));ExpectTree(a.Value,"{\"Value\":\"chosen\"}");ExpectReads(a,1,0,1,0);Check(!a.Value.Nodes.Contains(3u),"unchosen owned branch not reached");"#,
        ),
        (
            "C538-lazy-false",
            false,
            r#"var p=new CheckedProbe(Array.Empty<NamedInput>());var a=CheckedReach("H22",p,()=>ExecuteCheckedOwnedBasic(Input,p,Limits));ExpectOwned(a.Value,FerruleOwnedSourceErrorKind.SourceUnavailable,false,"Unused",null);ExpectReads(a,1,0,1,0);Check(a.Value.Nodes.Count(n=>n==3u)==1,"chosen owned branch reached once");"#,
        ),
    ] {
        let p = basic(
            vec![
                expression(
                    1,
                    Expression::Const {
                        value: Value::Bool(flag),
                    },
                ),
                expression(
                    2,
                    Expression::Const {
                        value: Value::String("chosen".into()),
                    },
                ),
                expression(3, field(named("Unused"), &["Value"], None)),
                expression(
                    4,
                    Expression::If {
                        condition: 1,
                        then: 2,
                        else_: 3,
                    },
                ),
            ],
            group(vec![binding("Value", 4, ScalarType::String)]),
        );
        retain_checked_host(name, &p, body);
    }
    let eager = basic(
        vec![
            expression(
                1,
                Expression::Const {
                    value: Value::Bool(true),
                },
            ),
            expression(
                2,
                Expression::Const {
                    value: Value::String("chosen".into()),
                },
            ),
            expression(
                3,
                Expression::SourceField {
                    path: path(&["Ref", "Value"]),
                    frame: None,
                },
            ),
            expression(
                4,
                Expression::If {
                    condition: 1,
                    then: 2,
                    else_: 3,
                },
            ),
        ],
        group(vec![binding("Value", 4, ScalarType::String)]),
    );
    retain_checked_host(
        "C538-potential-legacy",
        &eager,
        r#"var p=new CheckedProbe(Array.Empty<NamedInput>());var a=CheckedReach("H23",p,()=>ExecuteCheckedOwnedBasic(Input,p,Limits));ExpectInputError(a.Value,FerruleRuntimeError.MissingNamedSource,"Ref","named source is required by this mapping");ExpectReads(a,1,0,1,0);Check(a.Value.Nodes.Length==0,"admission before any expression");"#,
    );
    let mut raise = basic(
        vec![
            expression(1, field(SourceOwner::Primary, &["Value"], None)),
            expression(71, Expression::Raise { message: None }),
        ],
        group(vec![binding("Value", 1, ScalarType::String)]),
    );
    add_target(
        &mut raise,
        "fail",
        group(vec![binding("Value", 71, ScalarType::String)]),
    );
    retain_checked_host(
        "C538-raise",
        &raise,
        r#"var p=new CheckedProbe(Array.Empty<NamedInput>());var a=CheckedReach("H24 primary",p,()=>ExecuteSelectedCheckedOwnedBasic(Input,FerruleTargetSelection.Primary(),p,Limits),FerruleTargetSelection.Primary());ExpectTree(a.Value,"{\"Value\":\"P.root\"}");Check(!a.Value.Nodes.Contains(71u),"unselected ordinary Raise stays inert");ExpectReads(a,1,0,1,0);p=new CheckedProbe(Array.Empty<NamedInput>());var b=CheckedReach("H24 all",p,()=>ExecuteCheckedOwnedBasic(Input,p,Limits));var e=ExpectRuntime(b.Value,FerruleRuntimeError.MappingException);Check(e.Node==71 && e.MappingExceptionMessage is null && b.Value.Result is null,"exact ordinary Raise payload/no partial outcome");ExpectReads(b,1,0,1,0);p=new CheckedProbe(Array.Empty<NamedInput>());var c=CheckedReach("H24 named",p,()=>ExecuteSelectedCheckedOwnedBasic(Input,FerruleTargetSelection.Named("fail"),p,Limits),FerruleTargetSelection.Named("fail"));var f=ExpectRuntime(c.Value,FerruleRuntimeError.MappingException);Check(f.Node==71 && f.MappingExceptionMessage is null && c.Value.Nodes.SequenceEqual(new uint[]{71}),"selected ordinary Raise only");ExpectReads(c,1,0,1,0);"#,
    );
}

#[test]
fn checked_empty_catalogue_scalar_root_has_no_implicit_policy() {
    let mut p = basic(
        vec![expression(1, field(SourceOwner::Primary, &[], None))],
        group(vec![binding("Value", 1, ScalarType::String)]),
    );
    p.source = SchemaNode::scalar("Input", ScalarType::String);
    p.extra_sources.clear();
    retain_checked_host(
        "C538-zero",
        &p,
        r#"Input=S("P.root");Limits=new(1,0,0,0,0);var p=new CheckedProbe(Array.Empty<NamedInput>());var a=CheckedReach("H25",p,()=>ExecuteCheckedOwnedBasic(Input,p,Limits));ExpectTree(a.Value,"{\"Value\":\"P.root\"}");ExpectReads(a,1,0,1,0);"#,
    );
}

const CHECKED_HOST_EXTENSION: &str = r#"
    private static int CheckedFactoryCalls,CheckedAccessorReads;
    static partial void ObserveCheckedInputBoundary(bool factory) { if(factory)CheckedFactoryCalls++;else CheckedAccessorReads++; }
    private sealed class CheckedProbe : IReadOnlyList<NamedInput>
    {
        private readonly NamedInput[] _items;private readonly bool _throwCount,_throwIndex;
        internal CheckedProbe(NamedInput[] items,bool throwCount=false,bool throwIndex=false) { _items=items;_throwCount=throwCount;_throwIndex=throwIndex; }
        internal NamedInput[] CapturedItems=>_items;
        internal bool ThrowsCount=>_throwCount;internal bool ThrowsIndex=>_throwIndex;
        internal readonly Exception Failure=new InvalidOperationException("checked caller sentinel");
        internal int CountReads,IndexReads;
        public int Count {get{CountReads++;if(_throwCount)throw Failure;return _items.Length;}}
        public NamedInput this[int index]{get{IndexReads++;if(_throwIndex)throw Failure;return _items[index];}}
        public IEnumerator<NamedInput> GetEnumerator()=>throw new InvalidOperationException("input enumeration forbidden");
        System.Collections.IEnumerator System.Collections.IEnumerable.GetEnumerator()=>GetEnumerator();
    }
    private sealed record CheckedReached(Reached Value,int Factory,int Accessor,int Count,int Index);
    private static object CheckedCatalogueSnapshot(IReadOnlyList<FerruleBasicNamedSourceDeclaration> catalogue)=>catalogue.Select((d,index)=>new {index,d.Name,units=d.Name.Select(c=>(int)c).ToArray(),mode=d.Mode.ToString()}).ToArray();
    private static CheckedReached CheckedReach(string id,CheckedProbe probe,Func<object?> subject,FerruleTargetSelection? selection=null)
    {
        NodeTrace.Clear();FrameTrace.Clear();CheckedFactoryCalls=0;CheckedAccessorReads=0;probe.CountReads=0;probe.IndexReads=0;
        var input=Json(new {classification="complete_checked_typed_input",source=InstanceSnapshot(Input),supplied=probe.CapturedItems.Select(item=>item is null?null:new {item.Name,units=item.Name?.Select(c=>(int)c).ToArray(),instance=InstanceSnapshot(item.Instance)}).ToArray(),
            selection=selection is null?null:new {selection.IsPrimary,selection.Name},limits=new {roots=(ulong)Limits.MaxRoots,frames=(ulong)Limits.MaxActiveFrames,segments=(ulong)Limits.MaxPathSegments,bytes=(ulong)Limits.MaxMetadataBytes,contexts=(ulong)Limits.MaxWalkContexts},
            execution=Execution is null?null:new {Execution.MappingFilePath,Execution.MainMappingFilePath,Execution.CurrentDateTime,parameterCount=Execution.RuntimeParameters.Count,parameters=ParameterValues.Select(p=>new {p.Key,value=ValueSnapshot(p.Value)}).ToArray()},
            catalogueAll=CheckedCatalogueSnapshot(CheckedAllInputs),cataloguePrimary=CheckedCatalogueSnapshot(CheckedPrimaryInputs),catalogueNamed=__NAMED_CATALOGUES__,probeCountThrows=probe.ThrowsCount,probeIndexThrows=probe.ThrowsIndex });
        Console.WriteLine(Json(new {id,classification="before_subject",completeInput=input}));object? result=null;Exception? error=null;
        try {result=subject();}catch(Exception actual){error=actual;}
        var nodes=NodeTrace.ToArray();var frames=FrameTrace.ToArray();var factory=CheckedFactoryCalls;var accessor=CheckedAccessorReads;var count=probe.CountReads;var index=probe.IndexReads;
        var complete=Json(new {classification="complete_checked_actual",result=ResultSnapshot(result),error=error is null?null:ErrorSnapshot(error),nodes,frames,factoryCalls=factory,accessorReads=accessor,countReads=count,indexReads=index});
        Console.WriteLine(Json(new {id,classification="before_comparisons",completeInput=input,completeActual=complete}));return new(new(result,error,nodes,frames,input,complete),factory,accessor,count,index);
    }
    private static void ExpectReads(CheckedReached actual,int factory,int accessor,int count,int index)
    { Check(actual.Factory==factory && actual.Accessor==accessor && actual.Count==count && actual.Index==index,"exact factory/accessor/list reached calls, not allocations"); }
    private static void ExpectInputError(Reached actual,FerruleRuntimeError category,string detail,string message)
    {
        var e=ExpectRuntime(actual,category);Check(e.Detail==detail && e.Message==message && e.InnerException is null,"exact category/detail/message/chain");
        Check(e.Node is null && e.Function is null && e.ExpectedArity is null && e.ActualArity is null && e.FoundKind is null && e.AggregateOperation is null && e.RequestedItems is null && e.MaximumItems is null && e.MaximumDepth is null && e.RuntimeValue is null && e.FailureRule is null && e.MappingFailureMessage is null && e.MappingExceptionMessage is null && e.Join is null && e.UserFunction is null && e.FunctionParameter is null && e.ExpectedScalarType is null && e.RuntimeParameter is null && e.SourceField is null && e.FoundInstance is null && e.PrimaryRoot is null && e.OwnedSourceFailure is null && e.BasicSourceBridgeFailure is null && actual.Result is null,"full untouched typed envelope/no partial output");
    }
    private static void ExpectPairedWalkLimit(Reached actual)
    {
        var outer=ExpectRuntime(actual,FerruleRuntimeError.BasicSourceBridge);var cause=outer.BasicSourceBridgeFailure;
        Check(cause is not null && ReferenceEquals(cause,outer.InnerException),"exact bare paired inner cause");var error=cause!;
        Check(error.Error==FerruleBasicSourceBridgeErrorKind.PairedMetadataLimit && error.Component==FerruleBasicSourceBridgeComponent.WalkContexts && error.Used==0 && error.Requested==2 && error.Maximum==1,"full paired0/2/1 WalkContexts payload");
        Check(error.InputOrdinal is null && error.CandidateOrdinal is null && error.DeclarationIndex is null && error.DeclaredCount is null && error.Found is null && error.InnerException is null,"unchanged absent paired witness fields");
        Check(outer.Node is null && outer.Function is null && outer.ExpectedArity is null && outer.ActualArity is null && outer.FoundKind is null && outer.AggregateOperation is null && outer.Detail is null && outer.RequestedItems is null && outer.MaximumItems is null && outer.MaximumDepth is null && outer.RuntimeValue is null && outer.FailureRule is null && outer.MappingFailureMessage is null && outer.MappingExceptionMessage is null && outer.Join is null && outer.UserFunction is null && outer.FunctionParameter is null && outer.ExpectedScalarType is null && outer.RuntimeParameter is null && outer.SourceField is null && outer.FoundInstance is null && outer.PrimaryRoot is null && outer.OwnedSourceFailure is null,"complete untouched outer paired envelope");
        Check(actual.Result is null && actual.Nodes.Length==0,"paired refusal before any expression or partial outcome");
    }
    private static void ExpectExactLimit(Reached actual,FerruleOwnedSourceLimitKind kind,nuint used,nuint requested,nuint maximum)
    {var error=ExpectOwned(actual,FerruleOwnedSourceErrorKind.MetadataLimit,null,null,null);Check(error.LimitKind==kind && error.Used==used && error.Requested==requested && error.Maximum==maximum,"complete exact limit payload");}
"#;

#[test]
fn checked_inventory_preserves_full_dynamic_refusal_and_present_empty_frame() {
    let mut p = checked_program();
    p.expressions[0].expression = Expression::SourceField {
        path: path(&["Unused", "Value"]),
        frame: None,
    };
    let actual = checked_result("S04 retained dynamic path", &p);
    let expected = ProgramValidationError::OwnedSource {
        diagnostic: Box::new(codegen::OwnedSourceDiagnostic::Capability {
            location: codegen::OwnedSourceDiagnosticLocation::Expression {
                namespace: codegen::OwnedSourceNamespace::Main,
                node: 1,
                scope: None,
            },
            request: None,
            reason: codegen::OwnedSourceCapabilityReason::UnsupportedBasicContext,
            origin: codegen::OwnedSourceCapabilityOrigin::RetainedExpression {
                namespace: codegen::OwnedSourceNamespace::Main,
                node: 1,
            },
        }),
    };
    match actual {
        Err(EmitError::ProgramValidation(error)) => assert_eq!(error, expected),
        other => panic!("complete raw refusal captured: {other:#?}"),
    }
    p.expressions[0].expression = Expression::SourceField {
        path: path(&["Unused", "Value"]),
        frame: Some(Vec::new()),
    };
    let actual = checked_result("S04 present empty frame", &p);
    assert!(actual.is_ok());
    let source = actual.expect("full source captured");
    let prepared = render_basic_owned_mapping(&p);
    eprintln!("S04 prepared present-empty source={prepared:#?}");
    assert!(source.starts_with(&prepared.expect("prepared result captured")));
}
