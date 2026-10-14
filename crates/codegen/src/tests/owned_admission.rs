use super::*;
use crate::{
    Binding, NamedInputAdmissionPlan, NamedInputAvailability as A, NamedSourceProgram,
    NamedTargetProgram, TargetConstruction, TargetScope, legacy_named_input_admission_plan,
};
use mapping::{FunctionId, FunctionParameterId, OwnedSourceField, OwnedSourceRef, SourceOwner};

fn path(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|part| (*part).into()).collect()
}
fn schema(name: &str) -> SchemaNode {
    SchemaNode::group(
        name,
        vec![
            scalar("Value"),
            SchemaNode::group("Ref", vec![scalar("Value")]),
            SchemaNode::group(
                "Rows",
                vec![scalar("Value"), typed_scalar("Score", ScalarType::Int)],
            )
            .repeating(),
        ],
    )
}
fn binding(name: &str, node: u32) -> Binding {
    Binding {
        target_field: name.into(),
        expression: node,
        target_domain: ScalarType::String.into(),
        repeating: false,
    }
}
fn scope(bindings: Vec<Binding>) -> TargetScope {
    TargetScope {
        target_field: String::new(),
        repeating: false,
        iteration: None,
        construction: TargetConstruction::Group,
        bindings,
        children: vec![],
    }
}
fn target_schema(scope: &TargetScope, name: &str) -> SchemaNode {
    let mut fields = scope
        .bindings
        .iter()
        .map(|binding| scalar(&binding.target_field))
        .collect::<Vec<_>>();
    fields.extend(
        scope
            .children
            .iter()
            .map(|child| target_schema(child, &child.target_field)),
    );
    let node = SchemaNode::group(name, fields);
    if scope.repeating {
        node.repeating()
    } else {
        node
    }
}
fn input(expressions: Vec<crate::ExpressionNode>, root: TargetScope) -> crate::Program {
    crate::Program {
        xml_boundary: None,
        source: schema("Primary"),
        extra_sources: [
            "Unused",
            "Ref",
            "Other",
            "Guard",
            "Path",
            "Driver",
            "Dynamic",
            " Ref 雪 ",
        ]
        .iter()
        .map(|name| NamedSourceProgram {
            name: (*name).into(),
            source: schema("Static"),
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
fn node(id: u32, expression: Expression) -> crate::ExpressionNode {
    crate::ExpressionNode { id, expression }
}
fn legacy(parts: &[&str], frame: Option<&[&str]>) -> Expression {
    Expression::SourceField {
        path: path(parts),
        frame: frame.map(path),
    }
}
fn owned(name: &str) -> Expression {
    Expression::OwnedSourceField(OwnedSourceField {
        source: OwnedSourceRef {
            owner: SourceOwner::Named { name: name.into() },
            path: path(&["Value"]),
        },
        frame: None,
    })
}
fn constant(id: u32) -> crate::ExpressionNode {
    node(
        id,
        Expression::Const {
            value: Value::String("fixed".into()),
        },
    )
}
fn refresh(program: &mut crate::Program) {
    program.target = target_schema(&program.root, "Output");
}
fn capture<'a>(id: &str, program: &'a crate::Program) -> NamedInputAdmissionPlan<'a> {
    let actual = legacy_named_input_admission_plan(program);
    eprintln!(
        "{id} complete typed Program={program:#?} actual complete admission Result={actual:#?}"
    );
    actual.expect("valid source-generation fixture")
}
fn availability(plan: &crate::NamedInputCatalogue<'_>) -> Vec<A> {
    plan.entries
        .iter()
        .map(|entry| entry.availability)
        .collect()
}
fn visits(id: &str, program: &crate::Program) -> Vec<(Option<FunctionId>, usize, u32)> {
    let actual = crate::owned_admission::primary_visits_for_test(program);
    eprintln!(
        "{id} complete Program={program:#?} actual full namespace/context/node visit Result={actual:#?}"
    );
    actual.expect("validated analysis trace")
}
#[test]
fn complete_catalogues_preserve_original_ordinals_exact_names_and_reserved_dynamic_slots() {
    let mut p = input(
        vec![constant(0), constant(40)],
        scope(vec![binding("Value", 0)]),
    );
    p.extra_sources[6].dynamic = Some(crate::DynamicSourceProgram {
        path: 40,
        driver: SourceIteration::new(vec![]),
    });
    let actual = capture("N536-01", &p);
    assert_eq!(
        actual
            .primary
            .entries
            .iter()
            .map(|entry| (entry.declaration_index, entry.name))
            .collect::<Vec<_>>(),
        vec![
            (0, "Unused"),
            (1, "Ref"),
            (2, "Other"),
            (3, "Guard"),
            (4, "Path"),
            (5, "Driver"),
            (6, "Dynamic"),
            (7, " Ref 雪 ")
        ]
    );
    assert_eq!(
        availability(&actual.primary),
        [
            A::Optional,
            A::Optional,
            A::Optional,
            A::Optional,
            A::Optional,
            A::Optional,
            A::ReservedUnavailable,
            A::Optional
        ]
    );
    assert_eq!(actual.all_outputs, actual.primary);
    assert!(actual.named_targets.is_empty());
    assert!(std::ptr::eq(
        actual.primary.entries[7].name.as_ptr(),
        p.extra_sources[7].name.as_ptr()
    ));
}
#[test]
fn all_outputs_and_each_named_catalogue_keep_unrelated_raise_target_dependencies_separate() {
    let mut p = input(
        vec![
            node(1, owned("Ref")),
            node(2, Expression::Raise { message: Some(3) }),
            node(3, legacy(&["Other", "Value"], None)),
        ],
        scope(vec![binding("Value", 1)]),
    );
    let root = scope(vec![binding("Value", 2)]);
    p.extra_targets.push(NamedTargetProgram {
        name: "raise target".into(),
        target: target_schema(&root, "Named"),
        root,
    });
    let actual = capture("N536-02", &p);
    assert_eq!(availability(&actual.primary), [A::Optional; 8]);
    assert_eq!(
        availability(&actual.all_outputs),
        [
            A::Optional,
            A::Optional,
            A::RequiredLegacy,
            A::Optional,
            A::Optional,
            A::Optional,
            A::Optional,
            A::Optional
        ]
    );
    assert_eq!(actual.named_targets.len(), 1);
    assert_eq!(actual.named_targets[0].name, "raise target");
    assert_eq!(actual.named_targets[0].target_index, 0);
    assert_eq!(actual.named_targets[0].catalogue, actual.all_outputs);
}
#[test]
fn global_failure_dependencies_are_required_in_every_target_catalogue() {
    let mut p = input(
        vec![constant(0), node(1, legacy(&["Other", "Value"], None))],
        scope(vec![binding("Value", 0)]),
    );
    p.failure_rules.push(crate::FailureRule {
        iteration: crate::FailureIteration::Source(SourceIteration::new(path(&["Guard", "Rows"]))),
        selection: crate::FailureSelection::All,
        message: Some(1),
    });
    let root = scope(vec![binding("Value", 0)]);
    p.extra_targets.push(NamedTargetProgram {
        name: "named".into(),
        target: target_schema(&root, "Named"),
        root,
    });
    let actual = capture("N536-03", &p);
    let expected = [
        A::Optional,
        A::Optional,
        A::RequiredLegacy,
        A::RequiredLegacy,
        A::Optional,
        A::Optional,
        A::Optional,
        A::Optional,
    ];
    assert_eq!(availability(&actual.primary), expected);
    assert_eq!(availability(&actual.all_outputs), expected);
    assert_eq!(availability(&actual.named_targets[0].catalogue), expected);
}
#[test]
fn both_potential_if_branches_keep_legacy_eager_reads_and_owned_queries_optional() {
    let p = input(
        vec![
            node(
                0,
                Expression::Const {
                    value: Value::Bool(true),
                },
            ),
            node(1, owned("Ref")),
            node(2, legacy(&["Other", "Value"], None)),
            node(
                3,
                Expression::If {
                    condition: 0,
                    then: 1,
                    else_: 2,
                },
            ),
        ],
        scope(vec![binding("Value", 3)]),
    );
    let actual = capture("N536-04-legacy", &p);
    assert_eq!(
        availability(&actual.primary),
        [
            A::Optional,
            A::Optional,
            A::RequiredLegacy,
            A::Optional,
            A::Optional,
            A::Optional,
            A::Optional,
            A::Optional
        ]
    );
    let mut p = p.clone();
    p.expressions[2].expression = owned("Other");
    let actual = capture("N536-04-owned", &p);
    assert_eq!(availability(&actual.primary), [A::Optional; 8]);
}
#[test]
fn owned_fields_positions_and_owned_selectors_alone_never_make_inputs_eager() {
    let p = input(
        vec![
            node(1, owned("Ref")),
            node(
                2,
                Expression::OwnedPosition(mapping::OwnedPosition {
                    owner: SourceOwner::Named {
                        name: "Other".into(),
                    },
                    collection: None,
                }),
            ),
        ],
        scope(vec![binding("Value", 1)]),
    );
    let actual = capture("N536-05-leaves", &p);
    assert_eq!(availability(&actual.primary), [A::Optional; 8]);
    let mut p = input(
        vec![
            node(
                1,
                Expression::OwnedSourceField(OwnedSourceField {
                    source: OwnedSourceRef {
                        owner: SourceOwner::Named { name: "Ref".into() },
                        path: path(&["Rows", "Value"]),
                    },
                    frame: None,
                }),
            ),
            node(2, legacy(&["Other", "Value"], None)),
        ],
        scope(vec![binding("Owned", 1), binding("Legacy", 2)]),
    );
    p.root.repeating = true;
    p.root.iteration = Some(IterationPlan::owned_source(OwnedSourceRef {
        owner: SourceOwner::Named { name: "Ref".into() },
        path: path(&["Rows"]),
    }));
    refresh(&mut p);
    let actual = capture("N536-05-selector", &p);
    assert_eq!(
        availability(&actual.primary),
        [
            A::Optional,
            A::Optional,
            A::RequiredLegacy,
            A::Optional,
            A::Optional,
            A::Optional,
            A::Optional,
            A::Optional
        ]
    );
}
#[test]
fn frame_overrides_path_including_explicit_empty_and_primary_alias_shadow_stays_conservative() {
    for (frame, expected) in [
        (None, vec![1]),
        (Some(&["Other", "Rows"][..]), vec![2]),
        (Some(&[][..]), vec![]),
    ] {
        let p = input(
            vec![node(1, legacy(&["Ref", "Value"], frame))],
            scope(vec![binding("Value", 1)]),
        );
        let actual = capture("N536-06", &p);
        assert_eq!(
            actual
                .primary
                .entries
                .iter()
                .filter(|e| e.availability == A::RequiredLegacy)
                .map(|e| e.declaration_index)
                .collect::<Vec<_>>(),
            expected
        );
    }
}
#[test]
fn unused_graph_reads_and_uncalled_function_bodies_do_not_create_eager_requirements() {
    let mut p = input(
        vec![constant(0), node(99, legacy(&["Other", "Value"], None))],
        scope(vec![binding("Value", 0)]),
    );
    p.user_functions.push(crate::UserFunctionProgram {
        id: FunctionId::new(7),
        library: "tests".into(),
        name: "unused".into(),
        parameters: vec![],
        output_type: ScalarType::String,
        expressions: vec![constant(99)],
        output: 99,
    });
    let actual = capture("N536-07", &p);
    let trace = visits("N536-07", &p);
    assert_eq!(availability(&actual.primary), [A::Optional; 8]);
    assert!(!trace.iter().any(|(_, _, node)| *node == 99));
}
#[test]
fn namespace_collisions_and_reused_node_in_parent_and_source_item_are_visited_separately() {
    let mut p = input(
        vec![
            node(
                1,
                Expression::UserFunctionCall {
                    function: FunctionId::new(7),
                    args: vec![30],
                },
            ),
            node(20, legacy(&["Other", "Value"], None)),
            constant(30),
        ],
        scope(vec![binding("Call", 1), binding("Main", 20)]),
    );
    p.user_functions.push(crate::UserFunctionProgram {
        id: FunctionId::new(7),
        library: "tests".into(),
        name: "identity".into(),
        parameters: vec![crate::UserFunctionParameter {
            id: FunctionParameterId::new(1),
            ty: ScalarType::String,
        }],
        output_type: ScalarType::String,
        expressions: vec![node(
            20,
            Expression::FunctionParameter {
                parameter: FunctionParameterId::new(1),
            },
        )],
        output: 20,
    });
    let actual = capture("N536-08-namespace", &p);
    let trace = visits("N536-08-namespace", &p);
    assert_eq!(actual.primary.entries[2].availability, A::RequiredLegacy);
    assert!(trace.iter().any(|(ns, _, id)| ns.is_none() && *id == 20));
    assert!(
        trace
            .iter()
            .any(|(ns, _, id)| *ns == Some(FunctionId::new(7)) && *id == 20)
    );
    let mut p = input(
        vec![node(20, legacy(&["Other", "Value"], None))],
        scope(vec![binding("Parent", 20)]),
    );
    let mut child = scope(vec![binding("Value", 20)]);
    child.target_field = "Rows".into();
    child.repeating = true;
    child.iteration = Some(IterationPlan::source(path(&["Ref", "Rows"])));
    p.root.children.push(child);
    refresh(&mut p);
    let actual = capture("N536-08-context", &p);
    let trace = visits("N536-08-context", &p);
    assert_eq!(actual.primary.entries[1].availability, A::RequiredLegacy);
    assert_eq!(actual.primary.entries[2].availability, A::RequiredLegacy);
    let contexts = trace
        .iter()
        .filter(|(ns, _, id)| ns.is_none() && *id == 20)
        .map(|(_, ctx, _)| *ctx)
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(contexts.len(), 2);
}
#[test]
fn demanded_dynamic_paths_and_drivers_close_only_their_legacy_requirements() {
    let mut p = input(
        vec![constant(0), node(40, legacy(&["Path", "Value"], None))],
        scope(vec![binding("Value", 0)]),
    );
    p.extra_sources[6].dynamic = Some(crate::DynamicSourceProgram {
        path: 40,
        driver: SourceIteration::new(path(&["Driver", "Rows"])),
    });
    let inactive = capture("N536-09-unused", &p);
    assert_eq!(
        availability(&inactive.primary),
        [
            A::Optional,
            A::Optional,
            A::Optional,
            A::Optional,
            A::Optional,
            A::Optional,
            A::ReservedUnavailable,
            A::Optional
        ]
    );
    p.root.iteration = Some(IterationPlan::source(path(&["Dynamic", "Rows"])));
    p.root.repeating = true;
    refresh(&mut p);
    let demanded = capture("N536-09-used", &p);
    let trace = visits("N536-09-used", &p);
    assert_eq!(
        availability(&demanded.primary),
        [
            A::Optional,
            A::Optional,
            A::Optional,
            A::Optional,
            A::RequiredLegacy,
            A::RequiredLegacy,
            A::ReservedUnavailable,
            A::Optional
        ]
    );
    assert!(trace.iter().any(|(_, _, id)| *id == 40));
}
#[test]
fn aggregate_and_generated_parent_item_reuse_keeps_contexts_without_reading_item_metadata() {
    let p = input(
        vec![
            node(
                1,
                Expression::Aggregate {
                    function: crate::AggregateFunction::Join,
                    collection: path(&["Rows"]),
                    value: crate::AggregateValue::Expression(20),
                    arg: Some(20),
                },
            ),
            node(20, legacy(&["Other", "Value"], None)),
        ],
        scope(vec![binding("Value", 1)]),
    );
    let actual = capture("N536-10-aggregate", &p);
    let trace = visits("N536-10-aggregate", &p);
    assert_eq!(actual.primary.entries[2].availability, A::RequiredLegacy);
    assert_eq!(
        trace
            .iter()
            .filter(|(ns, _, id)| ns.is_none() && *id == 20)
            .count(),
        2
    );
    let p = input(
        vec![
            node(
                1,
                Expression::SequenceAggregate {
                    function: crate::AggregateFunction::Join,
                    sequence: GeneratedSequence::Tokenize {
                        input: 20,
                        delimiter: 21,
                        item: 30,
                    },
                    predicate: None,
                    expression: Some(20),
                    arg: Some(20),
                },
            ),
            node(20, legacy(&["Other", "Value"], None)),
            node(
                21,
                Expression::Const {
                    value: Value::String(",".into()),
                },
            ),
            node(30, legacy(&[], None)),
        ],
        scope(vec![binding("Value", 1)]),
    );
    let actual = capture("N536-10-generated", &p);
    let trace = visits("N536-10-generated", &p);
    assert_eq!(actual.primary.entries[2].availability, A::RequiredLegacy);
    assert_eq!(
        trace
            .iter()
            .filter(|(ns, _, id)| ns.is_none() && *id == 20)
            .count(),
        2
    );
    assert!(!trace.iter().any(|(_, _, id)| *id == 30));
}
#[test]
fn concatenation_source_segments_keep_both_original_named_requirements() {
    let mut first = scope(vec![binding("Value", 0)]);
    first.repeating = true;
    first.iteration = Some(IterationPlan::source(path(&["Ref", "Rows"])));
    let mut second = first.clone();
    second.iteration = Some(IterationPlan::source(path(&["Other", "Rows"])));
    let mut wrapper = scope(vec![]);
    wrapper.target_field = "Results".into();
    wrapper.repeating = true;
    wrapper.iteration = Some(IterationPlan::concatenate(
        first,
        vec![second],
        crate::IterationOutput::Repeated,
    ));
    let mut root = scope(vec![]);
    root.children.push(wrapper);
    let mut p = input(vec![constant(0)], root);
    p.target = SchemaNode::group(
        "Output",
        vec![SchemaNode::group("Results", vec![scalar("Value")]).repeating()],
    );
    let actual = capture("N536-11", &p);
    let trace = visits("N536-11", &p);
    assert_eq!(
        availability(&actual.primary),
        [
            A::Optional,
            A::RequiredLegacy,
            A::RequiredLegacy,
            A::Optional,
            A::Optional,
            A::Optional,
            A::Optional,
            A::Optional
        ]
    );
    assert_eq!(trace.iter().filter(|(_, _, node)| *node == 0).count(), 2);
}
#[test]
fn helper_forwards_the_complete_existing_first_validation_error_without_selection_errors() {
    let mut p = input(
        vec![node(1, owned("Ref"))],
        scope(vec![binding("Value", 1)]),
    );
    p.extra_sources[1].name = " Unused ".into();
    let expected = validate_program(&p);
    let actual = legacy_named_input_admission_plan(&p);
    eprintln!(
        "N536-12 complete malformed Program={p:#?} existing validation Result={expected:#?} admission Result={actual:#?}"
    );
    assert_eq!(actual.unwrap_err(), expected.unwrap_err());
    let p = input(
        vec![node(1, owned("unknown raw name"))],
        scope(vec![binding("Value", 1)]),
    );
    let expected = validate_program(&p);
    let actual = legacy_named_input_admission_plan(&p);
    eprintln!(
        "N536-12 unknown owner complete Program={p:#?} existing validation Result={expected:#?} admission Result={actual:#?}"
    );
    assert_eq!(actual.unwrap_err(), expected.unwrap_err());
}

#[test]
fn dynamic_path_revisits_have_fixed_environments_and_terminate_without_inherited_growth() {
    let mut p = input(
        vec![
            constant(0),
            node(40, legacy(&["Unused", "Value"], None)),
            node(41, legacy(&["Dynamic", "Value"], None)),
        ],
        scope(vec![binding("Value", 0)]),
    );
    p.extra_sources[0].dynamic = Some(crate::DynamicSourceProgram {
        path: 41,
        driver: SourceIteration::new(vec![]),
    });
    p.extra_sources[6].dynamic = Some(crate::DynamicSourceProgram {
        path: 40,
        driver: SourceIteration::new(vec![]),
    });
    p.root.iteration = Some(IterationPlan::source(path(&["Dynamic", "Rows"])));
    p.root.repeating = true;
    refresh(&mut p);
    let actual = capture("N536-13", &p);
    let trace = visits("N536-13", &p);
    assert_eq!(
        availability(&actual.primary),
        [
            A::ReservedUnavailable,
            A::Optional,
            A::Optional,
            A::Optional,
            A::Optional,
            A::Optional,
            A::ReservedUnavailable,
            A::Optional
        ]
    );
    assert_eq!(trace.iter().filter(|(_, _, id)| *id == 40).count(), 1);
    assert_eq!(trace.iter().filter(|(_, _, id)| *id == 41).count(), 1);
}

// Explicit filter admission remains generation-time borrowed declaration metadata.
mod filter_admission {
    use super::*;
    use crate::{
        OwnedSourceCapabilityOrigin as Origin, OwnedSourceCapabilityReason as Reason,
        OwnedSourceDiagnostic as Diagnostic, OwnedSourceDiagnosticLocation as Location,
        OwnedSourceDiagnosticOwner as Owner, OwnedSourceDiagnosticRequest as Request,
        OwnedSourceNamespace as Namespace, OwnedSourceStructureReason as Structure,
        ProgramValidationError as Error,
        legacy_named_input_admission_plan_with_basic_owned_filters,
    };
    use mapping::{SourceScopeLocation, SourceScopeStep, SourceScopeTarget};

    fn primary() -> SourceScopeLocation {
        SourceScopeLocation::root(SourceScopeTarget::Primary)
    }
    fn query(owner: SourceOwner) -> Expression {
        Expression::OwnedSourceField(OwnedSourceField {
            source: OwnedSourceRef {
                owner,
                path: path(&["Rows", "Value"]),
            },
            frame: Some(path(&["Rows"])),
        })
    }
    fn fixture(owner: Option<SourceOwner>) -> crate::Program {
        let mut p = input(
            vec![
                constant(0),
                constant(1),
                node(
                    2,
                    Expression::Const {
                        value: Value::Bool(true),
                    },
                ),
            ],
            scope(vec![binding("Value", 1)]),
        );
        let source: crate::IterationSource = match owner {
            Some(owner) => {
                p.expressions[1].expression = query(owner.clone());
                crate::OwnedSourceIteration::new(OwnedSourceRef {
                    owner,
                    path: path(&["Rows"]),
                })
                .into()
            }
            None => SourceIteration::new(path(&["Ref", "Rows"])).into(),
        };
        p.root.repeating = true;
        p.root.iteration = Some(IterationPlan::new(
            source,
            Some(2),
            None,
            vec![],
            crate::IterationOutput::Repeated,
        ));
        refresh(&mut p);
        p
    }
    fn predicate(p: &mut crate::Program, id: u32) {
        let source = p.root.iteration.as_ref().unwrap().input().clone();
        p.root.iteration = Some(IterationPlan::new(
            source,
            Some(id),
            None,
            vec![],
            crate::IterationOutput::Repeated,
        ));
    }
    fn observed<'a>(
        label: &str,
        p: &'a crate::Program,
    ) -> Result<NamedInputAdmissionPlan<'a>, Error> {
        let before = format!("{p:#?}");
        let result = legacy_named_input_admission_plan_with_basic_owned_filters(p);
        eprintln!("{label} complete Program={p:#?} complete filter admission Result={result:#?}");
        assert_eq!(format!("{p:#?}"), before);
        result
    }
    fn catalogue_expected<'a>(
        p: &'a crate::Program,
        required: &[usize],
    ) -> crate::NamedInputCatalogue<'a> {
        crate::NamedInputCatalogue {
            entries: p
                .extra_sources
                .iter()
                .enumerate()
                .map(|(index, source)| crate::NamedInputAdmission {
                    declaration_index: index,
                    name: &source.name,
                    availability: if source.dynamic.is_some() {
                        A::ReservedUnavailable
                    } else if required.contains(&index) {
                        A::RequiredLegacy
                    } else {
                        A::Optional
                    },
                })
                .collect(),
        }
    }
    fn expected<'a>(p: &'a crate::Program, required: &[usize]) -> NamedInputAdmissionPlan<'a> {
        NamedInputAdmissionPlan {
            all_outputs: catalogue_expected(p, required),
            primary: catalogue_expected(p, required),
            named_targets: vec![],
        }
    }
    fn scope_error(p: &crate::Program, reason: Reason) -> Error {
        let route = primary();
        Error::OwnedSource {
            diagnostic: Box::new(Diagnostic::Capability {
                location: Location::Iteration {
                    scope: route.clone(),
                },
                request: p
                    .root
                    .iteration
                    .as_ref()
                    .unwrap()
                    .owned_source_iteration()
                    .map(|source| Request::Iteration {
                        owner: (&source.source().owner).into(),
                        path: source.source().path.clone(),
                    }),
                reason,
                origin: Origin::Scope { scope: route },
            }),
        }
    }
    fn retained_error(node: u32, reason: Reason) -> Error {
        Error::OwnedSource {
            diagnostic: Box::new(Diagnostic::Capability {
                location: Location::Expression {
                    namespace: Namespace::Main,
                    node,
                    scope: None,
                },
                request: None,
                reason,
                origin: Origin::RetainedExpression {
                    namespace: Namespace::Main,
                    node,
                },
            }),
        }
    }

    #[test]
    fn control_free_default_results_catalogues_and_exact_borrowed_names_remain_unchanged() {
        for selected in 0..5 {
            let expression = match selected {
                0 => Expression::Const {
                    value: Value::String("fixed".into()),
                },
                1 => owned("Ref"),
                2 => owned(" Ref 雪 "),
                3 => legacy(&["Other", "Value"], None),
                _ => legacy(&["Ref", "Value"], Some(&[])),
            };
            let p = input(vec![node(1, expression)], scope(vec![binding("Value", 1)]));
            let ordinary = legacy_named_input_admission_plan(&p);
            let actual = observed("N657-01 control-free", &p);
            assert_eq!(actual, ordinary);
            let plan = actual.unwrap();
            assert_eq!(plan, expected(&p, if selected == 3 { &[2] } else { &[] }));
            for (index, entry) in plan.primary.entries.iter().enumerate() {
                assert_eq!(
                    (entry.declaration_index, entry.name),
                    (index, p.extra_sources[index].name.as_str())
                );
                assert!(std::ptr::eq(
                    entry.name.as_ptr(),
                    p.extra_sources[index].name.as_ptr()
                ));
            }
        }
    }

    #[test]
    fn filter_predicates_and_owned_legacy_coexistence_keep_original_slots_and_literal_names() {
        for owner in [
            None,
            Some(SourceOwner::Primary),
            Some(SourceOwner::Named { name: "Ref".into() }),
            Some(SourceOwner::Named {
                name: " Ref 雪 ".into(),
            }),
        ] {
            let legacy_selector = owner.is_none();
            let mut p = fixture(owner);
            p.expressions.extend([
                node(3, legacy(&["Guard", "Value"], None)),
                node(
                    4,
                    Expression::Call {
                        function: crate::ScalarFunction::Exists,
                        args: vec![3],
                    },
                ),
                node(99, legacy(&["Driver", "Value"], None)),
            ]);
            predicate(&mut p, 4);
            let actual = observed("N657-02 complete predicate catalogue", &p).unwrap();
            assert_eq!(
                actual,
                expected(&p, if legacy_selector { &[1, 3] } else { &[3] })
            );
            assert_eq!(actual.primary.entries.len(), 8);
            assert_eq!(actual.primary.entries[7].name, " Ref 雪 ");
            assert_eq!(actual.primary.entries[5].availability, A::Optional);
            assert_eq!(
                actual.primary.entries[1].availability,
                if legacy_selector {
                    A::RequiredLegacy
                } else {
                    A::Optional
                }
            );
            assert_eq!(
                legacy_named_input_admission_plan(&p),
                if legacy_selector {
                    Ok(expected(&p, &[1, 3]))
                } else {
                    Err(scope_error(&p, Reason::ScopeControl))
                }
            );
        }
        for name in ["primary", "ref", " Ref 雪 "] {
            let mut p = fixture(Some(SourceOwner::Primary));
            p.extra_sources[7].name = name.into();
            p.expressions.extend([
                node(3, legacy(&[name, "Value"], None)),
                node(
                    4,
                    Expression::Call {
                        function: crate::ScalarFunction::Exists,
                        args: vec![3],
                    },
                ),
            ]);
            predicate(&mut p, 4);
            let actual = observed("N657-03 literal named predicate alias", &p).unwrap();
            assert_eq!(actual, expected(&p, &[7]));
            assert_eq!(actual.primary.entries[7].name, name);
            assert!(std::ptr::eq(
                actual.primary.entries[7].name.as_ptr(),
                p.extra_sources[7].name.as_ptr()
            ));
        }
    }

    #[test]
    fn every_potential_lazy_branch_default_raise_and_value_map_input_remains_eager() {
        for operator in 0..4 {
            let mut p = fixture(Some(SourceOwner::Primary));
            p.expressions.extend([
                node(3, legacy(&["Guard", "Value"], None)),
                node(
                    4,
                    Expression::Call {
                        function: crate::ScalarFunction::Exists,
                        args: vec![3],
                    },
                ),
                node(5, legacy(&["Other", "Value"], None)),
                node(
                    6,
                    Expression::Call {
                        function: crate::ScalarFunction::Exists,
                        args: vec![5],
                    },
                ),
                node(
                    7,
                    match operator {
                        0 => Expression::If {
                            condition: 2,
                            then: 4,
                            else_: 6,
                        },
                        1 => Expression::RuntimeParameterDefault {
                            name: " optional 雪 ".into(),
                            ty: ScalarType::Bool,
                            default: 4,
                        },
                        2 => Expression::Raise { message: Some(3) },
                        _ => Expression::ValueMap {
                            input: 4,
                            input_type: Some(ScalarType::Bool),
                            table: vec![(Value::Bool(false), Value::Bool(true))],
                            default: Some(Value::Bool(true)),
                        },
                    },
                ),
            ]);
            predicate(&mut p, 7);
            assert_eq!(
                observed("N657-04 complete potential predicate edges", &p),
                Ok(expected(&p, if operator == 0 { &[2, 3] } else { &[3] }))
            );
        }
    }

    #[test]
    fn predicate_frame_overrides_path_without_compacting_or_recovering_names() {
        for (frame, required) in [
            (None, vec![3]),
            (Some(&["Other", "Rows"][..]), vec![2]),
            (Some(&[][..]), vec![]),
        ] {
            let mut p = fixture(Some(SourceOwner::Primary));
            p.expressions.extend([
                node(3, legacy(&["Guard", "Value"], frame)),
                node(
                    4,
                    Expression::Call {
                        function: crate::ScalarFunction::Exists,
                        args: vec![3],
                    },
                ),
            ]);
            predicate(&mut p, 4);
            assert_eq!(
                observed("N657-05 exact frame catalogue", &p),
                Ok(expected(&p, &required))
            );
        }
    }

    #[test]
    fn named_targets_and_nested_filters_keep_complete_independent_catalogues() {
        let mut p = fixture(None);
        let mut child = scope(vec![binding("Value", 0)]);
        child.target_field = "Child".into();
        child.repeating = true;
        child.iteration = Some(IterationPlan::new(
            crate::OwnedSourceIteration::new(OwnedSourceRef {
                owner: SourceOwner::Named {
                    name: "Other".into(),
                },
                path: path(&["Rows"]),
            }),
            Some(4),
            None,
            vec![],
            crate::IterationOutput::Repeated,
        ));
        p.root.children.push(child);
        p.expressions.extend([
            node(3, legacy(&["Guard", "Value"], None)),
            node(
                4,
                Expression::Call {
                    function: crate::ScalarFunction::Exists,
                    args: vec![3],
                },
            ),
            node(5, legacy(&["Path", "Value"], None)),
            node(
                6,
                Expression::Call {
                    function: crate::ScalarFunction::Exists,
                    args: vec![5],
                },
            ),
        ]);
        refresh(&mut p);
        let mut named = scope(vec![binding("Value", 0)]);
        named.repeating = true;
        named.iteration = Some(IterationPlan::new(
            SourceIteration::new(path(&[" Ref 雪 ", "Rows"])),
            Some(6),
            None,
            vec![],
            crate::IterationOutput::Repeated,
        ));
        p.extra_targets.push(NamedTargetProgram {
            name: " Target 雪 ".into(),
            target: target_schema(&named, "Named"),
            root: named,
        });
        let actual = observed("N657-06 all outputs and independent named target", &p).unwrap();
        assert_eq!(
            actual,
            NamedInputAdmissionPlan {
                all_outputs: catalogue_expected(&p, &[1, 3, 4, 7]),
                primary: catalogue_expected(&p, &[1, 3]),
                named_targets: vec![crate::NamedTargetInputCatalogue {
                    target_index: 0,
                    name: " Target 雪 ",
                    catalogue: catalogue_expected(&p, &[4, 7]),
                }],
            }
        );
        assert!(std::ptr::eq(
            actual.named_targets[0].name.as_ptr(),
            p.extra_targets[0].name.as_ptr()
        ));
    }

    #[test]
    fn complete_validator_first_errors_and_unselected_owned_structure_are_forwarded_exactly() {
        for malformed in 0..5 {
            let mut p = fixture(Some(SourceOwner::Primary));
            let expected = match malformed {
                0 => {
                    p.source.numeric_range = Some(ir::NumericRange::Integer(
                        ir::IntegerRange::new(Some(1), Some(2)).unwrap(),
                    ));
                    Error::InvalidSchemaMetadata {
                        boundary: "source".into(),
                        path: vec![],
                    }
                }
                1 => {
                    p.expressions.push(node(
                        2,
                        Expression::Const {
                            value: Value::Bool(false),
                        },
                    ));
                    Error::DuplicateExpression { node: 2 }
                }
                2 => {
                    p.expressions.pop();
                    Error::MissingFilterExpression {
                        target_path: vec![],
                        expression: 2,
                    }
                }
                _ => {
                    p.expressions.push(node(99, owned("Absent")));
                    if malformed == 4 {
                        p.expressions.push(node(
                            3,
                            Expression::If {
                                condition: 2,
                                then: 2,
                                else_: 99,
                            },
                        ));
                        predicate(&mut p, 3);
                    }
                    Error::OwnedSource {
                        diagnostic: Box::new(Diagnostic::Structure {
                            location: Location::Expression {
                                namespace: Namespace::Main,
                                node: 99,
                                scope: None,
                            },
                            request: Request::Field {
                                owner: Owner::Named {
                                    name: "Absent".into(),
                                },
                                path: path(&["Value"]),
                                frame: None,
                            },
                            reason: Structure::UnknownOwner,
                        }),
                    }
                }
            };
            assert_eq!(
                crate::validate_program_with_basic_owned_filters(&p),
                Err(expected.clone())
            );
            assert_eq!(observed("N657-07 complete first error", &p), Err(expected));
            assert_eq!(
                legacy_named_input_admission_plan(&p).unwrap_err(),
                validate_program(&p).unwrap_err()
            );
        }
    }

    #[test]
    fn sort_windows_grouping_and_nonrepeated_filter_outputs_keep_full_closed_envelopes() {
        for control in 0..9 {
            let mut p = fixture(Some(SourceOwner::Primary));
            p.expressions.push(node(
                3,
                Expression::Const {
                    value: Value::Int(1),
                },
            ));
            let sort = (control == 0).then(|| {
                crate::SortPlan::new(
                    crate::SortKey {
                        expression: 1,
                        descending: false,
                    },
                    vec![],
                    crate::SortFilterOrder::FilterThenSort,
                )
            });
            let windows = match control {
                1 => vec![crate::SequenceWindow::SkipFirst { count: 3 }],
                2 => vec![crate::SequenceWindow::First { count: 3 }],
                3 => vec![crate::SequenceWindow::From { position: 3 }],
                4 => vec![crate::SequenceWindow::FromTo { first: 3, last: 3 }],
                5 => vec![crate::SequenceWindow::Last { count: 3 }],
                _ => vec![],
            };
            let output = if control == 8 {
                p.root.repeating = false;
                p.target.repeating = false;
                crate::IterationOutput::First
            } else {
                crate::IterationOutput::Repeated
            };
            let source = p.root.iteration.as_ref().unwrap().input().clone();
            let mut iteration = IterationPlan::new(source, Some(2), sort, windows, output);
            if control == 6 {
                iteration = iteration.with_grouping(crate::GroupingPlan::By { key: 1 });
            }
            if control == 7 {
                iteration = iteration.with_filtered_grouping(crate::GroupingPlan::By { key: 1 }, 2);
            }
            p.root.iteration = Some(iteration);
            let expected = scope_error(
                &p,
                if control == 6 || control == 7 {
                    Reason::Grouping
                } else {
                    Reason::ScopeControl
                },
            );
            assert_eq!(
                observed("N657-08 combined controls closed", &p),
                Err(expected.clone())
            );
            assert_eq!(legacy_named_input_admission_plan(&p), Err(expected));
        }
    }

    #[test]
    fn inherited_parent_closure_and_named_target_routes_keep_exact_origin_order() {
        let mut p = fixture(None);
        p.expressions.push(node(
            3,
            Expression::Const {
                value: Value::Int(1),
            },
        ));
        let source = p.root.iteration.as_ref().unwrap().input().clone();
        p.root.iteration = Some(IterationPlan::new(
            source,
            Some(2),
            None,
            vec![crate::SequenceWindow::First { count: 3 }],
            crate::IterationOutput::Repeated,
        ));
        let mut child = scope(vec![binding("Value", 0)]);
        child.target_field = "Child".into();
        child.repeating = true;
        child.iteration = Some(IterationPlan::new(
            crate::OwnedSourceIteration::new(OwnedSourceRef {
                owner: SourceOwner::Named { name: "Ref".into() },
                path: path(&["Rows"]),
            }),
            Some(2),
            None,
            vec![],
            crate::IterationOutput::Repeated,
        ));
        p.root.children.push(child);
        refresh(&mut p);
        let expected = Error::OwnedSource {
            diagnostic: Box::new(Diagnostic::Capability {
                location: Location::Iteration {
                    scope: primary().descendant(SourceScopeStep::Child(0)),
                },
                request: Some(Request::Iteration {
                    owner: Owner::Named { name: "Ref".into() },
                    path: path(&["Rows"]),
                }),
                reason: Reason::ScopeControl,
                origin: Origin::Scope { scope: primary() },
            }),
        };
        assert_eq!(
            observed("N657-09 inherited first parent closure", &p),
            Err(expected)
        );
        let mut p = input(
            vec![
                constant(0),
                node(
                    2,
                    Expression::Const {
                        value: Value::Bool(true),
                    },
                ),
                node(
                    3,
                    Expression::Const {
                        value: Value::Int(1),
                    },
                ),
            ],
            scope(vec![binding("Value", 0)]),
        );
        let mut root = scope(vec![binding("Value", 0)]);
        root.repeating = true;
        root.iteration = Some(IterationPlan::new(
            crate::OwnedSourceIteration::new(OwnedSourceRef {
                owner: SourceOwner::Named { name: "Ref".into() },
                path: path(&["Rows"]),
            }),
            Some(2),
            None,
            vec![crate::SequenceWindow::Last { count: 3 }],
            crate::IterationOutput::Repeated,
        ));
        p.extra_targets.push(NamedTargetProgram {
            name: " Target 雪 ".into(),
            target: target_schema(&root, "Named"),
            root,
        });
        let route = SourceScopeLocation::root(SourceScopeTarget::Named {
            index: 0,
            name: " Target 雪 ".into(),
        });
        let expected = Error::OwnedSource {
            diagnostic: Box::new(Diagnostic::Capability {
                location: Location::Iteration {
                    scope: route.clone(),
                },
                request: Some(Request::Iteration {
                    owner: Owner::Named { name: "Ref".into() },
                    path: path(&["Rows"]),
                }),
                reason: Reason::ScopeControl,
                origin: Origin::Scope { scope: route },
            }),
        };
        assert_eq!(
            observed("N657-10 exact named closure route", &p),
            Err(expected)
        );
    }

    #[test]
    fn generated_document_dynamic_and_isolated_function_domains_stay_closed() {
        let mut p = fixture(None);
        p.expressions.extend([
            node(
                3,
                Expression::Const {
                    value: Value::Int(2),
                },
            ),
            node(4, legacy(&[], None)),
        ]);
        p.root.iteration = Some(IterationPlan::new(
            GeneratedSequence::Range {
                from: None,
                to: 3,
                item: 4,
            },
            Some(2),
            None,
            vec![],
            crate::IterationOutput::Repeated,
        ));
        assert_eq!(
            observed("N657-11 generated source", &p),
            Err(scope_error(&p, Reason::GeneratedItem))
        );
        let mut p = fixture(None);
        p.expressions.push(node(99, Expression::SourceDocumentPath));
        assert_eq!(
            observed("N657-12 stored document leaf", &p),
            Err(retained_error(99, Reason::Document))
        );
        let mut p = fixture(None);
        p.extra_sources[6].dynamic = Some(crate::DynamicSourceProgram {
            path: 0,
            driver: SourceIteration::new(path(&["Rows"])),
        });
        assert_eq!(
            observed("N657-13 unused reserved dynamic declaration", &p),
            Ok(expected(&p, &[1]))
        );
        p.root.iteration = Some(IterationPlan::new(
            SourceIteration::new(path(&["Dynamic", "Rows"])),
            Some(2),
            None,
            vec![],
            crate::IterationOutput::Repeated,
        ));
        assert_eq!(
            observed("N657-14 demanded dynamic source", &p),
            Err(scope_error(&p, Reason::DynamicLoader))
        );
        let mut p = fixture(None);
        p.user_functions.push(crate::UserFunctionProgram {
            id: FunctionId::new(7),
            library: "tests".into(),
            name: "unused".into(),
            parameters: vec![],
            output_type: ScalarType::String,
            expressions: vec![constant(0)],
            output: 0,
        });
        let expected = Error::OwnedSource {
            diagnostic: Box::new(Diagnostic::Capability {
                location: Location::Expression {
                    namespace: Namespace::Function(FunctionId::new(7)),
                    node: 0,
                    scope: None,
                },
                request: None,
                reason: Reason::IsolatedFunction,
                origin: Origin::FunctionBody {
                    function: FunctionId::new(7),
                },
            }),
        };
        assert_eq!(
            observed("N657-15 even uncalled function remains closed", &p),
            Err(expected)
        );
    }

    #[test]
    fn ordinary_failure_ownership_keeps_complete_failure_origin_before_catalogue_analysis() {
        let mut p = fixture(Some(SourceOwner::Primary));
        p.failure_rules.push(crate::FailureRule {
            iteration: crate::FailureIteration::Source(SourceIteration::new(path(&["Rows"]))),
            selection: crate::FailureSelection::All,
            message: Some(1),
        });
        let expected = Error::OwnedSource {
            diagnostic: Box::new(Diagnostic::Capability {
                location: Location::Expression {
                    namespace: Namespace::Main,
                    node: 1,
                    scope: None,
                },
                request: Some(Request::Field {
                    owner: Owner::Primary,
                    path: path(&["Rows", "Value"]),
                    frame: Some(path(&["Rows"])),
                }),
                reason: Reason::FailureRule,
                origin: Origin::Failure { index: 0 },
            }),
        };
        assert_eq!(
            observed("N657-16 ordinary failure ownership stays closed", &p),
            Err(expected)
        );
    }
}

