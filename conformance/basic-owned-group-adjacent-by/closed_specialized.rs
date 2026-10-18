//! Independent Q05 refusal recipes, not functional execution cases. No expected
//! product is obtained by lowering, validating or emitting an actual fixture.
use super::*;
use codegen::{
    Expression as E, GeneratedSequence as G, OwnedSourceCapabilityOrigin as O,
    OwnedSourceCapabilityReason as R, OwnedSourceDiagnosticLocation as L,
    OwnedSourceDiagnosticOwner as Owner, OwnedSourceDiagnosticRequest as Q,
    OwnedSourceNamespace as N, ProgramValidationError as Error,
};

#[derive(Clone, Copy)]
enum Recipe {
    FunctionArgument,
    FunctionInIndex,
    FunctionBody,
    JoinReducer,
    JoinInIndex,
    JoinMember,
    JoinSibling,
    DynamicLoader,
    DynamicField,
    DocumentPath,
    DocumentAncestor,
    DocumentIndex,
    XmlSerialize,
    XmlMixedExpression,
    XmlMixedConstruction,
    PathHierarchy,
    RecursiveFilter,
    AdjacencyTree,
    Concatenate,
    DynamicTarget,
    GeneratedSibling,
}

const RECIPES: [(&str, Recipe); 21] = [
    ("Q05S-function-ItemAt-argument", Recipe::FunctionArgument),
    ("Q05S-function-in-ItemAt-index", Recipe::FunctionInIndex),
    ("Q05S-isolated-function-body", Recipe::FunctionBody),
    ("Q05S-join-reducer-ItemAt-value", Recipe::JoinReducer),
    ("Q05S-join-in-ItemAt-index", Recipe::JoinInIndex),
    ("Q05S-join-member-iteration", Recipe::JoinMember),
    ("Q05S-isolated-join-ItemAt-index", Recipe::JoinSibling),
    ("Q05S-stored-dynamic-loader", Recipe::DynamicLoader),
    ("Q05S-runtime-named-source-field", Recipe::DynamicField),
    ("Q05S-document-path-in-ItemAt", Recipe::DocumentPath),
    ("Q05S-document-ancestor", Recipe::DocumentAncestor),
    (
        "Q05S-document-receiving-ItemAt-index",
        Recipe::DocumentIndex,
    ),
    ("Q05S-XML-serialize-in-ItemAt", Recipe::XmlSerialize),
    (
        "Q05S-XML-mixed-expression-in-ItemAt",
        Recipe::XmlMixedExpression,
    ),
    ("Q05S-XML-mixed-construction", Recipe::XmlMixedConstruction),
    ("Q05S-path-hierarchy", Recipe::PathHierarchy),
    (
        "Q05S-recursive-filter-ItemAt-predicate",
        Recipe::RecursiveFilter,
    ),
    ("Q05S-adjacency-tree-ItemAt-root", Recipe::AdjacencyTree),
    ("Q05S-concatenation", Recipe::Concatenate),
    ("Q05S-computed-target-ItemAt-value", Recipe::DynamicTarget),
    (
        "Q05S-isolated-generated-ItemAt-index",
        Recipe::GeneratedSibling,
    ),
];

fn root() -> mapping::SourceScopeLocation {
    mapping::SourceScopeLocation::root(mapping::SourceScopeTarget::Primary)
}

fn grouped() -> mapping::SourceScopeLocation {
    root().descendant(mapping::SourceScopeStep::Child(0))
}

fn member() -> mapping::SourceScopeLocation {
    grouped().descendant(mapping::SourceScopeStep::Child(0))
}

fn joined_member() -> mapping::SourceScopeLocation {
    member().descendant(mapping::SourceScopeStep::Child(0))
}

fn special() -> mapping::SourceScopeLocation {
    grouped().descendant(mapping::SourceScopeStep::Child(1))
}

fn sibling() -> mapping::SourceScopeLocation {
    root().descendant(mapping::SourceScopeStep::Child(1))
}

fn capability(location: L, request: Option<Q>, reason: R, origin: O) -> Error {
    Error::OwnedSource {
        diagnostic: Box::new(codegen::OwnedSourceDiagnostic::Capability {
            location,
            request,
            reason,
            origin,
        }),
    }
}

fn iteration_request() -> Q {
    Q::Iteration {
        owner: Owner::Primary,
        path: projects::path(&["Rows"]),
    }
}

fn position_request() -> Q {
    Q::Position {
        owner: Owner::Primary,
        collection: Some(projects::path(&["Rows"])),
    }
}

fn scope_error(scope: mapping::SourceScopeLocation, reason: R) -> Error {
    capability(
        L::Iteration {
            scope: scope.clone(),
        },
        Some(iteration_request()),
        reason,
        O::Scope { scope },
    )
}

fn expression_error(reason: R) -> Error {
    capability(
        L::Expression {
            namespace: N::Main,
            node: 2013,
            scope: Some(grouped()),
        },
        None,
        reason,
        O::Expression {
            namespace: N::Main,
            node: 2013,
        },
    )
}

