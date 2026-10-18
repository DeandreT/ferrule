//! Serialized-Project/public-consumer coverage for the explicit ItemAt profile.
//! Literal oracles are hand-derived; no native execution is used as an oracle.

use super::*;
use mapping::{OwnedPosition, OwnedSourceField, OwnedSourceRef, SourceOwner};

fn path(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|part| (*part).into()).collect()
}

fn named(name: &str) -> SourceOwner {
    SourceOwner::Named { name: name.into() }
}

fn owned(owner: SourceOwner, parts: &[&str], frame: Option<&[&str]>) -> mapping::Node {
    mapping::Node::OwnedSourceField(OwnedSourceField {
        source: OwnedSourceRef {
            owner,
            path: path(parts),
        },
        frame: frame.map(path),
    })
}

fn constant(value: Value) -> mapping::Node {
    mapping::Node::Const { value }
}

fn text(value: &str) -> mapping::Node {
    constant(Value::String(value.into()))
}

fn binding(name: &str, node: u32) -> mapping::Binding {
    mapping::Binding {
        target_field: name.into(),
        node,
    }
}

fn item_at(sequence: mapping::SequenceExpr, index: u32) -> mapping::Node {
    mapping::Node::SequenceItemAt { sequence, index }
}

fn tokens(item: u32) -> mapping::SequenceExpr {
    mapping::SequenceExpr::Tokenize {
        input: 10,
        delimiter: 11,
        item,
    }
}

fn source_schema(index: ScalarType) -> SchemaNode {
    SchemaNode::group(
        "Source",
        vec![
            SchemaNode::group(
                "Cfg",
                vec![
                    SchemaNode::scalar("Index", index),
                    SchemaNode::scalar("Text", ScalarType::String),
                ],
            ),
            SchemaNode::group(
                "Rows",
                vec![
                    SchemaNode::scalar("Key", ScalarType::String),
                    SchemaNode::scalar("Index", ScalarType::Int),
                    SchemaNode::scalar("Text", ScalarType::String),
                ],
            )
            .repeating(),
        ],
    )
}

fn project(index: ScalarType, result: ScalarType) -> mapping::Project {
    let root = mapping::Scope {
        bindings: vec![binding("Result", 40)],
        ..Default::default()
    };
    let target = SchemaNode::group("Output", vec![SchemaNode::scalar("Result", result)]);
    mapping::Project {
        source: source_schema(index),
        target: target.clone(),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: ["Cfg", " Cfg💠 "]
            .into_iter()
            .map(|name| mapping::NamedSource {
                name: name.into(),
                path: "unused.json".into(),
                schema: source_schema(index),
                options: Default::default(),
                dynamic_path: None,
            })
            .collect(),
        extra_targets: vec![mapping::NamedTarget {
            name: " Audit💠 ".into(),
            path: None,
            schema: target,
            options: Default::default(),
            root: root.clone(),
        }],
        graph: mapping::Graph {
            nodes: BTreeMap::from([
                (10, text("a|β|終")),
                (11, text("|")),
                (12, text("")),
                (13, constant(Value::Int(10))),
                (14, constant(Value::Int(12))),
                (20, text("PRODUCER")),
                (21, text("INDEX")),
                (30, owned(named("Cfg"), &["Cfg", "Index"], Some(&[]))),
                (31, constant(Value::Int(1))),
                (32, constant(Value::Int(2))),
                (40, item_at(tokens(90), 30)),
                (
                    90,
                    mapping::Node::SourceField {
                        path: Vec::new(),
                        frame: None,
                    },
                ),
            ]),
        },
        root,
        user_functions: Default::default(),
        failure_rules: Vec::new(),
    }
}

fn lower(project: &mapping::Project) -> Program {
    let saved = serde_json::to_vec(project).unwrap();
    let restored: mapping::Project = serde_json::from_slice(&saved).unwrap();
    assert_eq!(serde_json::to_vec(&restored).unwrap(), saved);
    codegen::lower_with_basic_owned_sequence_item_at(&restored).unwrap()
}

fn options() -> Options {
    Options {
        package_name: "owned_item_at_host".into(),
        runtime_dependency: RuntimeDependency::Path(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../codegen-runtime")
                .canonicalize()
                .unwrap()
                .to_str()
                .unwrap()
                .into(),
        ),
    }
}

fn source(files: &ArtifactSet) -> &str {
    std::str::from_utf8(
        &files
            .files()
            .iter()
            .find(|file| file.path.as_str() == "src/lib.rs")
            .unwrap()
            .contents,
    )
    .unwrap()
}

