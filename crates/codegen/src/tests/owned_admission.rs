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
