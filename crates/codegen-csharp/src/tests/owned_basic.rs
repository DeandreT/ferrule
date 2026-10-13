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
            // Retain the direct malformed-position probe through a target
            // dependency while the plain projection selects its legacy value.
            expression(
                4,
                Expression::Const {
                    value: Value::Bool(false),
                },
            ),
            expression(
                5,
                Expression::If {
                    condition: 4,
                    then: 3,
                    else_: 2,
                },
            ),
        ],
        plain_iteration(
            &["Details"],
            vec![
                binding("Counter", 1, ScalarType::Int),
                binding("LegacyPosition", 5, ScalarType::Int),
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

// #542 adds separate prospective controls. The complete preceding #518 test
// module and every frozen source/host recipe above remain byte-for-byte exact.
fn source_failure(
    parts: &[&str],
    selection: codegen::FailureSelection,
    message: Option<u32>,
) -> codegen::FailureRule {
    codegen::FailureRule {
        iteration: codegen::FailureIteration::Source(codegen::SourceIteration::new(path(parts))),
        selection,
        message,
    }
}
fn failure_program(mut nodes: Vec<ExpressionNode>) -> Program {
    nodes.insert(
        0,
        expression(1, field(SourceOwner::Primary, &["Value"], None)),
    );
    basic(nodes, group(vec![binding("Value", 1, ScalarType::String)]))
}
fn failure_legacy(id: u32, parts: &[&str], frame: Option<&[&str]>) -> ExpressionNode {
    expression(
        id,
        Expression::SourceField {
            path: path(parts),
            frame: frame.map(path),
        },
    )
}
fn failure_scores(selection: codegen::FailureSelection) -> Program {
    let mut program = failure_program(vec![
        expression(
            100,
            Expression::Call {
                function: ScalarFunction::LessThan,
                args: vec![101, 102],
            },
        ),
        failure_legacy(101, &["Score"], Some(&["Rows"])),
        expression(
            102,
            Expression::Const {
                value: Value::Int(20),
            },
        ),
        failure_legacy(103, &["Value"], Some(&["Rows"])),
    ]);
    program
        .failure_rules
        .push(source_failure(&["Rows"], selection, Some(103)));
    program
}
fn retain_failure_host(id: &str, program: &Program, body: &str) -> String {
    let actual = render_basic_owned_mapping(program);
    eprintln!("INPUT {id} complete Program={program:#?} complete bridge host subject={body}");
    eprintln!("RAW {id} complete private source Result={actual:#?}");
    if let Err(error) = &actual {
        use std::error::Error as _;
        eprintln!(
            "RAW {id} complete typed cause={:?}",
            error.source().map(|value| format!("{value:#?}"))
        );
    }
    let source = actual.expect("complete bridge source result retained before comparison");
    let mut host = HOST_PREFIX.to_string();
    host.push_str(HOST_ERROR_SNAPSHOT);
    host.push_str(FAILURE_HOST_HELPERS);
    host.push_str("\n    public static void Main()\n    {\n");
    host.push_str(body);
    host.push_str(HOST_SUFFIX);
    eprintln!("RAW {id} complete prospective typed bridge host={host}");
    if let Some(directory) = std::env::var_os("FERRULE_PRIVATE_OWNED_CSHARP_HOST_DIR") {
        let directory = std::path::PathBuf::from(directory).join(id);
        std::fs::create_dir_all(&directory).expect("protected root supplied private host output");
        std::fs::write(directory.join("GeneratedMapping.cs"), &source)
            .expect("retain bridge mapping");
        std::fs::write(
            directory.join("GeneratedTargetBuilder.cs"),
            crate::runtime::TARGET_BUILDER,
        )
        .expect("retain exact existing helper");
        std::fs::write(directory.join("Program.cs"), host)
            .expect("retain complete prospective host");
        std::fs::write(directory.join("Program.ir.txt"), format!("{program:#?}\n"))
            .expect("retain complete Program");
    }
    source
}

#[test]
fn private_source_failure_scalar_order_and_lazy_message_bridge_hosts() {
    use codegen::FailureSelection::{All, WhenTrue};
    let mut scalar = failure_program(vec![expression(
        101,
        Expression::Const {
            value: Value::String(String::new()),
        },
    )]);
    scalar.expressions[0].expression = field(SourceOwner::Primary, &[], None);
    scalar.source = SchemaNode::scalar("Primary", ScalarType::String);
    scalar.extra_sources.clear();
    scalar.failure_rules.push(source_failure(&[], All, None));
    let source = retain_failure_host(
        "F542-01",
        &scalar,
        r#"Input=S("scalar");Declared=Array.Empty<FerruleDeclaredNamedSourceRoot>();var a=BridgeReach("F542-01",()=>ExecuteOwnedBasic(Input,Declared,Limits));ExpectFailure(a,1,null);Check(a.Actual.Nodes.Length==0,"All/no-message before target");ExpectCohort(a,0,1,Array.Empty<string>(),new long[]{1});"#,
    );
    assert!(source.contains("context.WalkSourceCohort(path_failure_0)"));
    assert!(!source.contains("context.IterateSource("));
    assert!(source.contains("finally { global::System.GC.KeepAlive(cohort_failure_0); }"));
    scalar.failure_rules[0].message = Some(101);
    retain_failure_host(
        "F542-02",
        &scalar,
        r#"Input=S("scalar");Declared=Array.Empty<FerruleDeclaredNamedSourceRoot>();var a=BridgeReach("F542-02",()=>ExecuteOwnedBasic(Input,Declared,Limits));ExpectFailure(a,1,"");Check(a.Actual.Nodes.SequenceEqual(new uint[]{101}),"empty text message is present and evaluated once");"#,
    );
    let mut ordered = failure_program(vec![
        expression(
            100,
            Expression::Const {
                value: Value::Bool(false),
            },
        ),
        expression(101, Expression::Raise { message: None }),
        expression(
            102,
            Expression::Const {
                value: Value::String("later".into()),
            },
        ),
    ]);
    ordered.failure_rules = vec![
        source_failure(&[], WhenTrue(100), Some(101)),
        source_failure(&[], All, None),
        source_failure(&[], All, Some(102)),
    ];
    retain_failure_host(
        "F542-03",
        &ordered,
        r#"var a=BridgeReach("F542-03",()=>ExecuteOwnedBasic(Input,Declared,Limits));ExpectFailure(a,2,null);Check(a.Actual.Nodes.SequenceEqual(new uint[]{100}),"ordered rules/first selected rule/lazy message/no target");Check(a.Cohorts.Length==2,"third rule not walked");"#,
    );
    let mut first = failure_scores(All);
    retain_failure_host(
        "F542-04",
        &first,
        r#"var a=BridgeReach("F542-04",()=>ExecuteOwnedBasic(Input,Declared,Limits));ExpectFailure(a,1,"P1");ExpectCohort(a,0,3,new[]{"Rows"},new long[]{1,2,3});Check(a.Actual.Nodes.SequenceEqual(new uint[]{103}),"first candidate only, all cohort rows retained first");"#,
    );
    first.failure_rules[0].selection = WhenTrue(100);
    first
        .expressions
        .iter_mut()
        .find(|node| node.id == 100)
        .unwrap()
        .expression = Expression::Const {
        value: Value::Bool(false),
    };
    first.failure_rules[0].message = Some(101);
    first
        .expressions
        .iter_mut()
        .find(|node| node.id == 101)
        .unwrap()
        .expression = Expression::Raise { message: None };
    retain_failure_host(
        "F542-07",
        &first,
        r#"var a=BridgeReach("F542-07",()=>ExecuteOwnedBasic(Input,Declared,Limits));ExpectTree(a.Actual,"{\"Value\":\"P.root\"}");ExpectCohort(a,0,3,new[]{"Rows"},new long[]{1,2,3});Check(a.Actual.Nodes.SequenceEqual(new uint[]{100,100,100,1}),"all false skips Raise message and reaches target only after rules");"#,
    );
}

#[test]
fn private_source_failure_true_false_and_boolean_error_bridge_hosts() {
    let program = failure_scores(codegen::FailureSelection::WhenTrue(100));
    retain_failure_host(
        "F542-05",
        &program,
        r#"var a=BridgeReach("F542-05",()=>ExecuteOwnedBasic(Input,Declared,Limits));ExpectFailure(a,1,"P2");ExpectCohort(a,0,3,new[]{"Rows"},new long[]{1,2,3});Check(a.Actual.Nodes.SequenceEqual(new uint[]{100,101,102,100,101,102,103}),"WhenTrue scans raw rows1,2 then lazy selected message");"#,
    );
    let program = failure_scores(codegen::FailureSelection::WhenFalse(100));
    retain_failure_host(
        "F542-06",
        &program,
        r#"var a=BridgeReach("F542-06",()=>ExecuteOwnedBasic(Input,Declared,Limits));ExpectFailure(a,1,"P1");Check(a.Actual.Nodes.SequenceEqual(new uint[]{100,101,102,103}),"WhenFalse first raw candidate, no remaining predicate/message");"#,
    );
    let mut wrong = failure_scores(codegen::FailureSelection::WhenTrue(100));
    wrong
        .expressions
        .iter_mut()
        .find(|node| node.id == 100)
        .unwrap()
        .expression = Expression::Const {
        value: Value::Int(7),
    };
    retain_failure_host(
        "F542-23",
        &wrong,
        r#"var a=BridgeReach("F542-23",()=>ExecuteOwnedBasic(Input,Declared,Limits));var e=ExpectRuntime(a.Actual,FerruleRuntimeError.NotABool);Check(e.Node==100 && e.FoundKind==FerruleValueKind.Int64 && e.FailureRule is null && e.MappingFailureMessage is null && e.InnerException is null && a.Actual.Result is null,"predicate7 retains NotABool payload before message/target");Check(a.Actual.Nodes.SequenceEqual(new uint[]{100}),"no message or target after invalid predicate");"#,
    );
}

#[test]
fn private_source_failure_selected_routes_and_legacy_named_shadow_bridge_hosts() {
    let mut selected = failure_program(vec![]);
    selected
        .failure_rules
        .push(source_failure(&[], codegen::FailureSelection::All, None));
    add_target(
        &mut selected,
        "named",
        group(vec![binding("Value", 1, ScalarType::String)]),
    );
    retain_failure_host(
        "F542-08",
        &selected,
        r#"var primarySelection=FerruleTargetSelection.Primary();var a=BridgeReach("F542-08.primary",()=>ExecuteSelectedOwnedBasic(Input,primarySelection,Declared,Limits),primarySelection);var namedSelection=FerruleTargetSelection.Named("named");var b=BridgeReach("F542-08.named",()=>ExecuteSelectedOwnedBasic(Input,namedSelection,Declared,Limits),namedSelection);var c=BridgeReach("F542-08.all",()=>ExecuteOwnedBasic(Input,Declared,Limits));Limits=new(0,0,0,0,0);var missingSelection=FerruleTargetSelection.Named("missing");var d=BridgeReach("F542-08.unknown",()=>ExecuteSelectedOwnedBasic(Input,missingSelection,Declared,Limits),missingSelection);ExpectFailure(a,1,null);ExpectFailure(b,1,null);ExpectFailure(c,1,null);var e=ExpectRuntime(d.Actual,FerruleRuntimeError.UnknownTarget);Check(e.Detail=="missing" && d.Cohorts.Length==0 && d.Actual.Nodes.Length==0,"unknown selection keeps prior priority over factory/failure");Check(a.Actual.Nodes.Length==0 && b.Actual.Nodes.Length==0 && c.Actual.Nodes.Length==0,"global failures before every selected/all target");"#,
    );
    let mut shadow = failure_program(vec![]);
    shadow.failure_rules.push(source_failure(
        &["Ref", "Rows"],
        codegen::FailureSelection::All,
        None,
    ));
    retain_failure_host(
        "F542-09",
        &shadow,
        r#"var a=BridgeReach("F542-09",()=>ExecuteOwnedBasic(Input,Declared,Limits));ExpectTree(a.Actual,"{\"Value\":\"P.root\"}");ExpectCohort(a,0,0,new[]{"Ref","Rows"},Array.Empty<long>());Check(a.Actual.Nodes.SequenceEqual(new uint[]{1}),"primary Ref shadows named Ref without rescue");"#,
    );
    let mut named_rule = failure_scores(codegen::FailureSelection::WhenTrue(100));
    named_rule.failure_rules[0].iteration =
        codegen::FailureIteration::Source(codegen::SourceIteration::new(path(&["ref", "Rows"])));
    named_rule
        .expressions
        .iter_mut()
        .find(|node| node.id == 101)
        .unwrap()
        .expression = Expression::SourceField {
        path: path(&["Score"]),
        frame: Some(path(&["ref", "Rows"])),
    };
    named_rule
        .expressions
        .iter_mut()
        .find(|node| node.id == 103)
        .unwrap()
        .expression = Expression::SourceField {
        path: path(&["Value"]),
        frame: Some(path(&["ref", "Rows"])),
    };
    retain_failure_host(
        "F542-10",
        &named_rule,
        r#"Declared[3]=new FerruleDeclaredNamedSourceRoot("ref",G(("Rows",R(G(("Value",S("N1")),("Score",I(30))),G(("Value",S("N2")),("Score",I(10)))))));var a=BridgeReach("F542-10",()=>ExecuteOwnedBasic(Input,Declared,Limits));ExpectFailure(a,1,"N2");ExpectCohort(a,0,2,new[]{"ref","Rows"},new long[]{1,2});Check(a.Actual.Nodes.SequenceEqual(new uint[]{100,101,102,100,101,102,103}),"exact declared lowercase ref parent context and second raw candidate");"#,
    );
}

#[test]
fn private_source_failure_nested_empty_and_real_empty_root_frame_bridge_hosts() {
    let mut nested = failure_program(vec![
        expression(
            100,
            Expression::Call {
                function: ScalarFunction::GreaterThan,
                args: vec![101, 102],
            },
        ),
        failure_legacy(101, &["Score"], Some(&["Rows", "Kids"])),
        expression(
            102,
            Expression::Const {
                value: Value::Int(1),
            },
        ),
        expression(
            110,
            Expression::Call {
                function: ScalarFunction::Concat,
                args: vec![111, 112, 113, 112, 114],
            },
        ),
        expression(
            111,
            Expression::Position {
                collection: path(&["Rows"]),
            },
        ),
        expression(
            112,
            Expression::Const {
                value: Value::String(":".into()),
            },
        ),
        expression(
            113,
            Expression::Position {
                collection: path(&["Rows", "Kids"]),
            },
        ),
        failure_legacy(114, &["Value"], Some(&["Rows", "Kids"])),
    ]);
    nested.source = SchemaNode::group(
        "Primary",
        vec![
            SchemaNode::scalar("Value", ScalarType::String),
            SchemaNode::group(
                "Rows",
                vec![
                    SchemaNode::group(
                        "Kids",
                        vec![
                            SchemaNode::scalar("Value", ScalarType::String),
                            SchemaNode::scalar("Score", ScalarType::Int),
                        ],
                    )
                    .repeating(),
                ],
            )
            .repeating(),
        ],
    );
    nested.failure_rules.push(source_failure(
        &["Rows", "Kids"],
        codegen::FailureSelection::WhenTrue(100),
        Some(110),
    ));
    retain_failure_host(
        "F542-11",
        &nested,
        r#"Input=G(("Value",S("P.root")),("Rows",R(G(("Kids",R(G(("Value",S("C1")),("Score",I(0))),G(("Value",S("C2")),("Score",I(2)))))),G(("Kids",R(G(("Value",S("C3")),("Score",I(3)))))))));ObservedFailureCollections=new[]{new[]{"Rows"},new[]{"Rows","Kids"}};var a=BridgeReach("F542-11",()=>ExecuteOwnedBasic(Input,Declared,Limits));ExpectFailure(a,1,"1:2:C2");ExpectCohort(a,0,3,new[]{"Rows","Kids"},new long[]{1,2,1});Check(a.Cohorts[0].Collections[0].SequenceEqual(new long[]{1,1,2}) && a.Cohorts[0].Collections[1].SequenceEqual(new long[]{1,2,1}),"uncompacted parent and child raw positions");Check(a.Actual.Nodes.SequenceEqual(new uint[]{100,101,102,100,101,102,110,111,112,113,112,114}),"first selected nested item, lazy concat arguments, no target");"#,
    );
    let empty = failure_scores(codegen::FailureSelection::All);
    retain_failure_host(
        "F542-12",
        &empty,
        r#"Input=G(("Value",S("P.root")),("Rows",R()));var a=BridgeReach("F542-12",()=>ExecuteOwnedBasic(Input,Declared,Limits));ExpectTree(a.Actual,"{\"Value\":\"P.root\"}");ExpectCohort(a,0,0,new[]{"Rows"},Array.Empty<long>());Check(a.Actual.Nodes.SequenceEqual(new uint[]{1}),"empty collection does not evaluate message");"#,
    );
    let mut flat = failure_program(vec![
        expression(
            100,
            Expression::Call {
                function: ScalarFunction::GreaterThan,
                args: vec![101, 102],
            },
        ),
        expression(101, Expression::Position { collection: vec![] }),
        expression(
            102,
            Expression::Const {
                value: Value::Int(1),
            },
        ),
        failure_legacy(103, &["Value"], Some(&[])),
    ]);
    flat.failure_rules.push(source_failure(
        &[],
        codegen::FailureSelection::WhenTrue(100),
        Some(103),
    ));
    retain_failure_host(
        "F542-13",
        &flat,
        r#"Input=R(G(("Value",S("flat1"))),G(("Value",S("flat2"))));var a=BridgeReach("F542-13",()=>ExecuteOwnedBasic(Input,Declared,Limits));ExpectFailure(a,1,"flat2");ExpectCohort(a,0,2,Array.Empty<string>(),new long[]{1,2});Check(a.Actual.Nodes.SequenceEqual(new uint[]{100,101,102,100,101,102,103}),"Some[] message pin has actual repeated empty-root frame and second raw position");"#,
    );
}

#[test]
fn private_source_failure_parent_execution_and_parameter_bridge_hosts() {
    let mut runtime = failure_program(vec![expression(
        101,
        Expression::RuntimeValue {
            value: RuntimeValue::MappingFilePath,
        },
    )]);
    runtime.failure_rules.push(source_failure(
        &[],
        codegen::FailureSelection::All,
        Some(101),
    ));
    retain_failure_host(
        "F542-14",
        &runtime,
        r#"Execution=FerruleExecutionContext.WithParameters("mapping.test",new FerruleRuntimeParameters(Array.Empty<KeyValuePair<string,FerruleValue>>()));var a=BridgeReach("F542-14",()=>ExecuteOwnedBasic(Input,Declared,Limits,Execution));ExpectFailure(a,1,"mapping.test");Check(a.Actual.Nodes.SequenceEqual(new uint[]{101}),"failure candidate retains parent execution");"#,
    );
    runtime
        .expressions
        .iter_mut()
        .find(|node| node.id == 101)
        .unwrap()
        .expression = Expression::RuntimeValue {
        value: RuntimeValue::CurrentDateTime,
    };
    retain_failure_host(
        "F542-15",
        &runtime,
        r#"var a=BridgeReach("F542-15",()=>ExecuteOwnedBasic(Input,Declared,Limits));var e=ExpectRuntime(a.Actual,FerruleRuntimeError.MissingRuntimeValue);Check(e.RuntimeValue==FerruleRuntimeValue.CurrentDateTime && e.Node is null && e.FailureRule is null && e.MappingFailureMessage is null && e.InnerException is null && a.Actual.Result is null,"missing clock exact typed cause before mapping failure");Check(a.Actual.Nodes.SequenceEqual(new uint[]{101}),"clock called before target");"#,
    );
    let mut parameter = failure_program(vec![expression(
        103,
        Expression::RuntimeParameter {
            name: "failure".into(),
            ty: ScalarType::Int,
        },
    )]);
    parameter.failure_rules.push(source_failure(
        &[],
        codegen::FailureSelection::All,
        Some(103),
    ));
    retain_failure_host(
        "F542-16",
        &parameter,
        r#"ParameterValues=new[]{new KeyValuePair<string,FerruleValue>("failure",FerruleValue.FromInt64(42))};Execution=FerruleExecutionContext.WithParameters("mapping.test",new FerruleRuntimeParameters(ParameterValues));var a=BridgeReach("F542-16.provided",()=>ExecuteOwnedBasic(Input,Declared,Limits,Execution));ParameterValues=Array.Empty<KeyValuePair<string,FerruleValue>>();Execution=FerruleExecutionContext.WithParameters("mapping.test",new FerruleRuntimeParameters(ParameterValues));var b=BridgeReach("F542-16.missing",()=>ExecuteOwnedBasic(Input,Declared,Limits,Execution));ParameterValues=new[]{new KeyValuePair<string,FerruleValue>("failure",FerruleValue.FromString("bad"))};Execution=FerruleExecutionContext.WithParameters("mapping.test",new FerruleRuntimeParameters(ParameterValues));var c=BridgeReach("F542-16.type",()=>ExecuteOwnedBasic(Input,Declared,Limits,Execution));ExpectFailure(a,1,"42");var m=ExpectRuntime(b.Actual,FerruleRuntimeError.MissingRuntimeParameter);Check(m.Node==103 && m.RuntimeParameter=="failure" && m.FoundKind is null && m.ExpectedScalarType is null && m.InnerException is null && m.FailureRule is null && b.Actual.Result is null,"missing parameter exact payload");var t=ExpectRuntime(c.Actual,FerruleRuntimeError.RuntimeParameterType);Check(t.Node==103 && t.RuntimeParameter=="failure" && t.FoundKind==FerruleValueKind.String && t.ExpectedScalarType==FerruleScalarType.Int64 && t.FailureRule is null && t.InnerException is null && c.Actual.Result is null,"invalid String cannot coerce to declared Int64");Check(a.Actual.Nodes.SequenceEqual(new uint[]{103}) && b.Actual.Nodes.SequenceEqual(new uint[]{103}) && c.Actual.Nodes.SequenceEqual(new uint[]{103}),"parameter access before target in every route");"#,
    );
}

#[test]
fn private_source_failure_complete_cohort_five_limits_bridge_host() {
    let mut program = failure_program(vec![expression(
        100,
        Expression::Const {
            value: Value::Bool(true),
        },
    )]);
    program.extra_sources.clear();
    program.failure_rules.push(source_failure(
        &["Rows"],
        codegen::FailureSelection::WhenTrue(100),
        None,
    ));
    retain_failure_host(
        "F542-17-22",
        &program,
        r#"Declared=Array.Empty<FerruleDeclaredNamedSourceRoot>();Limits=new(0,1,1,65536,3);var roots=BridgeReach("F542-17",()=>ExecuteOwnedBasic(Input,Declared,Limits));Limits=new(1,1,0,65536,3);var segments=BridgeReach("F542-18",()=>ExecuteOwnedBasic(Input,Declared,Limits));Limits=new(1,1,1,0,3);var bytes=BridgeReach("F542-19",()=>ExecuteOwnedBasic(Input,Declared,Limits));Limits=new(1,0,1,65536,3);var frames=BridgeReach("F542-20",()=>ExecuteOwnedBasic(Input,Declared,Limits));Limits=new(1,1,1,65536,2);var contexts=BridgeReach("F542-21",()=>ExecuteOwnedBasic(Input,Declared,Limits));Limits=new(1,1,1,65536,3);var admitted=BridgeReach("F542-22",()=>ExecuteOwnedBasic(Input,Declared,Limits));ExpectExactOwnedLimit(roots,FerruleOwnedSourceLimitKind.Roots,0,1,0);ExpectExactOwnedLimit(segments,FerruleOwnedSourceLimitKind.PathSegments,0,1,0);ExpectExactOwnedLimit(bytes,FerruleOwnedSourceLimitKind.MetadataBytes,0,4,0);ExpectExactBridgeLimit(frames,FerruleBasicSourceBridgeComponent.ActiveFrames,0,1,0);ExpectExactBridgeLimit(contexts,FerruleBasicSourceBridgeComponent.WalkContexts,0,3,2);ExpectFailure(admitted,1,null);ExpectCohort(admitted,0,3,new[]{"Rows"},new long[]{1,2,3});Check(admitted.Actual.Nodes.SequenceEqual(new uint[]{100}),"complete cohort before first selected predicate");Check(roots.Cohorts.Length==0 && segments.Cohorts.Length==0 && bytes.Cohorts.Length==0 && frames.Cohorts.Length==0 && contexts.Cohorts.Length==0,"no partial cohort published on refusal");"#,
    );
}

fn expected_failure_diagnostic(
    index: usize,
    node: Option<u32>,
    reason: codegen::OwnedSourceCapabilityReason,
) -> ProgramValidationError {
    let location = match node {
        Some(node) => codegen::OwnedSourceDiagnosticLocation::Expression {
            namespace: codegen::OwnedSourceNamespace::Main,
            node,
            scope: None,
        },
        None => codegen::OwnedSourceDiagnosticLocation::Iteration {
            scope: ::mapping::SourceScopeLocation::root(::mapping::SourceScopeTarget::Primary),
        },
    };
    ProgramValidationError::OwnedSource {
        diagnostic: Box::new(codegen::OwnedSourceDiagnostic::Capability {
            location,
            request: None,
            reason,
            origin: codegen::OwnedSourceCapabilityOrigin::Failure { index },
        }),
    }
}
#[test]
fn private_source_failure_dynamic_owned_generated_and_specialized_admission() {
    use codegen::{FailureSelection, OwnedSourceCapabilityReason as Reason};
    use std::error::Error as _;
    let mut base = failure_program(vec![
        expression(
            100,
            Expression::Const {
                value: Value::Bool(false),
            },
        ),
        failure_legacy(101, &["D", "Value"], None),
        expression(
            102,
            Expression::Const {
                value: Value::String("d.json".into()),
            },
        ),
    ]);
    base.source = SchemaNode::group(
        "Primary",
        vec![
            SchemaNode::scalar("Value", ScalarType::String),
            SchemaNode::scalar("Score", ScalarType::Int),
            SchemaNode::group(
                "Rows",
                vec![SchemaNode::scalar("Value", ScalarType::String)],
            )
            .repeating(),
            SchemaNode::group("D", vec![SchemaNode::scalar("Value", ScalarType::String)]),
        ],
    );
    base.extra_sources.push(NamedSourceProgram {
        name: "D".into(),
        source: source_schema("Dynamic"),
        dynamic: Some(codegen::DynamicSourceProgram {
            path: 102,
            driver: codegen::SourceIteration::new(path(&["Rows"])),
        }),
    });
    base.failure_rules
        .push(source_failure(&[], FailureSelection::WhenTrue(100), None));
    let mut cases: Vec<(&str, Program, Option<ProgramValidationError>)> =
        vec![("S542-07 dead/unused dynamic metadata", base.clone(), None)];
    let mut iteration = base.clone();
    iteration.failure_rules[0].iteration =
        codegen::FailureIteration::Source(codegen::SourceIteration::new(path(&["D", "Rows"])));
    cases.push((
        "S542-03 dynamic source iteration",
        iteration,
        Some(expected_failure_diagnostic(0, None, Reason::DynamicLoader)),
    ));
    let mut second_iteration = base.clone();
    second_iteration.failure_rules.push(source_failure(
        &["D", "Rows"],
        FailureSelection::All,
        None,
    ));
    cases.push((
        "S542-03 second rule exact failure index",
        second_iteration,
        Some(expected_failure_diagnostic(1, None, Reason::DynamicLoader)),
    ));
    let mut predicate = base.clone();
    predicate
        .expressions
        .iter_mut()
        .find(|node| node.id == 100)
        .unwrap()
        .expression = Expression::SourceField {
        path: path(&["D", "Value"]),
        frame: None,
    };
    cases.push((
        "S542-04 dynamic predicate path",
        predicate,
        Some(expected_failure_diagnostic(
            0,
            Some(100),
            Reason::DynamicLoader,
        )),
    ));
    let mut message = base.clone();
    message
        .expressions
        .iter_mut()
        .find(|node| node.id == 101)
        .unwrap()
        .expression = Expression::SourceField {
        path: path(&["Value"]),
        frame: Some(path(&["D", "Rows"])),
    };
    message.failure_rules[0].message = Some(101);
    cases.push((
        "S542-05 dynamic message frame",
        message,
        Some(expected_failure_diagnostic(
            0,
            Some(101),
            Reason::DynamicLoader,
        )),
    ));
    let mut lazy = base.clone();
    lazy.expressions.extend([
        expression(
            103,
            Expression::Const {
                value: Value::Bool(true),
            },
        ),
        expression(
            104,
            Expression::If {
                condition: 103,
                then: 100,
                else_: 101,
            },
        ),
    ]);
    lazy.failure_rules[0].selection = FailureSelection::WhenTrue(104);
    cases.push((
        "S542-06 potential unchosen dynamic dependency",
        lazy,
        Some(expected_failure_diagnostic(
            0,
            Some(101),
            Reason::DynamicLoader,
        )),
    ));
    let mut static_source = base.clone();
    static_source.extra_sources.last_mut().unwrap().dynamic = None;
    static_source.failure_rules[0].message = Some(101);
    cases.push(("S542-07 static same-name metadata", static_source, None));
    let mut empty_frame = base.clone();
    empty_frame
        .expressions
        .iter_mut()
        .find(|node| node.id == 101)
        .unwrap()
        .expression = Expression::SourceField {
        path: path(&["D", "Value"]),
        frame: Some(vec![]),
    };
    empty_frame.failure_rules[0].message = Some(101);
    cases.push((
        "S542-08 empty frame replaces path admission",
        empty_frame,
        None,
    ));
    let mut document = base.clone();
    document
        .expressions
        .iter_mut()
        .find(|node| node.id == 101)
        .unwrap()
        .expression = Expression::SourceDocumentPath;
    document.failure_rules[0].message = Some(101);
    cases.push((
        "S542-09 specialized document failure remains closed",
        document,
        Some(expected_failure_diagnostic(0, None, Reason::FailureRule)),
    ));
    let mut generated = base.clone();
    generated.expressions.extend([
        expression(
            110,
            Expression::Const {
                value: Value::Int(1),
            },
        ),
        expression(
            111,
            Expression::Const {
                value: Value::Int(2),
            },
        ),
        failure_legacy(112, &[], None),
    ]);
    generated.failure_rules[0] = codegen::FailureRule {
        iteration: codegen::FailureIteration::Generated(codegen::GeneratedSequence::Range {
            from: Some(110),
            to: 111,
            item: 112,
        }),
        selection: FailureSelection::All,
        message: None,
    };
    cases.push((
        "S542-01 generated failure remains closed",
        generated,
        Some(expected_failure_diagnostic(0, None, Reason::FailureRule)),
    ));
    for (id, program, expected) in cases {
        eprintln!("INPUT {id} complete Program={program:#?}");
        let actual = render_basic_owned_mapping(&program);
        eprintln!("RAW {id} complete Result={actual:#?}");
        if let Err(error) = &actual {
            eprintln!(
                "RAW {id} complete cause={:?}",
                error.source().map(|value| format!("{value:#?}"))
            );
        }
        eprintln!("EXPECTED {id} independent literal={expected:#?}");
        match expected {
            Some(expected) => match actual {
                Err(EmitError::ProgramValidation(actual)) => assert_eq!(actual, expected, "{id}"),
                other => panic!("{id} full retained result differs: {other:#?}"),
            },
            None => assert!(actual.is_ok(), "{id} full actual retained before assertion"),
        }
    }
    for message in [false, true] {
        let mut owned = failure_program(vec![expression(
            100,
            field(SourceOwner::Primary, &["Score"], None),
        )]);
        owned.failure_rules.push(source_failure(
            &[],
            if message {
                FailureSelection::All
            } else {
                FailureSelection::WhenTrue(100)
            },
            if message { Some(100) } else { None },
        ));
        let expected = ProgramValidationError::OwnedSource {
            diagnostic: Box::new(codegen::OwnedSourceDiagnostic::Capability {
                location: codegen::OwnedSourceDiagnosticLocation::Expression {
                    namespace: codegen::OwnedSourceNamespace::Main,
                    node: 100,
                    scope: None,
                },
                request: Some(codegen::OwnedSourceDiagnosticRequest::Field {
                    owner: codegen::OwnedSourceDiagnosticOwner::Primary,
                    path: path(&["Score"]),
                    frame: None,
                }),
                reason: Reason::FailureRule,
                origin: codegen::OwnedSourceCapabilityOrigin::Failure { index: 0 },
            }),
        };
        let actual = render_basic_owned_mapping(&owned);
        eprintln!(
            "INPUT S542-02 ownedfailure message={message} full Program={owned:#?} RAW Result={actual:#?} EXPECTED={expected:#?}"
        );
        if let Err(error) = &actual {
            eprintln!(
                "RAW S542-02 complete cause={:?}",
                error.source().map(|value| format!("{value:#?}"))
            );
        }
        match actual {
            Err(EmitError::ProgramValidation(actual)) => assert_eq!(actual, expected),
            other => panic!("full owned failure refusal differs: {other:#?}"),
        }
    }
}

#[test]
fn private_source_failure_preserves_ordinary_bytes_and_public_owned_gates() {
    let mut program = failure_program(vec![expression(
        100,
        Expression::Const {
            value: Value::Bool(false),
        },
    )]);
    program.failure_rules.push(source_failure(
        &[],
        codegen::FailureSelection::WhenTrue(100),
        None,
    ));
    let ordinary = emit(&program);
    let typed = emit_typed(&program);
    let private = render_basic_owned_mapping(&program);
    eprintln!(
        "INPUT S542-10 complete Program={program:#?} RAW ordinary={ordinary:#?} typed={typed:#?} private={private:#?}"
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
    let private = private.expect("complete private source result retained");
    assert!(private.contains("EvaluateBasicFailureRules(context);"));
    assert!(!private.contains("Reindex") && !private.contains("IterateGenerated"));
    program.expressions[0].expression = Expression::SourceField {
        path: path(&["Value"]),
        frame: None,
    };
    let ordinary = emit(&program);
    let typed = emit_typed(&program);
    let untyped_source = crate::mapping::render(&program);
    let typed_source = crate::mapping::render_typed(&program);
    eprintln!(
        "INPUT ordinary legacy failure complete Program={program:#?} RAW ordinary={ordinary:#?} typed={typed:#?} ordinary_source={untyped_source:#?} typed_source={typed_source:#?}"
    );
    assert!(ordinary.is_ok() && typed.is_ok());
    assert!(
        untyped_source
            .expect("complete ordinary source recorded")
            .contains("FailureRule_0(context);")
    );
    assert!(
        typed_source
            .expect("complete typed source recorded")
            .contains("FailureRule_0(context);")
    );
}

const FAILURE_HOST_HELPERS: &str = r#"
    // Private observation only: these are the exact public same-walk cohort
    // and context APIs. No private ordinal or owner is inferred from a path.
    private sealed record FailureCohortObservation(int Rule,string[] Path,int Count,object[] Frames,long[] Positions,long[][] Collections);
    private sealed record BridgeReached(Reached Actual,FailureCohortObservation[] Cohorts);
    private static readonly List<FailureCohortObservation> FailureCohorts=new();
    private static string[][] ObservedFailureCollections=Array.Empty<string[]>();
    static partial void ObserveBasicFailureCohort(int rule,IReadOnlyList<string> path,FerruleRetainedSourceCohort cohort)
    {
        var frames=new object[cohort.Count];var positions=new long[cohort.Count];
        var collections=new long[ObservedFailureCollections.Length][];
        for(var p=0;p<collections.Length;p++)collections[p]=new long[cohort.Count];
        for(var i=0;i<cohort.Count;i++)
        {
            var candidate=cohort[i];frames[i]=new {candidateEnumerationIndex=i,legacyFrames=candidate.Context.LegacyView.Frames.Select(InstanceSnapshot).ToArray()};
            positions[i]=candidate.Context.LegacyView.Position(path);
            for(var p=0;p<collections.Length;p++)collections[p][i]=candidate.Context.LegacyView.Position(ObservedFailureCollections[p]);
        }
        FailureCohorts.Add(new(rule,path.ToArray(),cohort.Count,frames,positions,collections));
    }
    private static BridgeReached BridgeReach(string id,Func<object?> subject,FerruleTargetSelection? selection=null)
    {
        FailureCohorts.Clear();
        Console.WriteLine(Json(new {id,classification="bridge_input_before_subject",observedCollections=ObservedFailureCollections.Select(path=>path.ToArray()).ToArray()}));
        var actual=Reach(id,subject,selection);var cohorts=FailureCohorts.ToArray();
        Console.WriteLine(Json(new {id,classification="complete_failure_cohorts_before_comparisons",completeInput=actual.CompleteInput,completeActual=actual.CompleteActual,cohorts}));
        return new(actual,cohorts);
    }
    private static void ExpectCohort(BridgeReached reached,int ordinal,int count,string[] path,long[] positions)
    {
        Check(reached.Cohorts.Length>ordinal,"complete cohort observation exists");var cohort=reached.Cohorts[ordinal];
        Check(cohort.Count==count && cohort.Path.SequenceEqual(path) && cohort.Frames.Length==count && cohort.Positions.SequenceEqual(positions),"literal complete cohort/path/raw legacy position sequence");
    }
    private static void ExpectFailure(BridgeReached reached,int rule,string? message)
    {
        var e=ExpectRuntime(reached.Actual,FerruleRuntimeError.MappingFailure);
        Check(e.FailureRule==rule && e.MappingFailureMessage==message && e.Message==(message is null?$"mapping failure rule {rule}: mapping exception was raised":$"mapping failure rule {rule}: {message}"),"full ordinary failure rule/message/display");
        Check(e.Node is null && e.Function is null && e.ExpectedArity is null && e.ActualArity is null && e.FoundKind is null && e.AggregateOperation is null && e.Detail is null && e.RequestedItems is null && e.MaximumItems is null && e.MaximumDepth is null && e.RuntimeValue is null && e.MappingExceptionMessage is null && e.Join is null && e.UserFunction is null && e.FunctionParameter is null && e.ExpectedScalarType is null && e.RuntimeParameter is null && e.SourceField is null && e.FoundInstance is null && e.PrimaryRoot is null && e.OwnedSourceFailure is null && e.BasicSourceBridgeFailure is null && e.InnerException is null && reached.Actual.Result is null,"complete untouched typed failure envelope/no partial outcome");
    }
    private static void ExpectExactOwnedLimit(BridgeReached reached,FerruleOwnedSourceLimitKind kind,nuint used,nuint requested,nuint maximum)
    {
        var e=ExpectOwned(reached.Actual,FerruleOwnedSourceErrorKind.MetadataLimit,null,null,null);
        Check(e.LimitKind==kind && e.Used==used && e.Requested==requested && e.Maximum==maximum,"literal complete core resource cause");
        Check(reached.Actual.Nodes.Length==0 && reached.Cohorts.Length==0,"preallocation refusal publishes no cohort or predicate");
    }
    private static void ExpectExactBridgeLimit(BridgeReached reached,FerruleBasicSourceBridgeComponent component,nuint used,nuint requested,nuint maximum)
    {
        var outer=ExpectRuntime(reached.Actual,FerruleRuntimeError.BasicSourceBridge);var e=outer.BasicSourceBridgeFailure;
        Check(e is not null && ReferenceEquals(e,outer.InnerException),"bare paired bridge cause");var cause=e!;
        Check(cause.Error==FerruleBasicSourceBridgeErrorKind.PairedMetadataLimit && cause.Component==component && cause.Used==used && cause.Requested==requested && cause.Maximum==maximum,"literal complete paired resource cause");
        Check(cause.InputOrdinal is null && cause.CandidateOrdinal is null && cause.DeclarationIndex is null && cause.DeclaredCount is null && cause.Found is null && cause.InnerException is null,"no invented bridge cause fields");
        Check(outer.Node is null && outer.FailureRule is null && outer.MappingFailureMessage is null && outer.OwnedSourceFailure is null && reached.Actual.Result is null && reached.Actual.Nodes.Length==0 && reached.Cohorts.Length==0,"whole cohort refused before predicates/messages/outputs");
    }
"#;

// #519 private interaction controls: checked admission plus ordinary global
// source failures. All source and host controls remain prospective until Root
// qualifies the composed corpus and generated C# friend-test assembly.
fn checked_failure_program() -> Program {
    use codegen::FailureSelection;
    let mut p = failure_scores(FailureSelection::WhenFalse(100));
    p.expressions
        .iter_mut()
        .find(|node| node.id == 101)
        .unwrap()
        .expression = failure_legacy(101, &["Score"], Some(&["ref", "Rows"])).expression;
    p.expressions
        .iter_mut()
        .find(|node| node.id == 103)
        .unwrap()
        .expression = failure_legacy(103, &["Value"], Some(&["ref", "Rows"])).expression;
    p.failure_rules[0] = source_failure(
        &["ref", "Rows"],
        FailureSelection::WhenFalse(100),
        Some(103),
    );
    p.expressions.extend([
        expression(
            90,
            Expression::Const {
                value: Value::Bool(false),
            },
        ),
        expression(91, Expression::Raise { message: None }),
        expression(92, Expression::Raise { message: None }),
        expression(2, Expression::Raise { message: None }),
    ]);
    p.failure_rules.insert(
        0,
        source_failure(&["Rows"], FailureSelection::WhenTrue(90), Some(91)),
    );
    p.failure_rules
        .push(source_failure(&[], FailureSelection::All, Some(92)));
    add_target(
        &mut p,
        "named",
        group(vec![binding("Value", 2, ScalarType::String)]),
    );
    p
}
fn retain_checked_failure_host(id: &str, program: &Program, body: &str) -> String {
    let result = checked_result(id, program);
    if let Err(error) = &result {
        use std::error::Error as _;
        eprintln!(
            "RAW {id} complete typed cause={:?}",
            error.source().map(|value| format!("{value:#?}"))
        );
    }
    let source = result.expect("complete composed source Result captured before comparison");
    let mut host = complete_checked_host(program, body);
    let marker = "\n    public static void Main()\n    {\n";
    let entry_matches = host.matches(marker).count();
    eprintln!(
        "RAW {id} complete checked host before helper composition={host} entry_matches={entry_matches}"
    );
    assert_eq!(entry_matches, 1, "one literal existing host entry");
    let mut helpers = FAILURE_HOST_HELPERS.to_string();
    helpers.push_str(CHECKED_FAILURE_HOST_HELPERS);
    helpers.push_str(marker);
    host = host.replacen(marker, &helpers, 1);
    eprintln!(
        "INPUT {id} complete Program={program:#?} complete prospective composed checked/failure host={host}"
    );
    if let Some(directory) = std::env::var_os("FERRULE_PRIVATE_CHECKED_CSHARP_HOST_DIR") {
        let directory = std::path::PathBuf::from(directory).join(id);
        std::fs::create_dir_all(&directory).expect("root-supplied fresh composed host directory");
        std::fs::write(directory.join("GeneratedMapping.cs"), &source)
            .expect("retain composed mapping");
        std::fs::write(
            directory.join("GeneratedTargetBuilder.cs"),
            crate::runtime::TARGET_BUILDER,
        )
        .expect("unchanged target helper");
        std::fs::write(directory.join("Program.cs"), host)
            .expect("complete composed host retained");
        std::fs::write(directory.join("Program.ir.txt"), format!("{program:#?}\n"))
            .expect("complete typed Program retained");
    }
    source
}

#[test]
fn checked_source_failure_catalogues_include_global_legacy_requirements_for_every_target() {
    use codegen::NamedInputAvailability::{Optional as O, RequiredLegacy as R};
    let p = checked_failure_program();
    let plan = codegen::legacy_named_input_admission_plan(&p);
    let source = checked_result("S519-global-catalogues", &p);
    eprintln!(
        "INPUT S519 complete Program={p:#?} RAW complete catalogue Result={plan:#?} RAW complete source Result={source:#?}"
    );
    let plan = plan.expect("complete catalogue Result captured");
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
    let expected = vec![
        (0, "Unused", O),
        (1, "Ref", O),
        (2, " Ref 雪 ", O),
        (3, "ref", R),
        (4, "primary", O),
    ];
    eprintln!(
        "RAW S519 all={all:#?} primary={primary:#?} named={named:#?} EXPECTED independent original-order rows={expected:#?}"
    );
    assert_eq!(all, expected);
    assert_eq!(primary, expected);
    assert_eq!(named, [(0, "named", expected)]);
    let source = source.expect("complete source Result captured");
    let seam = source
        .find("private static object ExecuteCheckedBasicContext(")
        .expect("checked context seam");
    let seam = &source[seam..];
    assert!(
        seam.find("EvaluateBasicFailureRules(context);").unwrap()
            < seam.find("if (selected.HasValue)").unwrap()
    );
}

#[test]
fn checked_source_failure_host_preserves_factory_first_and_ordered_global_failure_before_targets() {
    let p = checked_failure_program();
    retain_checked_failure_host(
        "C519-ordered-selection",
        &p,
        r#"
Input=G(("Value",S("P.root")),("Rows",R(G(("Value",S("P1")),("Score",I(10))),G(("Value",S("P2")),("Score",I(20))),G(("Value",S("P3")),("Score",I(30))))));
var namedRoot=G(("Rows",R(G(("Value",S("N1")),("Score",I(30))),G(("Value",S("N2")),("Score",I(10))))));
var probe=new CheckedProbe(new[]{new NamedInput("ref",namedRoot)});
var a=CheckedFailureReach("F519-01",probe,()=>ExecuteCheckedOwnedBasic(Input,probe,Limits));ExpectReads(a.Checked,1,1,1,1);ExpectOrderedFailure(a);
var primarySelection=FerruleTargetSelection.Primary();probe=new CheckedProbe(new[]{new NamedInput("ref",namedRoot)});
var b=CheckedFailureReach("F519-02",probe,()=>ExecuteSelectedCheckedOwnedBasic(Input,primarySelection,probe,Limits),primarySelection);ExpectReads(b.Checked,1,1,1,1);ExpectOrderedFailure(b);
var namedSelection=FerruleTargetSelection.Named("named");probe=new CheckedProbe(new[]{new NamedInput("ref",namedRoot)});
var c=CheckedFailureReach("F519-03",probe,()=>ExecuteSelectedCheckedOwnedBasic(Input,namedSelection,probe,Limits),namedSelection);ExpectReads(c.Checked,1,1,1,1);ExpectOrderedFailure(c);
Limits=new(0,0,0,0,0);var missingSelection=FerruleTargetSelection.Named("missing");probe=new CheckedProbe(new[]{new NamedInput("X",namedRoot)});
var d=CheckedFailureReach("F519-04",probe,()=>ExecuteSelectedCheckedOwnedBasic(Input,missingSelection,probe,Limits),missingSelection);ExpectInputError(d.Checked.Value,FerruleRuntimeError.UnknownTarget,"missing","project has no named target "+(char)96+"missing"+(char)96);ExpectReads(d.Checked,0,0,0,0);Check(d.Bridge.Cohorts.Length==0 && d.Checked.Value.Nodes.Length==0,"target refusal precedes factory/global failures");
Limits=new(16,8,16,1048576,64);probe=new CheckedProbe(Array.Empty<NamedInput>());
var e=CheckedFailureReach("F519-05",probe,()=>ExecuteSelectedCheckedOwnedBasic(Input,primarySelection,probe,Limits),primarySelection);ExpectInputError(e.Checked.Value,FerruleRuntimeError.MissingNamedSource,"ref","named source is required by this mapping");ExpectReads(e.Checked,1,0,1,0);Check(e.Bridge.Cohorts.Length==0 && e.Checked.Value.Nodes.Length==0,"missing global legacy requirement precedes failure walk");
probe=new CheckedProbe(new[]{new NamedInput("X",namedRoot),new NamedInput("ref",namedRoot)});
var f=CheckedFailureReach("F519-06",probe,()=>ExecuteSelectedCheckedOwnedBasic(Input,primarySelection,probe,Limits),primarySelection);ExpectInputError(f.Checked.Value,FerruleRuntimeError.UnexpectedNamedSource,"X","named source is not declared by this mapping");ExpectReads(f.Checked,1,1,1,1);Check(f.Bridge.Cohorts.Length==0 && f.Checked.Value.Nodes.Length==0,"first bad input preserves unread suffix/global failure ordering");
"#,
    );
}

#[test]
fn checked_source_failure_hosts_preserve_false_rule_laziness_and_first_selected_raw_position() {
    use codegen::FailureSelection;
    let mut success = failure_program(vec![
        expression(
            90,
            Expression::Const {
                value: Value::Bool(false),
            },
        ),
        expression(91, Expression::Raise { message: None }),
    ]);
    success.failure_rules.push(source_failure(
        &["Rows"],
        FailureSelection::WhenTrue(90),
        Some(91),
    ));
    add_target(
        &mut success,
        "named",
        group(vec![binding("Value", 1, ScalarType::String)]),
    );
    retain_checked_failure_host(
        "C519-false-lazy",
        &success,
        r#"
Input=G(("Value",S("P.root")),("Rows",R(G(("Value",S("P1")),("Score",I(10))),G(("Value",S("P2")),("Score",I(20))),G(("Value",S("P3")),("Score",I(30))))));
var probe=new CheckedProbe(Array.Empty<NamedInput>());var a=CheckedFailureReach("F519-07",probe,()=>ExecuteCheckedOwnedBasic(Input,probe,Limits));ExpectReads(a.Checked,1,0,1,0);ExpectTree(a.Checked.Value,"{\"Value\":\"P.root\"}");var all=(ExecutionOutputs)a.Checked.Value.Result!;Check(all.Extras.Count==1 && all.Extras[0].Name=="named" && Json(Plain(all.Extras[0].Instance))=="{\"Value\":\"P.root\"}","all ordinary outputs after successful global rule");ExpectCohort(a.Bridge,0,3,new[]{"Rows"},new long[]{1,2,3});Check(a.Bridge.Cohorts.Length==1 && a.Checked.Value.Nodes.SequenceEqual(new uint[]{90,90,90,1,1}),"false predicate complete walk before both output nodes; lazy Raise message");
var primarySelection=FerruleTargetSelection.Primary();probe=new CheckedProbe(Array.Empty<NamedInput>());var b=CheckedFailureReach("F519-08",probe,()=>ExecuteSelectedCheckedOwnedBasic(Input,primarySelection,probe,Limits),primarySelection);ExpectReads(b.Checked,1,0,1,0);ExpectTree(b.Checked.Value,"{\"Value\":\"P.root\"}");ExpectCohort(b.Bridge,0,3,new[]{"Rows"},new long[]{1,2,3});Check(b.Bridge.Cohorts.Length==1 && b.Checked.Value.Nodes.SequenceEqual(new uint[]{90,90,90,1}),"selected primary follows whole false rule");
var namedSelection=FerruleTargetSelection.Named("named");probe=new CheckedProbe(Array.Empty<NamedInput>());var c=CheckedFailureReach("F519-09",probe,()=>ExecuteSelectedCheckedOwnedBasic(Input,namedSelection,probe,Limits),namedSelection);ExpectReads(c.Checked,1,0,1,0);ExpectTree(c.Checked.Value,"{\"Value\":\"P.root\"}");ExpectCohort(c.Bridge,0,3,new[]{"Rows"},new long[]{1,2,3});Check(c.Bridge.Cohorts.Length==1 && c.Checked.Value.Nodes.SequenceEqual(new uint[]{90,90,90,1}),"selected named follows same global rule; unused static names optional");
"#,
    );
    let mut first = failure_scores(FailureSelection::WhenFalse(100));
    first
        .expressions
        .push(expression(91, Expression::Raise { message: None }));
    first
        .failure_rules
        .push(source_failure(&[], FailureSelection::All, Some(91)));
    retain_checked_failure_host(
        "C519-first-selected",
        &first,
        r#"
Input=G(("Value",S("P.root")),("Rows",R(G(("Value",S("P1")),("Score",I(10))),G(("Value",S("P2")),("Score",I(20))),G(("Value",S("P3")),("Score",I(30))))));
var probe=new CheckedProbe(Array.Empty<NamedInput>());var a=CheckedFailureReach("F519-10",probe,()=>ExecuteCheckedOwnedBasic(Input,probe,Limits));ExpectFirstSecondRow(a);ExpectReads(a.Checked,1,0,1,0);
var primarySelection=FerruleTargetSelection.Primary();probe=new CheckedProbe(Array.Empty<NamedInput>());var b=CheckedFailureReach("F519-11",probe,()=>ExecuteSelectedCheckedOwnedBasic(Input,primarySelection,probe,Limits),primarySelection);ExpectFirstSecondRow(b);ExpectReads(b.Checked,1,0,1,0);
"#,
    );
}

const CHECKED_FAILURE_HOST_HELPERS: &str = r#"
    private sealed record CheckedFailureReached(CheckedReached Checked,BridgeReached Bridge);
    private static CheckedFailureReached CheckedFailureReach(string id,CheckedProbe probe,Func<object?> subject,FerruleTargetSelection? selection=null)
    {
        FailureCohorts.Clear();
        Console.WriteLine(Json(new {id,classification="checked_failure_input_before_subject",observedCollections=ObservedFailureCollections.Select(path=>path.ToArray()).ToArray()}));
        var actual=CheckedReach(id,probe,subject,selection);var cohorts=FailureCohorts.ToArray();
        var complete=Json(new {classification="complete_checked_failure_actual",actual.Value.CompleteInput,actual.Value.CompleteActual,actual.Factory,actual.Accessor,actual.Count,actual.Index,cohorts});
        Console.WriteLine(Json(new {id,classification="before_comparisons",completeInput=actual.Value.CompleteInput,completeActual=complete}));
        return new(actual,new(actual.Value,cohorts));
    }
    private static void ExpectOrderedFailure(CheckedFailureReached actual)
    {
        ExpectFailure(actual.Bridge,2,"N1");
        Check(actual.Bridge.Cohorts.Length==2 && actual.Bridge.Cohorts[0].Rule==1 && actual.Bridge.Cohorts[1].Rule==2,"exact global rule order and stop before third rule");
        ExpectCohort(actual.Bridge,0,3,new[]{"Rows"},new long[]{1,2,3});
        ExpectCohort(actual.Bridge,1,2,new[]{"ref","Rows"},new long[]{1,2});
        Check(actual.Checked.Value.Nodes.SequenceEqual(new uint[]{90,90,90,100,101,102,103}),"first selected named row/lazy unselected message/no target scope entered");
    }
    private static void ExpectFirstSecondRow(CheckedFailureReached actual)
    {
        ExpectFailure(actual.Bridge,1,"P2");ExpectCohort(actual.Bridge,0,3,new[]{"Rows"},new long[]{1,2,3});
        Check(actual.Bridge.Cohorts.Length==1 && actual.Bridge.Cohorts[0].Rule==1,"second global rule never reached");
        Check(actual.Checked.Value.Nodes.SequenceEqual(new uint[]{100,101,102,100,101,102,103}),"predicate visits only first two raw items; message only on selected raw position2; no target output");
    }
"#;

fn public_basic_result(label: &str, program: &Program) -> Result<codegen::ArtifactSet, EmitError> {
    eprintln!("INPUT {label} complete Program={program:#?}");
    let actual = crate::emit_with_basic_owned_sources(program);
    eprintln!("RAW {label} complete artifact/error Result={actual:#?}");
    actual
}
fn retain_public_basic_consumer(label: &str, program: &Program, body: &str) -> String {
    let actual = public_basic_result(label, program);
    let artifacts = actual.expect("complete artifact/refusal retained before propagation");
    let source = String::from_utf8(
        artifacts
            .files()
            .iter()
            .find(|file| file.path.as_str() == "GeneratedMapping.cs")
            .unwrap()
            .contents
            .clone(),
    )
    .expect("complete emitted mapping is UTF-8");
    let host = PUBLIC_BASIC_CONSUMER
        .replace("__ERROR_SNAPSHOT__", HOST_ERROR_SNAPSHOT)
        .replace("__BODY__", body);
    eprintln!("RAW {label} complete independent public consumer={host}");
    if let Some(base) = std::env::var_os("FERRULE_PUBLIC_BASIC_CSHARP_HOST_DIR") {
        let base = std::path::PathBuf::from(base);
        assert!(
            base.is_absolute(),
            "root supplies a fresh absolute evidence namespace"
        );
        let root = base.join(label);
        std::fs::create_dir(&root).expect("fresh consumer namespace; preserve failure evidence");
        std::fs::create_dir(root.join("generated")).unwrap();
        std::fs::create_dir(root.join("consumer")).unwrap();
        for file in artifacts.files() {
            let path = root.join("generated").join(file.path.as_str());
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            let mut reached = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(path)
                .unwrap();
            std::io::Write::write_all(&mut reached, &file.contents).unwrap();
            reached.sync_all().unwrap();
        }
        let input_original = format!("{program:#?}\n");
        for (name, bytes) in [
            (
                "consumer/PublicConsumer.csproj",
                PUBLIC_BASIC_CONSUMER_PROJECT.as_bytes(),
            ),
            ("consumer/Program.cs", host.as_bytes()),
            ("Program.ir.original.txt", input_original.as_bytes()),
        ] {
            let mut reached = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(root.join(name))
                .unwrap();
            std::io::Write::write_all(&mut reached, bytes).unwrap();
            reached.sync_all().unwrap();
        }
    }
    source
}

const PUBLIC_BASIC_CONSUMER_PROJECT: &str = r#"<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup>
    <TargetFramework>net10.0</TargetFramework>
    <OutputType>Exe</OutputType>
    <ImplicitUsings>enable</ImplicitUsings>
    <Nullable>enable</Nullable>
    <TreatWarningsAsErrors>true</TreatWarningsAsErrors>
    <Deterministic>true</Deterministic>
    <InvariantGlobalization>true</InvariantGlobalization>
    <AssemblyName>Ferrule.PublicBasicConsumer</AssemblyName>
  </PropertyGroup>
  <ItemGroup><ProjectReference Include="../generated/Ferrule.Generated.csproj" /></ItemGroup>
</Project>
"#;

const PUBLIC_BASIC_CONSUMER: &str = r#"using System;
using System.Linq;
using System.Collections.Generic;
using System.Reflection;
using System.Text.Json;
using Ferrule.Generated;
using Ferrule.Runtime;
namespace Ferrule.PublicConsumer;
internal static class Program
{
    private sealed class Probe : IReadOnlyList<NamedInput>
    {
        internal readonly NamedInput[] Items;
        internal readonly bool ThrowCount, ThrowIndex;
        internal readonly Exception Failure = new InvalidOperationException("public caller sentinel");
        internal int CountReads, IndexReads;
        internal Probe(NamedInput[] items,bool throwCount=false,bool throwIndex=false)
        { Items=items;ThrowCount=throwCount;ThrowIndex=throwIndex; }
        public int Count { get { CountReads++;if(ThrowCount)throw Failure;return Items.Length; } }
        public NamedInput this[int index] { get { IndexReads++;if(ThrowIndex)throw Failure;return Items[index]; } }
        public IEnumerator<NamedInput> GetEnumerator()=>throw new InvalidOperationException("input enumeration forbidden");
        System.Collections.IEnumerator System.Collections.IEnumerable.GetEnumerator()=>GetEnumerator();
    }
    private sealed record Actual(object? Result,Exception? Error,int CountReads,int IndexReads,string Input,string Raw,bool CallerIdentity);
    private static string Json(object? value)=>JsonSerializer.Serialize(value);
    private static void Check(bool value,string expected)
    { if(!value)throw new InvalidOperationException("Frozen expected: "+expected); }
    private static object ValueSnapshot(FerruleValue value)=>new {kind=value.Kind.ToString(),value=value.Kind switch
    {FerruleValueKind.String=>(object?)value.StringValue,FerruleValueKind.Int64=>value.Int64Value,FerruleValueKind.Bool=>value.BooleanValue,FerruleValueKind.Double=>BitConverter.DoubleToInt64Bits(value.DoubleValue),_=>null}};
    private static object? InstanceSnapshot(FerruleInstance? value)=>value switch
    {
        null=>null,FerruleScalar scalar=>new {kind="Scalar",value=ValueSnapshot(scalar.Value)},
        FerruleGroup group=>new {kind="Group",fields=group.Fields.Select(f=>new {f.Name,value=InstanceSnapshot(f.Value)}).ToArray()},
        FerruleRepeated repeated=>new {kind="Repeated",items=repeated.Items.Select(InstanceSnapshot).ToArray()},
        _=>throw new InvalidOperationException("Unrecorded instance kind")
    };
    private static object? ResultSnapshot(object? value)=>value switch
    {
        ExecutionOutputs all=>new {kind="ExecutionOutputs",primary=InstanceSnapshot(all.Primary),extras=all.Extras.Select(e=>new {e.Name,instance=InstanceSnapshot(e.Instance)}).ToArray()},
        SelectedTargetOutput.Primary primary=>new {kind="Primary",instance=InstanceSnapshot(primary.Instance)},
        SelectedTargetOutput.Named named=>new {kind="Named",named.Output.Name,instance=InstanceSnapshot(named.Output.Instance)},
        null=>null,_=>throw new InvalidOperationException("Unrecorded public result shape")
    };
    private static Actual Reach(string id,FerruleInstance source,Probe supplied,FerruleOwnedSourceLimits limits,
        FerruleExecutionContext? execution,FerruleTargetSelection? selection,Func<object?> subject,
        KeyValuePair<string,FerruleValue>[]? parameters=null)
    {
        supplied.CountReads=0;supplied.IndexReads=0;
        var input=Json(new {source=InstanceSnapshot(source),supplied=supplied.Items.Select(item=>item is null?null:new {item.Name,units=item.Name?.Select(c=>(int)c).ToArray(),instance=InstanceSnapshot(item.Instance)}).ToArray(),
            limits=new {roots=(ulong)limits.MaxRoots,frames=(ulong)limits.MaxActiveFrames,segments=(ulong)limits.MaxPathSegments,bytes=(ulong)limits.MaxMetadataBytes,contexts=(ulong)limits.MaxWalkContexts},
            selection=selection is null?null:new {selection.IsPrimary,selection.Name},
            execution=execution is null?null:new {execution.MappingFilePath,execution.MainMappingFilePath,execution.CurrentDateTime,parameterCount=execution.RuntimeParameters.Count,parameters=(parameters??Array.Empty<KeyValuePair<string,FerruleValue>>()).Select(p=>new {p.Key,value=ValueSnapshot(p.Value)}).ToArray()},
            supplied.ThrowCount,supplied.ThrowIndex});
        Console.WriteLine(Json(new {id,classification="complete_input_before_subject",input}));
        object? result=null;Exception? error=null;try{result=subject();}catch(Exception reached){error=reached;}
        var count=supplied.CountReads;var index=supplied.IndexReads;var identity=ReferenceEquals(error,supplied.Failure);
        var raw=Json(new {result=ResultSnapshot(result),error=error is null?null:ErrorSnapshot(error),count,index,callerIdentity=identity});
        Console.WriteLine(Json(new {id,classification="complete_actual_before_comparison",input,raw}));
        return new(result,error,count,index,input,raw,identity);
    }
    private static object? Plain(FerruleInstance value)=>value switch
    {
        FerruleScalar scalar=>scalar.Value.Kind switch {FerruleValueKind.String=>(object?)scalar.Value.StringValue,FerruleValueKind.Int64=>scalar.Value.Int64Value,FerruleValueKind.Bool=>scalar.Value.BooleanValue,FerruleValueKind.Null=>"Null",FerruleValueKind.JsonNull=>"JsonNull",FerruleValueKind.XmlNil=>"XmlNil",FerruleValueKind.Double=>scalar.Value.DoubleValue,_=>throw new InvalidOperationException()},
        FerruleGroup group=>group.Fields.ToDictionary(f=>f.Name,f=>Plain(f.Value),StringComparer.Ordinal),
        FerruleRepeated repeated=>repeated.Items.Select(Plain).ToArray(),_=>throw new InvalidOperationException()
    };
    private static void Tree(Actual actual,string expected)
    {
        Check(actual.Error is null,"typed successful result");
        var root=actual.Result switch {ExecutionOutputs all=>all.Primary,SelectedTargetOutput.Primary primary=>primary.Instance,SelectedTargetOutput.Named named=>named.Output.Instance,_=>throw new InvalidOperationException("Missing public tree")};
        var found=Json(Plain(root));Console.WriteLine(Json(new {classification="comparison",actual=found,expected}));Check(found==expected,expected);
    }
    private static void Reads(Actual actual,int count,int index)
    { Check(actual.CountReads==count && actual.IndexReads==index,"actual caller-list reads only, not factory allocation"); }
    private static FerruleRuntimeException Runtime(Actual actual,FerruleRuntimeError category)
    { Check(actual.Error is FerruleRuntimeException e && e.Error==category && actual.Result is null,"exact typed outer/no partial returned output");return (FerruleRuntimeException)actual.Error!; }
    private static void NullMetadata(FerruleRuntimeException error,params string[] permitted)
    {
        var names=new[]{"Node","Function","ExpectedArity","ActualArity","FoundKind","AggregateOperation","Detail","RequestedItems","MaximumItems","MaximumDepth","RuntimeValue","FailureRule","MappingFailureMessage","MappingExceptionMessage","Join","UserFunction","FunctionParameter","ExpectedScalarType","RuntimeParameter","SourceField","FoundInstance","PrimaryRoot","OwnedSourceFailure","BasicSourceBridgeFailure"};
        foreach(var name in names)if(!permitted.Contains(name,StringComparer.Ordinal))
            Check(typeof(FerruleRuntimeException).GetProperty(name)!.GetValue(error) is null,"unchanged absent "+name);
    }
    private static void InputError(Actual actual,FerruleRuntimeError category,string detail,string message)
    {var e=Runtime(actual,category);Check(e.Detail==detail && e.Message==message && e.InnerException is null,"full input error");NullMetadata(e,"Detail");}
    private static void Failure(Actual actual,int rule,string message)
    {var e=Runtime(actual,FerruleRuntimeError.MappingFailure);Check(e.FailureRule==rule && e.MappingFailureMessage==message && e.Message==$"mapping failure rule {rule}: {message}" && e.InnerException is null,"full ordered global failure");NullMetadata(e,"FailureRule","MappingFailureMessage");}
    private static void OwnedAbsent(Actual actual,string name,uint node)
    {
        var outer=Runtime(actual,FerruleRuntimeError.OwnedSource);var cause=outer.OwnedSourceFailure;
        Check(cause is not null && ReferenceEquals(cause,outer.InnerException) && cause.Error==FerruleOwnedSourceErrorKind.SourceUnavailable,"exact bare owned inner cause");
        var c=cause!;Check(c.Owner?.IsPrimary==false && c.Owner?.Name==name && c.RequestedName==name && c.Message=="Owned source operation failed." && outer.Message=="owned source access failed","exact owner, requested name and display");
        Check(c.DeclarationIndices is null && c.Path is null && c.Frame is null && c.Collection is null && c.Segment is null && c.FoundInstance is null && c.RequestedIndex is null && c.LimitKind is null && c.Used is null && c.Requested is null && c.Maximum is null && c.InnerException is null,"complete absent owned metadata");Check(outer.Node==node,"exact owned leaf node");NullMetadata(outer,"Node","OwnedSourceFailure");
    }
    private static FerruleInstance S(string text)=>new FerruleScalar(FerruleValue.FromString(text));
    private static FerruleInstance I(long value)=>new FerruleScalar(FerruleValue.FromInt64(value));
    private static FerruleInstance G(params (string Name,FerruleInstance Value)[] fields)=>new FerruleGroup(fields.Select(f=>new FerruleField(f.Name,f.Value)));
    private static FerruleInstance R(params FerruleInstance[] items)=>new FerruleRepeated(items);
    private static FerruleInstance Primary()=>G(("Value",S("P.root")),("Ref",G(("Value",S("P.collision")))),("Rows",R(G(("Value",S("P1")),("Score",I(10))),G(("Value",S("P2")),("Score",I(20))),G(("Value",S("P3")),("Score",I(30))))));
    private static FerruleOwnedSourceLimits Policy()=>new(16,8,16,1048576,64);
    private static void Api()
    {
        var methods=typeof(GeneratedMapping).GetMethods(BindingFlags.Public|BindingFlags.Static|BindingFlags.DeclaredOnly).OrderBy(m=>m.Name,StringComparer.Ordinal).ToArray();
        var properties=typeof(FerruleOwnedSourceLimits).GetProperties(BindingFlags.Public|BindingFlags.Instance|BindingFlags.DeclaredOnly).OrderBy(p=>p.Name,StringComparer.Ordinal).ToArray();
        Console.WriteLine(Json(new {classification="complete_public_API_before_comparison",methods=methods.Select(m=>new {m.Name,result=m.ReturnType.FullName,parameters=m.GetParameters().Select(p=>new {p.Name,type=p.ParameterType.FullName,p.IsOptional,p.HasDefaultValue}).ToArray()}).ToArray(),limits=properties.Select(p=>new {p.Name,type=p.PropertyType.FullName}).ToArray()}));
        Check(methods.Select(m=>m.Name).SequenceEqual(new[]{"ExecuteSelectedWithBasicOwnedSources","ExecuteWithBasicOwnedSources"}),"only two public methods; no metadata/observer/counter/transport or eager entry");
        var all=methods[1];var selected=methods[0];
        Check(all.ReturnType==typeof(ExecutionOutputs) && all.GetParameters().Select(p=>p.ParameterType).SequenceEqual(new[]{typeof(FerruleInstance),typeof(IReadOnlyList<NamedInput>),typeof(FerruleOwnedSourceLimits),typeof(FerruleExecutionContext)}),"exact all entry");
        Check(selected.ReturnType==typeof(SelectedTargetOutput) && selected.GetParameters().Select(p=>p.ParameterType).SequenceEqual(new[]{typeof(FerruleInstance),typeof(FerruleTargetSelection),typeof(IReadOnlyList<NamedInput>),typeof(FerruleOwnedSourceLimits),typeof(FerruleExecutionContext)}),"exact selected entry");
        Check(methods.SelectMany(m=>m.GetParameters()).All(p=>!p.IsOptional && !p.HasDefaultValue),"every public argument mandatory");
        Check(properties.Select(p=>p.Name).SequenceEqual(new[]{"MaxActiveFrames","MaxMetadataBytes","MaxPathSegments","MaxRoots","MaxWalkContexts"}) && properties.All(p=>p.PropertyType==typeof(nuint)),"all five native policy fields unchanged");
    }
__ERROR_SNAPSHOT__
    public static void Main()
    {
        Api();
__BODY__
    }
}
"#;

#[test]
fn public_basic_external_consumer_has_mandatory_policy_and_input_order() {
    let p = basic(
        vec![expression(1, field(SourceOwner::Primary, &["Value"], None))],
        group(vec![binding("Value", 1, ScalarType::String)]),
    );
    let source = retain_public_basic_consumer("public-input-order", &p, PUBLIC_INPUT_ORDER);
    assert!(source.contains("public static ExecutionOutputs ExecuteWithBasicOwnedSources("));
    assert!(
        source.contains("public static SelectedTargetOutput ExecuteSelectedWithBasicOwnedSources(")
    );
    assert!(source.contains("=> ExecuteCheckedOwnedBasic(source, supplied, limits, execution);"));
    assert!(source.contains(
        "=> ExecuteSelectedCheckedOwnedBasic(source, selection, supplied, limits, execution);"
    ));
    let selected = source
        .find("private static SelectedTargetOutput ExecuteSelectedCheckedOwnedBasic(")
        .unwrap();
    let selected = &source[selected..];
    assert!(
        selected.find("ResolveOwnedBasicTarget(selection)").unwrap()
            < selected
                .find("new CheckedNamedInputAccessor(supplied)")
                .unwrap()
    );
    assert!(
        selected
            .find("new CheckedNamedInputAccessor(supplied)")
            .unwrap()
            < selected.find("CreateFromNamedInputs(").unwrap()
    );
}

#[test]
fn public_basic_external_consumer_keeps_lazy_optional_named_owner() {
    let p = basic(
        vec![
            expression(
                1,
                Expression::RuntimeParameter {
                    name: "flag".into(),
                    ty: ScalarType::Bool,
                },
            ),
            expression(2, field(SourceOwner::Primary, &["Value"], None)),
            expression(3, field(named("Ref"), &["Value"], None)),
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
    let source = retain_public_basic_consumer("public-lazy-owner", &p, PUBLIC_LAZY_OWNER);
    assert!(source.contains("FerruleOwnedSourceOwner.Named(\"Ref\")"));
    assert!(source.contains("FerruleBasicNamedSourceMode.Optional"));
}

#[test]
fn public_basic_external_consumer_preserves_selected_target_and_ordinary_raise() {
    let mut p = basic(
        vec![
            expression(1, field(SourceOwner::Primary, &["Value"], None)),
            expression(2, Expression::Raise { message: None }),
        ],
        group(vec![binding("Value", 1, ScalarType::String)]),
    );
    add_target(
        &mut p,
        "Bad",
        TargetScope {
            target_field: String::new(),
            repeating: false,
            iteration: None,
            construction: TargetConstruction::Scalar {
                expression: 2,
                target_domain: ScalarType::String.into(),
            },
            bindings: vec![],
            children: vec![],
        },
    );
    let source = retain_public_basic_consumer("public-P19-selected", &p, PUBLIC_P19_SELECTED);
    assert!(source.contains("FerruleFailures.MappingException(2U, null)"));
    assert!(source.contains("if (selected.HasValue)"));
}

#[test]
fn public_basic_external_consumer_runs_ordered_global_failures_for_every_selection() {
    let p = checked_failure_program();
    let source = retain_public_basic_consumer("public-global-order", &p, PUBLIC_GLOBAL_ORDER);
    let context = source
        .find("private static object ExecuteCheckedBasicContext(")
        .unwrap();
    let context = &source[context..];
    assert!(
        context.find("EvaluateBasicFailureRules(context);").unwrap()
            < context.find("if (selected.HasValue)").unwrap()
    );
    assert_eq!(
        source
            .matches("FerruleBasicNamedSourceMode.RequiredLegacy")
            .count(),
        3
    );
    assert!(source.contains("finally { global::System.GC.KeepAlive(cohort_failure_1); }"));
}

#[test]
fn public_basic_external_consumer_keeps_complete_raw_position_before_failure() {
    use codegen::FailureSelection;
    let mut p = failure_scores(FailureSelection::WhenFalse(100));
    p.expressions
        .iter_mut()
        .find(|node| node.id == 103)
        .unwrap()
        .expression = Expression::Position {
        collection: path(&["Rows"]),
    };
    p.expressions.extend([
        expression(91, Expression::Raise { message: None }),
        expression(2, Expression::Raise { message: None }),
    ]);
    p.failure_rules
        .push(source_failure(&[], FailureSelection::All, Some(91)));
    add_target(
        &mut p,
        "named",
        group(vec![binding("Value", 2, ScalarType::String)]),
    );
    let source = retain_public_basic_consumer("public-global-position", &p, PUBLIC_GLOBAL_POSITION);
    assert!(source.contains("var path_failure_0 = new string[] { \"Rows\" };"));
    assert!(source.contains("cohort_failure_0 = context.WalkSourceCohort(path_failure_0)"));
    assert!(source.contains("finally { global::System.GC.KeepAlive(cohort_failure_0); }"));
    assert!(source.contains("LegacyPosition(new string[] { \"Rows\" })"));
}

#[test]
fn public_basic_external_consumer_keeps_named_occurrence_identity_and_positions() {
    let p = basic(
        vec![
            expression(1, field(named("Ref"), &["Rows", "Value"], Some(&["Rows"]))),
            expression(2, position(named("Ref"), Some(&["Rows"]))),
        ],
        owned_iteration(
            named("Ref"),
            &["Rows"],
            vec![
                binding("Value", 1, ScalarType::String),
                binding("Position", 2, ScalarType::Int),
            ],
        ),
    );
    let source = retain_public_basic_consumer("public-owned-walk", &p, PUBLIC_OWNED_WALK);
    assert!(source.contains("WalkOwnedSourceCohort(global::Ferrule.Runtime.FerruleOwnedSourceOwner.Named(\"Ref\"), new string[] { \"Rows\" })"));
    assert!(source.contains("global::System.GC.KeepAlive("));
}

#[test]
fn public_basic_external_consumer_preserves_scalar_root_empty_catalogue_and_zero_text() {
    let mut p = basic(
        vec![expression(1, field(SourceOwner::Primary, &[], None))],
        group(vec![binding("Value", 1, ScalarType::String)]),
    );
    p.source = SchemaNode::scalar("Input", ScalarType::String);
    p.extra_sources.clear();
    let source = retain_public_basic_consumer("public-scalar-zero", &p, PUBLIC_SCALAR_ZERO);
    assert!(
        source.contains(
            "FerruleOwnedSourceOwner.Primary, global::System.Array.Empty<string>(), null"
        )
    );
    assert!(source.contains("CheckedAllInputs"));
}

#[test]
fn public_basic_compiler_preserves_structure_closed_controls_and_ordinary_family() {
    use codegen::{
        OwnedSourceCapabilityOrigin as O, OwnedSourceCapabilityReason as R,
        OwnedSourceDiagnostic as D, OwnedSourceDiagnosticLocation as L,
        OwnedSourceDiagnosticOwner as W, OwnedSourceDiagnosticRequest as Q,
        OwnedSourceNamespace as N,
    };
    fn extract(actual: Result<codegen::ArtifactSet, EmitError>) -> ProgramValidationError {
        match actual {
            Err(EmitError::ProgramValidation(error)) => error,
            other => panic!("complete retained refusal expected: {other:#?}"),
        }
    }
    let mut unknown = basic(
        vec![expression(1, field(named("Absent"), &["Value"], None))],
        group(vec![binding("Value", 1, ScalarType::String)]),
    );
    let unknown_actual = public_basic_result("S519-C01", &unknown);
    let unknown_expected = ProgramValidationError::OwnedSource {
        diagnostic: Box::new(D::Structure {
            location: L::Expression {
                namespace: N::Main,
                node: 1,
                scope: None,
            },
            request: Q::Field {
                owner: W::Named {
                    name: "Absent".into(),
                },
                path: path(&["Value"]),
                frame: None,
            },
            reason: codegen::OwnedSourceStructureReason::UnknownOwner,
        }),
    };
    eprintln!("EXPECTED S519-C01 complete independent diagnostic={unknown_expected:#?}");
    assert_eq!(extract(unknown_actual), unknown_expected);
    unknown.expressions[0].expression = field(SourceOwner::Primary, &["Value"], None);
    let mut value_map = unknown.clone();
    value_map.expressions.push(expression(
        2,
        Expression::ValueMap {
            input: 1,
            input_type: None,
            table: vec![(
                Value::String("P.root".into()),
                Value::String("mapped".into()),
            )],
            default: None,
        },
    ));
    value_map.root.bindings[0].expression = 2;
    let value_actual = public_basic_result("S519-C02", &value_map);
    let value_files =
        value_actual.expect("complete successful target ValueMap artifact result retained");
    let value_source = std::str::from_utf8(
        &value_files
            .files()
            .iter()
            .find(|file| file.path.as_str() == "GeneratedMapping.cs")
            .expect("successful public artifact contains mapping")
            .contents,
    )
    .expect("complete emitted mapping is UTF-8");
    assert_eq!(
        value_source
            .matches("var input_2 = Node_1(context);")
            .count(),
        1
    );
    assert!(value_source.contains("FerruleValueMaps.Apply("));
    assert!(value_source.contains("public static ExecutionOutputs ExecuteWithBasicOwnedSources("));
    let mut controlled = basic(
        vec![
            expression(
                1,
                field(SourceOwner::Primary, &["Rows", "Value"], Some(&["Rows"])),
            ),
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
    controlled.root.iteration = Some(IterationPlan::new(
        codegen::OwnedSourceIteration::new(OwnedSourceRef {
            owner: SourceOwner::Primary,
            path: path(&["Rows"]),
        }),
        Some(2),
        None,
        vec![],
        codegen::IterationOutput::Repeated,
    ));
    let control_actual = public_basic_result("S519-C03", &controlled);
    let route = ::mapping::SourceScopeLocation::root(::mapping::SourceScopeTarget::Primary);
    let control_expected = ProgramValidationError::OwnedSource {
        diagnostic: Box::new(D::Capability {
            location: L::Iteration {
                scope: route.clone(),
            },
            request: Some(Q::Iteration {
                owner: W::Primary,
                path: path(&["Rows"]),
            }),
            reason: R::ScopeControl,
            origin: O::Scope { scope: route },
        }),
    };
    eprintln!("EXPECTED S519-C03 complete independent diagnostic={control_expected:#?}");
    assert_eq!(extract(control_actual), control_expected);
    eprintln!("INPUT S519-C04 ordinary family complete Program={unknown:#?}");
    let old = crate::emit(&unknown);
    eprintln!("RAW S519-C04 ordinary family full Result={old:#?}");
    let old_expected = ProgramValidationError::OwnedSource {
        diagnostic: Box::new(D::Capability {
            location: L::Expression {
                namespace: N::Main,
                node: 1,
                scope: None,
            },
            request: Some(Q::Field {
                owner: W::Primary,
                path: path(&["Value"]),
                frame: None,
            }),
            reason: R::UnsupportedBasicContext,
            origin: O::RetainedExpression {
                namespace: N::Main,
                node: 1,
            },
        }),
    };
    assert_eq!(extract(old), old_expected);
    let mut legacy = unknown;
    legacy.expressions[0].expression = Expression::SourceField {
        frame: None,
        path: path(&["Value"]),
    };
    eprintln!("INPUT S519-C04 legacy complete Program={legacy:#?}");
    let ordinary = crate::emit(&legacy);
    eprintln!("RAW S519-C04 legacy full artifact Result={ordinary:#?}");
    let ordinary = ordinary.expect("complete old legacy output retained");
    let mapping = String::from_utf8(
        ordinary
            .files()
            .iter()
            .find(|file| file.path.as_str() == "GeneratedMapping.cs")
            .unwrap()
            .contents
            .clone(),
    )
    .unwrap();
    assert!(mapping.contains("public static ExecutionOutputs ExecuteOutputs("));
    assert!(!mapping.contains("ExecuteWithBasicOwnedSources("));
}

const PUBLIC_INPUT_ORDER: &str = r#"
var input=Primary();var limits=Policy();var selection=FerruleTargetSelection.Primary();
var p0=new Probe(Array.Empty<NamedInput>());var a=Reach("C01.1",input,p0,limits,null,null,()=>GeneratedMapping.ExecuteWithBasicOwnedSources(input,p0,limits,null));
var p1=new Probe(Array.Empty<NamedInput>());var b=Reach("C01.2",input,p1,limits,null,selection,()=>GeneratedMapping.ExecuteSelectedWithBasicOwnedSources(input,selection,p1,limits,null));
var missing=FerruleTargetSelection.Named("missing");var zero=new FerruleOwnedSourceLimits(0,0,0,0,0);var p2=new Probe(new[]{new NamedInput("X",S("x"))},throwCount:true);var c=Reach("C01.3",input,p2,zero,null,missing,()=>GeneratedMapping.ExecuteSelectedWithBasicOwnedSources(input,missing,p2,zero,null));
var p3=new Probe(new[]{new NamedInput("X",S("x"))});var d=Reach("C01.4",input,p3,limits,null,null,()=>GeneratedMapping.ExecuteWithBasicOwnedSources(input,p3,limits,null));
var p4=new Probe(new[]{new NamedInput("Ref",S("a")),new NamedInput("Ref",S("b"))});var e=Reach("C01.5",input,p4,limits,null,null,()=>GeneratedMapping.ExecuteWithBasicOwnedSources(input,p4,limits,null));
var p5=new Probe(new NamedInput[]{null!});var f=Reach("C01.6",input,p5,limits,null,null,()=>GeneratedMapping.ExecuteWithBasicOwnedSources(input,p5,limits,null));
var p6=new Probe(new[]{new NamedInput("Ref",S("a"))},throwCount:true);var g=Reach("C01.7",input,p6,limits,null,null,()=>GeneratedMapping.ExecuteWithBasicOwnedSources(input,p6,limits,null));
var p7=new Probe(new[]{new NamedInput("Ref",S("a"))},throwIndex:true);var h=Reach("C01.8",input,p7,limits,null,null,()=>GeneratedMapping.ExecuteWithBasicOwnedSources(input,p7,limits,null));
Tree(a,"{\"Value\":\"P.root\"}");Tree(b,"{\"Value\":\"P.root\"}");Check(((ExecutionOutputs)a.Result!).Extras.Count==0,"all extras empty");
InputError(c,FerruleRuntimeError.UnknownTarget,"missing","project has no named target `missing`");Check(!c.CallerIdentity,"selected validation precedes caller sentinel");
InputError(d,FerruleRuntimeError.UnexpectedNamedSource,"X","named source is not declared by this mapping");InputError(e,FerruleRuntimeError.DuplicateNamedSource,"Ref","named source was supplied more than once");
Check(f.Error is ArgumentNullException n && n.ParamName=="supplied" && n.InnerException is null && f.Result is null,"null record retains ArgumentNullException supplied");
Check(g.CallerIdentity && ReferenceEquals(g.Error,p6.Failure) && g.Result is null,"exact Count caller exception, no partial output");Check(h.CallerIdentity && ReferenceEquals(h.Error,p7.Failure) && h.Result is null,"exact Index caller exception, no partial output");
Reads(a,1,0);Reads(b,1,0);Reads(c,0,0);Reads(d,1,1);Reads(e,1,2);Reads(f,1,1);Reads(g,1,0);Reads(h,1,1);
"#;

const PUBLIC_LAZY_OWNER: &str = r#"
var input=Primary();var limits=Policy();var t=new[]{new KeyValuePair<string,FerruleValue>("flag",FerruleValue.FromBoolean(true))};var f=new[]{new KeyValuePair<string,FerruleValue>("flag",FerruleValue.FromBoolean(false))};
var yes=FerruleExecutionContext.WithParameters("mapping.test",new FerruleRuntimeParameters(t));var no=FerruleExecutionContext.WithParameters("mapping.test",new FerruleRuntimeParameters(f));
var p0=new Probe(Array.Empty<NamedInput>());var a=Reach("C02.1",input,p0,limits,yes,null,()=>GeneratedMapping.ExecuteWithBasicOwnedSources(input,p0,limits,yes),t);
var p1=new Probe(Array.Empty<NamedInput>());var b=Reach("C02.2",input,p1,limits,no,null,()=>GeneratedMapping.ExecuteWithBasicOwnedSources(input,p1,limits,no),f);
var p2=new Probe(new[]{new NamedInput("Ref",G(("Value",S("N.root"))))});var c=Reach("C02.3",input,p2,limits,no,null,()=>GeneratedMapping.ExecuteWithBasicOwnedSources(input,p2,limits,no),f);
Tree(a,"{\"Value\":\"P.root\"}");OwnedAbsent(b,"Ref",3U);Tree(c,"{\"Value\":\"N.root\"}");Reads(a,1,0);Reads(b,1,0);Reads(c,1,1);
"#;

const PUBLIC_P19_SELECTED: &str = r#"
var input=Primary();var limits=Policy();var selection=FerruleTargetSelection.Primary();var p0=new Probe(Array.Empty<NamedInput>());var a=Reach("C03.1",input,p0,limits,null,selection,()=>GeneratedMapping.ExecuteSelectedWithBasicOwnedSources(input,selection,p0,limits,null));
var p1=new Probe(Array.Empty<NamedInput>());var b=Reach("C03.2",input,p1,limits,null,null,()=>GeneratedMapping.ExecuteWithBasicOwnedSources(input,p1,limits,null));
Tree(a,"{\"Value\":\"P.root\"}");var e=Runtime(b,FerruleRuntimeError.MappingException);Check(e.Node==2u && e.MappingExceptionMessage is null && e.Message=="node 2: mapping exception: mapping exception was raised" && e.InnerException is null,"complete ordinary Raise");NullMetadata(e,"Node");Reads(a,1,0);Reads(b,1,0);
"#;

const PUBLIC_GLOBAL_ORDER: &str = r#"
var input=Primary();var limits=Policy();var namedRoot=G(("Rows",R(G(("Value",S("N1")),("Score",I(30))),G(("Value",S("N2")),("Score",I(10))))));var primary=FerruleTargetSelection.Primary();var named=FerruleTargetSelection.Named("named");
var p0=new Probe(new[]{new NamedInput("ref",namedRoot)});var a=Reach("C04.1",input,p0,limits,null,null,()=>GeneratedMapping.ExecuteWithBasicOwnedSources(input,p0,limits,null));
var p1=new Probe(new[]{new NamedInput("ref",namedRoot)});var b=Reach("C04.2",input,p1,limits,null,primary,()=>GeneratedMapping.ExecuteSelectedWithBasicOwnedSources(input,primary,p1,limits,null));
var p2=new Probe(new[]{new NamedInput("ref",namedRoot)});var c=Reach("C04.3",input,p2,limits,null,named,()=>GeneratedMapping.ExecuteSelectedWithBasicOwnedSources(input,named,p2,limits,null));
var p3=new Probe(Array.Empty<NamedInput>());var d=Reach("C04.4",input,p3,limits,null,primary,()=>GeneratedMapping.ExecuteSelectedWithBasicOwnedSources(input,primary,p3,limits,null));
var p4=new Probe(new[]{new NamedInput("X",S("x")),new NamedInput("ref",namedRoot)});var e=Reach("C04.5",input,p4,limits,null,null,()=>GeneratedMapping.ExecuteWithBasicOwnedSources(input,p4,limits,null));
Failure(a,2,"N1");Failure(b,2,"N1");Failure(c,2,"N1");InputError(d,FerruleRuntimeError.MissingNamedSource,"ref","named source is required by this mapping");InputError(e,FerruleRuntimeError.UnexpectedNamedSource,"X","named source is not declared by this mapping");Reads(a,1,1);Reads(b,1,1);Reads(c,1,1);Reads(d,1,0);Reads(e,1,1);
"#;

const PUBLIC_GLOBAL_POSITION: &str = r#"
var input=Primary();var limits=Policy();var primary=FerruleTargetSelection.Primary();var named=FerruleTargetSelection.Named("named");
var p0=new Probe(Array.Empty<NamedInput>());var a=Reach("C05.1",input,p0,limits,null,null,()=>GeneratedMapping.ExecuteWithBasicOwnedSources(input,p0,limits,null));
var p1=new Probe(Array.Empty<NamedInput>());var b=Reach("C05.2",input,p1,limits,null,primary,()=>GeneratedMapping.ExecuteSelectedWithBasicOwnedSources(input,primary,p1,limits,null));
var p2=new Probe(Array.Empty<NamedInput>());var c=Reach("C05.3",input,p2,limits,null,named,()=>GeneratedMapping.ExecuteSelectedWithBasicOwnedSources(input,named,p2,limits,null));
Failure(a,1,"2");Failure(b,1,"2");Failure(c,1,"2");Reads(a,1,0);Reads(b,1,0);Reads(c,1,0);
"#;

const PUBLIC_OWNED_WALK: &str = r#"
var input=Primary();var limits=Policy();var root=G(("Rows",R(G(("Value",S("N1"))),G(("Value",S("N2"))))));var selection=FerruleTargetSelection.Primary();
var p0=new Probe(new[]{new NamedInput("Ref",root)});var a=Reach("C06.1",input,p0,limits,null,null,()=>GeneratedMapping.ExecuteWithBasicOwnedSources(input,p0,limits,null));
var p1=new Probe(new[]{new NamedInput("Ref",root)});var b=Reach("C06.2",input,p1,limits,null,selection,()=>GeneratedMapping.ExecuteSelectedWithBasicOwnedSources(input,selection,p1,limits,null));
Tree(a,"[{\"Value\":\"N1\",\"Position\":1},{\"Value\":\"N2\",\"Position\":2}]");Tree(b,"[{\"Value\":\"N1\",\"Position\":1},{\"Value\":\"N2\",\"Position\":2}]");Check(((ExecutionOutputs)a.Result!).Extras.Count==0,"complete all extras empty");Reads(a,1,1);Reads(b,1,1);
"#;

const PUBLIC_SCALAR_ZERO: &str = r#"
var input=S("P.root");var supplied=new Probe(Array.Empty<NamedInput>());var limits=new FerruleOwnedSourceLimits(1,0,0,0,0);var a=Reach("C07.1",input,supplied,limits,null,null,()=>GeneratedMapping.ExecuteWithBasicOwnedSources(input,supplied,limits,null));
Tree(a,"{\"Value\":\"P.root\"}");Check(((ExecutionOutputs)a.Result!).Extras.Count==0,"empty catalogue/all extras");Reads(a,1,0);
"#;

// #566 target ValueMap only: the shared Basic-aware ordinary renderer remains
// authoritative. Defaults/table entries are literal Values, never graph nodes.
fn target_value_map(
    input: u32,
    input_type: Option<ScalarType>,
    table: Vec<(Value, Value)>,
    default: Option<Value>,
) -> Expression {
    Expression::ValueMap {
        input,
        input_type,
        table,
        default,
    }
}
fn mapped_string(value: &str) -> Value {
    Value::String(value.into())
}

#[test]
fn private_target_value_map_ordered_owned_null_and_literal_default_host() {
    let p = basic(
        vec![
            expression(
                1,
                Expression::SourceField {
                    path: path(&["Value"]),
                    frame: None,
                },
            ),
            expression(
                2,
                target_value_map(
                    1,
                    None,
                    vec![
                        (mapped_string("P.root"), mapped_string("first")),
                        (mapped_string("P.root"), mapped_string("second")),
                    ],
                    Some(mapped_string("miss")),
                ),
            ),
            expression(3, field(SourceOwner::Primary, &["Value"], None)),
            expression(
                4,
                target_value_map(
                    3,
                    None,
                    vec![(mapped_string("P.root"), mapped_string("owned"))],
                    None,
                ),
            ),
            expression(5, field(SourceOwner::Primary, &["Score"], None)),
            expression(
                6,
                target_value_map(
                    5,
                    None,
                    vec![(Value::Null, mapped_string("absent"))],
                    Some(mapped_string("wrong")),
                ),
            ),
            expression(
                7,
                Expression::Const {
                    value: Value::json_null(),
                },
            ),
            expression(
                8,
                target_value_map(
                    7,
                    None,
                    vec![
                        (Value::Null, mapped_string("wrong")),
                        (Value::json_null(), mapped_string("json")),
                    ],
                    None,
                ),
            ),
            expression(
                9,
                target_value_map(
                    7,
                    Some(ScalarType::Int),
                    vec![
                        (Value::Null, mapped_string("null")),
                        (Value::json_null(), mapped_string("wrong")),
                    ],
                    None,
                ),
            ),
            expression(10, Expression::Const { value: Value::Null }),
            expression(
                11,
                target_value_map(
                    10,
                    None,
                    vec![
                        (Value::Null, mapped_string("null")),
                        (Value::json_null(), mapped_string("wrong")),
                    ],
                    None,
                ),
            ),
            expression(
                12,
                Expression::Const {
                    value: mapped_string("miss"),
                },
            ),
            expression(
                13,
                target_value_map(12, None, vec![], Some(mapped_string("fallback"))),
            ),
            expression(14, target_value_map(12, None, vec![], None)),
            expression(
                15,
                target_value_map(
                    14,
                    None,
                    vec![(Value::Null, mapped_string("no-default-null"))],
                    Some(mapped_string("wrong")),
                ),
            ),
            expression(16, target_value_map(12, None, vec![], Some(Value::Null))),
            expression(
                17,
                target_value_map(
                    16,
                    None,
                    vec![(Value::Null, mapped_string("literal-null"))],
                    Some(mapped_string("wrong")),
                ),
            ),
        ],
        group(vec![
            binding("Legacy", 2, ScalarType::String),
            binding("Owned", 4, ScalarType::String),
            binding("Absent", 6, ScalarType::String),
            binding("UntypedJson", 8, ScalarType::String),
            binding("TypedJson", 9, ScalarType::String),
            binding("Null", 11, ScalarType::String),
            binding("Default", 13, ScalarType::String),
            binding("NoDefault", 15, ScalarType::String),
            binding("LiteralNull", 17, ScalarType::String),
        ]),
    );
    let source = retain_checked_host(
        "V566-ordered-null-default",
        &p,
        r#"
Input=G(("Value",S("P.root")));Limits=new(16,8,16,1048576,64);var probe=new CheckedProbe(Array.Empty<NamedInput>());
var a=CheckedReach("V566-01",probe,()=>ExecuteCheckedOwnedBasic(Input,probe,Limits));
ExpectTree(a.Value,"{\"Legacy\":\"first\",\"Owned\":\"owned\",\"Absent\":\"absent\",\"UntypedJson\":\"json\",\"TypedJson\":\"null\",\"Null\":\"null\",\"Default\":\"fallback\",\"NoDefault\":\"no-default-null\",\"LiteralNull\":\"literal-null\"}");ExpectReads(a,1,0,1,0);Check(a.Value.Nodes.SequenceEqual(new uint[]{2,1,4,3,6,5,8,7,9,7,11,10,13,12,15,14,12,17,16,12}),"one input per map; first exact row, distinct null tags, literal defaults");
"#,
    );
    assert!(source.contains("FerruleValueMaps.Apply("));
    assert!(!source.contains("FerruleValueMaps.ApplyUserFunction("));
    assert!(source.contains("var input_2 = Node_1(context);"));
    assert!(
        source.contains("if (input_9.Kind == global::Ferrule.Runtime.FerruleValueKind.JsonNull)")
    );
    assert!(
        !source.contains("if (input_8.Kind == global::Ferrule.Runtime.FerruleValueKind.JsonNull)")
    );
}

#[test]
fn private_target_value_map_numeric_coercion_and_original_tag_host() {
    let rows = [
        (
            Value::String(" 7 ".into()),
            Some(ScalarType::Int),
            Value::Int(7),
            "TrimInt",
            "trim-int",
            "wrong",
        ),
        (
            Value::Float(7.0),
            Some(ScalarType::Int),
            Value::Int(7),
            "FloatInt",
            "float-int",
            "wrong",
        ),
        (
            Value::Int(7),
            Some(ScalarType::Float),
            Value::Float(7.0),
            "IntFloat",
            "int-float",
            "wrong",
        ),
        (
            mapped_string("bad"),
            Some(ScalarType::Int),
            mapped_string("bad"),
            "RetainedString",
            "retained-string",
            "wrong",
        ),
        (
            Value::Bool(true),
            Some(ScalarType::String),
            mapped_string("true"),
            "BoolString",
            "bool-string",
            "wrong",
        ),
        (
            mapped_string(" true "),
            Some(ScalarType::Bool),
            Value::Bool(true),
            "TrimBool",
            "trim-bool",
            "wrong",
        ),
        (
            Value::Int(7),
            None,
            Value::Float(7.0),
            "TaggedInt",
            "wrong",
            "tagged-int",
        ),
        (
            Value::Float(1.5),
            Some(ScalarType::Int),
            Value::Float(1.5),
            "RetainedFloat",
            "retained-float",
            "wrong",
        ),
    ];
    let mut expressions = Vec::new();
    let mut bindings = Vec::new();
    for (index, (input, ty, key, name, output, default)) in rows.into_iter().enumerate() {
        let input_id = u32::try_from(index * 2 + 1).expect("eight frozen literal rows");
        expressions.push(expression(input_id, Expression::Const { value: input }));
        expressions.push(expression(
            input_id + 1,
            target_value_map(
                input_id,
                ty,
                vec![(key, mapped_string(output))],
                Some(mapped_string(default)),
            ),
        ));
        bindings.push(binding(name, input_id + 1, ScalarType::String));
    }
    let p = basic(expressions, group(bindings));
    retain_checked_host(
        "V566-numeric",
        &p,
        r#"
Input=G(("Value",S("P.root")));Limits=new(16,8,16,1048576,64);var probe=new CheckedProbe(Array.Empty<NamedInput>());
var a=CheckedReach("V566-02",probe,()=>ExecuteCheckedOwnedBasic(Input,probe,Limits));
ExpectTree(a.Value,"{\"TrimInt\":\"trim-int\",\"FloatInt\":\"float-int\",\"IntFloat\":\"int-float\",\"RetainedString\":\"retained-string\",\"BoolString\":\"bool-string\",\"TrimBool\":\"trim-bool\",\"TaggedInt\":\"tagged-int\",\"RetainedFloat\":\"retained-float\"}");ExpectReads(a,1,0,1,0);Check(a.Value.Nodes.SequenceEqual(new uint[]{2,1,4,3,6,5,8,7,10,9,12,11,14,13,16,15}),"optional ordinary coercion, failed coercion retains original tag, no-type exact Int/Double distinction");
"#,
    );
}

#[test]
fn private_target_value_map_lazy_conditional_owner_host() {
    let p = basic(
        vec![
            expression(
                1,
                Expression::RuntimeParameter {
                    name: "flag".into(),
                    ty: ScalarType::Bool,
                },
            ),
            expression(2, field(SourceOwner::Primary, &["Value"], None)),
            expression(
                3,
                target_value_map(
                    2,
                    None,
                    vec![(mapped_string("P.root"), mapped_string("primary"))],
                    Some(mapped_string("fallback")),
                ),
            ),
            expression(4, field(named("Ref"), &["Value"], None)),
            expression(
                5,
                target_value_map(
                    4,
                    None,
                    vec![(mapped_string("N.root"), mapped_string("named"))],
                    Some(mapped_string("fallback")),
                ),
            ),
            expression(
                6,
                Expression::If {
                    condition: 1,
                    then: 3,
                    else_: 5,
                },
            ),
        ],
        group(vec![binding("Value", 6, ScalarType::String)]),
    );
    retain_checked_host(
        "V566-lazy",
        &p,
        r#"
Input=G(("Value",S("P.root")));Limits=new(16,8,16,1048576,64);ParameterValues=new[]{new KeyValuePair<string,FerruleValue>("flag",FerruleValue.FromBoolean(true))};Execution=FerruleExecutionContext.WithParameters("mapping.test",new FerruleRuntimeParameters(ParameterValues));var probe=new CheckedProbe(Array.Empty<NamedInput>());
var a=CheckedReach("V566-03",probe,()=>ExecuteCheckedOwnedBasic(Input,probe,Limits,Execution));
ParameterValues=new[]{new KeyValuePair<string,FerruleValue>("flag",FerruleValue.FromBoolean(false))};Execution=FerruleExecutionContext.WithParameters("mapping.test",new FerruleRuntimeParameters(ParameterValues));probe=new CheckedProbe(Array.Empty<NamedInput>());
var b=CheckedReach("V566-04",probe,()=>ExecuteCheckedOwnedBasic(Input,probe,Limits,Execution));
probe=new CheckedProbe(new[]{new NamedInput("Ref",G(("Value",S("N.root"))))});
var c=CheckedReach("V566-05",probe,()=>ExecuteCheckedOwnedBasic(Input,probe,Limits,Execution));
ExpectTree(a.Value,"{\"Value\":\"primary\"}");ExpectReads(a,1,0,1,0);Check(a.Value.Nodes.SequenceEqual(new uint[]{6,1,3,2}),"unchosen missing named input never evaluated");
ExpectOwned(b.Value,FerruleOwnedSourceErrorKind.SourceUnavailable,false,"Ref",null);ExpectReads(b,1,0,1,0);Check(b.Value.Nodes.SequenceEqual(new uint[]{6,1,5,4}),"input owner failure precedes table/default, no rescue or partial output");
ExpectTree(c.Value,"{\"Value\":\"named\"}");ExpectReads(c,1,1,1,1);Check(c.Value.Nodes.SequenceEqual(new uint[]{6,1,5,4}),"same false branch succeeds with exact optional Ref supplied");
"#,
    );
}

#[test]
fn private_target_value_map_input_errors_precede_literal_default_hosts() {
    let raise = basic(
        vec![
            expression(1, Expression::Raise { message: None }),
            expression(
                2,
                target_value_map(1, None, vec![], Some(mapped_string("fallback"))),
            ),
        ],
        group(vec![binding("Value", 2, ScalarType::String)]),
    );
    retain_checked_host(
        "V566-raise",
        &raise,
        r#"
Input=G(("Value",S("P.root")));Limits=new(16,8,16,1048576,64);var probe=new CheckedProbe(Array.Empty<NamedInput>());
var a=CheckedReach("V566-06",probe,()=>ExecuteCheckedOwnedBasic(Input,probe,Limits));
var e=ExpectRuntime(a.Value,FerruleRuntimeError.MappingException);Check(e.Node==1 && e.MappingExceptionMessage is null && e.Message=="node 1: mapping exception: mapping exception was raised" && e.InnerException is null && a.Value.Result is null,"complete existing Raise metadata precedes literal default");ExpectReads(a,1,0,1,0);Check(a.Value.Nodes.SequenceEqual(new uint[]{2,1}),"map then failing input once, no callback for a literal default");Check(e.Function is null && e.ExpectedArity is null && e.ActualArity is null && e.FoundKind is null && e.AggregateOperation is null && e.Detail is null && e.RequestedItems is null && e.MaximumItems is null && e.MaximumDepth is null && e.RuntimeValue is null && e.FailureRule is null && e.MappingFailureMessage is null && e.Join is null && e.UserFunction is null && e.FunctionParameter is null && e.ExpectedScalarType is null && e.SourceField is null && e.FoundInstance is null && e.PrimaryRoot is null && e.OwnedSourceFailure is null && e.BasicSourceBridgeFailure is null && e.RuntimeParameter is null,"complete unchanged Raise outer metadata");
"#,
    );
    let parameter = basic(
        vec![
            expression(
                1,
                Expression::RuntimeParameter {
                    name: "value".into(),
                    ty: ScalarType::Int,
                },
            ),
            expression(
                2,
                target_value_map(
                    1,
                    None,
                    vec![(Value::Int(7), mapped_string("seven"))],
                    Some(mapped_string("fallback")),
                ),
            ),
        ],
        group(vec![binding("Value", 2, ScalarType::String)]),
    );
    retain_checked_host(
        "V566-parameter",
        &parameter,
        r#"
Input=G(("Value",S("P.root")));Limits=new(16,8,16,1048576,64);ParameterValues=Array.Empty<KeyValuePair<string,FerruleValue>>();Execution=FerruleExecutionContext.WithParameters("mapping.test",new FerruleRuntimeParameters(ParameterValues));var probe=new CheckedProbe(Array.Empty<NamedInput>());
var a=CheckedReach("V566-07",probe,()=>ExecuteCheckedOwnedBasic(Input,probe,Limits,Execution));
var e=ExpectRuntime(a.Value,FerruleRuntimeError.MissingRuntimeParameter);Check(e.Node==1 && e.RuntimeParameter=="value" && e.Message=="node 1: execution context does not provide runtime parameter 'value'" && e.FoundKind is null && e.ExpectedScalarType is null && e.InnerException is null && a.Value.Result is null,"missing input parameter precedes table/default without altered error");ExpectReads(a,1,0,1,0);Check(a.Value.Nodes.SequenceEqual(new uint[]{2,1}),"one map/input call, no default node exists");Check(e.Function is null && e.ExpectedArity is null && e.ActualArity is null && e.FoundKind is null && e.AggregateOperation is null && e.Detail is null && e.RequestedItems is null && e.MaximumItems is null && e.MaximumDepth is null && e.RuntimeValue is null && e.FailureRule is null && e.MappingFailureMessage is null && e.Join is null && e.UserFunction is null && e.FunctionParameter is null && e.ExpectedScalarType is null && e.SourceField is null && e.FoundInstance is null && e.PrimaryRoot is null && e.OwnedSourceFailure is null && e.BasicSourceBridgeFailure is null && e.MappingExceptionMessage is null,"complete unchanged missing-parameter outer metadata");
"#,
    );
}

#[test]
fn private_target_value_map_retains_legacy_input_catalogues_and_dynamic_guard() {
    let mut p = basic(
        vec![
            expression(
                1,
                Expression::SourceField {
                    path: path(&["Unused", "Value"]),
                    frame: None,
                },
            ),
            expression(
                2,
                target_value_map(1, None, vec![], Some(mapped_string("fallback"))),
            ),
            expression(
                99,
                Expression::Const {
                    value: mapped_string("unused.json"),
                },
            ),
        ],
        group(vec![binding("Value", 2, ScalarType::String)]),
    );
    p.extra_sources[0].dynamic = Some(codegen::DynamicSourceProgram {
        path: 99,
        driver: codegen::SourceIteration::new(vec![]),
    });
    let refused = checked_result("S566-01 retained ValueMap input", &p);
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
    eprintln!("EXPECTED S566-01 complete independent typed diagnostic={expected:#?}");
    match refused {
        Err(EmitError::ProgramValidation(actual)) => assert_eq!(actual, expected),
        other => panic!("retained raw S566-01 result differs: {other:#?}"),
    }
    p.expressions[0].expression = Expression::SourceField {
        path: path(&["Unused", "Value"]),
        frame: Some(vec![]),
    };
    let empty_frame = checked_result("S566-02 exact empty-frame precedence", &p);
    assert!(
        empty_frame.is_ok(),
        "existing empty frame replaces path and remains exact"
    );

    let p = basic(
        vec![
            expression(
                1,
                Expression::SourceField {
                    path: path(&["ref", "Value"]),
                    frame: None,
                },
            ),
            expression(
                2,
                target_value_map(
                    1,
                    None,
                    vec![(mapped_string("N.root"), mapped_string("legacy-named"))],
                    Some(mapped_string("fallback")),
                ),
            ),
        ],
        group(vec![binding("Value", 2, ScalarType::String)]),
    );
    let raw_plan = codegen::legacy_named_input_admission_plan(&p);
    eprintln!("INPUT S566 exact Program={p:#?} RAW complete catalogue plan={raw_plan:#?}");
    let plan = raw_plan.expect("full catalogue Result retained before comparison");
    let all = checked_rows(&plan.all_outputs);
    let primary = checked_rows(&plan.primary);
    use codegen::NamedInputAvailability::{Optional as O, RequiredLegacy as R};
    let expected = vec![
        (0, "Unused", O),
        (1, "Ref", O),
        (2, " Ref 雪 ", O),
        (3, "ref", R),
        (4, "primary", O),
    ];
    eprintln!(
        "RAW full all={all:#?} primary={primary:#?} named={:#?} EXPECTED={expected:#?}",
        plan.named_targets
    );
    assert_eq!(all, expected);
    assert_eq!(primary, expected);
    assert!(plan.named_targets.is_empty());
    retain_checked_host(
        "V566-legacy-named",
        &p,
        r#"
Input=G(("Value",S("P.root")));Limits=new(16,8,16,1048576,64);var probe=new CheckedProbe(new[]{new NamedInput("ref",G(("Value",S("N.root"))))});
var a=CheckedReach("V566-08",probe,()=>ExecuteCheckedOwnedBasic(Input,probe,Limits));
ExpectTree(a.Value,"{\"Value\":\"legacy-named\"}");ExpectReads(a,1,1,1,1);Check(a.Value.Nodes.SequenceEqual(new uint[]{2,1}),"ordinary named input remains eager-required through target ValueMap input");
"#,
    );
}

#[test]
fn private_target_value_map_failure_and_public_gates_remain_closed() {
    let mut failure = failure_program(vec![
        expression(
            90,
            Expression::Const {
                value: Value::Bool(false),
            },
        ),
        expression(
            100,
            target_value_map(
                90,
                None,
                vec![(Value::Bool(false), Value::Bool(false))],
                Some(Value::Bool(true)),
            ),
        ),
    ]);
    failure.failure_rules.push(source_failure(
        &["Rows"],
        codegen::FailureSelection::WhenTrue(100),
        None,
    ));
    let actual = render_basic_owned_mapping(&failure);
    eprintln!(
        "INPUT S566-03 complete Program={failure:#?} RAW complete source Result={actual:#?} EXPECTED private failure ValueMap admission"
    );
    assert!(
        actual.is_ok(),
        "S566-03 complete source result retained before private ValueMap admission comparison"
    );
    let p = basic(
        vec![
            expression(1, field(SourceOwner::Primary, &["Value"], None)),
            expression(
                2,
                target_value_map(1, None, vec![], Some(mapped_string("fallback"))),
            ),
        ],
        group(vec![binding("Value", 2, ScalarType::String)]),
    );
    let actual = codegen::validate_owned_emission(&p);
    let expected = ProgramValidationError::OwnedSource {
        diagnostic: Box::new(codegen::OwnedSourceDiagnostic::Capability {
            location: codegen::OwnedSourceDiagnosticLocation::Expression {
                namespace: codegen::OwnedSourceNamespace::Main,
                node: 1,
                scope: None,
            },
            request: Some(codegen::OwnedSourceDiagnosticRequest::Field {
                owner: codegen::OwnedSourceDiagnosticOwner::Primary,
                path: path(&["Value"]),
                frame: None,
            }),
            reason: codegen::OwnedSourceCapabilityReason::UnsupportedBasicContext,
            origin: codegen::OwnedSourceCapabilityOrigin::RetainedExpression {
                namespace: codegen::OwnedSourceNamespace::Main,
                node: 1,
            },
        }),
    };
    eprintln!(
        "INPUT S566-04 complete Program={p:#?} RAW full public gate Result={actual:#?} EXPECTED={expected:#?}"
    );
    assert_eq!(actual, Err(expected), "public emitter gate remains closed");
}

fn retained585_result(id: &str, p: &Program) -> Result<String, EmitError> {
    use std::error::Error as _;
    eprintln!("INPUT {id} complete Program={p:#?}");
    let actual = render_basic_owned_mapping(p);
    eprintln!("RAW {id} complete Result={actual:#?}");
    if let Err(error) = &actual {
        eprintln!(
            "RAW {id} complete cause={:?}",
            error.source().map(|cause| format!("{cause:#?}"))
        );
    }
    actual
}
fn retained585_extract(actual: Result<String, EmitError>) -> ProgramValidationError {
    match actual {
        Err(EmitError::ProgramValidation(error)) => error,
        other => panic!("complete retained inventory refusal expected: {other:#?}"),
    }
}
fn retained585_function(id: ::mapping::FunctionId, output: u32) -> codegen::UserFunctionProgram {
    codegen::UserFunctionProgram {
        id,
        library: "fixture".into(),
        name: format!("scalar_{}", id.get()),
        parameters: Vec::new(),
        output_type: ScalarType::String,
        expressions: vec![expression(
            output,
            Expression::Const {
                value: Value::String("function body".into()),
            },
        )],
        output,
    }
}
fn retained585_function_expected(id: ::mapping::FunctionId, output: u32) -> ProgramValidationError {
    ProgramValidationError::OwnedSource {
        diagnostic: Box::new(codegen::OwnedSourceDiagnostic::Capability {
            location: codegen::OwnedSourceDiagnosticLocation::Expression {
                namespace: codegen::OwnedSourceNamespace::Function(id),
                node: output,
                scope: None,
            },
            request: None,
            reason: codegen::OwnedSourceCapabilityReason::IsolatedFunction,
            origin: codegen::OwnedSourceCapabilityOrigin::FunctionBody { function: id },
        }),
    }
}
fn retained585_leaf_expected(
    node: u32,
    reason: codegen::OwnedSourceCapabilityReason,
) -> ProgramValidationError {
    ProgramValidationError::OwnedSource {
        diagnostic: Box::new(codegen::OwnedSourceDiagnostic::Capability {
            location: codegen::OwnedSourceDiagnosticLocation::Expression {
                namespace: codegen::OwnedSourceNamespace::Main,
                node,
                scope: None,
            },
            request: None,
            reason,
            origin: codegen::OwnedSourceCapabilityOrigin::RetainedExpression {
                namespace: codegen::OwnedSourceNamespace::Main,
                node,
            },
        }),
    }
}

#[test]
fn private_retained_inventory_omits_dead_ordinary_nodes_and_unreferenced_functions() {
    let mut baseline = basic(
        vec![expression(1, field(SourceOwner::Primary, &["Value"], None))],
        group(vec![binding("Value", 1, ScalarType::String)]),
    );
    baseline.extra_sources.push(NamedSourceProgram {
        name: "D".into(),
        source: source_schema("Dynamic"),
        dynamic: Some(codegen::DynamicSourceProgram {
            path: 98,
            driver: codegen::SourceIteration::new(path(&["Rows"])),
        }),
    });
    baseline.expressions.push(expression(
        98,
        Expression::Const {
            value: Value::String("unused.json".into()),
        },
    ));
    let original = retained585_result("S585-01 baseline", &baseline).unwrap();
    let mut p = baseline;
    p.expressions.extend([
        expression(90, Expression::SourceDocumentPath),
        expression(
            91,
            Expression::Call {
                function: ScalarFunction::Upper,
                args: vec![92],
            },
        ),
        expression(
            92,
            Expression::Const {
                value: Value::String("dead call input".into()),
            },
        ),
        expression(
            93,
            Expression::RuntimeParameterDefault {
                name: "Unused".into(),
                ty: ScalarType::String,
                default: 94,
            },
        ),
        expression(94, Expression::Raise { message: Some(95) }),
        expression(
            95,
            Expression::Const {
                value: Value::String("dead Raise message".into()),
            },
        ),
        expression(96, target_value_map(97, None, Vec::new(), None)),
        expression(
            97,
            Expression::SourceField {
                path: path(&["D", "Value"]),
                frame: None,
            },
        ),
        expression(99, field(named("Ref"), &["Value"], None)),
    ]);
    p.user_functions = vec![
        retained585_function(::mapping::FunctionId::new(7), 17),
        retained585_function(::mapping::FunctionId::new(9), 29),
    ];
    let before = p.clone();
    let actual = retained585_result("S585-02 dead valid nodes and functions", &p).unwrap();
    assert_eq!(actual, original);
    for id in 90..=99 {
        assert!(!actual.contains(&format!("FerruleValue Node_{id}(")));
        assert!(!actual.contains(&format!("FerruleValue NodeBody_{id}(")));
    }
    assert!(!actual.contains("UserFunction_"));
    assert!(!actual.contains("unused.json"));
    assert_eq!(p, before);
}

#[test]
fn private_retained_inventory_keeps_target_failure_and_potential_scalar_dependencies() {
    let mut root = group(vec![
        binding("Own", 1, ScalarType::String),
        binding("Value", 9, ScalarType::String),
    ]);
    root.children.push(TargetScope {
        target_field: "Child".into(),
        repeating: false,
        iteration: None,
        construction: TargetConstruction::Scalar {
            expression: 13,
            target_domain: ScalarType::String.into(),
        },
        bindings: Vec::new(),
        children: Vec::new(),
    });
    let mut p = basic(
        vec![
            expression(1, field(SourceOwner::Primary, &["Value"], None)),
            expression(
                2,
                Expression::Const {
                    value: Value::Bool(true),
                },
            ),
            expression(
                3,
                Expression::SourceField {
                    path: path(&["Value"]),
                    frame: None,
                },
            ),
            expression(
                4,
                Expression::Call {
                    function: ScalarFunction::Upper,
                    args: vec![3],
                },
            ),
            expression(
                5,
                Expression::If {
                    condition: 2,
                    then: 4,
                    else_: 6,
                },
            ),
            expression(
                6,
                Expression::RuntimeParameterDefault {
                    name: "Potential".into(),
                    ty: ScalarType::String,
                    default: 7,
                },
            ),
            expression(7, Expression::Raise { message: Some(8) }),
            expression(
                8,
                Expression::Const {
                    value: Value::String("potential error".into()),
                },
            ),
            expression(9, target_value_map(5, None, Vec::new(), Some(Value::Null))),
            expression(
                12,
                Expression::Const {
                    value: Value::String("named".into()),
                },
            ),
            expression(
                13,
                Expression::RuntimeValue {
                    value: RuntimeValue::MappingFilePath,
                },
            ),
            expression(
                21,
                Expression::Const {
                    value: Value::Bool(false),
                },
            ),
            expression(
                22,
                Expression::Call {
                    function: ScalarFunction::Upper,
                    args: vec![23],
                },
            ),
            expression(
                23,
                Expression::Const {
                    value: Value::String("lazy failure message".into()),
                },
            ),
            expression(24, Expression::SourceDocumentPath),
        ],
        root,
    );
    add_target(
        &mut p,
        "named",
        group(vec![binding("Value", 12, ScalarType::String)]),
    );
    p.failure_rules.push(source_failure(
        &[],
        codegen::FailureSelection::WhenTrue(21),
        Some(22),
    ));
    let before = p.clone();
    let actual =
        retained585_result("S585-03 complete roots and potential dependencies", &p).unwrap();
    for id in [1, 2, 3, 4, 5, 6, 7, 8, 9, 12, 13, 21, 22, 23] {
        assert!(
            actual.contains(&format!("FerruleValue Node_{id}(")),
            "retained node {id}"
        );
        assert!(
            actual.contains(&format!("FerruleValue NodeBody_{id}(")),
            "retained node body {id}"
        );
    }
    assert!(!actual.contains("FerruleValue Node_24("));
    assert!(actual.contains("case 1: result = new SelectedTargetOutput.Named("));
    assert!(actual.contains("EvaluateBasicFailureRules(context);"));
    assert_eq!(p, before);
}

#[test]
fn private_retained_inventory_rejects_exact_called_function_in_stored_call_order() {
    for target_call in [false, true] {
        for definitions_reversed in [false, true] {
            for calls_reversed in [false, true] {
                let mut p = basic(
                    vec![
                        expression(
                            1,
                            Expression::Const {
                                value: Value::String("primary".into()),
                            },
                        ),
                        expression(
                            2,
                            Expression::Const {
                                value: Value::Bool(true),
                            },
                        ),
                        expression(
                            40,
                            Expression::If {
                                condition: 2,
                                then: 1,
                                else_: 50,
                            },
                        ),
                    ],
                    group(vec![binding(
                        "Value",
                        if target_call { 40 } else { 1 },
                        ScalarType::String,
                    )]),
                );
                let first = retained585_function(::mapping::FunctionId::new(7), 17);
                let second = retained585_function(::mapping::FunctionId::new(9), 29);
                p.user_functions = if definitions_reversed {
                    vec![second, first]
                } else {
                    vec![first, second]
                };
                let first_call = expression(
                    50,
                    Expression::UserFunctionCall {
                        function: ::mapping::FunctionId::new(9),
                        args: Vec::new(),
                    },
                );
                let second_call = expression(
                    60,
                    Expression::UserFunctionCall {
                        function: ::mapping::FunctionId::new(7),
                        args: Vec::new(),
                    },
                );
                p.expressions.extend(if calls_reversed {
                    [second_call, first_call]
                } else {
                    [first_call, second_call]
                });
                let expected = if calls_reversed {
                    retained585_function_expected(::mapping::FunctionId::new(7), 17)
                } else {
                    retained585_function_expected(::mapping::FunctionId::new(9), 29)
                };
                let actual =
                    retained585_result("S585-04 exact referenced body and stored call policy", &p);
                eprintln!("EXPECTED S585-04 complete literal={expected:#?}");
                assert_eq!(retained585_extract(actual), expected);
            }
        }
    }
}

#[test]
fn private_retained_inventory_keeps_scope_failure_and_stored_main_priority() {
    use codegen::OwnedSourceCapabilityReason as Reason;
    for failure in [false, true] {
        for argument_first in [false, true] {
            let mut p = basic(
                vec![expression(
                    1,
                    Expression::Const {
                        value: Value::String("primary".into()),
                    },
                )],
                group(vec![binding("Value", 1, ScalarType::String)]),
            );
            let mut function = retained585_function(::mapping::FunctionId::new(9), 29);
            function.parameters.push(codegen::UserFunctionParameter {
                id: ::mapping::FunctionParameterId::new(1),
                ty: ScalarType::String,
            });
            p.user_functions.push(function);
            let argument = expression(90, Expression::SourceDocumentPath);
            let call = expression(
                50,
                Expression::UserFunctionCall {
                    function: ::mapping::FunctionId::new(9),
                    args: vec![90],
                },
            );
            p.expressions.extend(if argument_first {
                [argument, call]
            } else {
                [call, argument]
            });
            if failure {
                p.failure_rules.push(source_failure(
                    &[],
                    codegen::FailureSelection::All,
                    Some(90),
                ));
            }
            let expected = if failure {
                expected_failure_diagnostic(0, None, Reason::FailureRule)
            } else if argument_first {
                retained585_leaf_expected(90, Reason::Document)
            } else {
                retained585_function_expected(::mapping::FunctionId::new(9), 29)
            };
            let actual = retained585_result(
                "S585-05 failure before stored call and retained argument order",
                &p,
            );
            eprintln!("EXPECTED S585-05 complete literal={expected:#?}");
            assert_eq!(retained585_extract(actual), expected);
        }
    }
    let mut p = basic(
        vec![
            expression(
                1,
                Expression::Const {
                    value: Value::String("primary".into()),
                },
            ),
            expression(
                50,
                Expression::UserFunctionCall {
                    function: ::mapping::FunctionId::new(9),
                    args: Vec::new(),
                },
            ),
        ],
        group(vec![binding("Value", 1, ScalarType::String)]),
    );
    p.user_functions
        .push(retained585_function(::mapping::FunctionId::new(9), 29));
    p.root.iteration = Some(IterationPlan::new(
        codegen::SourceIteration::new(path(&["Rows"])),
        Some(50),
        None,
        Vec::new(),
        codegen::IterationOutput::Repeated,
    ));
    p.root.repeating = true;
    p.target = target_schema(&p.root, "Output");
    let route = ::mapping::SourceScopeLocation::root(::mapping::SourceScopeTarget::Primary);
    let expected = ProgramValidationError::OwnedSource {
        diagnostic: Box::new(codegen::OwnedSourceDiagnostic::Capability {
            location: codegen::OwnedSourceDiagnosticLocation::Iteration {
                scope: route.clone(),
            },
            request: None,
            reason: Reason::ScopeControl,
            origin: codegen::OwnedSourceCapabilityOrigin::Scope { scope: route },
        }),
    };
    let actual = retained585_result("S585-06 scope before stored function call", &p);
    eprintln!("EXPECTED S585-06 complete literal={expected:#?}");
    assert_eq!(retained585_extract(actual), expected);
}

#[test]
fn private_retained_inventory_keeps_unconditional_owned_structure_and_public_gates() {
    let mut p = basic(
        vec![
            expression(1, field(SourceOwner::Primary, &["Value"], None)),
            expression(99, field(named("Absent"), &["Value"], None)),
        ],
        group(vec![binding("Value", 1, ScalarType::String)]),
    );
    let expected = ProgramValidationError::OwnedSource {
        diagnostic: Box::new(codegen::OwnedSourceDiagnostic::Structure {
            location: codegen::OwnedSourceDiagnosticLocation::Expression {
                namespace: codegen::OwnedSourceNamespace::Main,
                node: 99,
                scope: None,
            },
            request: codegen::OwnedSourceDiagnosticRequest::Field {
                owner: codegen::OwnedSourceDiagnosticOwner::Named {
                    name: "Absent".into(),
                },
                path: path(&["Value"]),
                frame: None,
            },
            reason: codegen::OwnedSourceStructureReason::UnknownOwner,
        }),
    };
    let actual = retained585_result("S585-07 dead owned field remains structural", &p);
    assert_eq!(retained585_extract(actual), expected);
    p.expressions.pop();
    let mut function = retained585_function(::mapping::FunctionId::new(7), 17);
    function.expressions[0].expression = field(named("Absent"), &["Value"], None);
    p.user_functions.push(function);
    let expected = ProgramValidationError::OwnedSource {
        diagnostic: Box::new(codegen::OwnedSourceDiagnostic::Structure {
            location: codegen::OwnedSourceDiagnosticLocation::Expression {
                namespace: codegen::OwnedSourceNamespace::Function(::mapping::FunctionId::new(7)),
                node: 17,
                scope: None,
            },
            request: codegen::OwnedSourceDiagnosticRequest::Field {
                owner: codegen::OwnedSourceDiagnosticOwner::Named {
                    name: "Absent".into(),
                },
                path: path(&["Value"]),
                frame: None,
            },
            reason: codegen::OwnedSourceStructureReason::UnknownOwner,
        }),
    };
    let actual = retained585_result(
        "S585-08 unreferenced owned function body remains structural",
        &p,
    );
    assert_eq!(retained585_extract(actual), expected);
    p.user_functions[0] = retained585_function(::mapping::FunctionId::new(7), 17);
    p.user_functions[0].output = 18;
    let actual = retained585_result(
        "S585-09 malformed unreferenced ordinary function remains structural",
        &p,
    );
    assert_eq!(
        retained585_extract(actual),
        ProgramValidationError::MissingUserFunctionOutput {
            function: ::mapping::FunctionId::new(7),
            output: 18
        }
    );
    p.user_functions[0].output = 17;
    assert!(retained585_result("S585-10 valid unreferenced function is private inert", &p).is_ok());
    let public = public_basic_result("S585-11 unchanged public stored function gate", &p);
    eprintln!(
        "EXPECTED S585-11 complete literal={:#?}",
        retained585_function_expected(::mapping::FunctionId::new(7), 17)
    );
    match public {
        Err(EmitError::ProgramValidation(error)) => assert_eq!(
            error,
            retained585_function_expected(::mapping::FunctionId::new(7), 17)
        ),
        other => panic!("complete public stored function refusal expected: {other:#?}"),
    }
    p.user_functions.clear();
    p.expressions
        .push(expression(99, Expression::SourceDocumentPath));
    assert!(retained585_result("S585-12 dead ordinary document private inert", &p).is_ok());
    let public = public_basic_result("S585-13 unchanged public stored document gate", &p);
    let expected = retained585_leaf_expected(99, codegen::OwnedSourceCapabilityReason::Document);
    eprintln!("EXPECTED S585-13 complete literal={expected:#?}");
    match public {
        Err(EmitError::ProgramValidation(error)) => assert_eq!(error, expected),
        other => panic!("complete public stored document refusal expected: {other:#?}"),
    }
}

fn public582_map(
    input: u32,
    input_type: Option<ScalarType>,
    table: Vec<(Value, Value)>,
    default: Option<Value>,
) -> Expression {
    Expression::ValueMap {
        input,
        input_type,
        table,
        default,
    }
}
fn public582_matrix_program() -> Program {
    let mut p = basic(
        vec![
            expression(
                1,
                Expression::SourceField {
                    path: path(&["Value"]),
                    frame: None,
                },
            ),
            expression(
                2,
                public582_map(
                    1,
                    None,
                    vec![
                        (
                            Value::String("P.root".into()),
                            Value::String("first".into()),
                        ),
                        (
                            Value::String("P.root".into()),
                            Value::String("second".into()),
                        ),
                    ],
                    Some(Value::String("miss".into())),
                ),
            ),
            expression(3, field(SourceOwner::Primary, &["Value"], None)),
            expression(
                4,
                public582_map(
                    3,
                    None,
                    vec![(
                        Value::String("P.root".into()),
                        Value::String("owned".into()),
                    )],
                    None,
                ),
            ),
            expression(
                5,
                field(SourceOwner::Primary, &["Details", "Counter"], None),
            ),
            expression(
                6,
                public582_map(
                    5,
                    None,
                    vec![(Value::Null, Value::String("absent".into()))],
                    Some(Value::String("wrong".into())),
                ),
            ),
            expression(
                7,
                Expression::Const {
                    value: Value::json_null(),
                },
            ),
            expression(
                8,
                public582_map(
                    7,
                    None,
                    vec![
                        (Value::Null, Value::String("wrong".into())),
                        (Value::json_null(), Value::String("json".into())),
                    ],
                    None,
                ),
            ),
            expression(
                9,
                public582_map(
                    7,
                    Some(ScalarType::Int),
                    vec![
                        (Value::Null, Value::String("null".into())),
                        (Value::json_null(), Value::String("wrong".into())),
                    ],
                    None,
                ),
            ),
            expression(10, Expression::Const { value: Value::Null }),
            expression(
                11,
                public582_map(
                    10,
                    None,
                    vec![
                        (Value::Null, Value::String("null".into())),
                        (Value::json_null(), Value::String("wrong".into())),
                    ],
                    None,
                ),
            ),
            expression(
                12,
                Expression::Const {
                    value: Value::String("miss".into()),
                },
            ),
            expression(
                13,
                public582_map(12, None, vec![], Some(Value::String("fallback".into()))),
            ),
            expression(14, public582_map(12, None, vec![], None)),
            expression(
                15,
                public582_map(
                    14,
                    None,
                    vec![(Value::Null, Value::String("no-default-null".into()))],
                    Some(Value::String("wrong".into())),
                ),
            ),
            expression(16, public582_map(12, None, vec![], Some(Value::Null))),
            expression(
                17,
                public582_map(
                    16,
                    None,
                    vec![(Value::Null, Value::String("literal-null".into()))],
                    Some(Value::String("wrong".into())),
                ),
            ),
            expression(
                18,
                Expression::Const {
                    value: Value::XmlNil(ir::XmlNil),
                },
            ),
            expression(
                19,
                public582_map(
                    18,
                    None,
                    vec![
                        (Value::Null, Value::String("wrong".into())),
                        (Value::XmlNil(ir::XmlNil), Value::String("xmlnil".into())),
                    ],
                    None,
                ),
            ),
            expression(
                20,
                Expression::Const {
                    value: Value::String(" 7 ".into()),
                },
            ),
            expression(
                21,
                public582_map(
                    20,
                    Some(ScalarType::Int),
                    vec![(Value::Int(7), Value::String("trim-int".into()))],
                    Some(Value::String("wrong".into())),
                ),
            ),
            expression(
                22,
                Expression::Const {
                    value: Value::Float(7.0),
                },
            ),
            expression(
                23,
                public582_map(
                    22,
                    Some(ScalarType::Int),
                    vec![(Value::Int(7), Value::String("float-int".into()))],
                    Some(Value::String("wrong".into())),
                ),
            ),
            expression(
                24,
                Expression::Const {
                    value: Value::Int(7),
                },
            ),
            expression(
                25,
                public582_map(
                    24,
                    Some(ScalarType::Float),
                    vec![(Value::Float(7.0), Value::String("int-float".into()))],
                    Some(Value::String("wrong".into())),
                ),
            ),
            expression(
                26,
                Expression::Const {
                    value: Value::String("bad".into()),
                },
            ),
            expression(
                27,
                public582_map(
                    26,
                    Some(ScalarType::Int),
                    vec![(
                        Value::String("bad".into()),
                        Value::String("retained-string".into()),
                    )],
                    Some(Value::String("wrong".into())),
                ),
            ),
            expression(
                28,
                Expression::Const {
                    value: Value::Bool(true),
                },
            ),
            expression(
                29,
                public582_map(
                    28,
                    Some(ScalarType::String),
                    vec![(
                        Value::String("true".into()),
                        Value::String("bool-string".into()),
                    )],
                    Some(Value::String("wrong".into())),
                ),
            ),
            expression(
                30,
                Expression::Const {
                    value: Value::String(" true ".into()),
                },
            ),
            expression(
                31,
                public582_map(
                    30,
                    Some(ScalarType::Bool),
                    vec![(Value::Bool(true), Value::String("trim-bool".into()))],
                    Some(Value::String("wrong".into())),
                ),
            ),
            expression(
                32,
                Expression::Const {
                    value: Value::Int(7),
                },
            ),
            expression(
                33,
                public582_map(
                    32,
                    None,
                    vec![(Value::Float(7.0), Value::String("wrong".into()))],
                    Some(Value::String("tagged-int".into())),
                ),
            ),
            expression(
                34,
                Expression::Const {
                    value: Value::Float(1.5),
                },
            ),
            expression(
                35,
                public582_map(
                    34,
                    Some(ScalarType::Int),
                    vec![(Value::Float(1.5), Value::String("retained-float".into()))],
                    Some(Value::String("wrong".into())),
                ),
            ),
            expression(
                36,
                Expression::SourceField {
                    path: path(&["Ref", "Value"]),
                    frame: Some(Vec::new()),
                },
            ),
            expression(
                37,
                public582_map(
                    36,
                    None,
                    vec![(
                        Value::String("P.collision".into()),
                        Value::String("primary-frame".into()),
                    )],
                    Some(Value::String("wrong".into())),
                ),
            ),
        ],
        group(vec![
            binding("Legacy", 2, ScalarType::String),
            binding("Owned", 4, ScalarType::String),
            binding("Absent", 6, ScalarType::String),
            binding("UntypedJson", 8, ScalarType::String),
            binding("TypedJson", 9, ScalarType::String),
            binding("Null", 11, ScalarType::String),
            binding("Default", 13, ScalarType::String),
            binding("NoDefault", 15, ScalarType::String),
            binding("LiteralNull", 17, ScalarType::String),
            binding("XmlNil", 19, ScalarType::String),
            binding("TrimInt", 21, ScalarType::String),
            binding("FloatInt", 23, ScalarType::String),
            binding("IntFloat", 25, ScalarType::String),
            binding("RetainedString", 27, ScalarType::String),
            binding("BoolString", 29, ScalarType::String),
            binding("TrimBool", 31, ScalarType::String),
            binding("TaggedInt", 33, ScalarType::String),
            binding("RetainedFloat", 35, ScalarType::String),
            binding("EmptyFrame", 37, ScalarType::String),
        ]),
    );
    // The flat repeated input creates a real collection at the empty frame.
    p.root.repeating = true;
    p.root.iteration = Some(IterationPlan::source(Vec::new()));
    p.target = p.target.repeating();
    p
}
fn public582_lazy_program() -> Program {
    let mut p = basic(
        vec![
            expression(
                1,
                Expression::RuntimeParameter {
                    name: "flag".into(),
                    ty: ScalarType::Bool,
                },
            ),
            expression(2, field(SourceOwner::Primary, &["Value"], None)),
            expression(
                3,
                public582_map(
                    2,
                    None,
                    vec![(
                        Value::String("P.root".into()),
                        Value::String("primary".into()),
                    )],
                    Some(Value::String("fallback".into())),
                ),
            ),
            expression(4, field(named("Ref"), &["Value"], None)),
            expression(
                5,
                public582_map(
                    4,
                    None,
                    vec![(
                        Value::String("N.root".into()),
                        Value::String("named".into()),
                    )],
                    Some(Value::String("fallback".into())),
                ),
            ),
            expression(
                6,
                Expression::If {
                    condition: 1,
                    then: 3,
                    else_: 5,
                },
            ),
            expression(
                70,
                Expression::Const {
                    value: Value::String("map input".into()),
                },
            ),
            expression(71, Expression::Raise { message: Some(70) }),
            expression(
                72,
                public582_map(71, None, Vec::new(), Some(Value::String("fallback".into()))),
            ),
            expression(
                81,
                Expression::RuntimeParameter {
                    name: "value".into(),
                    ty: ScalarType::Int,
                },
            ),
            expression(
                82,
                public582_map(
                    81,
                    None,
                    vec![(Value::Int(7), Value::String("seven".into()))],
                    Some(Value::String("fallback".into())),
                ),
            ),
        ],
        group(vec![binding("Value", 6, ScalarType::String)]),
    );
    for (name, expression) in [("Raise", 72), ("Missing", 82)] {
        p.extra_targets.push(NamedTargetProgram {
            name: name.into(),
            target: SchemaNode::scalar(name, ScalarType::String),
            root: TargetScope {
                target_field: String::new(),
                repeating: false,
                iteration: None,
                construction: TargetConstruction::Scalar {
                    expression,
                    target_domain: ScalarType::String.into(),
                },
                bindings: Vec::new(),
                children: Vec::new(),
            },
        });
    }
    p
}

#[test]
fn public_basic_external_consumer_value_maps_ordered_null_coercion_and_literal_defaults() {
    let p = public582_matrix_program();
    let source = retain_public_basic_consumer("public-value-map-matrix", &p, PUBLIC582_MATRIX);
    assert_eq!(source.matches("var input_2 = Node_1(context);").count(), 1);
    assert_eq!(source.matches("var input_4 = Node_3(context);").count(), 1);
    assert_eq!(source.matches("var input_6 = Node_5(context);").count(), 1);
    assert_eq!(source.matches("var input_8 = Node_7(context);").count(), 1);
    assert_eq!(source.matches("var input_9 = Node_7(context);").count(), 1);
    assert_eq!(
        source.matches("var input_11 = Node_10(context);").count(),
        1
    );
    assert_eq!(
        source.matches("var input_13 = Node_12(context);").count(),
        1
    );
    assert_eq!(
        source.matches("var input_14 = Node_12(context);").count(),
        1
    );
    assert_eq!(
        source.matches("var input_15 = Node_14(context);").count(),
        1
    );
    assert_eq!(
        source.matches("var input_16 = Node_12(context);").count(),
        1
    );
    assert_eq!(
        source.matches("var input_17 = Node_16(context);").count(),
        1
    );
    assert_eq!(
        source.matches("var input_19 = Node_18(context);").count(),
        1
    );
    assert_eq!(
        source.matches("var input_21 = Node_20(context);").count(),
        1
    );
    assert_eq!(
        source.matches("var input_23 = Node_22(context);").count(),
        1
    );
    assert_eq!(
        source.matches("var input_25 = Node_24(context);").count(),
        1
    );
    assert_eq!(
        source.matches("var input_27 = Node_26(context);").count(),
        1
    );
    assert_eq!(
        source.matches("var input_29 = Node_28(context);").count(),
        1
    );
    assert_eq!(
        source.matches("var input_31 = Node_30(context);").count(),
        1
    );
    assert_eq!(
        source.matches("var input_33 = Node_32(context);").count(),
        1
    );
    assert_eq!(
        source.matches("var input_35 = Node_34(context);").count(),
        1
    );
    assert_eq!(
        source.matches("var input_37 = Node_36(context);").count(),
        1
    );
    assert!(source.contains("FerruleValueMaps.Apply("));
    assert!(!source.contains("ApplyUserFunction("));
}
#[test]
fn public_basic_external_consumer_value_map_keeps_lazy_owner_and_input_error_precedence() {
    let p = public582_lazy_program();
    let source =
        retain_public_basic_consumer("public-value-map-lazy-errors", &p, PUBLIC582_LAZY_ERRORS);
    for (id, input) in [(3, 2), (5, 4), (72, 71), (82, 81)] {
        assert_eq!(
            source
                .matches(&format!("var input_{id} = Node_{input}(context);"))
                .count(),
            1
        );
    }
    assert!(source.contains("FerruleBasicNamedSourceMode.Optional"));
}
const PUBLIC582_MATRIX: &str = r#"
var input=R(Primary());var limits=Policy();var selection=FerruleTargetSelection.Primary();
var p0=new Probe(Array.Empty<NamedInput>());var a=Reach("VM582-M01",input,p0,limits,null,null,()=>GeneratedMapping.ExecuteWithBasicOwnedSources(input,p0,limits,null));
var p1=new Probe(Array.Empty<NamedInput>());var b=Reach("VM582-M02",input,p1,limits,null,selection,()=>GeneratedMapping.ExecuteSelectedWithBasicOwnedSources(input,selection,p1,limits,null));
Tree(a,"[{\"Legacy\":\"first\",\"Owned\":\"owned\",\"Absent\":\"absent\",\"UntypedJson\":\"json\",\"TypedJson\":\"null\",\"Null\":\"null\",\"Default\":\"fallback\",\"NoDefault\":\"no-default-null\",\"LiteralNull\":\"literal-null\",\"XmlNil\":\"xmlnil\",\"TrimInt\":\"trim-int\",\"FloatInt\":\"float-int\",\"IntFloat\":\"int-float\",\"RetainedString\":\"retained-string\",\"BoolString\":\"bool-string\",\"TrimBool\":\"trim-bool\",\"TaggedInt\":\"tagged-int\",\"RetainedFloat\":\"retained-float\",\"EmptyFrame\":\"primary-frame\"}]");Tree(b,"[{\"Legacy\":\"first\",\"Owned\":\"owned\",\"Absent\":\"absent\",\"UntypedJson\":\"json\",\"TypedJson\":\"null\",\"Null\":\"null\",\"Default\":\"fallback\",\"NoDefault\":\"no-default-null\",\"LiteralNull\":\"literal-null\",\"XmlNil\":\"xmlnil\",\"TrimInt\":\"trim-int\",\"FloatInt\":\"float-int\",\"IntFloat\":\"int-float\",\"RetainedString\":\"retained-string\",\"BoolString\":\"bool-string\",\"TrimBool\":\"trim-bool\",\"TaggedInt\":\"tagged-int\",\"RetainedFloat\":\"retained-float\",\"EmptyFrame\":\"primary-frame\"}]");Check(((ExecutionOutputs)a.Result!).Extras.Count==0,"complete ordered empty extras");Reads(a,1,0);Reads(b,1,0);
"#;
const PUBLIC582_LAZY_ERRORS: &str = r#"
var input=Primary();var limits=Policy();var selection=FerruleTargetSelection.Primary();
var t=new[]{new KeyValuePair<string,FerruleValue>("flag",FerruleValue.FromBoolean(true))};var f=new[]{new KeyValuePair<string,FerruleValue>("flag",FerruleValue.FromBoolean(false))};
var yes=FerruleExecutionContext.WithParameters("mapping.test",new FerruleRuntimeParameters(t));var no=FerruleExecutionContext.WithParameters("mapping.test",new FerruleRuntimeParameters(f));
var p0=new Probe(Array.Empty<NamedInput>());var a=Reach("VM582-L03",input,p0,limits,yes,selection,()=>GeneratedMapping.ExecuteSelectedWithBasicOwnedSources(input,selection,p0,limits,yes),t);
var p1=new Probe(Array.Empty<NamedInput>());var b=Reach("VM582-L04",input,p1,limits,no,selection,()=>GeneratedMapping.ExecuteSelectedWithBasicOwnedSources(input,selection,p1,limits,no),f);
var p2=new Probe(new[]{new NamedInput("Ref",G(("Value",S("N.root"))))});var c=Reach("VM582-L05",input,p2,limits,no,selection,()=>GeneratedMapping.ExecuteSelectedWithBasicOwnedSources(input,selection,p2,limits,no),f);
var raised=FerruleTargetSelection.Named("Raise");var p3=new Probe(Array.Empty<NamedInput>());var d=Reach("VM582-L06",input,p3,limits,null,raised,()=>GeneratedMapping.ExecuteSelectedWithBasicOwnedSources(input,raised,p3,limits,null));
var missing=FerruleTargetSelection.Named("Missing");var parameters=Array.Empty<KeyValuePair<string,FerruleValue>>();var execution=FerruleExecutionContext.WithParameters("mapping.test",new FerruleRuntimeParameters(parameters));var p4=new Probe(Array.Empty<NamedInput>());var e=Reach("VM582-L07",input,p4,limits,execution,missing,()=>GeneratedMapping.ExecuteSelectedWithBasicOwnedSources(input,missing,p4,limits,execution),parameters);
Tree(a,"{\"Value\":\"primary\"}");OwnedAbsent(b,"Ref",4U);Tree(c,"{\"Value\":\"named\"}");
var cause=Runtime(d,FerruleRuntimeError.MappingException);Check(cause.Node==71u && cause.MappingExceptionMessage=="map input" && cause.Message=="node 71: mapping exception: map input" && cause.InnerException is null,"full map-input Raise; literal default cannot rescue");NullMetadata(cause,"Node","MappingExceptionMessage");
cause=Runtime(e,FerruleRuntimeError.MissingRuntimeParameter);Check(cause.Node==81u && cause.RuntimeParameter=="value" && cause.Message=="node 81: execution context does not provide runtime parameter 'value'" && cause.InnerException is null,"full missing map-input parameter; literal default cannot rescue");NullMetadata(cause,"Node","RuntimeParameter");
Reads(a,1,0);Reads(b,1,0);Reads(c,1,1);Reads(d,1,0);Reads(e,1,0);
"#;

fn failure594_expression(program: &mut Program, id: u32, value: Expression) {
    program
        .expressions
        .iter_mut()
        .find(|node| node.id == id)
        .unwrap()
        .expression = value;
}

fn failure594_program() -> Program {
    let mut program = failure_program(vec![
        expression(
            10,
            Expression::Const {
                value: mapped_string("static"),
            },
        ),
        expression(
            20,
            Expression::Const {
                value: Value::Bool(false),
            },
        ),
        expression(
            30,
            target_value_map(
                20,
                None,
                vec![(Value::Bool(false), Value::Bool(false))],
                Some(Value::Bool(true)),
            ),
        ),
        expression(
            40,
            target_value_map(
                10,
                None,
                vec![(mapped_string("static"), mapped_string("mapped"))],
                None,
            ),
        ),
        expression(
            99,
            Expression::Const {
                value: mapped_string("unused.json"),
            },
        ),
    ]);
    program.failure_rules.push(source_failure(
        &["Rows"],
        codegen::FailureSelection::WhenTrue(30),
        Some(40),
    ));
    program
}

fn failure594_admit(label: &str, program: &Program) {
    let actual = retained585_result(label, program);
    assert!(
        actual.is_ok(),
        "{label}: complete source result/cause retained before admission comparison"
    );
}

fn failure594_refuse(label: &str, program: &Program, expected: ProgramValidationError) {
    let actual = retained585_result(label, program);
    eprintln!("EXPECTED {label} complete independent typed diagnostic={expected:#?}");
    assert_eq!(retained585_extract(actual), expected, "{label}");
}

// Generated consumers use these Programs to exercise predicate, message,
// default, lazy-input, and typed-error behavior.
fn failure594_matched_program_one() -> Program {
    let mut program = failure_program(vec![
        failure_legacy(10, &["Score"], None),
        failure_legacy(11, &["Value"], None),
        expression(
            20,
            target_value_map(
                10,
                Some(ScalarType::Int),
                vec![
                    (Value::Int(1), Value::Bool(true)),
                    (Value::Int(0), Value::Bool(false)),
                ],
                None,
            ),
        ),
        expression(
            21,
            target_value_map(
                11,
                Some(ScalarType::String),
                vec![
                    (mapped_string("hit"), mapped_string("mapped")),
                    (mapped_string("hit"), mapped_string("later duplicate")),
                ],
                Some(mapped_string("fallback")),
            ),
        ),
        expression(
            60,
            Expression::Const {
                value: Value::Bool(false),
            },
        ),
        expression(
            61,
            Expression::RuntimeParameterDefault {
                name: "later".into(),
                ty: ScalarType::Bool,
                default: 60,
            },
        ),
        expression(
            62,
            Expression::Const {
                value: mapped_string("later"),
            },
        ),
    ]);
    program.failure_rules.extend([
        source_failure(&["Rows"], codegen::FailureSelection::WhenTrue(20), Some(21)),
        source_failure(&["Rows"], codegen::FailureSelection::WhenTrue(61), Some(62)),
    ]);
    program
}

fn failure594_matched_program_two() -> Program {
    let mut program = failure_program(vec![
        expression(
            10,
            Expression::RuntimeParameterDefault {
                name: "mode".into(),
                ty: ScalarType::String,
                default: 11,
            },
        ),
        expression(11, Expression::Raise { message: Some(12) }),
        expression(
            12,
            Expression::Const {
                value: mapped_string("input default"),
            },
        ),
        expression(
            20,
            target_value_map(
                10,
                Some(ScalarType::String),
                vec![
                    (mapped_string("skip"), Value::Bool(false)),
                    (mapped_string("raise"), Value::Bool(true)),
                ],
                Some(Value::Bool(true)),
            ),
        ),
        expression(
            30,
            Expression::RuntimeParameterDefault {
                name: "message".into(),
                ty: ScalarType::String,
                default: 31,
            },
        ),
        expression(
            31,
            Expression::Const {
                value: mapped_string("hit"),
            },
        ),
        expression(
            40,
            target_value_map(
                30,
                None,
                vec![(mapped_string("hit"), mapped_string("mapped"))],
                None,
            ),
        ),
        expression(
            41,
            Expression::RuntimeParameterDefault {
                name: "explode".into(),
                ty: ScalarType::Bool,
                default: 44,
            },
        ),
        expression(
            42,
            Expression::If {
                condition: 41,
                then: 43,
                else_: 40,
            },
        ),
        expression(43, Expression::Raise { message: Some(45) }),
        expression(
            44,
            Expression::Const {
                value: Value::Bool(false),
            },
        ),
        expression(
            45,
            Expression::Const {
                value: mapped_string("message raise"),
            },
        ),
    ]);
    program.failure_rules.push(source_failure(
        &["Rows"],
        codegen::FailureSelection::WhenTrue(20),
        Some(42),
    ));
    program
}

#[test]
fn private_failure_value_maps_admit_predicate_message_hits_misses_and_defaults() {
    let base = failure594_program();
    for (label, input, table, default) in [
        (
            "S594-01 predicate false hit",
            Value::Bool(false),
            vec![(Value::Bool(false), Value::Bool(false))],
            None,
        ),
        (
            "S594-02 predicate true hit",
            Value::Bool(true),
            vec![(Value::Bool(true), Value::Bool(true))],
            None,
        ),
        (
            "S594-03 predicate miss literal default",
            Value::Bool(false),
            vec![],
            Some(Value::Bool(true)),
        ),
        (
            "S594-04 predicate miss absent default",
            Value::Bool(false),
            vec![],
            None,
        ),
    ] {
        let mut program = base.clone();
        failure594_expression(&mut program, 20, Expression::Const { value: input });
        failure594_expression(&mut program, 30, target_value_map(20, None, table, default));
        failure594_admit(label, &program);
    }
    for (label, table, default) in [
        (
            "S594-05 message hit",
            vec![(mapped_string("static"), mapped_string("mapped"))],
            None,
        ),
        (
            "S594-06 message miss literal default",
            vec![],
            Some(mapped_string("fallback")),
        ),
        ("S594-07 message miss absent default", vec![], None),
    ] {
        let mut program = base.clone();
        failure594_expression(&mut program, 40, target_value_map(10, None, table, default));
        failure594_admit(label, &program);
    }
}

#[test]
fn private_failure_value_maps_keep_potential_inputs_and_retained_dynamic_envelopes() {
    use codegen::OwnedSourceCapabilityReason as Reason;
    let mut base = failure594_program();
    base.extra_sources[0].dynamic = Some(codegen::DynamicSourceProgram {
        path: 99,
        driver: codegen::SourceIteration::new(vec![]),
    });
    for (label, input) in [
        (
            "S594-08 dynamic map input path",
            Expression::SourceField {
                path: path(&["Unused", "Value"]),
                frame: None,
            },
        ),
        (
            "S594-09 dynamic map input frame",
            Expression::SourceField {
                path: path(&["Value"]),
                frame: Some(path(&["Unused", "Rows"])),
            },
        ),
    ] {
        let mut program = base.clone();
        failure594_expression(&mut program, 10, input);
        failure594_refuse(
            label,
            &program,
            retained585_leaf_expected(10, Reason::UnsupportedBasicContext),
        );
    }
    let mut empty = base.clone();
    failure594_expression(
        &mut empty,
        10,
        Expression::SourceField {
            path: path(&["Unused", "Value"]),
            frame: Some(vec![]),
        },
    );
    failure594_admit("S594-10 empty frame replaces dynamic path", &empty);
    let mut static_source = base.clone();
    static_source.extra_sources[0].dynamic = None;
    failure594_expression(
        &mut static_source,
        10,
        Expression::SourceField {
            path: path(&["Unused", "Value"]),
            frame: None,
        },
    );
    failure594_admit("S594-11 same name static metadata", &static_source);
    let mut different = base.clone();
    failure594_expression(
        &mut different,
        10,
        Expression::SourceField {
            path: path(&["Ref", "Value"]),
            frame: None,
        },
    );
    failure594_admit("S594-12 distinct exact static source name", &different);
    for (label, input) in [
        (
            "S594-13 potential parameter default map input",
            Expression::RuntimeParameterDefault {
                name: "supplied".into(),
                ty: ScalarType::String,
                default: 10,
            },
        ),
        (
            "S594-14 potential unchosen If map input",
            Expression::If {
                condition: 20,
                then: 10,
                else_: 11,
            },
        ),
        (
            "S594-15 potential ordinary call map input",
            Expression::Call {
                function: codegen::ScalarFunction::Trim,
                args: vec![10],
            },
        ),
        (
            "S594-16 potential Raise message map input",
            Expression::Raise { message: Some(10) },
        ),
    ] {
        let mut program = base.clone();
        failure594_expression(
            &mut program,
            10,
            Expression::SourceField {
                path: path(&["Unused", "Value"]),
                frame: None,
            },
        );
        program.expressions.extend([
            expression(
                11,
                Expression::Const {
                    value: mapped_string("static"),
                },
            ),
            expression(50, input),
        ]);
        failure594_expression(
            &mut program,
            40,
            target_value_map(50, None, vec![], Some(mapped_string("fallback"))),
        );
        failure594_refuse(
            label,
            &program,
            retained585_leaf_expected(10, Reason::UnsupportedBasicContext),
        );
    }
}

#[test]
fn private_failure_value_maps_keep_owned_structure_and_actual_called_body_priority() {
    use codegen::OwnedSourceCapabilityReason as Reason;
    let base = failure594_program();
    let mut unknown = base.clone();
    failure594_expression(&mut unknown, 10, field(named("Absent"), &["Value"], None));
    failure594_refuse(
        "S594-17 unknown map input owner remains structural",
        &unknown,
        ProgramValidationError::OwnedSource {
            diagnostic: Box::new(codegen::OwnedSourceDiagnostic::Structure {
                location: codegen::OwnedSourceDiagnosticLocation::Expression {
                    namespace: codegen::OwnedSourceNamespace::Main,
                    node: 10,
                    scope: None,
                },
                request: codegen::OwnedSourceDiagnosticRequest::Field {
                    owner: codegen::OwnedSourceDiagnosticOwner::Named {
                        name: "Absent".into(),
                    },
                    path: path(&["Value"]),
                    frame: None,
                },
                reason: codegen::OwnedSourceStructureReason::UnknownOwner,
            }),
        },
    );
    let mut owned = base.clone();
    failure594_expression(
        &mut owned,
        10,
        field(SourceOwner::Primary, &["Value"], None),
    );
    failure594_refuse(
        "S594-18 owned map input keeps failure request and origin",
        &owned,
        ProgramValidationError::OwnedSource {
            diagnostic: Box::new(codegen::OwnedSourceDiagnostic::Capability {
                location: codegen::OwnedSourceDiagnosticLocation::Expression {
                    namespace: codegen::OwnedSourceNamespace::Main,
                    node: 10,
                    scope: None,
                },
                request: Some(codegen::OwnedSourceDiagnosticRequest::Field {
                    owner: codegen::OwnedSourceDiagnosticOwner::Primary,
                    path: path(&["Value"]),
                    frame: None,
                }),
                reason: Reason::FailureRule,
                origin: codegen::OwnedSourceCapabilityOrigin::Failure { index: 0 },
            }),
        },
    );
    let mut call = base.clone();
    call.user_functions.extend([
        retained585_function(::mapping::FunctionId::new(7), 17),
        retained585_function(::mapping::FunctionId::new(9), 29),
    ]);
    failure594_expression(
        &mut call,
        10,
        Expression::UserFunctionCall {
            function: ::mapping::FunctionId::new(9),
            args: vec![],
        },
    );
    failure594_refuse(
        "S594-19 actual map input called body",
        &call,
        retained585_function_expected(::mapping::FunctionId::new(9), 29),
    );
    call.user_functions[1]
        .parameters
        .push(codegen::UserFunctionParameter {
            id: ::mapping::FunctionParameterId::new(1),
            ty: ScalarType::String,
        });
    failure594_expression(
        &mut call,
        10,
        Expression::UserFunctionCall {
            function: ::mapping::FunctionId::new(9),
            args: vec![11],
        },
    );
    call.expressions.push(expression(
        11,
        Expression::SourceField {
            path: path(&["Unused", "Value"]),
            frame: None,
        },
    ));
    call.extra_sources[0].dynamic = Some(codegen::DynamicSourceProgram {
        path: 99,
        driver: codegen::SourceIteration::new(vec![]),
    });
    failure594_refuse(
        "S594-20 stored call before dynamic argument",
        &call,
        retained585_function_expected(::mapping::FunctionId::new(9), 29),
    );
    let call_index = call
        .expressions
        .iter()
        .position(|node| node.id == 10)
        .unwrap();
    let argument_index = call
        .expressions
        .iter()
        .position(|node| node.id == 11)
        .unwrap();
    call.expressions.swap(call_index, argument_index);
    failure594_refuse(
        "S594-21 stored dynamic argument before call",
        &call,
        retained585_leaf_expected(11, Reason::UnsupportedBasicContext),
    );
    let mut inert = base;
    inert
        .user_functions
        .push(retained585_function(::mapping::FunctionId::new(7), 17));
    inert
        .expressions
        .push(expression(98, Expression::SourceDocumentPath));
    failure594_admit(
        "S594-22 dead ordinary document and unreferenced function stay inert",
        &inert,
    );
}

#[test]
fn private_failure_value_maps_keep_per_rule_priority_and_shared_public_refusals() {
    use codegen::OwnedSourceCapabilityReason as Reason;
    let mut base = failure594_program();
    base.extra_sources[0].dynamic = Some(codegen::DynamicSourceProgram {
        path: 99,
        driver: codegen::SourceIteration::new(vec![]),
    });
    let mut later = base.clone();
    failure594_expression(
        &mut later,
        10,
        Expression::SourceField {
            path: path(&["Unused", "Value"]),
            frame: None,
        },
    );
    later.failure_rules.push(source_failure(
        &["Unused", "Rows"],
        codegen::FailureSelection::All,
        None,
    ));
    failure594_refuse(
        "S594-23 later dynamic iteration precedes retained map input phase",
        &later,
        expected_failure_diagnostic(1, None, Reason::DynamicLoader),
    );
    let mut predicate = base.clone();
    predicate
        .expressions
        .push(failure_legacy(50, &["Unused", "Value"], None));
    predicate.failure_rules[0].selection = codegen::FailureSelection::WhenTrue(50);
    failure594_refuse(
        "S594-24 ordinary dynamic predicate keeps exact failure origin",
        &predicate,
        expected_failure_diagnostic(0, Some(50), Reason::DynamicLoader),
    );
    let mut message = base.clone();
    message
        .expressions
        .push(failure_legacy(50, &["Unused", "Value"], None));
    message.failure_rules[0].message = Some(50);
    failure594_refuse(
        "S594-25 ordinary dynamic message remains potential",
        &message,
        expected_failure_diagnostic(0, Some(50), Reason::DynamicLoader),
    );
    let mut specialized = base.clone();
    specialized
        .expressions
        .push(expression(50, Expression::SourceDocumentPath));
    specialized.failure_rules.push(source_failure(
        &["Rows"],
        codegen::FailureSelection::All,
        Some(50),
    ));
    failure594_refuse(
        "S594-26 later specialized failure remains closed",
        &specialized,
        expected_failure_diagnostic(1, None, Reason::FailureRule),
    );
    let expected = expected_failure_diagnostic(0, None, Reason::FailureRule);
    let shared = codegen::validate_basic_owned_emission(&base);
    eprintln!("INPUT S594-27 full Program={base:#?} RAW shared={shared:#?} EXPECTED={expected:#?}");
    assert_eq!(shared, Err(expected.clone()));
    let public = public_basic_result("S594-27 unchanged public failure ValueMap gate", &base);
    match public {
        Err(EmitError::ProgramValidation(actual)) => assert_eq!(actual, expected),
        other => panic!("S594-27 complete public failure ValueMap refusal differs: {other:#?}"),
    }
}

#[test]
fn private_failure_value_maps_admit_lazy_inputs_and_frozen_matched_host_programs() {
    let mut lazy = failure594_program();
    lazy.expressions.extend([
        expression(
            11,
            Expression::Const {
                value: mapped_string("unchosen raise"),
            },
        ),
        expression(50, Expression::Raise { message: Some(11) }),
        expression(
            51,
            Expression::Const {
                value: mapped_string("static"),
            },
        ),
        expression(
            52,
            Expression::If {
                condition: 20,
                then: 50,
                else_: 51,
            },
        ),
    ]);
    failure594_expression(
        &mut lazy,
        40,
        target_value_map(
            52,
            None,
            vec![(mapped_string("static"), mapped_string("mapped"))],
            None,
        ),
    );
    failure594_admit(
        "S594-28 ordinary lazy If and Raise input source admission",
        &lazy,
    );
    failure594_admit(
        "S594-29 frozen matched predicate/message/default host Program",
        &failure594_matched_program_one(),
    );
    failure594_admit(
        "S594-30 frozen matched runtime-default/lazy/error host Program",
        &failure594_matched_program_two(),
    );
}

fn root_pin_632_source(label: &str, program: &Program) -> String {
    eprintln!("INPUT {label} complete_program={program:#?}");
    let result = crate::owned_basic::render_public_basic_owned_mapping(program);
    eprintln!("RAW {label} complete_public_mapping_or_error={result:#?}");
    result.expect("complete source return retained before comparison")
}

#[test]
fn public_basic_empty_root_pins_emit_exact_owner_schema_proof() {
    let mut p = basic(
        vec![
            expression(1, field(SourceOwner::Primary, &["Value"], Some(&[]))),
            expression(2, field(named("Ref"), &["Value"], Some(&[]))),
        ],
        group(vec![
            binding("Primary", 1, ScalarType::String),
            binding("Named", 2, ScalarType::String),
        ]),
    );
    p.extra_sources[1].source.repeating = true;
    let source = root_pin_632_source("E632-CS01", &p);
    assert_eq!(
        source
            .matches("private static bool OwnedRootIsDeclaredNonrepeating(")
            .count(),
        1
    );
    assert_eq!(
        source
            .matches("context.ResolveOwnedFieldWithRootPin(")
            .count(),
        2
    );
    assert!(source.contains("if (owner.IsPrimary) return true;"));
    assert!(
        source.contains(
            "owner.Name, \"Ref\", global::System.StringComparison.Ordinal)) return false;"
        )
    );
    assert!(source.contains(
        "owner.Name, \" Ref \\u96EA \", global::System.StringComparison.Ordinal)) return true;"
    ));
    assert!(source.contains(
        "OwnedRootIsDeclaredNonrepeating(global::Ferrule.Runtime.FerruleOwnedSourceOwner.Primary)"
    ));
    assert!(source.contains("OwnedRootIsDeclaredNonrepeating(global::Ferrule.Runtime.FerruleOwnedSourceOwner.Named(\"Ref\"))"));
}

#[test]
fn public_basic_root_pin_schema_proof_preserves_scalar_union_admission_and_repetition() {
    let mut p = basic(
        vec![expression(
            1,
            field(SourceOwner::Primary, &["Value"], Some(&[])),
        )],
        group(vec![binding("Value", 1, ScalarType::String)]),
    );
    p.extra_sources.clear();
    let union = ir::ScalarTypeSet::new([ScalarType::String, ScalarType::Int]).unwrap();
    p.source = SchemaNode::group("Input", vec![SchemaNode::scalar_union("Value", union)]);
    let nonrepeating = root_pin_632_source("E632-CS02-union-child", &p);
    assert!(nonrepeating.contains("if (owner.IsPrimary) return true;"));
    p.source = SchemaNode::scalar_union("Input", union);
    p.expressions[0].expression = field(SourceOwner::Primary, &[], Some(&[]));
    let refused = crate::owned_basic::render_public_basic_owned_mapping(&p);
    eprintln!("INPUT E632-CS02-union-root complete_program={p:#?}");
    eprintln!("RAW E632-CS02-union-root complete_refusal={refused:#?}");
    let expected = ProgramValidationError::OwnedSource {
        diagnostic: Box::new(codegen::OwnedSourceDiagnostic::Structure {
            location: codegen::OwnedSourceDiagnosticLocation::Expression {
                namespace: codegen::OwnedSourceNamespace::Main,
                node: 1,
                scope: None,
            },
            request: codegen::OwnedSourceDiagnosticRequest::Field {
                owner: codegen::OwnedSourceDiagnosticOwner::Primary,
                path: vec![],
                frame: Some(vec![]),
            },
            reason: codegen::OwnedSourceStructureReason::InvalidOwnedCollection {
                found: codegen::OwnedSourceSchemaShape::ScalarUnion { repeating: false },
            },
        }),
    };
    assert!(matches!(refused, Err(EmitError::ProgramValidation(actual)) if actual == expected));
    p.source.repeating = true;
    let repeating = root_pin_632_source("E632-CS02-repeating-union-root", &p);
    assert!(repeating.contains("if (owner.IsPrimary) return false;"));
    assert!(repeating.contains("context.ResolveOwnedFieldWithRootPin("));
}

#[test]
fn public_basic_none_and_nonempty_pins_keep_previous_field_and_position_routes() {
    for (label, frame, parts) in [
        ("E632-CS03-none", None, &["Value"][..]),
        (
            "E632-CS03-nonempty",
            Some(&["Rows"][..]),
            &["Rows", "Value"][..],
        ),
    ] {
        let p = basic(
            vec![expression(1, field(SourceOwner::Primary, parts, frame))],
            group(vec![binding("Value", 1, ScalarType::String)]),
        );
        let source = root_pin_632_source(label, &p);
        assert!(!source.contains("OwnedRootIsDeclaredNonrepeating"));
        assert!(!source.contains("ResolveOwnedFieldWithRootPin"));
        assert!(source.contains("context.ResolveOwnedField("));
    }
    let p = basic(
        vec![expression(1, position(SourceOwner::Primary, Some(&[])))],
        group(vec![binding("Value", 1, ScalarType::Int)]),
    );
    let position = root_pin_632_source("E632-CS03-position", &p);
    assert!(!position.contains("OwnedRootIsDeclaredNonrepeating"));
    assert!(position.contains("context.OwnedPosition("));
}

#[test]
fn public_basic_unused_empty_pin_does_not_add_source_metadata_or_open_ordinary_gate() {
    let p = basic(
        vec![
            expression(
                1,
                Expression::Const {
                    value: Value::String("ordinary".into()),
                },
            ),
            expression(2, field(SourceOwner::Primary, &["Value"], Some(&[]))),
        ],
        group(vec![binding("Value", 1, ScalarType::String)]),
    );
    let source = root_pin_632_source("E632-CS04-unused", &p);
    assert!(!source.contains("OwnedRootIsDeclaredNonrepeating"));
    assert!(!source.contains("ResolveOwnedFieldWithRootPin"));
    let owned = basic(
        vec![expression(
            1,
            field(SourceOwner::Primary, &["Value"], Some(&[])),
        )],
        group(vec![binding("Value", 1, ScalarType::String)]),
    );
    let ordinary = crate::emit(&owned);
    eprintln!("RAW E632-CS04 ordinary_emission_complete_error={ordinary:#?}");
    let expected = ProgramValidationError::OwnedSource {
        diagnostic: Box::new(codegen::OwnedSourceDiagnostic::Capability {
            location: codegen::OwnedSourceDiagnosticLocation::Expression {
                namespace: codegen::OwnedSourceNamespace::Main,
                node: 1,
                scope: None,
            },
            request: Some(codegen::OwnedSourceDiagnosticRequest::Field {
                owner: codegen::OwnedSourceDiagnosticOwner::Primary,
                path: path(&["Value"]),
                frame: Some(vec![]),
            }),
            reason: codegen::OwnedSourceCapabilityReason::UnsupportedBasicContext,
            origin: codegen::OwnedSourceCapabilityOrigin::RetainedExpression {
                namespace: codegen::OwnedSourceNamespace::Main,
                node: 1,
            },
        }),
    };
    assert!(matches!(ordinary, Err(EmitError::ProgramValidation(actual)) if actual == expected));
}

// Public Basic leaf provenance is separate from ordinary/private rendering.
fn public_651_wrapper(node: u32, annotated: bool) -> String {
    let body = if annotated {
        format!(
            "        try {{ return NodeBody_{node}(context); }}\n        catch (global::Ferrule.Runtime.FerruleRuntimeException error)\n        {{\n            error.WithOwnedSourceNode({node}U);\n            throw;\n        }}\n"
        )
    } else {
        format!("        return NodeBody_{node}(context);\n")
    };
    format!(
        "\n    private static global::Ferrule.Runtime.FerruleValue Node_{node}(\n        global::Ferrule.Runtime.FerruleBasicScopeContext context)\n    {{\n        ObserveOwnedBasicNode({node}U, context);\n{body}    }}\n"
    )
}

#[test]
fn public_basic_owned_leaf_nodes_preserve_private_profiles_and_uint_width() {
    let p = basic(
        vec![
            expression(0, field(SourceOwner::Primary, &["Value"], None)),
            expression(1, field(named("Ref"), &["Value"], Some(&[]))),
            expression(
                2,
                field(SourceOwner::Primary, &["Rows", "Value"], Some(&["Rows"])),
            ),
            expression(3, position(named("Ref"), Some(&["Rows"]))),
            expression(
                4,
                Expression::SourceField {
                    path: path(&["Value"]),
                    frame: None,
                },
            ),
            expression(
                5,
                Expression::Const {
                    value: Value::Bool(true),
                },
            ),
            expression(
                6,
                Expression::If {
                    condition: 5,
                    then: 0,
                    else_: 1,
                },
            ),
            expression(u32::MAX, position(SourceOwner::Primary, Some(&[]))),
        ],
        group(vec![
            binding("Nearest", 0, ScalarType::String),
            binding("Root", 1, ScalarType::String),
            binding("Row", 2, ScalarType::String),
            binding("Position", 3, ScalarType::Int),
            binding("Legacy", 4, ScalarType::String),
            binding("Choice", 6, ScalarType::String),
            binding("High", u32::MAX, ScalarType::Int),
        ]),
    );
    let private = render_basic_owned_mapping(&p);
    let checked = crate::render_checked_private_owned_basic_mapping(&p);
    let public = crate::owned_basic::render_public_basic_owned_mapping(&p);
    let ordinary = crate::emit(&p);
    eprintln!("INPUT S651-CS01 complete_program={p:#?}");
    eprintln!(
        "RAW S651-CS01 private={private:#?} checked={checked:#?} public={public:#?} ordinary={ordinary:#?}"
    );
    let private = private.unwrap();
    let checked = checked.unwrap();
    let public = public.unwrap();
    assert!(checked.starts_with(&private));
    assert!(!private.contains("WithOwnedSourceNode"));
    assert!(!checked.contains("WithOwnedSourceNode"));
    assert_eq!(public.matches("error.WithOwnedSourceNode(").count(), 5);
    let mut restored = public.clone();
    for node in [0, 1, 2, 3, u32::MAX] {
        let annotated = public_651_wrapper(node, true);
        assert_eq!(public.matches(&annotated).count(), 1);
        assert!(private.contains(&public_651_wrapper(node, false)));
        restored = restored.replace(&annotated, &public_651_wrapper(node, false));
    }
    assert!(restored.starts_with(&checked));
    for node in [4, 5, 6] {
        assert!(public.contains(&public_651_wrapper(node, false)));
    }
    assert!(matches!(
        ordinary,
        Err(EmitError::ProgramValidation(
            ProgramValidationError::OwnedSource { .. }
        ))
    ));
}

fn public_651_nested_field_program() -> Program {
    let mut p = basic(
        vec![
            expression(
                1,
                Expression::RuntimeParameter {
                    name: "flag".into(),
                    ty: ScalarType::Bool,
                },
            ),
            expression(2, field(named("Ref"), &["Rows", "Value"], Some(&["Rows"]))),
            expression(
                3,
                Expression::Const {
                    value: Value::String("!".into()),
                },
            ),
            expression(
                4,
                Expression::Call {
                    function: ScalarFunction::Concat,
                    args: vec![2, 3],
                },
            ),
            expression(
                5,
                public582_map(4, None, vec![], Some(Value::String("fallback".into()))),
            ),
            expression(
                6,
                Expression::If {
                    condition: 1,
                    then: 5,
                    else_: 7,
                },
            ),
            expression(
                7,
                Expression::Const {
                    value: Value::String("safe".into()),
                },
            ),
            expression(u32::MAX, field(named("Ref"), &["Details", "Counter"], None)),
            expression(91, Expression::Raise { message: None }),
        ],
        group(vec![binding("Value", 6, ScalarType::String)]),
    );
    add_target(
        &mut p,
        "High",
        group(vec![binding("Counter", u32::MAX, ScalarType::Int)]),
    );
    add_target(
        &mut p,
        "Raise",
        group(vec![binding("Value", 91, ScalarType::String)]),
    );
    p
}

#[test]
fn public_basic_owned_field_errors_keep_leaf_node_through_call_map_and_lazy_if() {
    let p = public_651_nested_field_program();
    let source = retain_public_basic_consumer("public-node-651-field", &p, PUBLIC651_FIELDS);
    assert_eq!(source.matches("error.WithOwnedSourceNode(").count(), 2);
    assert!(source.contains(&public_651_wrapper(2, true)));
    assert!(source.contains(&public_651_wrapper(u32::MAX, true)));
    for node in [1, 3, 4, 5, 6, 7, 91] {
        assert!(source.contains(&public_651_wrapper(node, false)));
    }
}

const PUBLIC651_FIELDS: &str = r#"
void Inactive(Actual actual,uint node)
{
    var outer=Runtime(actual,FerruleRuntimeError.OwnedSource);var cause=outer.OwnedSourceFailure!;
    Check(outer.Node==node && outer.Message=="owned source access failed" && ReferenceEquals(cause,outer.InnerException),"exact owned leaf wrapper");
    Check(cause.Error==FerruleOwnedSourceErrorKind.InactiveSourceFrame && cause.Owner?.IsPrimary==false && cause.Owner?.Name=="Ref" && cause.RequestedName=="Ref" && cause.Collection!.SequenceEqual(new[]{"Rows"}),"exact inactive owner and collection");
    Check(cause.Message=="Owned source operation failed." && cause.DeclarationIndices is null && cause.Path is null && cause.Frame is null && cause.Segment is null && cause.FoundInstance is null && cause.RequestedIndex is null && cause.LimitKind is null && cause.Used is null && cause.Requested is null && cause.Maximum is null && cause.InnerException is null,"complete unchanged inactive cause");
    NullMetadata(outer,"Node","OwnedSourceFailure");
}
var input=Primary();var limits=Policy();var primary=FerruleTargetSelection.Primary();
var t=new[]{new KeyValuePair<string,FerruleValue>("flag",FerruleValue.FromBoolean(true))};var f=new[]{new KeyValuePair<string,FerruleValue>("flag",FerruleValue.FromBoolean(false))};
var yes=FerruleExecutionContext.WithParameters("mapping.test",new FerruleRuntimeParameters(t));var no=FerruleExecutionContext.WithParameters("mapping.test",new FerruleRuntimeParameters(f));
var refRoot=G(("Value",S("N.root")),("Rows",R(G(("Value",S("N1"))))),("Details",S("blocked")));
var p0=new Probe(Array.Empty<NamedInput>());var a=Reach("N651-F01",input,p0,limits,no,primary,()=>GeneratedMapping.ExecuteSelectedWithBasicOwnedSources(input,primary,p0,limits,no),f);
var p1=new Probe(new[]{new NamedInput("Ref",refRoot)});var b=Reach("N651-F02",input,p1,limits,yes,null,()=>GeneratedMapping.ExecuteWithBasicOwnedSources(input,p1,limits,yes),t);
var p2=new Probe(new[]{new NamedInput("Ref",refRoot)});var c=Reach("N651-F03",input,p2,limits,yes,primary,()=>GeneratedMapping.ExecuteSelectedWithBasicOwnedSources(input,primary,p2,limits,yes),t);
var high=FerruleTargetSelection.Named("High");var p3=new Probe(new[]{new NamedInput("Ref",refRoot)});var d=Reach("N651-F04",input,p3,limits,null,high,()=>GeneratedMapping.ExecuteSelectedWithBasicOwnedSources(input,high,p3,limits,null));
var p4=new Probe(Array.Empty<NamedInput>());var e=Reach("N651-F05",input,p4,limits,null,high,()=>GeneratedMapping.ExecuteSelectedWithBasicOwnedSources(input,high,p4,limits,null));
var raised=FerruleTargetSelection.Named("Raise");var p5=new Probe(Array.Empty<NamedInput>());var f0=Reach("N651-F06",input,p5,limits,null,raised,()=>GeneratedMapping.ExecuteSelectedWithBasicOwnedSources(input,raised,p5,limits,null));
var missing=FerruleTargetSelection.Named("missing");var p6=new Probe(Array.Empty<NamedInput>(),throwCount:true);var g=Reach("N651-F07",input,p6,limits,null,missing,()=>GeneratedMapping.ExecuteSelectedWithBasicOwnedSources(input,missing,p6,limits,null));
Tree(a,"{\"Value\":\"safe\"}");Inactive(b,2U);Inactive(c,2U);
var outer=Runtime(d,FerruleRuntimeError.OwnedSource);var cause=outer.OwnedSourceFailure!;var traversal=cause.InnerException as FerruleOwnedSourceTraversalException;
Check(outer.Node==uint.MaxValue && outer.Message=="owned source access failed" && ReferenceEquals(cause,outer.InnerException),"maximum node stays unsigned and cause identity survives");
Check(cause.Error==FerruleOwnedSourceErrorKind.Traversal && cause.Owner?.IsPrimary==false && cause.Owner?.Name=="Ref" && cause.RequestedName=="Ref" && cause.Path!.SequenceEqual(new[]{"Details","Counter"}) && cause.Segment==(nuint)1 && cause.FoundInstance==FerruleOwnedSourceInstanceKind.Scalar,"complete malformed owner/path/segment");
Check(traversal is not null && traversal.Error==FerruleOwnedSourceTraversalKind.CannotTraverse && traversal.Found==FerruleOwnedSourceInstanceKind.Scalar && traversal.Segment==(nuint)1 && ReferenceEquals(cause.Path,traversal.Path) && traversal.InnerException is null,"exact nested traversal and shared path");
Check(cause.DeclarationIndices is null && cause.Frame is null && cause.Collection is null && cause.RequestedIndex is null && cause.LimitKind is null && cause.Used is null && cause.Requested is null && cause.Maximum is null,"unchanged absent traversal metadata");NullMetadata(outer,"Node","OwnedSourceFailure");
OwnedAbsent(e,"Ref",uint.MaxValue);
outer=Runtime(f0,FerruleRuntimeError.MappingException);Check(outer.Node==91U && outer.MappingExceptionMessage is null && outer.InnerException is null && outer.Message=="node 91: mapping exception: mapping exception was raised","ordinary error is not reannotated");NullMetadata(outer,"Node");
InputError(g,FerruleRuntimeError.UnknownTarget,"missing","project has no named target `missing`");
Reads(a,1,0);Reads(b,1,1);Reads(c,1,1);Reads(d,1,1);Reads(e,1,0);Reads(f0,1,0);Reads(g,0,0);
"#;

#[test]
fn public_basic_owned_position_errors_keep_complete_owner_and_empty_none_nonempty_frames() {
    let mut p = basic(
        vec![
            expression(2, position(SourceOwner::Primary, Some(&[]))),
            expression(u32::MAX, position(named("Ref"), Some(&[]))),
            expression(0, position(SourceOwner::Primary, Some(&["Rows"]))),
            expression(3, position(named("Ref"), None)),
            expression(4, position(named("Ref"), Some(&["Rows"]))),
        ],
        group(vec![binding("Value", 2, ScalarType::Int)]),
    );
    add_target(
        &mut p,
        "Named",
        group(vec![binding("Value", u32::MAX, ScalarType::Int)]),
    );
    add_target(
        &mut p,
        "Zero",
        group(vec![binding("Value", 0, ScalarType::Int)]),
    );
    add_target(
        &mut p,
        "Nearest",
        group(vec![binding("Value", 3, ScalarType::Int)]),
    );
    add_target(
        &mut p,
        "Active",
        owned_iteration(
            named("Ref"),
            &["Rows"],
            vec![binding("Value", 4, ScalarType::Int)],
        ),
    );
    let source = retain_public_basic_consumer("public-node-651-position", &p, PUBLIC651_POSITIONS);
    assert_eq!(source.matches("error.WithOwnedSourceNode(").count(), 5);
    for node in [0, 2, 3, 4, u32::MAX] {
        assert!(source.contains(&public_651_wrapper(node, true)));
    }
    assert!(!source.contains("OwnedRootIsDeclaredNonrepeating"));
}

const PUBLIC651_POSITIONS: &str = r#"
void Inactive(Actual actual,uint node,bool primary,string[]? collection)
{
    var outer=Runtime(actual,FerruleRuntimeError.OwnedSource);var cause=outer.OwnedSourceFailure!;
    Check(outer.Node==node && outer.Message=="owned source access failed" && ReferenceEquals(cause,outer.InnerException),"complete annotated position wrapper");
    Check(cause.Error==FerruleOwnedSourceErrorKind.InactiveSourceFrame && cause.Owner?.IsPrimary==primary && cause.Owner?.Name==(primary?null:"Ref") && cause.RequestedName==(primary?null:"Ref"),"exact owner-local inactive cause");
    Check(collection is null?cause.Collection is null:cause.Collection is not null && cause.Collection.SequenceEqual(collection),"null/empty/nonempty collection remains distinct");
    Check(cause.Message=="Owned source operation failed." && cause.DeclarationIndices is null && cause.Path is null && cause.Frame is null && cause.Segment is null && cause.FoundInstance is null && cause.RequestedIndex is null && cause.LimitKind is null && cause.Used is null && cause.Requested is null && cause.Maximum is null && cause.InnerException is null,"complete unchanged inactive position metadata");
    NullMetadata(outer,"Node","OwnedSourceFailure");
}
var input=Primary();var limits=Policy();var refRoot=G(("Value",S("N.root")),("Rows",R(G(("Value",S("N1"))),G(("Value",S("N2"))))));
var p0=new Probe(new[]{new NamedInput("Ref",refRoot)});var a=Reach("N651-P01",input,p0,limits,null,null,()=>GeneratedMapping.ExecuteWithBasicOwnedSources(input,p0,limits,null));
var primary=FerruleTargetSelection.Primary();var p1=new Probe(new[]{new NamedInput("Ref",refRoot)});var b=Reach("N651-P02",input,p1,limits,null,primary,()=>GeneratedMapping.ExecuteSelectedWithBasicOwnedSources(input,primary,p1,limits,null));
var named=FerruleTargetSelection.Named("Named");var p2=new Probe(new[]{new NamedInput("Ref",refRoot)});var c=Reach("N651-P03",input,p2,limits,null,named,()=>GeneratedMapping.ExecuteSelectedWithBasicOwnedSources(input,named,p2,limits,null));
var zero=FerruleTargetSelection.Named("Zero");var p3=new Probe(new[]{new NamedInput("Ref",refRoot)});var d=Reach("N651-P04",input,p3,limits,null,zero,()=>GeneratedMapping.ExecuteSelectedWithBasicOwnedSources(input,zero,p3,limits,null));
var nearest=FerruleTargetSelection.Named("Nearest");var p4=new Probe(new[]{new NamedInput("Ref",refRoot)});var e=Reach("N651-P05",input,p4,limits,null,nearest,()=>GeneratedMapping.ExecuteSelectedWithBasicOwnedSources(input,nearest,p4,limits,null));
var active=FerruleTargetSelection.Named("Active");var p5=new Probe(new[]{new NamedInput("Ref",refRoot)});var f0=Reach("N651-P06",input,p5,limits,null,active,()=>GeneratedMapping.ExecuteSelectedWithBasicOwnedSources(input,active,p5,limits,null));
Inactive(a,2U,true,Array.Empty<string>());Inactive(b,2U,true,Array.Empty<string>());Inactive(c,uint.MaxValue,false,Array.Empty<string>());Inactive(d,0U,true,new[]{"Rows"});Inactive(e,3U,false,null);
Tree(f0,"[{\"Value\":1},{\"Value\":2}]");
Reads(a,1,1);Reads(b,1,1);Reads(c,1,1);Reads(d,1,1);Reads(e,1,1);Reads(f0,1,1);
"#;

#[test]
fn public_basic_node_annotation_does_not_cover_legacy_leaves_or_setup() {
    let mut p = basic(
        vec![
            expression(
                1,
                Expression::SourceField {
                    path: path(&["Value"]),
                    frame: None,
                },
            ),
            expression(
                2,
                Expression::Const {
                    value: Value::String("ordinary".into()),
                },
            ),
        ],
        group(vec![binding("Value", 1, ScalarType::String)]),
    );
    p.extra_sources.clear();
    let source = crate::owned_basic::render_public_basic_owned_mapping(&p);
    let private = crate::render_checked_private_owned_basic_mapping(&p);
    eprintln!("INPUT S651-CS04 complete_program={p:#?}");
    eprintln!("RAW S651-CS04 public={source:#?} private={private:#?}");
    let source = source.unwrap();
    let private = private.unwrap();
    assert!(source.starts_with(&private));
    assert!(!source.contains("WithOwnedSourceNode"));
    assert!(source.contains("context.ResolveLegacyField("));
    assert!(source.contains("var selected = ResolveOwnedBasicTarget(selection);"));
    assert_eq!(
        source
            .matches("catch (global::Ferrule.Runtime.FerruleRuntimeException error)")
            .count(),
        0
    );
}

// #645 is a separate public compiler profile. Existing emitters and hosts above
// retain their exact control-free admission and capture behavior.
fn retain_public_filter_consumer(label: &str, program: &Program, body: &str) -> String {
    eprintln!("INPUT {label} complete filter Program={program:#?}");
    let actual = crate::emit_with_basic_owned_filters(program);
    eprintln!("RAW {label} complete filter artifact result={actual:#?}");
    let artifacts = actual.expect("complete artifact/refusal retained before propagation");
    let source = String::from_utf8(
        artifacts
            .files()
            .iter()
            .find(|file| file.path.as_str() == "GeneratedMapping.cs")
            .unwrap()
            .contents
            .clone(),
    )
    .expect("complete emitted mapping is UTF-8");
    let host = PUBLIC_BASIC_CONSUMER
        .replace("__ERROR_SNAPSHOT__", HOST_ERROR_SNAPSHOT)
        .replace("__BODY__", body);
    eprintln!("RAW {label} complete independent public consumer={host}");
    if let Some(base) = std::env::var_os("FERRULE_PUBLIC_BASIC_CSHARP_HOST_DIR") {
        let base = std::path::PathBuf::from(base);
        assert!(
            base.is_absolute(),
            "root supplies a fresh absolute evidence namespace"
        );
        let root = base.join(label);
        std::fs::create_dir(&root).expect("fresh consumer namespace; preserve failure evidence");
        std::fs::create_dir(root.join("generated")).unwrap();
        std::fs::create_dir(root.join("consumer")).unwrap();
        for file in artifacts.files() {
            let path = root.join("generated").join(file.path.as_str());
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            let mut reached = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(path)
                .unwrap();
            std::io::Write::write_all(&mut reached, &file.contents).unwrap();
            reached.sync_all().unwrap();
        }
        let input_original = format!("{program:#?}\n");
        for (name, bytes) in [
            (
                "consumer/PublicConsumer.csproj",
                PUBLIC_BASIC_CONSUMER_PROJECT.as_bytes(),
            ),
            ("consumer/Program.cs", host.as_bytes()),
            ("Program.ir.original.txt", input_original.as_bytes()),
        ] {
            let mut reached = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(root.join(name))
                .unwrap();
            std::io::Write::write_all(&mut reached, bytes).unwrap();
            reached.sync_all().unwrap();
        }
    }
    source
}

fn filter_645(scope: &mut TargetScope, predicate: u32) {
    let input = scope.iteration.as_ref().unwrap().input().clone();
    scope.iteration = Some(IterationPlan::new(
        input,
        Some(predicate),
        None,
        vec![],
        IterationOutput::Repeated,
    ));
}
fn filter_645_program() -> Program {
    let mut root = owned_iteration(
        named("Ref"),
        &["Rows"],
        vec![
            binding("Value", 3, ScalarType::String),
            binding("OwnedPosition", 1, ScalarType::Int),
            binding("LegacyPosition", 4, ScalarType::Int),
        ],
    );
    filter_645(&mut root, 2);
    let mut p = basic(
        vec![
            expression(1, position(named("Ref"), Some(&["Rows"]))),
            expression(
                2,
                public582_map(
                    1,
                    Some(ScalarType::Int),
                    vec![
                        (Value::Int(4), Value::Bool(true)),
                        (Value::Int(5), Value::Bool(true)),
                    ],
                    Some(Value::Bool(false)),
                ),
            ),
            expression(3, field(named("Ref"), &["Rows", "Value"], Some(&["Rows"]))),
            expression(
                4,
                Expression::Position {
                    collection: path(&["Ref", "Rows"]),
                },
            ),
        ],
        root.clone(),
    );
    add_target(&mut p, "Filtered", root);
    p
}
fn filter_645_mapping(artifacts: codegen::ArtifactSet) -> String {
    String::from_utf8(
        artifacts
            .files()
            .iter()
            .find(|file| file.path.as_str() == "GeneratedMapping.cs")
            .unwrap()
            .contents
            .clone(),
    )
    .unwrap()
}

#[test]
fn public_basic_filter_companion_preserves_control_free_bytes_and_closed_routes() {
    let p = filter_645_program();
    let old = crate::emit_with_basic_owned_sources(&p);
    let private = render_basic_owned_mapping(&p);
    let default = codegen::validate_basic_owned_emission(&p);
    let ordinary = codegen::validate_program(&p);
    let catalogue = codegen::legacy_named_input_admission_plan(&p);
    eprintln!(
        "N645 old public={old:#?}; private={private:#?}; basic={default:#?}; ordinary={ordinary:#?}; catalogue={catalogue:#?}; complete input={p:#?}"
    );
    for actual in [old.map(|_| ()), private.map(|_| ())] {
        assert!(
            matches!(actual,Err(EmitError::ProgramValidation(ProgramValidationError::OwnedSource{diagnostic})) if matches!(*diagnostic,codegen::OwnedSourceDiagnostic::Capability{reason:codegen::OwnedSourceCapabilityReason::ScopeControl,..}))
        );
    }
    for actual in [default, ordinary, catalogue.map(|_| ())] {
        assert!(
            matches!(actual,Err(ProgramValidationError::OwnedSource{diagnostic}) if matches!(*diagnostic,codegen::OwnedSourceDiagnostic::Capability{reason:codegen::OwnedSourceCapabilityReason::ScopeControl,..}))
        );
    }
    let plain = basic(
        vec![expression(1, field(SourceOwner::Primary, &["Value"], None))],
        group(vec![binding("Value", 1, ScalarType::String)]),
    );
    let old = filter_645_mapping(crate::emit_with_basic_owned_sources(&plain).unwrap());
    let explicit = filter_645_mapping(crate::emit_with_basic_owned_filters(&plain).unwrap());
    assert_eq!(
        old, explicit,
        "control-free generated mapping bytes are exact"
    );
}

#[test]
fn public_basic_filter_streams_raw_predicates_compact_outputs_and_all_selected_targets() {
    let p = filter_645_program();
    let source = retain_public_filter_consumer("public-filter-645-phases", &p, PUBLIC645_PHASES);
    assert_eq!(
        source.matches("BeginRetainedFilterPhase(cohort_").count(),
        2
    );
    assert_eq!(
        source.matches("RequireBoolean(Node_2(candidate_").count(),
        2
    );
    assert!(source.contains("phase_0.Produce((nuint)input_0)"));
    assert!(
        source.find("phase_0.Produce").unwrap()
            < source.find("ScopeItem_0(produced_0.Context)").unwrap()
    );
    assert!(!source.contains("WithPhasePosition("));
    assert!(!source.contains("ReindexRetainedPhase("));
    assert!(source.contains("error.WithOwnedSourceNode(3U)"));
}
const PUBLIC645_PHASES: &str = r#"
var input=G(("Value",S("P.root")),("Ref",G(("Rows",R(G(("Value",S("collision"))))))));
var named=G(("Rows",R(new FerruleScalar(FerruleValue.Null),G(("Value",S("N1"))),new FerruleScalar(FerruleValue.JsonNull),G(("Value",S("N2"))),G(("Value",S("N3"))))));
var limits=Policy();var supplied=new[]{new NamedInput("Ref",named)};
var primary=FerruleTargetSelection.Primary();var selected=FerruleTargetSelection.Named("Filtered");
var p0=new Probe(supplied);var a=Reach("N645-F01",input,p0,limits,null,null,()=>GeneratedMapping.ExecuteWithBasicOwnedSources(input,p0,limits,null));
var p1=new Probe(supplied);var b=Reach("N645-F02",input,p1,limits,null,primary,()=>GeneratedMapping.ExecuteSelectedWithBasicOwnedSources(input,primary,p1,limits,null));
var p2=new Probe(supplied);var c=Reach("N645-F03",input,p2,limits,null,selected,()=>GeneratedMapping.ExecuteSelectedWithBasicOwnedSources(input,selected,p2,limits,null));
var expected="[{\"Value\":\"N2\",\"OwnedPosition\":1,\"LegacyPosition\":1},{\"Value\":\"N3\",\"OwnedPosition\":2,\"LegacyPosition\":2}]";
Tree(a,expected);Tree(b,expected);Tree(c,expected);
var all=(ExecutionOutputs)a.Result!;Check(all.Extras.Count==1 && all.Extras[0].Name=="Filtered" && Json(Plain(all.Extras[0].Instance))==expected,"complete all-target extra");
Reads(a,1,1);Reads(b,1,1);Reads(c,1,1);
"#;

#[test]
fn public_basic_filter_target_raise_precedes_later_predicate_raise() {
    let mut root = owned_iteration(
        SourceOwner::Primary,
        &["Rows"],
        vec![binding("Value", 6, ScalarType::String)],
    );
    filter_645(&mut root, 5);
    let p = basic(
        vec![
            expression(1, position(SourceOwner::Primary, Some(&["Rows"]))),
            expression(
                2,
                public582_map(
                    1,
                    None,
                    vec![(Value::Int(1), Value::Bool(true))],
                    Some(Value::Bool(false)),
                ),
            ),
            expression(
                3,
                Expression::RuntimeParameter {
                    name: "keep_first".into(),
                    ty: ScalarType::Bool,
                },
            ),
            expression(4, Expression::Raise { message: Some(11) }),
            expression(
                5,
                Expression::If {
                    condition: 2,
                    then: 3,
                    else_: 4,
                },
            ),
            expression(6, Expression::Raise { message: Some(10) }),
            expression(
                10,
                Expression::Const {
                    value: Value::String("target first".into()),
                },
            ),
            expression(
                11,
                Expression::Const {
                    value: Value::String("later predicate".into()),
                },
            ),
        ],
        root,
    );
    retain_public_filter_consumer("public-filter-645-order", &p, PUBLIC645_ORDER);
}
const PUBLIC645_ORDER: &str = r#"
var input=Primary();var limits=Policy();var primary=FerruleTargetSelection.Primary();
var t=new[]{new KeyValuePair<string,FerruleValue>("keep_first",FerruleValue.FromBoolean(true))};var f=new[]{new KeyValuePair<string,FerruleValue>("keep_first",FerruleValue.FromBoolean(false))};
var yes=FerruleExecutionContext.WithParameters("mapping.test",new FerruleRuntimeParameters(t));var no=FerruleExecutionContext.WithParameters("mapping.test",new FerruleRuntimeParameters(f));
var p0=new Probe(Array.Empty<NamedInput>());var a=Reach("N645-O01",input,p0,limits,yes,primary,()=>GeneratedMapping.ExecuteSelectedWithBasicOwnedSources(input,primary,p0,limits,yes),t);
var p1=new Probe(Array.Empty<NamedInput>());var b=Reach("N645-O02",input,p1,limits,no,primary,()=>GeneratedMapping.ExecuteSelectedWithBasicOwnedSources(input,primary,p1,limits,no),f);
var first=Runtime(a,FerruleRuntimeError.MappingException);Check(first.Node==6U && first.MappingExceptionMessage=="target first" && first.Message=="node 6: mapping exception: target first" && first.InnerException is null,"earlier target wins before later predicate");NullMetadata(first,"Node","MappingExceptionMessage");
var later=Runtime(b,FerruleRuntimeError.MappingException);Check(later.Node==4U && later.MappingExceptionMessage=="later predicate" && later.Message=="node 4: mapping exception: later predicate" && later.InnerException is null,"false first target is lazy, later predicate wins");NullMetadata(later,"Node","MappingExceptionMessage");Reads(a,1,0);Reads(b,1,0);
"#;

#[test]
fn public_basic_filter_false_rows_do_not_read_target_and_non_bool_is_typed() {
    let mut root = owned_iteration(
        SourceOwner::Primary,
        &["Rows"],
        vec![binding("Value", 1, ScalarType::String)],
    );
    filter_645(&mut root, 2);
    let p = basic(
        vec![
            expression(1, field(named("Ref"), &["Value"], None)),
            expression(
                2,
                Expression::Const {
                    value: Value::Bool(false),
                },
            ),
        ],
        root.clone(),
    );
    retain_public_filter_consumer(
        "public-filter-645-drop-all",
        &p,
        r#"
var input=Primary();var limits=Policy();var probe=new Probe(Array.Empty<NamedInput>());
var a=Reach("N645-D01",input,probe,limits,null,null,()=>GeneratedMapping.ExecuteWithBasicOwnedSources(input,probe,limits,null));Tree(a,"[]");Reads(a,1,0);
"#,
    );
    let p = basic(
        vec![
            expression(1, field(named("Ref"), &["Value"], None)),
            expression(
                2,
                Expression::Const {
                    value: Value::Int(7),
                },
            ),
        ],
        root,
    );
    retain_public_filter_consumer(
        "public-filter-645-non-bool",
        &p,
        r#"
var input=Primary();var limits=Policy();var probe=new Probe(Array.Empty<NamedInput>());
var a=Reach("N645-B01",input,probe,limits,null,null,()=>GeneratedMapping.ExecuteWithBasicOwnedSources(input,probe,limits,null));
var e=Runtime(a,FerruleRuntimeError.NotABool);Check(e.Node==2U && e.FoundKind==FerruleValueKind.Int64 && e.Message=="Graph node 2 expected a bool, found int." && e.InnerException is null,"complete typed non-Bool predicate");NullMetadata(e,"Node","FoundKind");Reads(a,1,0);
"#,
    );
}

#[test]
fn public_basic_filter_retains_unsupported_potential_lazy_branch() {
    let mut root = owned_iteration(
        SourceOwner::Primary,
        &["Rows"],
        vec![binding("Value", 1, ScalarType::String)],
    );
    filter_645(&mut root, 5);
    let p = basic(
        vec![
            expression(1, field(SourceOwner::Primary, &["Rows", "Value"], None)),
            expression(
                2,
                Expression::Const {
                    value: Value::Bool(false),
                },
            ),
            expression(
                3,
                Expression::Const {
                    value: Value::Bool(true),
                },
            ),
            expression(
                5,
                Expression::If {
                    condition: 2,
                    then: 9,
                    else_: 3,
                },
            ),
            expression(9, Expression::SourceDocumentPath),
        ],
        root,
    );
    let result = crate::emit_with_basic_owned_filters(&p);
    eprintln!("N645 potential complete input={p:#?}, full refusal={result:#?}");
    let Err(EmitError::ProgramValidation(ProgramValidationError::OwnedSource { diagnostic })) =
        result
    else {
        panic!("potential node9 XML operator must be refused before rendering");
    };
    assert_eq!(
        *diagnostic,
        codegen::OwnedSourceDiagnostic::Capability {
            location: codegen::OwnedSourceDiagnosticLocation::Expression {
                namespace: codegen::OwnedSourceNamespace::Main,
                node: 9,
                scope: Some(::mapping::SourceScopeLocation::root(
                    ::mapping::SourceScopeTarget::Primary
                )),
            },
            request: None,
            reason: codegen::OwnedSourceCapabilityReason::XmlOperator,
            origin: codegen::OwnedSourceCapabilityOrigin::Expression {
                namespace: codegen::OwnedSourceNamespace::Main,
                node: 9,
            },
        }
    );
}

#[test]
fn public_basic_filter_companion_keeps_other_controls_closed() {
    let base = filter_645_program();
    for variant in 0..4 {
        let mut p = base.clone();
        let input = p.root.iteration.as_ref().unwrap().input().clone();
        p.expressions.push(expression(
            20,
            Expression::Const {
                value: Value::Int(1),
            },
        ));
        let sort = (variant == 0).then(|| {
            codegen::SortPlan::new(
                codegen::SortKey {
                    expression: 1,
                    descending: false,
                },
                vec![],
                codegen::SortFilterOrder::SortThenFilter,
            )
        });
        let windows = if variant == 1 {
            vec![codegen::SequenceWindow::First { count: 20 }]
        } else {
            vec![]
        };
        let output = match variant {
            2 => IterationOutput::First,
            3 => IterationOutput::MappedSequence,
            _ => IterationOutput::Repeated,
        };
        p.root.iteration = Some(IterationPlan::new(input, Some(2), sort, windows, output));
        if variant == 2 {
            p.root.repeating = false;
            p.target = target_schema(&p.root, "Output");
        } else if variant == 3 {
            let mut child = p.root.clone();
            child.target_field = "Mapped".into();
            child.repeating = false;
            p.root = group(vec![]);
            p.root.children.push(child);
            p.target = target_schema(&p.root, "Output");
        }
        let result = crate::emit_with_basic_owned_filters(&p);
        eprintln!("N645 closed variant={variant}, complete Program={p:#?}, refusal={result:#?}");
        assert!(
            matches!(result,Err(EmitError::ProgramValidation(ProgramValidationError::OwnedSource{diagnostic})) if matches!(*diagnostic,codegen::OwnedSourceDiagnostic::Capability{reason:codegen::OwnedSourceCapabilityReason::ScopeControl,..}))
        );
    }

    // Malformed cardinality/location still wins before Basic capability refusal.
    for output in [IterationOutput::First, IterationOutput::MappedSequence] {
        let mut p = base.clone();
        let input = p.root.iteration.as_ref().unwrap().input().clone();
        p.root.iteration = Some(IterationPlan::new(input, Some(2), None, vec![], output));
        let result = crate::emit_with_basic_owned_filters(&p);
        eprintln!(
            "N645 malformed output={output:?}, complete Program={p:#?}, full refusal={result:#?}"
        );
        assert!(matches!(result,
            Err(EmitError::ProgramValidation(ProgramValidationError::InvalidIterationOutput { target_path, output: actual }))
            if target_path.is_empty() && actual == output));
    }
    // A nonselected named target cannot bring dynamic-document artifacts into
    // the explicit Basic package; all target inventories are checked eagerly.
    let mut p = base.clone();
    p.expressions.push(expression(
        20,
        Expression::Const {
            value: Value::String("unused.xml".into()),
        },
    ));
    let mut dynamic = plain_iteration(
        &["Ref", "Rows"],
        vec![binding("Value", 20, ScalarType::String)],
    );
    dynamic.iteration = Some(IterationPlan::dynamic_documents(path(&["Ref", "Rows"]), 20));
    p.extra_targets[0].target = target_schema(&dynamic, "Filtered");
    p.extra_targets[0].root = dynamic;
    let result = crate::emit_with_basic_owned_filters(&p);
    eprintln!("N645 nonselected dynamic complete Program={p:#?}, full refusal={result:#?}");
    let Err(EmitError::ProgramValidation(ProgramValidationError::OwnedSource { diagnostic })) =
        result
    else {
        panic!("nonselected named dynamic documents must be refused before artifacts");
    };
    let scope = ::mapping::SourceScopeLocation::root(::mapping::SourceScopeTarget::Named {
        index: 0,
        name: "Filtered".into(),
    });
    assert_eq!(
        *diagnostic,
        codegen::OwnedSourceDiagnostic::Capability {
            location: codegen::OwnedSourceDiagnosticLocation::Iteration {
                scope: scope.clone()
            },
            request: None,
            reason: codegen::OwnedSourceCapabilityReason::Document,
            origin: codegen::OwnedSourceCapabilityOrigin::Scope { scope },
        }
    );
}

#[test]
fn public_basic_filter_nested_plain_identity_keeps_outer_phase_and_raw_null_gaps() {
    let mut kids = owned_iteration(
        SourceOwner::Primary,
        &["Rows", "Rows"],
        vec![
            binding("Value", 3, ScalarType::String),
            binding("ParentPosition", 1, ScalarType::Int),
            binding("Position", 2, ScalarType::Int),
        ],
    );
    kids.target_field = "Kids".into();
    filter_645(&mut kids, 4);
    let mut identity = owned_iteration(
        SourceOwner::Primary,
        &["Rows"],
        vec![
            binding("ParentPosition", 1, ScalarType::Int),
            binding("Value", 5, ScalarType::String),
        ],
    );
    identity.target_field = "Identity".into();
    filter_645(&mut identity, 4);
    let mut root = owned_iteration(SourceOwner::Primary, &["Rows"], vec![]);
    root.children = vec![kids, identity];
    filter_645(&mut root, 6);
    let p = basic(
        vec![
            expression(1, position(SourceOwner::Primary, Some(&["Rows"]))),
            expression(2, position(SourceOwner::Primary, Some(&["Rows", "Rows"]))),
            expression(
                3,
                field(SourceOwner::Primary, &["Rows", "Rows", "Value"], None),
            ),
            expression(
                4,
                Expression::Const {
                    value: Value::Bool(true),
                },
            ),
            expression(5, field(SourceOwner::Primary, &["Rows", "Value"], None)),
            expression(
                6,
                public582_map(
                    1,
                    None,
                    vec![(Value::Int(2), Value::Bool(true))],
                    Some(Value::Bool(false)),
                ),
            ),
        ],
        root,
    );
    retain_public_filter_consumer(
        "public-filter-645-nested",
        &p,
        r#"
// XmlNil is a real scalar candidate in strict owned walks; only Null/JsonNull
// are absent. Preserve this original input and its complete typed traversal.
var input=G(("Rows",R(G(("Value",S("r1")),("Rows",R(G(("Value",S("unused")))))),G(("Value",S("r2")),("Rows",R(new FerruleScalar(FerruleValue.Null),G(("Value",S("K2"))),new FerruleScalar(FerruleValue.XmlNil),G(("Value",S("K3")))))))));
var nullInput=G(("Rows",R(G(("Value",S("r1")),("Rows",R(G(("Value",S("unused")))))),G(("Value",S("r2")),("Rows",R(new FerruleScalar(FerruleValue.Null),G(("Value",S("K2"))),new FerruleScalar(FerruleValue.JsonNull),G(("Value",S("K3")))))))));
var limits=Policy();var probe=new Probe(Array.Empty<NamedInput>());var nullProbe=new Probe(Array.Empty<NamedInput>());
var a=Reach("N645-N01",input,probe,limits,null,null,()=>GeneratedMapping.ExecuteWithBasicOwnedSources(input,probe,limits,null));
var b=Reach("N645-N02",nullInput,nullProbe,limits,null,null,()=>GeneratedMapping.ExecuteWithBasicOwnedSources(nullInput,nullProbe,limits,null));
var outer=Runtime(a,FerruleRuntimeError.OwnedSource);var cause=outer.OwnedSourceFailure;
Check(outer.Node==3U && outer.Message=="owned source access failed" && cause is not null && ReferenceEquals(cause,outer.InnerException),"XmlNil exact node3 typed inner identity");NullMetadata(outer,"Node","OwnedSourceFailure");
var c=cause!;Check(c.Error==FerruleOwnedSourceErrorKind.Traversal && c.Message=="Owned source operation failed." && c.Owner?.IsPrimary==true && c.Owner?.Name is null && c.RequestedName is null && c.Path!.SequenceEqual(new[]{"Rows","Rows","Value"}) && c.Segment==2 && c.FoundInstance==FerruleOwnedSourceInstanceKind.Scalar,"XmlNil complete primary traversal metadata");
Check(c.DeclarationIndices is null && c.Frame is null && c.Collection is null && c.RequestedIndex is null && c.LimitKind is null && c.Used is null && c.Requested is null && c.Maximum is null,"XmlNil absent owned metadata");
Check(c.InnerException is FerruleOwnedSourceTraversalException,"XmlNil exact nested traversal type");var t=(FerruleOwnedSourceTraversalException)c.InnerException!;
Check(t.Error==FerruleOwnedSourceTraversalKind.CannotTraverse && t.Message=="Owned source traversal failed." && t.Path.SequenceEqual(new[]{"Rows","Rows","Value"}) && t.Segment==2 && t.Found==FerruleOwnedSourceInstanceKind.Scalar && t.InnerException is null,"XmlNil complete traversal cause");
Tree(b,"[{\"Kids\":[{\"Value\":\"K2\",\"ParentPosition\":1,\"Position\":1},{\"Value\":\"K3\",\"ParentPosition\":1,\"Position\":2}],\"Identity\":[{\"ParentPosition\":1,\"Value\":\"r2\"}]}]");Reads(a,1,0);Reads(b,1,0);
"#,
    );
}

#[test]
fn public_basic_filter_predicate_owned_error_keeps_leaf_node_identity_and_full_owner() {
    let mut root = owned_iteration(
        SourceOwner::Primary,
        &["Rows"],
        vec![binding("Value", 1, ScalarType::String)],
    );
    filter_645(&mut root, 2);
    let p = basic(
        vec![
            expression(1, field(SourceOwner::Primary, &["Rows", "Value"], None)),
            expression(2, field(named("Ref"), &["Value"], None)),
        ],
        root,
    );
    retain_public_filter_consumer(
        "public-filter-645-leaf",
        &p,
        r#"
var input=Primary();var limits=Policy();var probe=new Probe(Array.Empty<NamedInput>());
var a=Reach("N645-L01",input,probe,limits,null,null,()=>GeneratedMapping.ExecuteWithBasicOwnedSources(input,probe,limits,null));
OwnedAbsent(a,"Ref",2U);Reads(a,1,0);
"#,
    );
}

#[test]
fn public_basic_filter_plain_named_walk_keeps_raw_predicates_and_compact_paired_outputs() {
    let mut p = filter_645_program();
    p.root.iteration = Some(IterationPlan::new(
        codegen::SourceIteration::new(path(&["Ref", "Rows"])),
        Some(2),
        None,
        vec![],
        IterationOutput::Repeated,
    ));
    p.extra_targets[0].root.iteration = p.root.iteration.clone();
    // Legacy selection prefers the real primary Ref collision. It cannot
    // borrow NamedRef ownership from the path spelling.
    let source =
        retain_public_filter_consumer("public-filter-645-plain", &p, PUBLIC645_PLAIN_COLLISION);
    let positive = filter_645_plain_success_program();
    let positive_source = retain_public_filter_consumer(
        "public-filter-645-plain-success",
        &positive,
        PUBLIC645_PLAIN_SUCCESS,
    );
    assert_eq!(
        positive_source
            .matches("WalkSourceCohort(new string[] { \"Ref\", \"Rows\" })")
            .count(),
        2
    );
    assert!(!positive_source.contains("WalkOwnedSourceCohort("));
    assert!(positive_source.contains("ResolveOwnedFieldWithRootPin"));
    assert_eq!(
        source
            .matches("WalkSourceCohort(new string[] { \"Ref\", \"Rows\" })")
            .count(),
        2
    );
    assert!(!source.contains("WalkOwnedSourceCohort("));
    assert_eq!(
        source.matches("BeginRetainedFilterPhase(cohort_").count(),
        2
    );
    assert_eq!(
        source.matches("RequireBoolean(Node_2(candidate_").count(),
        2
    );
}

fn filter_645_plain_success_program() -> Program {
    let mut root = plain_iteration(
        &["Ref", "Rows"],
        vec![
            binding("Value", 3, ScalarType::String),
            binding("Position", 1, ScalarType::Int),
            binding("RootValue", 5, ScalarType::String),
        ],
    );
    filter_645(&mut root, 2);
    let mut p = basic(
        vec![
            expression(
                1,
                Expression::Position {
                    collection: path(&["Ref", "Rows"]),
                },
            ),
            expression(
                2,
                public582_map(
                    1,
                    Some(ScalarType::Int),
                    vec![
                        (Value::Int(4), Value::Bool(true)),
                        (Value::Int(5), Value::Bool(true)),
                    ],
                    Some(Value::Bool(false)),
                ),
            ),
            expression(
                3,
                Expression::SourceField {
                    path: path(&["Ref", "Rows", "Value"]),
                    frame: None,
                },
            ),
            expression(5, field(SourceOwner::Primary, &["Value"], Some(&[]))),
        ],
        root.clone(),
    );
    add_target(&mut p, "Filtered", root);
    p
}

const PUBLIC645_PLAIN_COLLISION: &str = r#"
var input=G(("Value",S("P.root")),("Ref",G(("Rows",R(G(("Value",S("collision"))))))));
var named=G(("Rows",R(new FerruleScalar(FerruleValue.Null),G(("Value",S("N1"))),new FerruleScalar(FerruleValue.JsonNull),G(("Value",S("N2"))),G(("Value",S("N3"))))));
var limits=Policy();var supplied=new[]{new NamedInput("Ref",named)};var primary=FerruleTargetSelection.Primary();var selected=FerruleTargetSelection.Named("Filtered");
var p0=new Probe(supplied);var a=Reach("N645-P01",input,p0,limits,null,null,()=>GeneratedMapping.ExecuteWithBasicOwnedSources(input,p0,limits,null));
var p1=new Probe(supplied);var b=Reach("N645-P02",input,p1,limits,null,primary,()=>GeneratedMapping.ExecuteSelectedWithBasicOwnedSources(input,primary,p1,limits,null));
var p2=new Probe(supplied);var c=Reach("N645-P03",input,p2,limits,null,selected,()=>GeneratedMapping.ExecuteSelectedWithBasicOwnedSources(input,selected,p2,limits,null));
foreach(var actual in new[]{a,b,c}) {
    var outer=Runtime(actual,FerruleRuntimeError.OwnedSource);var cause=outer.OwnedSourceFailure;
    Check(outer.Node==1U && outer.Message=="owned source access failed" && cause is not null && ReferenceEquals(cause,outer.InnerException),"plain primary collision cannot invent named frame");NullMetadata(outer,"Node","OwnedSourceFailure");
    var e=cause!;Check(e.Error==FerruleOwnedSourceErrorKind.InactiveSourceFrame && e.Message=="Owned source operation failed." && e.Owner?.IsPrimary==false && e.Owner?.Name=="Ref" && e.RequestedName=="Ref" && e.Collection!.SequenceEqual(new[]{"Rows"}),"complete Ref Rows inactive frame");
    Check(e.DeclarationIndices is null && e.Path is null && e.Frame is null && e.Segment is null && e.FoundInstance is null && e.RequestedIndex is null && e.LimitKind is null && e.Used is null && e.Requested is null && e.Maximum is null && e.InnerException is null,"complete absent inactive-frame metadata");Reads(actual,1,1);
}
"#;

const PUBLIC645_PLAIN_SUCCESS: &str = r#"
// The plain cohort retains all five raw rows. False raw1/2/3 predicates keep
// both null occurrences and the unselected group from evaluating their targets.
var input=G(("Value",S("P.root")),("Ref",G(("Rows",R(new FerruleScalar(FerruleValue.Null),G(("Value",S("P1"))),new FerruleScalar(FerruleValue.JsonNull),G(("Value",S("P2"))),G(("Value",S("P3"))))))));
var named=G(("Rows",R(G(("Value",S("N1"))),G(("Value",S("N2"))),G(("Value",S("N3"))))));
var limits=Policy();var supplied=new[]{new NamedInput("Ref",named)};var primary=FerruleTargetSelection.Primary();var selected=FerruleTargetSelection.Named("Filtered");
var p0=new Probe(supplied);var a=Reach("N645-P04",input,p0,limits,null,null,()=>GeneratedMapping.ExecuteWithBasicOwnedSources(input,p0,limits,null));
var p1=new Probe(supplied);var b=Reach("N645-P05",input,p1,limits,null,primary,()=>GeneratedMapping.ExecuteSelectedWithBasicOwnedSources(input,primary,p1,limits,null));
var p2=new Probe(supplied);var c=Reach("N645-P06",input,p2,limits,null,selected,()=>GeneratedMapping.ExecuteSelectedWithBasicOwnedSources(input,selected,p2,limits,null));
var expected="[{\"Value\":\"P2\",\"Position\":1,\"RootValue\":\"P.root\"},{\"Value\":\"P3\",\"Position\":2,\"RootValue\":\"P.root\"}]";
Tree(a,expected);Tree(b,expected);Tree(c,expected);
var all=(ExecutionOutputs)a.Result!;Check(all.Extras.Count==1 && all.Extras[0].Name=="Filtered" && Json(Plain(all.Extras[0].Instance))==expected,"complete plain all-target extra");Reads(a,1,1);Reads(b,1,1);Reads(c,1,1);
"#;