#[test]
fn item_at_companion_is_explicit_typed_lazy_and_every_old_profile_stays_closed() {
    let program = lower(&project(ScalarType::Int, ScalarType::String));
    for old in [
        emit,
        emit_with_basic_owned_sources,
        emit_with_basic_owned_filters,
        emit_with_basic_owned_sort,
        emit_with_basic_owned_windows,
        emit_with_basic_owned_row_failures,
        emit_with_basic_owned_group_by,
    ] {
        assert!(matches!(
            old(&program, &options()),
            Err(EmitError::InvalidProgram(
                ProgramValidationError::OwnedSource { .. }
            ))
        ));
    }
    let files = emit_with_basic_owned_sequence_item_at(&program, &options()).unwrap();
    let source = source(&files);
    assert_eq!(files.len(), 2);
    assert!(source.contains("pub fn execute_with_basic_owned_sources<'name, 'src>("));
    assert!(source.contains("pub fn execute_selected_with_basic_owned_sources<'name, 'src>("));
    assert!(
        source.contains("execution: Option<&ExecutionContext<'src>>, limits: OwnedSourceLimits,")
    );
    assert!(!source.contains("pub fn execute_json"));
    assert!(!source.contains("OwnedSourceLimits::default"));
    assert!(!source.contains("GeneratedItems"));
    assert!(!source.contains("fn expression_90("));
    assert!(source.contains("name: \"Cfg\", mode: BasicNamedSourceMode::Optional"));
    assert!(source.contains(r#"name: " Cfg\u{1f4a0} ", mode: BasicNamedSourceMode::Optional"#));
    let body = source
        .split_once("fn expression_40(")
        .unwrap()
        .1
        .split_once("\n}\n")
        .unwrap()
        .0;
    assert!(body.find("let sequence_values =").unwrap() < body.find("let index =").unwrap());
    assert_eq!(body.matches("expression_30(context)?").count(), 1);
    assert!(body.contains("aggregate(AggregateFunction::ItemAt, &sequence_values, Some(index))"));
    let mut invalid = options();
    invalid.package_name = "0bad".into();
    assert!(matches!(
        emit_with_basic_owned_sequence_item_at(&program, &invalid),
        Err(EmitError::InvalidPackageName(_))
    ));
    let mut malformed = program.clone();
    malformed.expressions.push(malformed.expressions[0].clone());
    assert!(matches!(
        emit_with_basic_owned_sequence_item_at(&malformed, &invalid),
        Err(EmitError::InvalidProgram(
            ProgramValidationError::DuplicateExpression { .. }
        ))
    ));
}

#[test]
fn no_item_at_keeps_by_artifacts_and_empty_named_catalogue_fix() {
    let mut project = project(ScalarType::Int, ScalarType::String);
    project
        .graph
        .nodes
        .insert(40, owned(SourceOwner::Primary, &["Cfg", "Text"], Some(&[])));
    project.graph.nodes.insert(
        30,
        owned(SourceOwner::Primary, &["Cfg", "Index"], Some(&[])),
    );
    project.extra_sources.clear();
    let program = lower(&project);
    let files = emit_with_basic_owned_sequence_item_at(&program, &options()).unwrap();
    assert_eq!(
        files,
        emit_with_basic_owned_group_by(&program, &options()).unwrap()
    );
    assert!(!source(&files).contains("BasicNamedSourceMode"));
    assert!(source(&files).contains("basic_declared_root_nonrepeating"));
}

fn catalogue_project() -> mapping::Project {
    let mut p = project(ScalarType::Int, ScalarType::String);
    // A genuinely dynamic, unused declaration reserves ordinal 0. Both
    // original static declarations remain accepted, including the unused one.
    p.extra_sources.insert(
        0,
        mapping::NamedSource {
            name: " Dyn💠 ".into(),
            path: "unused.json".into(),
            schema: source_schema(ScalarType::Int),
            options: Default::default(),
            dynamic_path: Some(mapping::DynamicSourcePath {
                node: 99,
                iteration: Vec::new(),
            }),
        },
    );
    p.graph.nodes.extend([
        (
            81,
            mapping::Node::SourceField {
                path: path(&["Cfg", "Text"]),
                frame: None,
            },
        ),
        (99, text("unused.json")),
    ]);
    // Primary.Cfg exists, but the conservative legacy alias still eagerly
    // requires the exact Named("Cfg") slot. Owned ItemAt stays optional/lazy.
    p.extra_targets.push(mapping::NamedTarget {
        name: " Legacy💠 ".into(),
        path: None,
        schema: p.target.clone(),
        options: Default::default(),
        root: mapping::Scope {
            bindings: vec![binding("Result", 81)],
            ..Default::default()
        },
    });
    p
}

#[test]
fn selected_catalogues_distinguish_static_optional_legacy_required_and_dynamic_reserved() {
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
    let program = lower(&catalogue_project());
    let plan =
        codegen::legacy_named_input_admission_plan_with_basic_owned_sequence_item_at(&program)
            .unwrap();
    let optional = vec![(0, " Dyn💠 ", U), (1, "Cfg", O), (2, " Cfg💠 ", O)];
    let required = vec![(0, " Dyn💠 ", U), (1, "Cfg", R), (2, " Cfg💠 ", O)];
    assert_eq!(rows(&plan.primary), optional);
    assert_eq!(rows(&plan.all_outputs), required);
    assert_eq!(
        plan.named_targets
            .iter()
            .map(|target| (target.target_index, target.name, rows(&target.catalogue)))
            .collect::<Vec<_>>(),
        vec![(0, " Audit💠 ", optional), (1, " Legacy💠 ", required)]
    );
}

#[test]
fn item_at_in_failure_predicate_or_message_stays_closed_even_when_shared_with_target() {
    for as_message in [false, true] {
        let mut p = project(ScalarType::Int, ScalarType::String);
        p.failure_rules.push(mapping::FailureRule {
            iteration: mapping::FailureIteration::Source {
                collection: Vec::new(),
            },
            selection: if as_message {
                mapping::FailureSelection::All
            } else {
                mapping::FailureSelection::WhenTrue { predicate: 40 }
            },
            message: as_message.then_some(40),
        });
        assert!(codegen::lower_with_basic_owned_sequence_item_at(&p).is_err());
        // Direct Program revalidation/emission must not use the target's visit
        // as proof that this distinct failure role was admitted.
        let mut program = lower(&project(ScalarType::Int, ScalarType::String));
        program.failure_rules.push(FailureRule {
            iteration: FailureIteration::Source(SourceIteration::new(Vec::new())),
            selection: if as_message {
                FailureSelection::All
            } else {
                FailureSelection::WhenTrue(40)
            },
            message: as_message.then_some(40),
        });
        assert!(emit_with_basic_owned_sequence_item_at(&program, &options()).is_err());
    }
}

#[test]
fn scalar_producer_shared_phase_keeps_frozen_ordinary_bytes_and_context_prefixes() {
    // These are the existing ordinary argument-phase bytes, not a second call
    // to the new renderer used as its own oracle.
    let pairs = [
        (
            GeneratedSequence::Tokenize {
                input: 10,
                delimiter: 11,
                item: 90,
            },
            "let sequence_input = expression_10(context)?;\nlet sequence_values = if sequence_input == Value::Null || sequence_input.is_json_null() {\n    Vec::new()\n} else {\n    let sequence_parameter = expression_11(context)?;\n    if sequence_parameter == Value::Null || sequence_parameter.is_json_null() {\n        Vec::new()\n    } else {\n        tokenize(sequence_input, sequence_parameter)?\n    }\n};\n",
        ),
        (
            GeneratedSequence::TokenizeByLength {
                input: 10,
                length: 11,
                item: 90,
            },
            "let sequence_input = expression_10(context)?;\nlet sequence_values = if sequence_input == Value::Null || sequence_input.is_json_null() {\n    Vec::new()\n} else {\n    let sequence_parameter = expression_11(context)?;\n    if sequence_parameter == Value::Null || sequence_parameter.is_json_null() {\n        Vec::new()\n    } else {\n        tokenize_by_length(sequence_input, sequence_parameter)?\n    }\n};\n",
        ),
        (
            GeneratedSequence::Range {
                from: Some(10),
                to: 11,
                item: 90,
            },
            "let sequence_from = expression_10(context)?;\nlet sequence_values = if sequence_from == Value::Null || sequence_from.is_json_null() {\n    Vec::new()\n} else {\n    let sequence_to = expression_11(context)?;\n    if sequence_to == Value::Null || sequence_to.is_json_null() {\n        Vec::new()\n    } else {\n        generate_sequence(Some(sequence_from), sequence_to)?\n    }\n};\n",
        ),
        (
            GeneratedSequence::Range {
                from: None,
                to: 11,
                item: 90,
            },
            "let sequence_to = expression_11(context)?;\nlet sequence_values = if sequence_to == Value::Null || sequence_to.is_json_null() {\n    Vec::new()\n} else {\n    generate_sequence(None, sequence_to)?\n};\n",
        ),
        (
            GeneratedSequence::TokenizeRegex {
                input: 10,
                pattern: 11,
                flags: None,
                item: 90,
            },
            "let sequence_input = expression_10(context)?;\nlet sequence_values = if sequence_input == Value::Null || sequence_input.is_json_null() {\n    Vec::new()\n} else {\n    let sequence_pattern = expression_11(context)?;\n    if sequence_pattern == Value::Null || sequence_pattern.is_json_null() {\n        Vec::new()\n    } else {\n        tokenize_regex(sequence_input, sequence_pattern, None)?\n    }\n};\n",
        ),
        (
            GeneratedSequence::TokenizeRegex {
                input: 10,
                pattern: 11,
                flags: Some(12),
                item: 90,
            },
            "let sequence_input = expression_10(context)?;\nlet sequence_values = if sequence_input == Value::Null || sequence_input.is_json_null() {\n    Vec::new()\n} else {\n    let sequence_pattern = expression_11(context)?;\n    if sequence_pattern == Value::Null || sequence_pattern.is_json_null() {\n        Vec::new()\n    } else {\n        let sequence_flags = expression_12(context)?;\n        if sequence_flags == Value::Null || sequence_flags.is_json_null() {\n            Vec::new()\n        } else {\n            tokenize_regex(sequence_input, sequence_pattern, Some(sequence_flags))?\n        }\n    }\n};\n",
        ),
    ];
    for (sequence, expected) in pairs {
        let mut ordinary = String::new();
        render_generated_values(&sequence, "", &mut ordinary).unwrap();
        assert_eq!(ordinary, expected);
        let mut paired = String::new();
        assert!(scalar_sequence::render_values(
            &sequence,
            "",
            &mut paired,
            &|node| { format!("owned_expression_{node}(parent)") }
        ));
        assert!(paired.contains("owned_expression_11(parent)?"));
        assert!(!paired.contains("generated_item_context"));
    }
    let recursive = GeneratedSequence::RecursiveCollect {
        collection: path(&["Rows"]),
        children: Vec::new(),
        descent_value: Vec::new(),
        values: Vec::new(),
        value: Vec::new(),
        prefix: 10,
        separator: 11,
        item: 90,
    };
    let mut output = "unchanged".to_owned();
    assert!(!scalar_sequence::render_values(
        &recursive,
        "",
        &mut output,
        &|_| panic!("closed producer")
    ));
    assert_eq!(output, "unchanged");
    assert_eq!(scalar_sequence::runtime_helper(&recursive), None);
}

#[test]
fn sibling_consumers_and_collection_reducers_keep_their_closed_owned_gates() {
    let base = lower(&project(ScalarType::Int, ScalarType::String));
    for closed in [
        Expression::SequenceExists {
            sequence: GeneratedSequence::Range {
                from: None,
                to: 30,
                item: 90,
            },
            predicate: 70,
        },
        Expression::SequenceAggregate {
            function: AggregateFunction::ItemAt,
            sequence: GeneratedSequence::Range {
                from: None,
                to: 30,
                item: 90,
            },
            predicate: None,
            expression: None,
            arg: Some(30),
        },
        Expression::Aggregate {
            function: AggregateFunction::ItemAt,
            collection: path(&["Rows"]),
            value: AggregateValue::Path(path(&["Index"])),
            arg: Some(30),
        },
        Expression::Lookup {
            collection: path(&["Rows"]),
            key: path(&["Index"]),
            matches: 30,
            value: path(&["Index"]),
        },
    ] {
        let mut program = base.clone();
        program.expressions.push(ExpressionNode {
            id: 70,
            expression: Expression::Const {
                value: Value::Bool(true),
            },
        });
        program
            .expressions
            .iter_mut()
            .find(|node| node.id == 40)
            .unwrap()
            .expression = closed;
        assert!(matches!(
            emit_with_basic_owned_sequence_item_at(&program, &options()),
            Err(EmitError::InvalidProgram(
                ProgramValidationError::OwnedSource { .. }
            ))
        ));
    }
}

fn configure_roots(project: &mut mapping::Project, root: mapping::Scope, target: SchemaNode) {
    project.root = root.clone();
    project.target = target.clone();
    for extra in &mut project.extra_targets {
        extra.root = root.clone();
        extra.schema = target.clone();
    }
}

fn rows_project(grouped: bool, position: bool, root_pin: bool) -> mapping::Project {
    let mut p = project(ScalarType::Int, ScalarType::String);
    p.graph.nodes.insert(
        30,
        if position {
            mapping::Node::OwnedPosition(OwnedPosition {
                owner: named("Cfg"),
                collection: Some(path(&["Rows"])),
            })
        } else if root_pin {
            owned(named("Cfg"), &["Cfg", "Index"], Some(&[]))
        } else {
            owned(named("Cfg"), &["Rows", "Index"], Some(&["Rows"]))
        },
    );
    p.graph.nodes.extend([
        (50, owned(named("Cfg"), &["Rows", "Key"], None)),
        (
            51,
            mapping::Node::OwnedPosition(OwnedPosition {
                owner: named("Cfg"),
                collection: Some(path(&["Rows"])),
            }),
        ),
        (52, item_at(tokens(91), 51)),
        (
            91,
            mapping::Node::SourceField {
                path: Vec::new(),
                frame: None,
            },
        ),
    ]);
    let iteration = || mapping::ScopeIteration::OwnedSource {
        source: OwnedSourceRef {
            owner: named("Cfg"),
            path: path(&["Rows"]),
        },
        output_path: None,
    };
    let mut scope = mapping::Scope {
        target_field: "Groups".into(),
        iteration: iteration(),
        group_by: grouped.then_some(50),
        bindings: vec![binding("Result", 40), binding("ByPosition", 52)],
        ..Default::default()
    };
    let mut fields = vec![
        SchemaNode::scalar("Result", ScalarType::String),
        SchemaNode::scalar("ByPosition", ScalarType::String),
    ];
    if grouped {
        scope.children.push(mapping::Scope {
            target_field: "Members".into(),
            iteration: iteration(),
            bindings: scope.bindings.clone(),
            ..Default::default()
        });
        fields.push(SchemaNode::group("Members", fields.clone()).repeating());
    }
    configure_roots(
        &mut p,
        mapping::Scope {
            children: vec![scope],
            ..Default::default()
        },
        SchemaNode::group(
            "Output",
            vec![SchemaNode::group("Groups", fields).repeating()],
        ),
    );
    p
}

fn cases() -> Vec<(&'static str, mapping::Project)> {
    let base = project(ScalarType::Int, ScalarType::String);
    let mut result = vec![("basic", base.clone())];
    let mut exact = base.clone();
    exact
        .graph
        .nodes
        .insert(30, owned(named(" Cfg💠 "), &["Cfg", "Index"], Some(&[])));
    result.push(("exact", exact));
    let mut arguments = base.clone();
    arguments
        .graph
        .nodes
        .insert(10, owned(named("Cfg"), &["Cfg", "Text"], Some(&[])));
    result.push(("arguments", arguments));
    let mut length = base.clone();
    length.graph.nodes.extend([
        (10, text("a💠βzQ")),
        (11, constant(Value::Int(2))),
        (
            40,
            item_at(
                mapping::SequenceExpr::TokenizeByLength {
                    input: 10,
                    length: 11,
                    item: 90,
                },
                30,
            ),
        ),
    ]);
    result.push(("length", length));
    for (name, flags) in [("regex", None), ("regex_flags", Some(12))] {
        let mut regex = base.clone();
        regex.graph.nodes.extend([
            (10, text("a,bb;終")),
            (11, text("[,;]")),
            (
                40,
                item_at(
                    mapping::SequenceExpr::TokenizeRegex {
                        input: 10,
                        pattern: 11,
                        flags,
                        item: 90,
                    },
                    30,
                ),
            ),
        ]);
        result.push((name, regex));
    }
    for (name, from, to) in [
        ("range", Some(13), 14),
        ("omitted", None, 30),
        ("shared", Some(31), 30),
        ("empty", Some(13), 31),
        ("too_large", None, 14),
    ] {
        let mut range = project(ScalarType::Int, ScalarType::Int);
        range.graph.nodes.insert(
            40,
            item_at(mapping::SequenceExpr::Generate { from, to, item: 90 }, 30),
        );
        if name == "too_large" {
            range
                .graph
                .nodes
                .insert(14, constant(Value::Int(1_000_001)));
        }
        result.push((name, range));
    }
    for (name, input, delimiter) in [
        ("null", Value::Null, true),
        ("json_null", Value::json_null(), true),
        ("producer_error", Value::Null, false),
        ("bad_delimiter", Value::String("a".into()), false),
        ("literal_empty", Value::String(String::new()), false),
    ] {
        let mut p = base.clone();
        p.graph.nodes.insert(10, constant(input));
        if delimiter {
            p.graph
                .nodes
                .insert(11, mapping::Node::Raise { message: Some(20) });
        }
        if name == "producer_error" {
            p.graph
                .nodes
                .insert(10, mapping::Node::Raise { message: Some(20) });
        }
        if name == "bad_delimiter" {
            p.graph.nodes.insert(11, text(""));
        }
        result.push((name, p));
    }
    for name in [
        "empty_index_error",
        "null_index_error",
        "producer_index_error",
    ] {
        let mut p = base.clone();
        p.graph
            .nodes
            .insert(30, mapping::Node::Raise { message: Some(21) });
        // Keep a real owned read in the producer graph even for error cases.
        p.graph
            .nodes
            .insert(10, owned(SourceOwner::Primary, &["Cfg", "Text"], Some(&[])));
        if name == "empty_index_error" {
            p.graph.nodes.insert(
                40,
                item_at(
                    mapping::SequenceExpr::Generate {
                        from: Some(13),
                        to: 31,
                        item: 90,
                    },
                    30,
                ),
            );
            p.target = SchemaNode::group(
                "Output",
                vec![SchemaNode::scalar("Result", ScalarType::Int)],
            );
            for extra in &mut p.extra_targets {
                extra.schema = p.target.clone();
            }
        } else if name == "null_index_error" {
            p.graph.nodes.insert(10, constant(Value::Null));
            p.graph
                .nodes
                .insert(11, mapping::Node::Raise { message: Some(20) });
        } else {
            p.graph
                .nodes
                .insert(10, mapping::Node::Raise { message: Some(20) });
        }
        result.push((name, p));
    }
    let mut regex_empty = base.clone();
    regex_empty.graph.nodes.extend([
        (10, text("")),
        (11, text(",")),
        (
            40,
            item_at(
                mapping::SequenceExpr::TokenizeRegex {
                    input: 10,
                    pattern: 11,
                    flags: None,
                    item: 90,
                },
                30,
            ),
        ),
    ]);
    result.push(("regex_empty", regex_empty));
    let mut nested = base.clone();
    nested.graph.nodes.extend([
        (
            41,
            item_at(
                mapping::SequenceExpr::Generate {
                    from: None,
                    to: 30,
                    item: 91,
                },
                30,
            ),
        ),
        (
            91,
            mapping::Node::SourceField {
                path: Vec::new(),
                frame: None,
            },
        ),
        (40, item_at(tokens(90), 41)),
    ]);
    result.push(("nested", nested));
    let mut nested_argument = base.clone();
    nested_argument.graph.nodes.extend([
        (41, item_at(tokens(91), 30)),
        (
            91,
            mapping::Node::SourceField {
                path: Vec::new(),
                frame: None,
            },
        ),
        (
            40,
            item_at(
                mapping::SequenceExpr::Tokenize {
                    input: 41,
                    delimiter: 11,
                    item: 90,
                },
                31,
            ),
        ),
    ]);
    result.push(("nested_argument", nested_argument));
    for (name, range) in [
        ("null_from", true),
        ("null_pattern", false),
        ("null_flags", false),
    ] {
        let mut p = base.clone();
        if range {
            p.graph.nodes.extend([
                (13, constant(Value::Null)),
                (14, mapping::Node::Raise { message: Some(20) }),
                (
                    40,
                    item_at(
                        mapping::SequenceExpr::Generate {
                            from: Some(13),
                            to: 14,
                            item: 90,
                        },
                        30,
                    ),
                ),
            ]);
            p.target = SchemaNode::group(
                "Output",
                vec![SchemaNode::scalar("Result", ScalarType::Int)],
            );
            for extra in &mut p.extra_targets {
                extra.schema = p.target.clone();
            }
        } else {
            p.graph.nodes.extend([
                (
                    11,
                    if name == "null_pattern" {
                        constant(Value::Null)
                    } else {
                        text("[,;]")
                    },
                ),
                (
                    12,
                    if name == "null_pattern" {
                        mapping::Node::Raise { message: Some(20) }
                    } else {
                        constant(Value::json_null())
                    },
                ),
                (
                    40,
                    item_at(
                        mapping::SequenceExpr::TokenizeRegex {
                            input: 10,
                            pattern: 11,
                            flags: Some(12),
                            item: 90,
                        },
                        30,
                    ),
                ),
            ]);
        }
        result.push((name, p));
    }
    result.push(("float", project(ScalarType::Float, ScalarType::String)));
    result.push(("string", project(ScalarType::String, ScalarType::String)));
    result.push(("boolean", project(ScalarType::Bool, ScalarType::String)));
    result.push(("rows", rows_project(false, false, false)));
    let mut row_arguments = rows_project(false, false, false);
    row_arguments
        .graph
        .nodes
        .insert(10, owned(named("Cfg"), &["Rows", "Text"], Some(&["Rows"])));
    result.push(("row_arguments", row_arguments));
    result.push(("positions", rows_project(false, true, false)));
    result.push(("root_pin", rows_project(false, false, true)));
    result.push(("by", rows_project(true, false, false)));
    let mut inactive = base.clone();
    inactive
        .graph
        .nodes
        .insert(30, owned(named("Cfg"), &["Rows", "Index"], Some(&["Rows"])));
    let mut scope = mapping::Scope {
        target_field: "Groups".into(),
        iteration: mapping::ScopeIteration::OwnedSource {
            source: OwnedSourceRef {
                owner: SourceOwner::Primary,
                path: path(&["Rows"]),
            },
            output_path: None,
        },
        bindings: vec![binding("Result", 40)],
        ..Default::default()
    };
    configure_roots(
        &mut inactive,
        mapping::Scope {
            children: vec![scope.clone()],
            ..Default::default()
        },
        SchemaNode::group(
            "Output",
            vec![
                SchemaNode::group(
                    "Groups",
                    vec![SchemaNode::scalar("Result", ScalarType::String)],
                )
                .repeating(),
            ],
        ),
    );
    result.push(("inactive", inactive));
    // A legacy-only sibling in a paired artifact must call legacy_expression_,
    // without altering values that contain the generated identifier spelling.
    let mut mixed = base.clone();
    mixed.graph.nodes.extend([
        (60, text("expression_ legacy_expression_")),
        (61, text("!")),
        (
            62,
            item_at(
                mapping::SequenceExpr::Tokenize {
                    input: 60,
                    delimiter: 61,
                    item: 92,
                },
                31,
            ),
        ),
        (
            92,
            mapping::Node::SourceField {
                path: Vec::new(),
                frame: None,
            },
        ),
    ]);
    scope = mapping::Scope {
        target_field: "Legacy".into(),
        bindings: vec![binding("Result", 62)],
        ..Default::default()
    };
    let mut root = mixed.root.clone();
    root.children.push(scope);
    let target = SchemaNode::group(
        "Output",
        vec![
            SchemaNode::scalar("Result", ScalarType::String),
            SchemaNode::group(
                "Legacy",
                vec![SchemaNode::scalar("Result", ScalarType::String)],
            ),
        ],
    );
    configure_roots(&mut mixed, root, target);
    result.push(("mixed", mixed));
    let mut primary_only = base.clone();
    primary_only.extra_sources.clear();
    primary_only.graph.nodes.insert(
        30,
        owned(SourceOwner::Primary, &["Cfg", "Index"], Some(&[])),
    );
    result.push(("primary_only", primary_only));
    let mut failure = base.clone();
    failure.graph.nodes.extend([
        (10, mapping::Node::Raise { message: Some(20) }),
        (30, mapping::Node::Raise { message: Some(21) }),
        (70, constant(Value::Bool(true))),
        (71, owned(SourceOwner::Primary, &["Rows", "Key"], None)),
    ]);
    failure.failure_rules.push(mapping::FailureRule {
        iteration: mapping::FailureIteration::Source {
            collection: path(&["Rows"]),
        },
        selection: mapping::FailureSelection::WhenTrue { predicate: 70 },
        message: Some(71),
    });
    result.push(("failure", failure));
    result.push(("catalogues", catalogue_project()));
    result
}

// Instrument generated definitions, not text inside user literals. Counters
// exist only in this throwaway consumer project; no runtime API is added.
fn observed_source(source: &str) -> String {
    let mut output = String::from(
        "std::thread_local! { static CALLS: std::cell::RefCell<Vec<u32>> = const { std::cell::RefCell::new(Vec::new()) }; }\npub fn drain_calls() -> Vec<u32> { CALLS.with(|calls| calls.take()) }\n",
    );
    for line in source.lines() {
        output.push_str(line);
        output.push('\n');
        if let Some(definition) = line
            .strip_prefix("fn expression_")
            .or_else(|| line.strip_prefix("fn legacy_expression_"))
        {
            let (node, _) = definition.split_once('(').unwrap();
            let node: u32 = node.parse().unwrap();
            output.push_str(&format!(
                "    CALLS.with(|calls| calls.borrow_mut().push({node}));\n"
            ));
        }
    }
    output
}

#[test]
fn generated_public_item_at_consumer_has_literal_outputs_errors_frames_and_edge_counts() {
    let directory = TempDir::new("owned_item_at_consumer");
    let mut library = String::new();
    for (name, project) in cases() {
        fs::write(
            directory.path().join(format!("{name}.Project.json")),
            serde_json::to_vec_pretty(&project).unwrap(),
        )
        .unwrap();
        let artifacts =
            emit_with_basic_owned_sequence_item_at(&lower(&project), &options()).unwrap();
        library.push_str(&format!(
            "pub mod {name} {{\n{}\n}}\n",
            observed_source(source(&artifacts))
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
        .args(["test", "--offline", "--test", "consumer"])
        .env_remove("CARGO_ENCODED_RUSTFLAGS")
        .env("RUSTFLAGS", "-D warnings")
        .current_dir(directory.path())
        .generated_host_output(directory.path())
        .unwrap();
    fs::write(directory.path().join("stdout.complete"), &output.stdout).unwrap();
    fs::write(directory.path().join("stderr.complete"), &output.stderr).unwrap();
    fs::write(
        directory.path().join("command.complete"),
        format!(
            "command: cargo test --offline --test consumer\nRUSTFLAGS: -D warnings\nCARGO_ENCODED_RUSTFLAGS: <unset>\nworking_directory: {}\nstatus: {}\n",
            directory.path().display(),
            output.status,
        ),
    )
    .unwrap();
    if !output.status.success() {
        let retained = directory.path().to_owned();
        std::mem::forget(directory);
        panic!(
            "owned ItemAt consumer retained at {}:\n{}\n{}",
            retained.display(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    if std::env::var_os("FERRULE_CODEGEN_KEEP_ARTIFACTS").is_some() {
        eprintln!(
            "owned ItemAt consumer success retained at {}",
            directory.path().display()
        );
        std::mem::forget(directory);
    }
}

const CONSUMER: &str = r#"
use owned_item_at_host::*;
use codegen_runtime::{BasicNamedSourceInput, FunctionError, Instance, InstanceKind,
    OwnedSourceError, OwnedSourceIdentity, OwnedSourceLimits, RuntimeError, SourcePathError,
    TargetSelection, Value, field, group, repeated, scalar};

fn fields(values: impl IntoIterator<Item=(&'static str,Instance)>) -> Instance {
    group(values.into_iter().map(|(name,value)|field(name,value)))
}
fn text(value:&str)->Instance {scalar(Value::String(value.into()))}
fn input(index:Value,label:&str)->Instance {fields([
    ("Cfg",fields([("Index",scalar(index)),("Text",text(label))])),
    ("Rows",repeated([row("A",3),row("A",1),row("B",2)])),
])}
fn row(key:&str,index:i64)->Instance {fields([("Key",text(key)),("Index",scalar(Value::Int(index))),
    ("Text",text(&format!("{key}-{index}a|{key}-{index}b|{key}-{index}c")))])}
fn primary()->Instance {fields([
    ("Cfg",fields([("Index",scalar(Value::Int(1))),("Text",text("P-a|P-b|P-c"))])),
    ("Rows",repeated([row("A",1),row("A",1),row("B",1)])),
])}
fn named()->Instance {input(Value::Int(3),"N-a|N-b|N-c")}
fn exact()->Instance {input(Value::Int(2),"U-a|U-b|U-c")}
fn wanted(value:Value)->Instance {fields([("Result",scalar(value))])}
fn limits()->OwnedSourceLimits {OwnedSourceLimits {max_roots:8,max_active_frames:16,
    max_path_segments:64,max_metadata_bytes:1_048_576,max_walk_contexts:128}}
macro_rules! check {
    ($module:ident,$p:expr,$inputs:expr,$wanted:expr)=>{{
        let p=$p;let inputs=$inputs;let wanted=$wanted;
        assert_eq!($module::execute_with_basic_owned_sources(p,inputs,None,limits()),
            Ok($module::ExecutionOutputs {primary:wanted.clone(),extras:vec![
                $module::NamedOutput {name:" Audit💠 ".into(),instance:wanted.clone()}]}));
        assert_eq!($module::execute_selected_with_basic_owned_sources(p,inputs,None,limits(),TargetSelection::Primary),
            Ok($module::SelectedTargetOutput::Primary(wanted.clone())));
        assert_eq!($module::execute_selected_with_basic_owned_sources(p,inputs,None,limits(),TargetSelection::Named(" Audit💠 ")),
            Ok($module::SelectedTargetOutput::Named($module::NamedOutput {name:" Audit💠 ".into(),instance:wanted})));
    }};
}
macro_rules! failed {
    ($module:ident,$p:expr,$inputs:expr,$wanted:expr)=>{{
        assert_eq!($module::execute_selected_with_basic_owned_sources($p,$inputs,None,limits(),TargetSelection::Primary),Err($wanted));
    }};
}

#[test] fn four_producers_preserve_exact_owners_scalar_tags_and_nested_parent_reads() {
    let p=primary();let n=named();let u=exact();
    let supplied=[BasicNamedSourceInput{name:"Cfg",instance:&n},BasicNamedSourceInput{name:" Cfg💠 ",instance:&u}];
    check!(basic,&p,&supplied,wanted(Value::String("終".into())));
    check!(exact,&p,&supplied,wanted(Value::String("β".into())));
    check!(arguments,&p,&supplied,wanted(Value::String("N-c".into())));
    check!(range,&p,&supplied,wanted(Value::Int(12)));
    check!(omitted,&p,&supplied,wanted(Value::Int(3)));
    check!(shared,&p,&supplied,wanted(Value::Int(3)));
    check!(nested,&p,&supplied,wanted(Value::String("終".into())));
    check!(nested_argument,&p,&supplied,wanted(Value::String("終".into())));
    let n2=input(Value::Int(2),"N-a|N-b|N-c");let supplied2=[BasicNamedSourceInput{name:"Cfg",instance:&n2}];
    check!(length,&p,&supplied2,wanted(Value::String("βz".into())));
    check!(regex,&p,&supplied2,wanted(Value::String("bb".into())));
    check!(regex_flags,&p,&supplied2,wanted(Value::String("bb".into())));
    check!(primary_only,&p,&[],wanted(Value::String("a".into())));
    let expected=fields([("Result",text("終")),("Legacy",fields([("Result",text("expression_ legacy_expression_"))]))]);
    check!(mixed,&p,&supplied,expected);
}

#[test] fn item_at_conversion_is_not_window_item_count_and_nonfinite_inputs_are_typed() {
    let p=primary();
    for (index,want) in [(Value::Int(0),Value::Null),(Value::Int(-1),Value::Null),
        (Value::Int(4),Value::Null),(Value::Int(i64::MAX),Value::Null),
        (Value::Null,Value::Null),
        (Value::json_null(),Value::Null),(Value::xml_nil(),Value::Null)]
    {
        let n=input(index,"unused");let supplied=[BasicNamedSourceInput{name:"Cfg",instance:&n}];
        check!(basic,&p,&supplied,wanted(want));
    }
    let n=input(Value::Bool(true),"unused");let supplied=[BasicNamedSourceInput{name:"Cfg",instance:&n}];
    check!(boolean,&p,&supplied,wanted(Value::Null));
    for (index,want) in [(" 2 ",Value::String("β".into())),("2.0",Value::Null),("2e0",Value::Null)] {
        let n=input(Value::String(index.into()),"unused");let supplied=[BasicNamedSourceInput{name:"Cfg",instance:&n}];
        check!(string,&p,&supplied,wanted(want));
    }
    for (index,want) in [(2.49,Value::String("β".into())),(2.5,Value::String("終".into())),
        (0.5,Value::String("a".into())),(-0.5,Value::Null),(f64::NAN,Value::Null),
        (f64::INFINITY,Value::Null),(f64::NEG_INFINITY,Value::Null)]
    {
        let n=input(Value::Float(index),"unused");let supplied=[BasicNamedSourceInput{name:"Cfg",instance:&n}];
        check!(float,&p,&supplied,wanted(want));
    }
}

#[test] fn complete_production_then_once_index_even_empty_and_null_suppresses_only_later_arguments() {
    let p=primary();let n=named();let supplied=[BasicNamedSourceInput{name:"Cfg",instance:&n}];
    shared::drain_calls();
    assert!(shared::execute_selected_with_basic_owned_sources(&p,&supplied,None,limits(),TargetSelection::Primary).is_ok());
    assert_eq!(shared::drain_calls(),vec![40,31,30,30]);
    empty::drain_calls();
    assert_eq!(empty::execute_selected_with_basic_owned_sources(&p,&supplied,None,limits(),TargetSelection::Primary),
        Ok(empty::SelectedTargetOutput::Primary(wanted(Value::Null))));
    assert_eq!(empty::drain_calls(),vec![40,13,31,30]);
    failed!(empty_index_error,&p,&[],RuntimeError::MappingException{node:30,message:Some("INDEX".into())});
    failed!(null_index_error,&p,&[],RuntimeError::MappingException{node:30,message:Some("INDEX".into())});
    assert_eq!(null_index_error::drain_calls(),vec![40,10,30,21]);
    failed!(producer_index_error,&p,&[],RuntimeError::MappingException{node:10,message:Some("PRODUCER".into())});
    assert_eq!(producer_index_error::drain_calls(),vec![40,10,20]);
    check!(null,&p,&supplied,wanted(Value::Null));
    check!(json_null,&p,&supplied,wanted(Value::Null));
    check!(null_from,&p,&supplied,wanted(Value::Null));
    check!(null_pattern,&p,&supplied,wanted(Value::Null));
    check!(null_flags,&p,&supplied,wanted(Value::Null));
    failed!(producer_error,&p,&[],RuntimeError::MappingException{node:10,message:Some("PRODUCER".into())});
    assert_eq!(producer_error::drain_calls(),vec![40,10,20]);
    failed!(bad_delimiter,&p,&[],RuntimeError::Function(FunctionError::InvalidArgument{
        function:"tokenize",message:"requires a non-empty delimiter"}));
    assert_eq!(bad_delimiter::drain_calls(),vec![40,10,11]);
    failed!(too_large,&p,&[],RuntimeError::GeneratedSequenceTooLarge{requested:1_000_001,max:1_000_000});
    assert_eq!(too_large::drain_calls(),vec![40,14]);
    let n1=input(Value::Int(1),"unused");let one=[BasicNamedSourceInput{name:"Cfg",instance:&n1}];
    check!(literal_empty,&p,&one,wanted(Value::String(String::new())));
    check!(regex_empty,&p,&one,wanted(Value::Null));
}

#[test] fn exact_absence_missing_field_inactive_foreign_frames_and_malformed_shapes() {
    let p=primary();let n=named();
    let unavailable=||RuntimeError::OwnedSourceAtNode{node:30,source:OwnedSourceError::SourceUnavailable{name:"Cfg".into()}};
    failed!(basic,&p,&[],unavailable());failed!(empty,&p,&[],unavailable());
    let missing=fields([("Cfg",fields([("Text",text("unused"))]))]);let supplied=[BasicNamedSourceInput{name:"Cfg",instance:&missing}];
    check!(basic,&p,&supplied,wanted(Value::Null));
    let supplied=[BasicNamedSourceInput{name:"Cfg",instance:&n}];
    failed!(inactive,&p,&supplied,RuntimeError::OwnedSourceAtNode{node:30,source:OwnedSourceError::InactiveSourceFrame{
        owner:OwnedSourceIdentity::Named("Cfg".into()),collection:Some(vec!["Rows".into()])}});
    let malformed=fields([("Cfg",scalar(Value::Int(7)))]);let supplied=[BasicNamedSourceInput{name:"Cfg",instance:&malformed}];
    failed!(basic,&p,&supplied,RuntimeError::OwnedSourceAtNode{node:30,source:OwnedSourceError::Traversal{
        owner:OwnedSourceIdentity::Named("Cfg".into()),source:SourcePathError::CannotTraverse{
            path:vec!["Cfg".into(),"Index".into()],segment:1,found:InstanceKind::Scalar}}});
    let malformed=fields([("Cfg",fields([("Index",repeated([scalar(Value::Int(3))]))]))]);
    let supplied=[BasicNamedSourceInput{name:"Cfg",instance:&malformed}];
    failed!(basic,&p,&supplied,RuntimeError::OwnedSourceAtNode{node:30,source:OwnedSourceError::UniteratedRepetition{
        owner:OwnedSourceIdentity::Named("Cfg".into()),path:vec!["Cfg".into(),"Index".into()],segment:2}});
}

fn output_row(result:&str,position:&str)->Instance {fields([("Result",text(result)),("ByPosition",text(position))])}
#[test] fn rows_positions_root_pins_and_by_representative_members_keep_the_parent_context() {
    let p=primary();let n=named();let supplied=[BasicNamedSourceInput{name:"Cfg",instance:&n}];
    check!(rows,&p,&supplied,fields([("Groups",repeated([
        output_row("終","a"),output_row("a","β"),output_row("β","終")]))]));
    check!(positions,&p,&supplied,fields([("Groups",repeated([
        output_row("a","a"),output_row("β","β"),output_row("終","終")]))]));
    check!(row_arguments,&p,&supplied,fields([("Groups",repeated([
        output_row("A-3c","A-3a"),output_row("A-1a","A-1b"),output_row("B-2b","B-2c")]))]));
    check!(root_pin,&p,&supplied,fields([("Groups",repeated([
        output_row("終","a"),output_row("終","β"),output_row("終","終")]))]));
    // Complete ordered group/member trees; neither group ordinal nor member
    // ordinal substitutes for the representative's endpoint Index.
    let expected=fields([("Groups",repeated([
        fields([("Result",text("終")),("ByPosition",text("a")),("Members",repeated([
            output_row("終","a"),output_row("a","β")]))]),
        fields([("Result",text("β")),("ByPosition",text("β")),("Members",repeated([
            output_row("β","a")]))]),
    ]))]);
    check!(by,&p,&supplied,expected);
}

#[test] fn selection_admission_and_global_row_failures_still_precede_target_callbacks() {
    let p=primary();let n=named();let mut zero=limits();zero.max_roots=0;
    basic::drain_calls();
    assert_eq!(basic::execute_selected_with_basic_owned_sources(&p,&[],None,zero,TargetSelection::Named("Audit💠")),
        Err(RuntimeError::UnknownTarget{name:"Audit💠".into()}));
    assert!(basic::drain_calls().is_empty());
    let duplicate=[BasicNamedSourceInput{name:"Cfg",instance:&n};2];
    failed!(basic,&p,&duplicate,RuntimeError::DuplicateNamedSource{name:"Cfg"});
    assert!(basic::drain_calls().is_empty());
    failed!(failure,&p,&[],RuntimeError::MappingFailure{rule:1,message:Some("A".into())});
    let calls=failure::drain_calls();
    assert!(!calls.contains(&10));assert!(!calls.contains(&30));assert!(!calls.contains(&40));
}

#[test] fn selected_owned_and_legacy_targets_keep_distinct_literal_catalogues_and_error_frontiers() {
    let p=primary();let n=named();let u=exact();
    let only_exact=[BasicNamedSourceInput{name:" Cfg💠 ",instance:&u}];
    let supplied=[BasicNamedSourceInput{name:"Cfg",instance:&n}];
    let owned=TargetSelection::Named(" Audit💠 ");let legacy=TargetSelection::Named(" Legacy💠 ");
    let unavailable=||RuntimeError::OwnedSourceAtNode{node:30,source:OwnedSourceError::SourceUnavailable{name:"Cfg".into()}};
    let missing=||RuntimeError::MissingNamedSource{name:"Cfg"};

    catalogues::drain_calls();
    assert_eq!(catalogues::execute_selected_with_basic_owned_sources(&p,&[],None,limits(),owned),Err(unavailable()));
    assert_eq!(catalogues::drain_calls(),vec![40,10,11,30]);
    // An unused static declaration is accepted, but its different exact name
    // neither satisfies nor repairs the unavailable owned Cfg endpoint.
    assert_eq!(catalogues::execute_selected_with_basic_owned_sources(&p,&only_exact,None,limits(),owned),Err(unavailable()));
    assert_eq!(catalogues::drain_calls(),vec![40,10,11,30]);
    assert_eq!(catalogues::execute_selected_with_basic_owned_sources(&p,&[],None,limits(),legacy),Err(missing()));
    assert!(catalogues::drain_calls().is_empty());
    assert_eq!(catalogues::execute_selected_with_basic_owned_sources(&p,&only_exact,None,limits(),legacy),Err(missing()));
    assert!(catalogues::drain_calls().is_empty());
    assert_eq!(catalogues::execute_with_basic_owned_sources(&p,&[],None,limits()),Err(missing()));
    assert!(catalogues::drain_calls().is_empty());

    let reserved=[BasicNamedSourceInput{name:" Dyn💠 ",instance:&u}];
    let duplicate=[BasicNamedSourceInput{name:" Cfg💠 ",instance:&u};2];
    let wrong_case=[BasicNamedSourceInput{name:"cfg",instance:&n}];
    // Supplied-record errors precede the later missing-required pass. The
    // dynamic slot is genuinely reserved, not an unused static declaration.
    for selection in [owned,legacy] {
        assert_eq!(catalogues::execute_selected_with_basic_owned_sources(&p,&reserved,None,limits(),selection),
            Err(RuntimeError::UnexpectedNamedSource{name:" Dyn💠 ".into()}));
        assert_eq!(catalogues::execute_selected_with_basic_owned_sources(&p,&duplicate,None,limits(),selection),
            Err(RuntimeError::DuplicateNamedSource{name:" Cfg💠 "}));
        assert_eq!(catalogues::execute_selected_with_basic_owned_sources(&p,&wrong_case,None,limits(),selection),
            Err(RuntimeError::UnexpectedNamedSource{name:"cfg".into()}));
        assert!(catalogues::drain_calls().is_empty());
    }
    let mut zero=limits();zero.max_roots=0;
    assert_eq!(catalogues::execute_selected_with_basic_owned_sources(&p,&reserved,None,zero,TargetSelection::Named("Legacy💠")),
        Err(RuntimeError::UnknownTarget{name:"Legacy💠".into()}));
    assert_eq!(catalogues::execute_selected_with_basic_owned_sources(&p,&reserved,None,zero,legacy),
        Err(RuntimeError::OwnedSource(OwnedSourceError::MetadataLimit{
            kind:codegen_runtime::OwnedSourceLimitKind::Roots,used:0,requested:4,max:0})));
    assert!(catalogues::drain_calls().is_empty());

    // Same spelling, two distinct outcomes: owned reads use N.Index=3; the
    // admitted legacy read still resolves the existing Primary.Cfg.Text.
    assert_eq!(catalogues::execute_selected_with_basic_owned_sources(&p,&supplied,None,limits(),owned),
        Ok(catalogues::SelectedTargetOutput::Named(catalogues::NamedOutput{
            name:" Audit💠 ".into(),instance:wanted(Value::String("終".into()))})));
    assert_eq!(catalogues::execute_selected_with_basic_owned_sources(&p,&supplied,None,limits(),legacy),
        Ok(catalogues::SelectedTargetOutput::Named(catalogues::NamedOutput{
            name:" Legacy💠 ".into(),instance:wanted(Value::String("P-a|P-b|P-c".into()))})));
    assert_eq!(catalogues::execute_with_basic_owned_sources(&p,&supplied,None,limits()),
        Ok(catalogues::ExecutionOutputs{primary:wanted(Value::String("終".into())),extras:vec![
            catalogues::NamedOutput{name:" Audit💠 ".into(),instance:wanted(Value::String("終".into()))},
            catalogues::NamedOutput{name:" Legacy💠 ".into(),instance:wanted(Value::String("P-a|P-b|P-c".into()))},
        ]}));
}
"#;