fn isolated_scope_error(reason: R) -> Error {
    capability(
        L::Expression {
            namespace: N::Main,
            node: 5,
            scope: Some(sibling()),
        },
        Some(position_request()),
        reason,
        O::Scope { scope: sibling() },
    )
}

fn function_body_error() -> Error {
    capability(
        L::Expression {
            namespace: N::Function(mapping::FunctionId::new(42)),
            node: 5,
            scope: None,
        },
        Some(Q::Field {
            owner: Owner::Primary,
            path: projects::path(&["Marker"]),
            frame: Some(vec![]),
        }),
        R::IsolatedFunction,
        O::FunctionBody {
            function: mapping::FunctionId::new(42),
        },
    )
}

fn expected(recipe: Recipe) -> Error {
    match recipe {
        Recipe::FunctionArgument | Recipe::FunctionInIndex => capability(
            L::Expression {
                namespace: N::Main,
                node: 2013,
                scope: Some(grouped()),
            },
            None,
            R::IsolatedFunction,
            O::FunctionCall {
                caller_namespace: N::Main,
                call: 2013,
                callee: mapping::FunctionId::new(42),
            },
        ),
        Recipe::FunctionBody => function_body_error(),
        Recipe::JoinReducer | Recipe::JoinInIndex => expression_error(R::Join),
        Recipe::JoinMember => scope_error(joined_member(), R::Join),
        Recipe::JoinSibling => isolated_scope_error(R::Join),
        Recipe::GeneratedSibling => isolated_scope_error(R::GeneratedItem),
        Recipe::DynamicLoader => capability(
            L::Expression {
                namespace: N::Main,
                node: 2005,
                scope: None,
            },
            Some(Q::Field {
                owner: Owner::Primary,
                path: projects::path(&["Marker"]),
                frame: Some(vec![]),
            }),
            R::DynamicLoader,
            O::DynamicSource {
                index: 3,
                name: "unavailableDynamic".into(),
            },
        ),
        // Runtime-named fields are a retained Basic-inventory refusal, unlike
        // owned reads in a stored loader or a dynamically declared exact owner.
        Recipe::DynamicField => capability(
            L::Expression {
                namespace: N::Main,
                node: 2013,
                scope: None,
            },
            None,
            R::DynamicLoader,
            O::RetainedExpression {
                namespace: N::Main,
                node: 2013,
            },
        ),
        Recipe::DocumentPath | Recipe::XmlSerialize | Recipe::XmlMixedExpression => {
            expression_error(R::XmlOperator)
        }
        // The ancestor is a document scope, not a new grouping permission.
        Recipe::DocumentAncestor => capability(
            L::Iteration { scope: grouped() },
            Some(iteration_request()),
            R::Document,
            O::Scope { scope: root() },
        ),
        Recipe::DocumentIndex => capability(
            L::Expression {
                namespace: N::Main,
                node: 5,
                scope: Some(root()),
            },
            Some(position_request()),
            R::Document,
            O::Scope { scope: root() },
        ),
        Recipe::XmlMixedConstruction => scope_error(special(), R::XmlOperator),
        Recipe::PathHierarchy | Recipe::RecursiveFilter | Recipe::AdjacencyTree => {
            scope_error(special(), R::TreeOperator)
        }
        Recipe::Concatenate => scope_error(member(), R::Concatenate),
        Recipe::DynamicTarget => scope_error(member(), R::DynamicTarget),
    }
}

fn diagnostic(error: Error) -> codegen::Diagnostic {
    let Error::OwnedSource { diagnostic } = error else {
        unreachable!()
    };
    codegen::Diagnostic::OwnedSource { diagnostic }
}

fn fields(schema: &mut SchemaNode) -> &mut Vec<SchemaNode> {
    let ir::SchemaKind::Group { children, .. } = &mut schema.kind else {
        panic!("authored group schema")
    };
    children
}

fn row_fields(schema: &mut SchemaNode) -> &mut Vec<SchemaNode> {
    let rows = fields(schema)
        .iter_mut()
        .find(|field| field.name == "Rows")
        .unwrap();
    fields(rows)
}

fn target_group_fields(schema: &mut SchemaNode) -> &mut Vec<SchemaNode> {
    let groups = fields(schema)
        .iter_mut()
        .find(|field| field.name == "Groups")
        .unwrap();
    fields(groups)
}

fn open_object() -> SchemaNode {
    SchemaNode::group("Open", vec![])
        .with_dynamic_fields(int("*"))
        .unwrap()
}

fn xml_element() -> SchemaNode {
    SchemaNode::group("Xml", vec![string("Value")])
}

fn mixed_element(name: &str) -> SchemaNode {
    SchemaNode::group(
        name,
        vec![
            SchemaNode::scalar(ir::XML_TEXT_FIELD, ScalarType::String).text(),
            string("part").repeating(),
        ],
    )
}

fn directory() -> SchemaNode {
    SchemaNode::group(
        "directory",
        vec![
            string("name"),
            SchemaNode::group("file", vec![string("name")]).repeating(),
            SchemaNode::recursive_group("directory", "directory").repeating(),
        ],
    )
}