// Explicit sort admission borrows the same complete declaration inventory.
// The planner does not execute keys or impose runtime phase ordering.
mod sort_admission {
    use super::*;
    use crate::{
        NamedInputCatalogue, NamedTargetInputCatalogue, OwnedSourceCapabilityOrigin as Origin,
        OwnedSourceCapabilityReason as Reason, OwnedSourceDiagnostic as Diagnostic,
        OwnedSourceDiagnosticLocation as Location, OwnedSourceDiagnosticOwner as Owner,
        OwnedSourceDiagnosticRequest as Request, OwnedSourceNamespace as Namespace,
        OwnedSourceStructureReason as Structure, ProgramValidationError as Error, SortFilterOrder,
        SortKey, SortPlan, legacy_named_input_admission_plan_with_basic_owned_filters,
        legacy_named_input_admission_plan_with_basic_owned_sort,
        validate_program_with_basic_owned_sort,
    };
    use mapping::{SourceScopeLocation, SourceScopeStep, SourceScopeTarget};

    fn primary() -> SourceScopeLocation {
        SourceScopeLocation::root(SourceScopeTarget::Primary)
    }
    fn row_query(owner: SourceOwner) -> Expression {
        Expression::OwnedSourceField(OwnedSourceField {
            source: OwnedSourceRef {
                owner,
                path: path(&["Rows", "Value"]),
            },
            frame: Some(path(&["Rows"])),
        })
    }
    fn configure(
        p: &mut crate::Program,
        key: u32,
        then: Vec<SortKey>,
        order: SortFilterOrder,
        predicate: Option<u32>,
    ) {
        let source = p.root.iteration.as_ref().unwrap().input().clone();
        p.root.iteration = Some(IterationPlan::new(
            source,
            predicate,
            Some(SortPlan::new(
                SortKey {
                    expression: key,
                    descending: false,
                },
                then,
                order,
            )),
            vec![],
            crate::IterationOutput::Repeated,
        ));
    }
    fn fixture(owner: Option<SourceOwner>, order: SortFilterOrder) -> crate::Program {
        let mut p = input(
            vec![
                constant(0),
                constant(1),
                node(
                    2,
                    Expression::Const {
                        value: Value::Bool(true),
                    },
                ),
                constant(u32::MAX),
            ],
            scope(vec![binding("Value", 1)]),
        );
        let source: crate::IterationSource = match owner {
            Some(owner) => {
                p.expressions[1].expression = row_query(owner.clone());
                crate::OwnedSourceIteration::new(OwnedSourceRef {
                    owner,
                    path: path(&["Rows"]),
                })
                .into()
            }
            None => SourceIteration::new(path(&["Ref", "Rows"])).into(),
        };
        p.root.repeating = true;
        p.root.iteration = Some(IterationPlan::new(
            source,
            None,
            Some(SortPlan::new(
                SortKey {
                    expression: 1,
                    descending: false,
                },
                vec![],
                order,
            )),
            vec![],
            crate::IterationOutput::Repeated,
        ));
        refresh(&mut p);
        p
    }
    fn catalogue<'a>(p: &'a crate::Program, required: &[usize]) -> NamedInputCatalogue<'a> {
        NamedInputCatalogue {
            entries: p
                .extra_sources
                .iter()
                .enumerate()
                .map(|(index, source)| crate::NamedInputAdmission {
                    declaration_index: index,
                    name: &source.name,
                    availability: if source.dynamic.is_some() {
                        A::ReservedUnavailable
                    } else if required.contains(&index) {
                        A::RequiredLegacy
                    } else {
                        A::Optional
                    },
                })
                .collect(),
        }
    }
    fn expected<'a>(
        p: &'a crate::Program,
        primary_required: &[usize],
        all_required: &[usize],
        named: &[(usize, &[usize])],
    ) -> NamedInputAdmissionPlan<'a> {
        NamedInputAdmissionPlan {
            all_outputs: catalogue(p, all_required),
            primary: catalogue(p, primary_required),
            named_targets: named
                .iter()
                .map(|(index, required)| NamedTargetInputCatalogue {
                    target_index: *index,
                    name: &p.extra_targets[*index].name,
                    catalogue: catalogue(p, required),
                })
                .collect(),
        }
    }
    fn observed<'a>(
        label: &str,
        p: &'a crate::Program,
    ) -> Result<NamedInputAdmissionPlan<'a>, Error> {
        let before = format!("{p:#?}");
        let actual = legacy_named_input_admission_plan_with_basic_owned_sort(p);
        eprintln!("{label} complete Program={p:#?} complete sort admission Result={actual:#?}");
        assert_eq!(format!("{p:#?}"), before);
        actual
    }
    fn borrowed(p: &crate::Program, plan: &NamedInputAdmissionPlan<'_>) {
        for entries in std::iter::once(&plan.all_outputs)
            .chain(std::iter::once(&plan.primary))
            .chain(plan.named_targets.iter().map(|target| &target.catalogue))
        {
            assert_eq!(entries.entries.len(), p.extra_sources.len());
            for (index, entry) in entries.entries.iter().enumerate() {
                assert_eq!(
                    (entry.declaration_index, entry.name),
                    (index, p.extra_sources[index].name.as_str())
                );
                assert!(std::ptr::eq(
                    entry.name.as_ptr(),
                    p.extra_sources[index].name.as_ptr()
                ));
            }
        }
        for (index, entry) in plan.named_targets.iter().enumerate() {
            assert_eq!(entry.target_index, index);
            assert!(std::ptr::eq(
                entry.name.as_ptr(),
                p.extra_targets[index].name.as_ptr()
            ));
        }
    }
    fn scope_error(p: &crate::Program, reason: Reason) -> Error {
        Error::OwnedSource {
            diagnostic: Box::new(Diagnostic::Capability {
                location: Location::Iteration { scope: primary() },
                request: p
                    .root
                    .iteration
                    .as_ref()
                    .and_then(IterationPlan::owned_source_iteration)
                    .map(|source| Request::Iteration {
                        owner: (&source.source().owner).into(),
                        path: source.source().path.clone(),
                    }),
                reason,
                origin: Origin::Scope { scope: primary() },
            }),
        }
    }

    #[test]
    fn sort_control_free_old_apis_and_exact_borrowed_catalogues_remain_unchanged() {
        for selected in 0..5 {
            let expression = match selected {
                0 => Expression::Const {
                    value: Value::String("fixed".into()),
                },
                1 => owned("Ref"),
                2 => owned(" Ref 雪 "),
                3 => legacy(&["Other", "Value"], None),
                _ => legacy(&["Ref", "Value"], Some(&[])),
            };
            let mut p = input(
                vec![node(1, expression), constant(40)],
                scope(vec![binding("Value", 1)]),
            );
            p.extra_sources[6].dynamic = Some(crate::DynamicSourceProgram {
                path: 40,
                driver: SourceIteration::new(vec![]),
            });
            let actual = observed("N667-A01 control-free", &p);
            assert_eq!(actual, legacy_named_input_admission_plan(&p));
            assert_eq!(
                actual,
                legacy_named_input_admission_plan_with_basic_owned_filters(&p)
            );
            let required: &[usize] = if selected == 3 { &[2] } else { &[] };
            assert_eq!(actual, Ok(expected(&p, required, required, &[])));
            borrowed(&p, actual.as_ref().unwrap());
        }
        let p = fixture(Some(SourceOwner::Primary), SortFilterOrder::SortThenFilter);
        assert_eq!(
            legacy_named_input_admission_plan(&p),
            Err(scope_error(&p, Reason::ScopeControl))
        );
        assert_eq!(
            legacy_named_input_admission_plan_with_basic_owned_filters(&p),
            Err(scope_error(&p, Reason::ScopeControl))
        );
        assert_eq!(
            observed("N667-A01 explicit sort", &p),
            Ok(expected(&p, &[], &[], &[]))
        );
    }

    #[test]
    fn sort_owners_key_ids_literal_duplicates_and_frame_pins_keep_all_original_slots() {
        for order in [
            SortFilterOrder::SortThenFilter,
            SortFilterOrder::FilterThenSort,
        ] {
            for owner in [
                None,
                Some(SourceOwner::Primary),
                Some(SourceOwner::Named { name: "Ref".into() }),
                Some(SourceOwner::Named {
                    name: " Ref 雪 ".into(),
                }),
            ] {
                let legacy_selector = owner.is_none();
                let mut p = fixture(owner, order);
                p.expressions
                    .iter_mut()
                    .find(|entry| entry.id == u32::MAX)
                    .unwrap()
                    .expression = legacy(&["Guard", "Value"], None);
                configure(
                    &mut p,
                    0,
                    vec![SortKey {
                        expression: u32::MAX,
                        descending: true,
                    }],
                    order,
                    Some(2),
                );
                let required: &[usize] = if legacy_selector { &[1, 3] } else { &[3] };
                let actual = observed("N667-A02 complete sort owners", &p);
                assert_eq!(actual, Ok(expected(&p, required, required, &[])));
                borrowed(&p, actual.as_ref().unwrap());
            }
            for (frame, required) in [
                (None, vec![3]),
                (Some(&["Other", "Rows"][..]), vec![2]),
                (Some(&[][..]), vec![]),
            ] {
                let mut p = fixture(Some(SourceOwner::Primary), order);
                p.expressions
                    .iter_mut()
                    .find(|entry| entry.id == u32::MAX)
                    .unwrap()
                    .expression = legacy(&["Guard", "Value"], frame);
                configure(&mut p, u32::MAX, vec![], order, None);
                assert_eq!(
                    observed("N667-A02 key frame override", &p),
                    Ok(expected(&p, &required, &required, &[]))
                );
            }
            for name in ["primary", "ref", " Ref 雪 "] {
                let mut p = fixture(Some(SourceOwner::Primary), order);
                p.extra_sources[7].name = name.into();
                p.expressions
                    .iter_mut()
                    .find(|entry| entry.id == u32::MAX)
                    .unwrap()
                    .expression = legacy(&[name, "Value"], None);
                configure(&mut p, u32::MAX, vec![], order, None);
                let actual = observed("N667-A02 literal named key", &p);
                assert_eq!(actual, Ok(expected(&p, &[7], &[7], &[])));
                borrowed(&p, actual.as_ref().unwrap());
            }
            let mut p = fixture(Some(SourceOwner::Primary), order);
            p.expressions
                .iter_mut()
                .find(|entry| entry.id == u32::MAX)
                .unwrap()
                .expression = legacy(&["Ref", "Value"], None);
            configure(&mut p, u32::MAX, vec![], order, None);
            let actual = observed("N667-A02 literal Ref and primary schema collision", &p);
            assert_eq!(actual, Ok(expected(&p, &[1], &[1], &[])));
            borrowed(&p, actual.as_ref().unwrap());
            p.extra_sources[0].name = "Ref".into();
            let duplicate = Error::DuplicateExtraSourceName {
                name: "Ref".into(),
                first: 0,
                duplicate: 1,
            };
            assert_eq!(
                validate_program_with_basic_owned_sort(&p),
                Err(duplicate.clone())
            );
            assert_eq!(
                observed(
                    "N667-A02 duplicate literal Ref remains a typed structural rejection",
                    &p
                ),
                Err(duplicate)
            );
        }
    }

    #[test]
    fn sort_potential_if_and_value_map_keys_and_scalar_body_keep_whole_catalogues() {
        for order in [
            SortFilterOrder::SortThenFilter,
            SortFilterOrder::FilterThenSort,
        ] {
            let mut p = fixture(Some(SourceOwner::Primary), order);
            p.expressions
                .iter_mut()
                .find(|entry| entry.id == 2)
                .unwrap()
                .expression = Expression::Const {
                value: Value::Bool(false),
            };
            p.expressions.extend([
                node(3, legacy(&["Guard", "Value"], None)),
                node(4, legacy(&["Other", "Value"], None)),
                node(
                    5,
                    Expression::ValueMap {
                        input: 3,
                        input_type: Some(ScalarType::String),
                        table: vec![(
                            Value::String("fixed".into()),
                            Value::String("mapped".into()),
                        )],
                        default: Some(Value::String("fallback".into())),
                    },
                ),
                node(
                    6,
                    Expression::If {
                        condition: 2,
                        then: 5,
                        else_: 4,
                    },
                ),
                node(99, legacy(&["Driver", "Value"], None)),
            ]);
            configure(
                &mut p,
                6,
                vec![SortKey {
                    expression: 5,
                    descending: true,
                }],
                order,
                Some(2),
            );
            assert_eq!(
                observed("N667-A03 inactive potential keys", &p),
                Ok(expected(&p, &[2, 3], &[2, 3], &[]))
            );
            let mut p = fixture(Some(SourceOwner::Primary), order);
            p.expressions
                .push(node(3, legacy(&["Guard", "Value"], None)));
            p.expressions
                .iter_mut()
                .find(|entry| entry.id == u32::MAX)
                .unwrap()
                .expression = legacy(&["Other", "Value"], None);
            configure(
                &mut p,
                1,
                vec![SortKey {
                    expression: u32::MAX,
                    descending: false,
                }],
                order,
                None,
            );
            p.root.bindings.clear();
            p.root.construction = TargetConstruction::Scalar {
                expression: 3,
                target_domain: ScalarType::String.into(),
            };
            p.target = typed_scalar("Output", ScalarType::String).repeating();
            let actual = observed("N667-A03 sorted scalar body", &p);
            assert_eq!(actual, Ok(expected(&p, &[2, 3], &[2, 3], &[])));
            borrowed(&p, actual.as_ref().unwrap());
            assert_eq!(
                legacy_named_input_admission_plan(&p),
                Err(scope_error(&p, Reason::ScopeControl))
            );
            assert_eq!(
                legacy_named_input_admission_plan_with_basic_owned_filters(&p),
                Err(scope_error(&p, Reason::ScopeControl))
            );
            let source = p.root.iteration.as_ref().unwrap().input().clone();
            p.root.iteration = Some(IterationPlan::new(
                source,
                None,
                None,
                vec![],
                crate::IterationOutput::Repeated,
            ));
            let error = scope_error(&p, Reason::SpecialConstruction);
            assert_eq!(
                observed("N667-A03 unsorted scalar still closed", &p),
                Err(error.clone())
            );
            assert_eq!(legacy_named_input_admission_plan(&p), Err(error.clone()));
            assert_eq!(
                legacy_named_input_admission_plan_with_basic_owned_filters(&p),
                Err(error)
            );
        }
    }

    #[test]
    fn sort_primary_nested_named_and_failure_catalogues_remain_complete_and_independent() {
        let mut p = fixture(Some(SourceOwner::Primary), SortFilterOrder::SortThenFilter);
        p.expressions.extend([
            node(3, legacy(&["Ref", "Value"], None)),
            node(4, legacy(&["Path", "Value"], None)),
            node(6, legacy(&["Other", "Value"], None)),
            node(
                7,
                row_query(SourceOwner::Named {
                    name: "Other".into(),
                }),
            ),
            node(8, legacy(&["Driver", "Value"], None)),
        ]);
        configure(&mut p, 3, vec![], SortFilterOrder::SortThenFilter, None);
        let mut child = scope(vec![binding("Value", 7)]);
        child.target_field = "Child".into();
        child.repeating = true;
        child.iteration = Some(IterationPlan::new(
            crate::OwnedSourceIteration::new(OwnedSourceRef {
                owner: SourceOwner::Named {
                    name: "Other".into(),
                },
                path: path(&["Rows"]),
            }),
            None,
            Some(SortPlan::new(
                SortKey {
                    expression: 8,
                    descending: false,
                },
                vec![],
                SortFilterOrder::FilterThenSort,
            )),
            vec![],
            crate::IterationOutput::Repeated,
        ));
        p.root.children.push(child);
        refresh(&mut p);
        let mut named = scope(vec![binding("Value", 0)]);
        named.repeating = true;
        named.iteration = Some(IterationPlan::new(
            crate::OwnedSourceIteration::new(OwnedSourceRef {
                owner: SourceOwner::Named { name: "Ref".into() },
                path: path(&["Rows"]),
            }),
            None,
            Some(SortPlan::new(
                SortKey {
                    expression: 4,
                    descending: true,
                },
                vec![],
                SortFilterOrder::SortThenFilter,
            )),
            vec![],
            crate::IterationOutput::Repeated,
        ));
        p.extra_targets.push(NamedTargetProgram {
            name: " Target 雪 ".into(),
            target: target_schema(&named, "Named"),
            root: named,
        });
        p.failure_rules.push(crate::FailureRule {
            iteration: crate::FailureIteration::Source(SourceIteration::new(path(&[
                "Guard", "Rows",
            ]))),
            selection: crate::FailureSelection::All,
            message: Some(6),
        });
        let actual = observed("N667-A04 target separation with global legacy failure", &p);
        assert_eq!(
            actual,
            Ok(expected(
                &p,
                &[1, 2, 3, 5],
                &[1, 2, 3, 4, 5],
                &[(0, &[2, 3, 4])]
            ))
        );
        borrowed(&p, actual.as_ref().unwrap());
    }

    #[test]
    fn sort_forwards_complete_typed_validation_errors_and_inherited_closure() {
        for malformed in 0..5 {
            let mut p = fixture(Some(SourceOwner::Primary), SortFilterOrder::SortThenFilter);
            let error = match malformed {
                0 => {
                    configure(
                        &mut p,
                        1,
                        vec![SortKey {
                            expression: 99,
                            descending: false,
                        }],
                        SortFilterOrder::SortThenFilter,
                        None,
                    );
                    Error::MissingSortExpression {
                        target_path: vec![],
                        key: 1,
                        expression: 99,
                    }
                }
                1 => {
                    p.expressions.push(node(
                        2,
                        Expression::Const {
                            value: Value::Bool(false),
                        },
                    ));
                    Error::DuplicateExpression { node: 2 }
                }
                2 => {
                    p.expressions.push(node(99, owned("Absent")));
                    p.expressions.push(node(
                        3,
                        Expression::If {
                            condition: 2,
                            then: 1,
                            else_: 99,
                        },
                    ));
                    configure(&mut p, 3, vec![], SortFilterOrder::SortThenFilter, None);
                    Error::OwnedSource {
                        diagnostic: Box::new(Diagnostic::Structure {
                            location: Location::Expression {
                                namespace: Namespace::Main,
                                node: 99,
                                scope: None,
                            },
                            request: Request::Field {
                                owner: Owner::Named {
                                    name: "Absent".into(),
                                },
                                path: path(&["Value"]),
                                frame: None,
                            },
                            reason: Structure::UnknownOwner,
                        }),
                    }
                }
                _ => {
                    let source = p.root.iteration.as_ref().unwrap().input().clone();
                    let output = if malformed == 3 {
                        p.root.repeating = false;
                        crate::IterationOutput::First
                    } else {
                        crate::IterationOutput::Repeated
                    };
                    p.root.iteration = Some(IterationPlan::new(
                        source,
                        None,
                        Some(SortPlan::new(
                            SortKey {
                                expression: 1,
                                descending: false,
                            },
                            vec![],
                            SortFilterOrder::SortThenFilter,
                        )),
                        if malformed == 4 {
                            vec![crate::SequenceWindow::First { count: 0 }]
                        } else {
                            vec![]
                        },
                        output,
                    ));
                    refresh(&mut p);
                    scope_error(&p, Reason::ScopeControl)
                }
            };
            assert_eq!(
                validate_program_with_basic_owned_sort(&p),
                Err(error.clone())
            );
            assert_eq!(
                observed("N667-A05 complete first validation error", &p),
                Err(error)
            );
            assert_eq!(
                legacy_named_input_admission_plan(&p).unwrap_err(),
                validate_program(&p).unwrap_err()
            );
        }
        let mut p = fixture(None, SortFilterOrder::SortThenFilter);
        p.root.bindings = vec![binding("Value", 0)];
        p.root.iteration = Some(IterationPlan::new(
            SourceIteration::new(path(&["Rows"])),
            None,
            None,
            vec![crate::SequenceWindow::First { count: 0 }],
            crate::IterationOutput::Repeated,
        ));
        let mut child = scope(vec![]);
        child.target_field = "Child".into();
        child.repeating = true;
        child.construction = TargetConstruction::Scalar {
            expression: 1,
            target_domain: ScalarType::String.into(),
        };
        child.iteration = Some(IterationPlan::new(
            crate::OwnedSourceIteration::new(OwnedSourceRef {
                owner: SourceOwner::Primary,
                path: path(&["Rows"]),
            }),
            None,
            Some(SortPlan::new(
                SortKey {
                    expression: 0,
                    descending: false,
                },
                vec![],
                SortFilterOrder::SortThenFilter,
            )),
            vec![],
            crate::IterationOutput::Repeated,
        ));
        p.expressions[1].expression = row_query(SourceOwner::Primary);
        p.root.children.push(child);
        p.target = SchemaNode::group(
            "Output",
            vec![
                scalar("Value"),
                typed_scalar("Child", ScalarType::String).repeating(),
            ],
        )
        .repeating();
        let error = Error::OwnedSource {
            diagnostic: Box::new(Diagnostic::Capability {
                location: Location::Iteration {
                    scope: primary().descendant(SourceScopeStep::Child(0)),
                },
                request: Some(Request::Iteration {
                    owner: Owner::Primary,
                    path: path(&["Rows"]),
                }),
                reason: Reason::ScopeControl,
                origin: Origin::Scope { scope: primary() },
            }),
        };
        assert_eq!(
            validate_program_with_basic_owned_sort(&p),
            Err(error.clone())
        );
        assert_eq!(
            observed(
                "N667-A05 inherited window remains closed for sorted scalar",
                &p
            ),
            Err(error)
        );
    }
}