fn recursive_source() -> SchemaNode {
    SchemaNode::group(
        "Tree",
        vec![
            string("name"),
            SchemaNode::group("file", vec![string("name")]).repeating(),
            SchemaNode::recursive_group("directory", "Tree").repeating(),
        ],
    )
}

fn adjacency_target() -> SchemaNode {
    SchemaNode::group(
        "Type",
        vec![
            string("name"),
            SchemaNode::recursive_group("children", "Type").repeating(),
        ],
    )
}

fn links() -> SchemaNode {
    SchemaNode::group("Links", vec![string("Key"), string("Parent")]).repeating()
}

fn add_join_schemas(source: &mut SchemaNode) {
    for name in ["A", "B"] {
        let collection = SchemaNode::group(name, vec![int("Key")]).repeating();
        // Root declarations satisfy the complete static join catalogue; row-local
        // declarations are anchors only in candidate or fresh member contexts,
        // not in the structurally anchorless grouped target view.
        row_fields(source).push(collection.clone());
        fields(source).push(collection);
    }
}

fn mapping_join() -> mapping::JoinPlan {
    mapping::JoinPlan::new(
        mapping::JoinSource::new(projects::path(&["A"])),
        mapping::JoinSource::new(projects::path(&["B"])),
        mapping::JoinConditions::new(mapping::JoinKey::new(
            projects::path(&["A"]),
            projects::path(&["Key"]),
            projects::path(&["Key"]),
        )),
    )
    .unwrap()
}

fn program_join() -> codegen::InnerJoin {
    codegen::InnerJoin::new(
        codegen::JoinId::new(43),
        codegen::JoinPlan::new(
            codegen::JoinSource::new(projects::path(&["A"])),
            codegen::JoinSource::new(projects::path(&["B"])),
            codegen::JoinConditions::new(codegen::JoinKey::new(
                projects::path(&["A"]),
                projects::path(&["Key"]),
                projects::path(&["Key"]),
            )),
        )
        .unwrap(),
    )
}

fn project_join_member(p: &mut Project) {
    // Grouping clears the structural active_source for grouped targets. Keep
    // the existing ordinary owned member iteration as the actual row anchor,
    // then introduce the correlated join beneath it. Schema declarations alone
    // cannot authenticate a grouped representative as an active ordinary row.
    let mut joined = p.root.children[0].children[0].clone();
    joined.target_field = "Joined".into();
    joined.iteration = ScopeIteration::InnerJoin {
        id: mapping::JoinId::new(43),
        plan: mapping_join(),
    };
    joined.bindings[1].node = 2003;
    let members = target_group_fields(&mut p.target)
        .iter_mut()
        .find(|field| field.name == "Members")
        .unwrap();
    let mut joined_target = members.clone();
    joined_target.name = "Joined".into();
    fields(members).push(joined_target);
    p.root.children[0].children[0].children.push(joined);
}

fn program_join_member(p: &mut codegen::Program) {
    let mut joined = p.root.children[0].children[0].clone();
    joined.target_field = "Joined".into();
    joined.iteration = Some(codegen::IterationPlan::join(program_join()));
    joined.bindings[1].expression = 2003;
    let members = target_group_fields(&mut p.target)
        .iter_mut()
        .find(|field| field.name == "Members")
        .unwrap();
    let mut joined_target = members.clone();
    joined_target.name = "Joined".into();
    fields(members).push(joined_target);
    p.root.children[0].children[0].children.push(joined);
}

fn project_core() -> Project {
    let mut p = projects::base();
    for (id, node) in [
        (
            2000,
            Node::Const {
                value: Value::Int(1),
            },
        ),
        (
            2001,
            Node::Const {
                value: Value::Int(3),
            },
        ),
        (
            2002,
            Node::SourceField {
                path: vec![],
                frame: None,
            },
        ),
        (
            2003,
            Node::SequenceItemAt {
                sequence: mapping::SequenceExpr::Generate {
                    from: Some(2000),
                    to: 2001,
                    item: 2002,
                },
                index: 5,
            },
        ),
        (
            2004,
            Node::Const {
                value: Value::String("computed".into()),
            },
        ),
        (2005, projects::field("Primary", &["Marker"], Some(&[]))),
        (
            2006,
            Node::Const {
                value: Value::Bool(true),
            },
        ),
    ] {
        p.graph.nodes.insert(id, node);
    }
    p.root.children[0].bindings[1].node = 2003;
    p
}

fn put(p: &mut codegen::Program, id: u32, expression: E) {
    // The supplied base is lowered and pruned. Every new dependency is inserted
    // here, rather than relying on an unused node from projects::base().
    p.expressions.retain(|node| node.id != id);
    p.expressions
        .push(codegen::ExpressionNode { id, expression });
}

fn program_core(base: &codegen::Program) -> codegen::Program {
    let mut p = base.clone();
    for (id, expression) in [
        (
            2000,
            E::Const {
                value: Value::Int(1),
            },
        ),
        (
            2001,
            E::Const {
                value: Value::Int(3),
            },
        ),
        (
            2002,
            E::SourceField {
                path: vec![],
                frame: None,
            },
        ),
        (
            2003,
            E::SequenceItemAt {
                sequence: G::Range {
                    from: Some(2000),
                    to: 2001,
                    item: 2002,
                },
                index: 5,
            },
        ),
        (
            2004,
            E::Const {
                value: Value::String("computed".into()),
            },
        ),
        (
            2005,
            E::OwnedSourceField(mapping::OwnedSourceField {
                source: mapping::OwnedSourceRef {
                    owner: mapping::SourceOwner::Primary,
                    path: projects::path(&["Marker"]),
                },
                frame: Some(vec![]),
            }),
        ),
        (
            2006,
            E::Const {
                value: Value::Bool(true),
            },
        ),
    ] {
        put(&mut p, id, expression);
    }
    p.root.children[0].bindings[1].expression = 2003;
    p
}

fn project_index(p: &mut Project, index: u32) {
    let Node::SequenceItemAt {
        index: receiving, ..
    } = p.graph.nodes.get_mut(&2003).unwrap()
    else {
        unreachable!()
    };
    *receiving = index;
}

fn program_index(p: &mut codegen::Program, index: u32) {
    let E::SequenceItemAt {
        index: receiving, ..
    } = &mut p
        .expressions
        .iter_mut()
        .find(|node| node.id == 2003)
        .unwrap()
        .expression
    else {
        unreachable!()
    };
    *receiving = index;
}

fn project_text_consumer(p: &mut Project, input: u32) {
    p.graph.nodes.insert(
        2010,
        Node::Const {
            value: Value::String("|".into()),
        },
    );
    p.graph.nodes.insert(
        2003,
        Node::SequenceItemAt {
            sequence: mapping::SequenceExpr::Tokenize {
                input,
                delimiter: 2010,
                item: 2002,
            },
            index: 5,
        },
    );
    p.root.children[0].bindings[0].node = 2003;
    p.root.children[0].bindings[1].node = 5;
}

fn program_text_consumer(p: &mut codegen::Program, input: u32) {
    put(
        p,
        2010,
        E::Const {
            value: Value::String("|".into()),
        },
    );
    put(
        p,
        2003,
        E::SequenceItemAt {
            sequence: G::Tokenize {
                input,
                delimiter: 2010,
                item: 2002,
            },
            index: 5,
        },
    );
    p.root.children[0].bindings[0].expression = 2003;
    p.root.children[0].bindings[1].expression = 5;
}

fn project_function(p: &mut Project, owned_body: bool) {
    p.user_functions.insert(
        mapping::FunctionId::new(42),
        mapping::UserFunction {
            library: "closed442".into(),
            name: "isolated".into(),
            description: None,
            parameters: if owned_body {
                vec![]
            } else {
                vec![mapping::FunctionParameter {
                    id: mapping::FunctionParameterId::new(1),
                    name: "value".into(),
                    ty: ScalarType::Int,
                }]
            },
            output_name: "value".into(),
            output_type: if owned_body {
                ScalarType::String
            } else {
                ScalarType::Int
            },
            body: Graph {
                nodes: BTreeMap::from([(
                    5,
                    if owned_body {
                        projects::field("Primary", &["Marker"], Some(&[]))
                    } else {
                        Node::FunctionParameter {
                            parameter: mapping::FunctionParameterId::new(1),
                        }
                    },
                )]),
            },
            output: 5,
        },
    );
}

fn program_function(p: &mut codegen::Program, owned_body: bool) {
    p.user_functions.push(codegen::UserFunctionProgram {
        id: mapping::FunctionId::new(42),
        library: "closed442".into(),
        name: "isolated".into(),
        parameters: if owned_body {
            vec![]
        } else {
            vec![codegen::UserFunctionParameter {
                id: mapping::FunctionParameterId::new(1),
                ty: ScalarType::Int,
            }]
        },
        output_type: if owned_body {
            ScalarType::String
        } else {
            ScalarType::Int
        },
        expressions: vec![codegen::ExpressionNode {
            id: 5,
            expression: if owned_body {
                E::OwnedSourceField(mapping::OwnedSourceField {
                    source: mapping::OwnedSourceRef {
                        owner: mapping::SourceOwner::Primary,
                        path: projects::path(&["Marker"]),
                    },
                    frame: Some(vec![]),
                })
            } else {
                E::FunctionParameter {
                    parameter: mapping::FunctionParameterId::new(1),
                }
            },
        }],
        output: 5,
    });
}

fn project_special(p: &mut Project, target: SchemaNode, scope: Scope) {
    let mut scope = scope;
    scope.target_field = target.name.clone();
    target_group_fields(&mut p.target).push(target);
    p.root.children[0].children.push(scope);
}

fn program_special(p: &mut codegen::Program, target: SchemaNode, scope: codegen::TargetScope) {
    let mut scope = scope;
    scope.target_field = target.name.clone();
    target_group_fields(&mut p.target).push(target);
    p.root.children[0].children.push(scope);
}