mod window_admission {
    use super::*;
    use crate::{
        IterationOutput, NamedInputCatalogue, NamedTargetInputCatalogue,
        OwnedSourceCapabilityOrigin as Origin, OwnedSourceCapabilityReason as Reason,
        OwnedSourceDiagnostic as Diagnostic, OwnedSourceDiagnosticLocation as Location,
        OwnedSourceDiagnosticOwner as Owner, OwnedSourceDiagnosticRequest as Request,
        OwnedSourceNamespace as Namespace, OwnedSourceStructureReason as Structure,
        ProgramValidationError as Error, SequenceWindow, SortFilterOrder, SortKey, SortPlan,
        legacy_named_input_admission_plan_with_basic_owned_filters,
        legacy_named_input_admission_plan_with_basic_owned_sort,
        legacy_named_input_admission_plan_with_basic_owned_windows,
        validate_program_with_basic_owned_windows,
    };
    use mapping::{SourceScopeLocation, SourceScopeStep, SourceScopeTarget};

    fn primary() -> SourceScopeLocation {
        SourceScopeLocation::root(SourceScopeTarget::Primary)
    }
    fn row_query(owner: SourceOwner) -> Expression {
        Expression::OwnedSourceField(OwnedSourceField {
            source: OwnedSourceRef {
                owner,
                path: path(&["Rows", "Value"]),
            },
            frame: Some(path(&["Rows"])),
        })
    }
    fn fixture(owner: Option<SourceOwner>) -> crate::Program {
        let mut p = input(
            vec![
                constant(0),
                constant(1),
                node(
                    2,
                    Expression::Const {
                        value: Value::Bool(true),
                    },
                ),
                node(
                    3,
                    Expression::Const {
                        value: Value::Int(2),
                    },
                ),
                node(
                    4,
                    Expression::Const {
                        value: Value::Int(3),
                    },
                ),
                constant(u32::MAX),
            ],
            scope(vec![binding("Value", 1)]),
        );
        let source: crate::IterationSource = match owner {
            Some(owner) => {
                p.expressions[1].expression = row_query(owner.clone());
                crate::OwnedSourceIteration::new(OwnedSourceRef {
                    owner,
                    path: path(&["Rows"]),
                })
                .into()
            }
            None => SourceIteration::new(path(&["Ref", "Rows"])).into(),
        };
        p.root.iteration = Some(IterationPlan::new(
            source,
            None,
            None,
            vec![],
            IterationOutput::Repeated,
        ));
        configure(&mut p, None, None, vec![], IterationOutput::Repeated);
        p
    }
    fn configure(
        p: &mut crate::Program,
        predicate: Option<u32>,
        sort: Option<SortPlan>,
        windows: Vec<SequenceWindow>,
        output: IterationOutput,
    ) {
        let source = p.root.iteration.as_ref().unwrap().input().clone();
        p.root.iteration = Some(IterationPlan::new(source, predicate, sort, windows, output));
        p.root.repeating = output == IterationOutput::Repeated;
        p.target = match &p.root.construction {
            TargetConstruction::Scalar { .. } => {
                let target = typed_scalar("Output", ScalarType::String);
                if p.root.repeating {
                    target.repeating()
                } else {
                    target
                }
            }
            _ => target_schema(&p.root, "Output"),
        };
    }
    fn sort(key: u32, secondary: &[u32], order: SortFilterOrder) -> SortPlan {
        SortPlan::new(
            SortKey {
                expression: key,
                descending: false,
            },
            secondary
                .iter()
                .map(|expression| SortKey {
                    expression: *expression,
                    descending: true,
                })
                .collect(),
            order,
        )
    }
    fn catalogue<'a>(p: &'a crate::Program, required: &[usize]) -> NamedInputCatalogue<'a> {
        NamedInputCatalogue {
            entries: p
                .extra_sources
                .iter()
                .enumerate()
                .map(|(index, source)| crate::NamedInputAdmission {
                    declaration_index: index,
                    name: &source.name,
                    availability: if source.dynamic.is_some() {
                        A::ReservedUnavailable
                    } else if required.contains(&index) {
                        A::RequiredLegacy
                    } else {
                        A::Optional
                    },
                })
                .collect(),
        }
    }
    fn expected<'a>(
        p: &'a crate::Program,
        primary_required: &[usize],
        all_required: &[usize],
        named: &[(usize, &[usize])],
    ) -> NamedInputAdmissionPlan<'a> {
        NamedInputAdmissionPlan {
            all_outputs: catalogue(p, all_required),
            primary: catalogue(p, primary_required),
            named_targets: named
                .iter()
                .map(|(index, required)| NamedTargetInputCatalogue {
                    target_index: *index,
                    name: &p.extra_targets[*index].name,
                    catalogue: catalogue(p, required),
                })
                .collect(),
        }
    }
    fn observed<'a>(
        label: &str,
        p: &'a crate::Program,
    ) -> Result<NamedInputAdmissionPlan<'a>, Error> {
        let before = format!("{p:#?}");
        let actual = legacy_named_input_admission_plan_with_basic_owned_windows(p);
        eprintln!("{label} complete Program={p:#?} complete window admission Result={actual:#?}");
        assert_eq!(format!("{p:#?}"), before);
        actual
    }
    fn traced(
        label: &str,
        p: &crate::Program,
    ) -> Result<Vec<(Option<FunctionId>, usize, u32)>, Error> {
        let before = format!("{p:#?}");
        let actual = crate::owned_admission::primary_visits_for_test_with_basic_owned_windows(p);
        eprintln!(
            "{label} complete Program={p:#?} complete window namespace/context/node visits={actual:#?}"
        );
        assert_eq!(format!("{p:#?}"), before);
        actual
    }
    fn borrowed(p: &crate::Program, plan: &NamedInputAdmissionPlan<'_>) {
        for entries in std::iter::once(&plan.all_outputs)
            .chain(std::iter::once(&plan.primary))
            .chain(plan.named_targets.iter().map(|target| &target.catalogue))
        {
            assert_eq!(entries.entries.len(), p.extra_sources.len());
            for (index, entry) in entries.entries.iter().enumerate() {
                assert_eq!(
                    (entry.declaration_index, entry.name),
                    (index, p.extra_sources[index].name.as_str())
                );
                assert!(std::ptr::eq(
                    entry.name.as_ptr(),
                    p.extra_sources[index].name.as_ptr()
                ));
            }
        }
        for (index, entry) in plan.named_targets.iter().enumerate() {
            assert_eq!(entry.target_index, index);
            assert_eq!(entry.name, p.extra_targets[index].name.as_str());
            assert!(std::ptr::eq(
                entry.name.as_ptr(),
                p.extra_targets[index].name.as_ptr()
            ));
        }
    }
    fn scope_error(p: &crate::Program, reason: Reason) -> Error {
        Error::OwnedSource {
            diagnostic: Box::new(Diagnostic::Capability {
                location: Location::Iteration { scope: primary() },
                request: p
                    .root
                    .iteration
                    .as_ref()
                    .and_then(IterationPlan::owned_source_iteration)
                    .map(|source| Request::Iteration {
                        owner: (&source.source().owner).into(),
                        path: source.source().path.clone(),
                    }),
                reason,
                origin: Origin::Scope { scope: primary() },
            }),
        }
    }

    #[test]
    fn window_operators_first_scalar_and_frame_pins_keep_complete_borrowed_catalogues() {
        for order in [
            SortFilterOrder::SortThenFilter,
            SortFilterOrder::FilterThenSort,
        ] {
            for owner in [
                None,
                Some(SourceOwner::Primary),
                Some(SourceOwner::Named { name: "Ref".into() }),
                Some(SourceOwner::Named {
                    name: " Ref 雪 ".into(),
                }),
            ] {
                let legacy_source = owner.is_none();
                for control in 0..8 {
                    let mut p = fixture(owner.clone());
                    p.expressions[3].expression = legacy(&["Guard", "Value"], None);
                    let windows = match control {
                        0 => vec![SequenceWindow::SkipFirst { count: 3 }],
                        1 => vec![SequenceWindow::First { count: 3 }],
                        2 => vec![SequenceWindow::From { position: 3 }],
                        3 => vec![SequenceWindow::FromTo { first: 3, last: 4 }],
                        4 => vec![SequenceWindow::Last { count: 3 }],
                        5 | 7 => vec![
                            SequenceWindow::Last { count: 3 },
                            SequenceWindow::From { position: 4 },
                        ],
                        _ => vec![],
                    };
                    let first = control == 6 || control == 7;
                    if control == 5 {
                        p.root.bindings.clear();
                        p.root.construction = TargetConstruction::Scalar {
                            expression: 1,
                            target_domain: ScalarType::String.into(),
                        };
                    }
                    configure(
                        &mut p,
                        Some(2),
                        Some(sort(0, &[4], order)),
                        windows,
                        if first {
                            IterationOutput::First
                        } else {
                            IterationOutput::Repeated
                        },
                    );
                    let required: &[usize] = match (legacy_source, control == 6) {
                        (true, true) => &[1],
                        (true, false) => &[1, 3],
                        (false, true) => &[],
                        (false, false) => &[3],
                    };
                    let actual = observed("N698-A01 ordered operators and output shape", &p);
                    assert_eq!(actual, Ok(expected(&p, required, required, &[])));
                    borrowed(&p, actual.as_ref().unwrap());
                    if !legacy_source {
                        let error = scope_error(&p, Reason::ScopeControl);
                        assert_eq!(legacy_named_input_admission_plan(&p), Err(error.clone()));
                        assert_eq!(
                            legacy_named_input_admission_plan_with_basic_owned_filters(&p),
                            Err(error.clone())
                        );
                        assert_eq!(
                            legacy_named_input_admission_plan_with_basic_owned_sort(&p),
                            Err(error)
                        );
                    }
                }
            }
        }
        for (frame, required) in [
            (None, vec![3]),
            (Some(&["Other", "Rows"][..]), vec![2]),
            (Some(&[][..]), vec![]),
        ] {
            let mut p = fixture(Some(SourceOwner::Primary));
            p.expressions[3].expression = legacy(&["Guard", "Value"], frame);
            configure(
                &mut p,
                None,
                None,
                vec![SequenceWindow::First { count: 3 }],
                IterationOutput::Repeated,
            );
            let actual = observed("N698-A01 parent bound explicit frame pin", &p);
            assert_eq!(actual, Ok(expected(&p, &required, &required, &[])));
            borrowed(&p, actual.as_ref().unwrap());
        }
        let mut p = fixture(Some(SourceOwner::Primary));
        p.expressions[3].expression = owned("Guard");
        configure(
            &mut p,
            None,
            None,
            vec![SequenceWindow::First { count: 3 }],
            IterationOutput::Repeated,
        );
        let actual = observed("N698-A01 explicit owned bound remains demand-driven", &p);
        assert_eq!(actual, Ok(expected(&p, &[], &[], &[])));
        borrowed(&p, actual.as_ref().unwrap());
        let mut p = input(
            vec![node(1, owned("Ref")), constant(40)],
            scope(vec![binding("Value", 1)]),
        );
        p.extra_sources[6].dynamic = Some(crate::DynamicSourceProgram {
            path: 40,
            driver: SourceIteration::new(vec![]),
        });
        let actual = observed("N698-A01 old control-free reserved slot", &p);
        assert_eq!(actual, Ok(expected(&p, &[], &[], &[])));
        assert_eq!(actual, legacy_named_input_admission_plan(&p));
        assert_eq!(
            actual,
            legacy_named_input_admission_plan_with_basic_owned_filters(&p)
        );
        assert_eq!(
            actual,
            legacy_named_input_admission_plan_with_basic_owned_sort(&p)
        );
        borrowed(&p, actual.as_ref().unwrap());
    }

    #[test]
    fn window_bounds_parent_and_item_dual_visits_preserve_outer_environments() {
        for order in [
            SortFilterOrder::SortThenFilter,
            SortFilterOrder::FilterThenSort,
        ] {
            let mut p = fixture(Some(SourceOwner::Primary));
            p.expressions[2].expression = Expression::Call {
                function: crate::ScalarFunction::Exists,
                args: vec![4],
            };
            p.expressions[3].expression = legacy(&["Ref", "Value"], None);
            p.expressions[4].expression = legacy(&["Guard", "Value"], None);
            p.expressions.push(node(5, owned("Other")));
            configure(
                &mut p,
                Some(2),
                Some(sort(3, &[5], order)),
                vec![
                    SequenceWindow::FromTo { first: 3, last: 3 },
                    SequenceWindow::Last { count: 5 },
                ],
                IterationOutput::Repeated,
            );
            assert_eq!(
                observed("N698-A02 shared key and bound dependencies", &p),
                Ok(expected(&p, &[1, 3], &[1, 3], &[]))
            );
            assert_eq!(
                traced("N698-A02 top-level complete dual-environment trace", &p),
                Ok(vec![
                    (None, 0, 3),
                    (None, 0, 5),
                    (None, 1, 1),
                    (None, 1, 2),
                    (None, 1, 3),
                    (None, 1, 4),
                    (None, 1, 5),
                ])
            );

            let mut p = fixture(Some(SourceOwner::Primary));
            p.root.bindings = vec![binding("Value", 0)];
            p.expressions[3].expression = legacy(&["Ref", "Value"], None);
            p.expressions[4].expression = legacy(&["Guard", "Value"], None);
            p.expressions.extend([
                node(5, owned("Other")),
                node(
                    6,
                    row_query(SourceOwner::Named {
                        name: "Other".into(),
                    }),
                ),
            ]);
            configure(
                &mut p,
                Some(2),
                Some(sort(4, &[], order)),
                vec![SequenceWindow::First { count: 3 }],
                IterationOutput::Repeated,
            );
            let mut child = scope(vec![binding("Value", 6)]);
            child.target_field = "Child".into();
            child.repeating = true;
            child.iteration = Some(IterationPlan::new(
                crate::OwnedSourceIteration::new(OwnedSourceRef {
                    owner: SourceOwner::Named {
                        name: "Other".into(),
                    },
                    path: path(&["Rows"]),
                }),
                Some(2),
                Some(sort(3, &[], order)),
                vec![SequenceWindow::FromTo { first: 3, last: 5 }],
                IterationOutput::Repeated,
            ));
            p.root.children.push(child);
            refresh(&mut p);
            let actual = observed("N698-A02 nested bounds retain outer source ancestry", &p);
            assert_eq!(actual, Ok(expected(&p, &[1, 3], &[1, 3], &[])));
            borrowed(&p, actual.as_ref().unwrap());
            assert_eq!(
                traced("N698-A02 nested complete parent/item trace", &p),
                Ok(vec![
                    (None, 0, 3),
                    (None, 1, 0),
                    (None, 1, 2),
                    (None, 1, 4),
                    (None, 2, 3),
                    (None, 2, 5),
                    (None, 3, 2),
                    (None, 3, 3),
                    (None, 3, 6),
                ])
            );
        }
    }

    #[test]
    fn window_potential_branches_defaults_value_maps_targets_and_failures_keep_complete_plans() {
        for order in [
            SortFilterOrder::SortThenFilter,
            SortFilterOrder::FilterThenSort,
        ] {
            let mut p = fixture(Some(SourceOwner::Primary));
            p.expressions[2].expression = Expression::Const {
                value: Value::Bool(false),
            };
            p.expressions[3].expression = legacy(&["Ref", "Value"], None);
            p.expressions[4].expression = legacy(&["Path", "Value"], None);
            p.expressions.extend([
                node(
                    5,
                    Expression::RuntimeParameterDefault {
                        name: "count".into(),
                        ty: ScalarType::String,
                        default: 7,
                    },
                ),
                node(
                    6,
                    Expression::ValueMap {
                        input: 4,
                        input_type: Some(ScalarType::String),
                        table: vec![(Value::String("3".into()), Value::String("2".into()))],
                        default: Some(Value::String("0".into())),
                    },
                ),
                node(7, legacy(&[" Ref 雪 ", "Value"], None)),
                node(
                    8,
                    Expression::If {
                        condition: 2,
                        then: 3,
                        else_: 5,
                    },
                ),
                node(9, legacy(&["Driver", "Value"], None)),
                node(11, legacy(&["Other", "Value"], None)),
            ]);
            p.expressions
                .iter_mut()
                .find(|entry| entry.id == u32::MAX)
                .unwrap()
                .expression = legacy(&["Unused", "Value"], None);
            configure(
                &mut p,
                Some(2),
                Some(sort(8, &[6], order)),
                vec![
                    SequenceWindow::First { count: 6 },
                    SequenceWindow::Last { count: 8 },
                ],
                IterationOutput::Repeated,
            );
            let mut named = scope(vec![binding("Value", 0)]);
            named.repeating = true;
            named.iteration = Some(IterationPlan::new(
                crate::OwnedSourceIteration::new(OwnedSourceRef {
                    owner: SourceOwner::Named { name: "Ref".into() },
                    path: path(&["Rows"]),
                }),
                None,
                Some(sort(9, &[], order)),
                vec![SequenceWindow::From { position: 5 }],
                IterationOutput::Repeated,
            ));
            p.extra_targets.push(NamedTargetProgram {
                name: " Window Target 雪 ".into(),
                target: target_schema(&named, "Named"),
                root: named,
            });
            let passive = scope(vec![binding("Value", 0)]);
            p.extra_targets.push(NamedTargetProgram {
                name: " passive ".into(),
                target: target_schema(&passive, "Passive"),
                root: passive,
            });
            p.failure_rules.push(crate::FailureRule {
                iteration: crate::FailureIteration::Source(SourceIteration::new(path(&[
                    "Guard", "Rows",
                ]))),
                selection: crate::FailureSelection::WhenTrue(2),
                message: Some(11),
            });
            let actual = observed(
                "N698-A03 all potential bound and selected-target requirements",
                &p,
            );
            assert_eq!(
                actual,
                Ok(expected(
                    &p,
                    &[1, 2, 3, 4, 7],
                    &[1, 2, 3, 4, 5, 7],
                    &[(0, &[2, 3, 5, 7]), (1, &[2, 3])]
                ))
            );
            borrowed(&p, actual.as_ref().unwrap());
        }
    }

    #[test]
    fn window_planner_forwards_full_structural_errors_and_inherited_grouping_closure() {
        for malformed in 0..4 {
            let mut p = fixture(Some(SourceOwner::Primary));
            configure(
                &mut p,
                Some(2),
                Some(sort(0, &[], SortFilterOrder::SortThenFilter)),
                vec![
                    SequenceWindow::SkipFirst { count: 3 },
                    SequenceWindow::FromTo { first: 3, last: 4 },
                ],
                IterationOutput::Repeated,
            );
            let error = match malformed {
                0 => {
                    configure(
                        &mut p,
                        Some(2),
                        Some(sort(0, &[], SortFilterOrder::SortThenFilter)),
                        vec![
                            SequenceWindow::SkipFirst { count: 3 },
                            SequenceWindow::FromTo { first: 3, last: 99 },
                        ],
                        IterationOutput::Repeated,
                    );
                    Error::MissingWindowExpression {
                        target_path: vec![],
                        window: 1,
                        bound: 1,
                        expression: 99,
                    }
                }
                1 => {
                    p.expressions.push(node(
                        3,
                        Expression::Const {
                            value: Value::Int(0),
                        },
                    ));
                    Error::DuplicateExpression { node: 3 }
                }
                2 => {
                    p.expressions[3].expression = owned("Absent");
                    Error::OwnedSource {
                        diagnostic: Box::new(Diagnostic::Structure {
                            location: Location::Expression {
                                namespace: Namespace::Main,
                                node: 3,
                                scope: None,
                            },
                            request: Request::Field {
                                owner: Owner::Named {
                                    name: "Absent".into(),
                                },
                                path: path(&["Value"]),
                                frame: None,
                            },
                            reason: Structure::UnknownOwner,
                        }),
                    }
                }
                _ => {
                    p.root.bindings.clear();
                    p.root.construction = TargetConstruction::Scalar {
                        expression: 1,
                        target_domain: ScalarType::String.into(),
                    };
                    configure(
                        &mut p,
                        Some(2),
                        None,
                        vec![SequenceWindow::First { count: 3 }],
                        IterationOutput::First,
                    );
                    Error::InvalidIterationOutput {
                        target_path: vec![],
                        output: IterationOutput::First,
                    }
                }
            };
            assert_eq!(
                validate_program_with_basic_owned_windows(&p),
                Err(error.clone())
            );
            assert_eq!(
                observed("N698-A04 complete forwarded first validation error", &p),
                Err(error.clone())
            );
            assert_eq!(
                traced("N698-A04 trace retains exact validator refusal", &p),
                Err(error.clone())
            );
            assert_eq!(legacy_named_input_admission_plan(&p).unwrap_err(), error);
        }
        let mut p = fixture(None);
        p.root.bindings = vec![binding("Value", 0)];
        p.root.iteration = Some(
            IterationPlan::new(
                SourceIteration::new(path(&["Rows"])),
                None,
                None,
                vec![SequenceWindow::First { count: 3 }],
                IterationOutput::Repeated,
            )
            .with_grouping(crate::GroupingPlan::By { key: 0 }),
        );
        let mut child = scope(vec![binding("Value", 1)]);
        child.target_field = "Child".into();
        child.repeating = true;
        child.iteration = Some(IterationPlan::new(
            crate::OwnedSourceIteration::new(OwnedSourceRef {
                owner: SourceOwner::Named {
                    name: "Other".into(),
                },
                path: path(&["Rows"]),
            }),
            None,
            None,
            vec![SequenceWindow::Last { count: 4 }],
            IterationOutput::Repeated,
        ));
        p.expressions[1].expression = row_query(SourceOwner::Named {
            name: "Other".into(),
        });
        p.root.children.push(child);
        refresh(&mut p);
        let error = Error::OwnedSource {
            diagnostic: Box::new(Diagnostic::Capability {
                location: Location::Iteration {
                    scope: primary().descendant(SourceScopeStep::Child(0)),
                },
                request: Some(Request::Iteration {
                    owner: Owner::Named {
                        name: "Other".into(),
                    },
                    path: path(&["Rows"]),
                }),
                reason: Reason::Grouping,
                origin: Origin::Scope { scope: primary() },
            }),
        };
        assert_eq!(
            validate_program_with_basic_owned_windows(&p),
            Err(error.clone())
        );
        assert_eq!(
            observed(
                "N698-A04 inherited grouping remains closed around owned windows",
                &p
            ),
            Err(error)
        );
    }
}