fn program_scope(construction: codegen::TargetConstruction) -> codegen::TargetScope {
    codegen::TargetScope {
        target_field: String::new(),
        repeating: false,
        iteration: None,
        construction,
        bindings: vec![],
        children: vec![],
    }
}

fn project_recipe(recipe: Recipe) -> Project {
    let mut p = project_core();
    match recipe {
        Recipe::FunctionArgument | Recipe::FunctionInIndex | Recipe::FunctionBody => {
            let owned_body = matches!(recipe, Recipe::FunctionBody);
            project_function(&mut p, owned_body);
            p.graph.nodes.insert(
                2013,
                Node::UserFunctionCall {
                    function: mapping::FunctionId::new(42),
                    args: if owned_body {
                        vec![]
                    } else if matches!(recipe, Recipe::FunctionInIndex) {
                        vec![2000]
                    } else {
                        vec![2003]
                    },
                },
            );
            if owned_body {
                p.extra_targets[0].root.bindings[0].node = 2013;
            } else if matches!(recipe, Recipe::FunctionInIndex) {
                project_index(&mut p, 2013);
            } else {
                p.root.children[0].bindings[1].node = 2013;
            }
        }
        Recipe::JoinReducer | Recipe::JoinInIndex | Recipe::JoinMember | Recipe::JoinSibling => {
            add_join_schemas(&mut p.source);
            if matches!(recipe, Recipe::JoinReducer | Recipe::JoinInIndex) {
                p.graph.nodes.insert(
                    2013,
                    Node::JoinAggregate {
                        function: mapping::AggregateOp::Count,
                        join: mapping::JoinId::new(43),
                        plan: mapping_join(),
                        expression: Some(if matches!(recipe, Recipe::JoinInIndex) {
                            2000
                        } else {
                            2003
                        }),
                        arg: None,
                    },
                );
                if matches!(recipe, Recipe::JoinInIndex) {
                    project_index(&mut p, 2013);
                    // Candidate key validation still has active_source=Rows.
                    // Do not also retain this join-containing consumer as a
                    // grouped target binding, where that anchor is absent.
                    p.root.children[0].group_adjacent_by = Some(2003);
                    p.root.children[0].bindings[1].node = 5;
                } else {
                    p.root.children[0].group_adjacent_by = Some(2013);
                }
            } else if matches!(recipe, Recipe::JoinMember) {
                project_join_member(&mut p);
            } else {
                fields(&mut p.target)
                    .push(SchemaNode::group("Isolated", vec![int("Value")]).repeating());
                p.root.children.push(Scope {
                    target_field: "Isolated".into(),
                    iteration: ScopeIteration::InnerJoin {
                        id: mapping::JoinId::new(43),
                        plan: mapping_join(),
                    },
                    bindings: projects::bindings(&[("Value", 2003)]),
                    ..Default::default()
                });
            }
        }
        Recipe::DynamicLoader => {
            p.graph.nodes.insert(
                2013,
                Node::If {
                    condition: 2006,
                    then: 2005,
                    else_: 2004,
                },
            );
            p.extra_sources[3].dynamic_path.as_mut().unwrap().node = 2013;
        }
        Recipe::DynamicField => {
            row_fields(&mut p.source).push(open_object());
            p.graph.nodes.insert(
                2013,
                Node::DynamicSourceField {
                    object: projects::path(&["Rows", "Open"]),
                    frame: None,
                    key: 2004,
                },
            );
            p.root.children[0].bindings[1].node = 2013;
        }
        Recipe::DocumentPath => {
            // SourceDocumentPath requires a genuine local XML file-set input
            // before common ownership capability validation can be reached.
            p.source_path = Some("records-*.xml".into());
            p.source_options = mapping::FormatOptions {
                xml_document: true,
                local_xml_file_set: true,
                ..Default::default()
            };
            p.graph.nodes.insert(2013, Node::SourceDocumentPath);
            project_text_consumer(&mut p, 2013);
        }
        Recipe::DocumentAncestor | Recipe::DocumentIndex => {
            if matches!(recipe, Recipe::DocumentIndex) {
                p.graph.nodes.insert(
                    2011,
                    Node::Const {
                        value: Value::String("one.json|two.json".into()),
                    },
                );
                project_text_consumer(&mut p, 2011);
            }
            p.root.iteration = ScopeIteration::DynamicDocuments {
                source: vec![],
                output_path: if matches!(recipe, Recipe::DocumentIndex) {
                    2003
                } else {
                    2004
                },
            };
        }
        Recipe::XmlSerialize => {
            row_fields(&mut p.source).push(xml_element());
            p.graph.nodes.insert(
                2013,
                Node::XmlSerialize {
                    path: projects::path(&["Rows", "Xml"]),
                    frame: None,
                    schema: Box::new(xml_element()),
                    declaration: false,
                    indent: false,
                    namespace: None,
                },
            );
            project_text_consumer(&mut p, 2013);
        }
        Recipe::XmlMixedExpression => {
            row_fields(&mut p.source).push(mixed_element("Mixed"));
            p.graph.nodes.insert(
                2013,
                Node::XmlMixedContent {
                    path: projects::path(&["Rows", "Mixed"]),
                    frame: None,
                    replacements: vec![],
                },
            );
            project_text_consumer(&mut p, 2013);
        }
        Recipe::XmlMixedConstruction => {
            row_fields(&mut p.source).push(mixed_element("Mixed"));
            project_special(
                &mut p,
                mixed_element("MixedOut").repeating(),
                Scope {
                    iteration: ScopeIteration::Source(projects::path(&["Mixed"])),
                    construction: mapping::ScopeConstruction::XmlMixedContent {
                        elements: vec![mapping::XmlMixedContentElement {
                            source: "part".into(),
                            target: "part".into(),
                        }],
                    },
                    ..Default::default()
                },
            );
        }
        Recipe::PathHierarchy => {
            row_fields(&mut p.source).push(string("Paths").repeating());
            project_special(
                &mut p,
                directory(),
                Scope {
                    construction: mapping::ScopeConstruction::PathHierarchy {
                        plan: mapping::PathHierarchyPlan::new(
                            projects::path(&["Paths"]),
                            "/".into(),
                            "directory".into(),
                            "file".into(),
                            "name".into(),
                        )
                        .unwrap(),
                    },
                    ..Default::default()
                },
            );
        }
        Recipe::RecursiveFilter => {
            row_fields(&mut p.source).push(recursive_source());
            p.graph.nodes.insert(
                2014,
                Node::Call {
                    function: "equal".into(),
                    args: vec![2003, 2000],
                },
            );
            project_special(
                &mut p,
                recursive_source().repeating(),
                Scope {
                    iteration: ScopeIteration::Source(projects::path(&["Tree"])),
                    construction: mapping::ScopeConstruction::RecursiveFilter {
                        plan: mapping::RecursiveFilterPlan::new(
                            "directory".into(),
                            "file".into(),
                            2014,
                        )
                        .unwrap(),
                    },
                    ..Default::default()
                },
            );
        }
        Recipe::AdjacencyTree => {
            row_fields(&mut p.source).push(links());
            project_text_consumer(&mut p, 2004);
            project_special(
                &mut p,
                adjacency_target(),
                Scope {
                    construction: mapping::ScopeConstruction::AdjacencyTree {
                        plan: mapping::AdjacencyTreePlan::new(
                            projects::path(&["Links"]),
                            projects::path(&["Key"]),
                            projects::path(&["Parent"]),
                            "name".into(),
                            "children".into(),
                            Some(2003),
                        )
                        .unwrap(),
                    },
                    ..Default::default()
                },
            );
        }
        Recipe::Concatenate => {
            let mut segment = p.root.children[0].children[0].clone();
            segment.target_field.clear();
            segment.iteration = ScopeIteration::Source(vec![]);
            segment.bindings[1].node = 2003;
            p.root.children[0].children[0] = Scope {
                target_field: "Members".into(),
                iteration: ScopeIteration::Concatenate(mapping::ScopeSequence::new(
                    segment.clone(),
                    vec![segment],
                )),
                ..Default::default()
            };
        }
        Recipe::DynamicTarget => {
            let members = target_group_fields(&mut p.target)
                .iter_mut()
                .find(|field| field.name == "Members")
                .unwrap();
            *members = members.clone().with_dynamic_fields(int("*")).unwrap();
            p.root.children[0].children[0]
                .dynamic_bindings
                .push(mapping::DynamicBinding {
                    key: 2004,
                    value: 2003,
                });
        }
        Recipe::GeneratedSibling => {
            p.graph.nodes.insert(
                2015,
                Node::SourceField {
                    path: vec![],
                    frame: None,
                },
            );
            fields(&mut p.target)
                .push(SchemaNode::group("Isolated", vec![int("Value")]).repeating());
            p.root.children.push(Scope {
                target_field: "Isolated".into(),
                iteration: ScopeIteration::Sequence(mapping::SequenceExpr::Generate {
                    from: Some(2000),
                    to: 2001,
                    item: 2015,
                }),
                bindings: projects::bindings(&[("Value", 2003)]),
                ..Default::default()
            });
        }
    }
    p
}

fn program_recipe(base: &codegen::Program, recipe: Recipe) -> codegen::Program {
    let mut p = program_core(base);
    match recipe {
        Recipe::FunctionArgument | Recipe::FunctionInIndex | Recipe::FunctionBody => {
            let owned_body = matches!(recipe, Recipe::FunctionBody);
            program_function(&mut p, owned_body);
            put(
                &mut p,
                2013,
                E::UserFunctionCall {
                    function: mapping::FunctionId::new(42),
                    args: if owned_body {
                        vec![]
                    } else if matches!(recipe, Recipe::FunctionInIndex) {
                        vec![2000]
                    } else {
                        vec![2003]
                    },
                },
            );
            if owned_body {
                p.extra_targets[0].root.bindings[0].expression = 2013;
            } else if matches!(recipe, Recipe::FunctionInIndex) {
                program_index(&mut p, 2013);
            } else {
                p.root.children[0].bindings[1].expression = 2013;
            }
        }
        Recipe::JoinReducer | Recipe::JoinInIndex | Recipe::JoinMember | Recipe::JoinSibling => {
            add_join_schemas(&mut p.source);
            if matches!(recipe, Recipe::JoinReducer | Recipe::JoinInIndex) {
                put(
                    &mut p,
                    2013,
                    E::JoinAggregate {
                        function: codegen::AggregateFunction::Count,
                        join: program_join(),
                        expression: Some(if matches!(recipe, Recipe::JoinInIndex) {
                            2000
                        } else {
                            2003
                        }),
                        arg: None,
                    },
                );
                if matches!(recipe, Recipe::JoinInIndex) {
                    program_index(&mut p, 2013);
                    let iteration = p.root.children[0].iteration.take().unwrap();
                    p.root.children[0].iteration = Some(
                        iteration.with_grouping(codegen::GroupingPlan::AdjacentBy { key: 2003 }),
                    );
                    p.root.children[0].bindings[1].expression = 5;
                } else {
                    let iteration = p.root.children[0].iteration.take().unwrap();
                    p.root.children[0].iteration = Some(
                        iteration.with_grouping(codegen::GroupingPlan::AdjacentBy { key: 2013 }),
                    );
                }
            } else if matches!(recipe, Recipe::JoinMember) {
                program_join_member(&mut p);
            } else {
                fields(&mut p.target)
                    .push(SchemaNode::group("Isolated", vec![int("Value")]).repeating());
                let mut scope = program_scope(codegen::TargetConstruction::Group);
                scope.target_field = "Isolated".into();
                scope.repeating = true;
                scope.iteration = Some(codegen::IterationPlan::join(program_join()));
                scope.bindings.push(codegen::Binding {
                    target_field: "Value".into(),
                    expression: 2003,
                    target_domain: ScalarType::Int.into(),
                    repeating: false,
                });
                p.root.children.push(scope);
            }
        }
        Recipe::DynamicLoader => {
            put(
                &mut p,
                2013,
                E::If {
                    condition: 2006,
                    then: 2005,
                    else_: 2004,
                },
            );
            p.extra_sources[3].dynamic.as_mut().unwrap().path = 2013;
        }
        Recipe::DynamicField => {
            row_fields(&mut p.source).push(open_object());
            put(
                &mut p,
                2013,
                E::DynamicSourceField {
                    object: projects::path(&["Rows", "Open"]),
                    frame: None,
                    key: 2004,
                },
            );
            p.root.children[0].bindings[1].expression = 2013;
        }
        Recipe::DocumentPath => {
            put(&mut p, 2013, E::SourceDocumentPath);
            program_text_consumer(&mut p, 2013);
        }
        Recipe::DocumentAncestor | Recipe::DocumentIndex => {
            if matches!(recipe, Recipe::DocumentIndex) {
                put(
                    &mut p,
                    2011,
                    E::Const {
                        value: Value::String("one.json|two.json".into()),
                    },
                );
                program_text_consumer(&mut p, 2011);
            }
            p.root.iteration = Some(codegen::IterationPlan::dynamic_documents(
                vec![],
                if matches!(recipe, Recipe::DocumentIndex) {
                    2003
                } else {
                    2004
                },
            ));
        }
        Recipe::XmlSerialize => {
            row_fields(&mut p.source).push(xml_element());
            put(
                &mut p,
                2013,
                E::XmlSerialize {
                    path: projects::path(&["Rows", "Xml"]),
                    frame: None,
                    schema: Box::new(xml_element()),
                    declaration: false,
                    indent: false,
                    namespace: None,
                },
            );
            program_text_consumer(&mut p, 2013);
        }
        Recipe::XmlMixedExpression => {
            row_fields(&mut p.source).push(mixed_element("Mixed"));
            put(
                &mut p,
                2013,
                E::XmlMixedContent {
                    path: projects::path(&["Rows", "Mixed"]),
                    frame: None,
                    replacements: vec![],
                },
            );
            program_text_consumer(&mut p, 2013);
        }
        Recipe::XmlMixedConstruction => {
            row_fields(&mut p.source).push(mixed_element("Mixed"));
            let mut scope = program_scope(codegen::TargetConstruction::XmlMixedContent {
                elements: vec![codegen::XmlMixedContentElement {
                    source: "part".into(),
                    target: "part".into(),
                }],
            });
            scope.repeating = true;
            scope.iteration = Some(codegen::IterationPlan::source(projects::path(&["Mixed"])));
            program_special(&mut p, mixed_element("MixedOut").repeating(), scope);
        }
        Recipe::PathHierarchy => {
            row_fields(&mut p.source).push(string("Paths").repeating());
            program_special(
                &mut p,
                directory(),
                program_scope(codegen::TargetConstruction::PathHierarchy {
                    collection: projects::path(&["Paths"]),
                    separator: "/".into(),
                    directories: "directory".into(),
                    files: "file".into(),
                    name: "name".into(),
                }),
            );
        }
        Recipe::RecursiveFilter => {
            row_fields(&mut p.source).push(recursive_source());
            put(
                &mut p,
                2014,
                E::Call {
                    function: codegen::ScalarFunction::Equal,
                    args: vec![2003, 2000],
                },
            );
            let mut scope = program_scope(codegen::TargetConstruction::RecursiveFilter {
                children: "directory".into(),
                items: "file".into(),
                predicate: 2014,
            });
            scope.repeating = true;
            scope.iteration = Some(codegen::IterationPlan::source(projects::path(&["Tree"])));
            program_special(&mut p, recursive_source().repeating(), scope);
        }
        Recipe::AdjacencyTree => {
            row_fields(&mut p.source).push(links());
            program_text_consumer(&mut p, 2004);
            program_special(
                &mut p,
                adjacency_target(),
                program_scope(codegen::TargetConstruction::AdjacencyTree {
                    collection: projects::path(&["Links"]),
                    key: projects::path(&["Key"]),
                    parent: projects::path(&["Parent"]),
                    target_key: "name".into(),
                    target_children: "children".into(),
                    root: Some(2003),
                }),
            );
        }
        Recipe::Concatenate => {
            let mut segment = p.root.children[0].children[0].clone();
            segment.target_field.clear();
            segment.iteration = Some(codegen::IterationPlan::source(vec![]));
            segment.bindings[1].expression = 2003;
            let mut scope = program_scope(codegen::TargetConstruction::Group);
            scope.target_field = "Members".into();
            scope.repeating = true;
            scope.iteration = Some(codegen::IterationPlan::concatenate(
                segment.clone(),
                vec![segment],
                codegen::IterationOutput::Repeated,
            ));
            p.root.children[0].children[0] = scope;
        }
        Recipe::DynamicTarget => {
            let members = target_group_fields(&mut p.target)
                .iter_mut()
                .find(|field| field.name == "Members")
                .unwrap();
            *members = members.clone().with_dynamic_fields(int("*")).unwrap();
            p.root.children[0].children[0].construction =
                codegen::TargetConstruction::DynamicGroup {
                    fixed_fields: projects::path(&["Label", "MemberPosition", "LegacyPosition"]),
                    bindings: vec![codegen::DynamicTargetBinding {
                        key: 2004,
                        value: 2003,
                        target_domain: ScalarType::Int.into(),
                    }],
                    children: vec![],
                    merge: false,
                };
        }
        Recipe::GeneratedSibling => {
            put(
                &mut p,
                2015,
                E::SourceField {
                    path: vec![],
                    frame: None,
                },
            );
            fields(&mut p.target)
                .push(SchemaNode::group("Isolated", vec![int("Value")]).repeating());
            let mut scope = program_scope(codegen::TargetConstruction::Group);
            scope.target_field = "Isolated".into();
            scope.repeating = true;
            scope.iteration = Some(codegen::IterationPlan::generated(G::Range {
                from: Some(2000),
                to: 2001,
                item: 2015,
            }));
            scope.bindings.push(codegen::Binding {
                target_field: "Value".into(),
                expression: 2003,
                target_domain: ScalarType::Int.into(),
                repeating: false,
            });
            p.root.children.push(scope);
        }
    }
    p
}

/// Twenty-one paired Project/Program recipes plus the Project-only owned
/// document-output boundary, which is deliberately refused before IR lowering.
pub fn projects() -> Vec<(&'static str, Project, Vec<codegen::Diagnostic>)> {
    let mut controls = RECIPES
        .into_iter()
        .map(|(id, recipe)| {
            (
                id,
                project_recipe(recipe),
                vec![diagnostic(expected(recipe))],
            )
        })
        .collect::<Vec<_>>();
    let mut p = project_core();
    p.graph.nodes.insert(
        2011,
        Node::Const {
            value: Value::String("one.json|two.json".into()),
        },
    );
    project_text_consumer(&mut p, 2011);
    p.root.iteration = ScopeIteration::OwnedSource {
        source: mapping::OwnedSourceRef {
            owner: mapping::SourceOwner::Primary,
            path: projects::path(&["Rows"]),
        },
        output_path: Some(2003),
    };
    controls.push((
        "Q05S-owned-document-output-project",
        p,
        vec![diagnostic(capability(
            L::Iteration { scope: root() },
            Some(iteration_request()),
            R::Document,
            O::Expression {
                namespace: N::Main,
                node: 2003,
            },
        ))],
    ));
    controls
}

/// The borrowed, already-pruned base is never modified. The direct-only extra
/// keeps an uncalled isolated body, a retained inventory unavailable to Project
/// reachability. Existing closed.rs owns Q06 failure-ItemAt and missing-node
/// controls; these recipes intentionally introduce no missing dependency.
pub fn programs(
    base: &codegen::Program,
) -> Vec<(
    &'static str,
    codegen::Program,
    codegen::ProgramValidationError,
)> {
    let mut controls = RECIPES
        .into_iter()
        .map(|(id, recipe)| (id, program_recipe(base, recipe), expected(recipe)))
        .collect::<Vec<_>>();
    let mut p = program_core(base);
    program_function(&mut p, true);
    controls.push((
        "Q05S-uncalled-isolated-body-direct",
        p,
        function_body_error(),
    ));
    controls
}
