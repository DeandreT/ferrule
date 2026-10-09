//! Four complete independent typed constructor recipes from the frozen controls.
use ir::{ScalarType, SchemaNode, Value};
use mapping::{Binding, FunctionId, Graph, Node, Project, Scope, ScopeIteration, UserFunction};

pub(super) fn projects() -> Vec<Project> {
    vec![
        Project {
            source: SchemaNode::group("Input", vec![]),
            target: SchemaNode::group(
                "Primary",
                vec![SchemaNode::scalar("Value", ScalarType::String)],
            ),
            source_path: None,
            target_path: None,
            source_options: Default::default(),
            target_options: Default::default(),
            extra_sources: vec![],
            extra_targets: vec![
                mapping::NamedTarget {
                    name: "chosen".into(),
                    path: None,
                    schema: SchemaNode::group(
                        "chosen",
                        vec![SchemaNode::scalar("Value", ScalarType::String)],
                    ),
                    options: Default::default(),
                    root: Scope {
                        target_field: "".into(),
                        iteration: ScopeIteration::None,
                        construction: mapping::ScopeConstruction::Constructed,
                        filter: None,
                        post_group_filter: None,
                        group_by: None,
                        group_adjacent_by: None,
                        group_starting_with: None,
                        group_ending_with: None,
                        group_into_blocks: None,
                        sort_by: None,
                        sort_descending: false,
                        sort_then_by: vec![],
                        sort_filter_order: mapping::SortFilterOrder::SortThenFilter,
                        windows: vec![],
                        iteration_output: mapping::IterationOutput::Repeated,
                        bindings: vec![Binding {
                            target_field: "Value".into(),
                            node: 2,
                        }],
                        dynamic_bindings: vec![],
                        children: vec![],
                        dynamic_children: vec![],
                        merge_dynamic_fields: false,
                    },
                },
                mapping::NamedTarget {
                    name: "broken".into(),
                    path: None,
                    schema: SchemaNode::group(
                        "broken",
                        vec![SchemaNode::scalar("Value", ScalarType::String)],
                    ),
                    options: Default::default(),
                    root: Scope {
                        target_field: "".into(),
                        iteration: ScopeIteration::None,
                        construction: mapping::ScopeConstruction::Constructed,
                        filter: None,
                        post_group_filter: None,
                        group_by: None,
                        group_adjacent_by: None,
                        group_starting_with: None,
                        group_ending_with: None,
                        group_into_blocks: None,
                        sort_by: None,
                        sort_descending: false,
                        sort_then_by: vec![],
                        sort_filter_order: mapping::SortFilterOrder::SortThenFilter,
                        windows: vec![],
                        iteration_output: mapping::IterationOutput::Repeated,
                        bindings: vec![Binding {
                            target_field: "Value".into(),
                            node: 3,
                        }],
                        dynamic_bindings: vec![],
                        children: vec![],
                        dynamic_children: vec![],
                        merge_dynamic_fields: false,
                    },
                },
            ],
            failure_rules: vec![],
            user_functions: [].into_iter().collect(),
            graph: Graph {
                nodes: [
                    (
                        1,
                        Node::Const {
                            value: Value::String("main untouched".into()),
                        },
                    ),
                    (
                        2,
                        Node::Const {
                            value: Value::String("chosen complete".into()),
                        },
                    ),
                    (3, Node::Raise { message: Some(4) }),
                    (
                        4,
                        Node::Const {
                            value: Value::String("unselected boom".into()),
                        },
                    ),
                ]
                .into_iter()
                .collect(),
            },
            root: Scope {
                target_field: "".into(),
                iteration: ScopeIteration::None,
                construction: mapping::ScopeConstruction::Constructed,
                filter: None,
                post_group_filter: None,
                group_by: None,
                group_adjacent_by: None,
                group_starting_with: None,
                group_ending_with: None,
                group_into_blocks: None,
                sort_by: None,
                sort_descending: false,
                sort_then_by: vec![],
                sort_filter_order: mapping::SortFilterOrder::SortThenFilter,
                windows: vec![],
                iteration_output: mapping::IterationOutput::Repeated,
                bindings: vec![Binding {
                    target_field: "Value".into(),
                    node: 1,
                }],
                dynamic_bindings: vec![],
                children: vec![],
                dynamic_children: vec![],
                merge_dynamic_fields: false,
            },
        },
        Project {
            source: SchemaNode::group("Input", vec![]),
            target: SchemaNode::group(
                "Primary",
                vec![SchemaNode::scalar("Value", ScalarType::String)],
            ),
            source_path: None,
            target_path: None,
            source_options: Default::default(),
            target_options: Default::default(),
            extra_sources: vec![],
            extra_targets: vec![
                mapping::NamedTarget {
                    name: "chosen".into(),
                    path: None,
                    schema: SchemaNode::group(
                        "chosen",
                        vec![SchemaNode::scalar("Value", ScalarType::String)],
                    ),
                    options: Default::default(),
                    root: Scope {
                        target_field: "".into(),
                        iteration: ScopeIteration::None,
                        construction: mapping::ScopeConstruction::Constructed,
                        filter: None,
                        post_group_filter: None,
                        group_by: None,
                        group_adjacent_by: None,
                        group_starting_with: None,
                        group_ending_with: None,
                        group_into_blocks: None,
                        sort_by: None,
                        sort_descending: false,
                        sort_then_by: vec![],
                        sort_filter_order: mapping::SortFilterOrder::SortThenFilter,
                        windows: vec![],
                        iteration_output: mapping::IterationOutput::Repeated,
                        bindings: vec![Binding {
                            target_field: "Value".into(),
                            node: 2,
                        }],
                        dynamic_bindings: vec![],
                        children: vec![],
                        dynamic_children: vec![],
                        merge_dynamic_fields: false,
                    },
                },
                mapping::NamedTarget {
                    name: "broken".into(),
                    path: None,
                    schema: SchemaNode::group(
                        "broken",
                        vec![SchemaNode::scalar("Value", ScalarType::String)],
                    ),
                    options: Default::default(),
                    root: Scope {
                        target_field: "".into(),
                        iteration: ScopeIteration::None,
                        construction: mapping::ScopeConstruction::Constructed,
                        filter: None,
                        post_group_filter: None,
                        group_by: None,
                        group_adjacent_by: None,
                        group_starting_with: None,
                        group_ending_with: None,
                        group_into_blocks: None,
                        sort_by: None,
                        sort_descending: false,
                        sort_then_by: vec![],
                        sort_filter_order: mapping::SortFilterOrder::SortThenFilter,
                        windows: vec![],
                        iteration_output: mapping::IterationOutput::Repeated,
                        bindings: vec![Binding {
                            target_field: "Value".into(),
                            node: 3,
                        }],
                        dynamic_bindings: vec![],
                        children: vec![],
                        dynamic_children: vec![],
                        merge_dynamic_fields: false,
                    },
                },
            ],
            failure_rules: vec![
                mapping::FailureRule {
                    iteration: mapping::FailureIteration::Source { collection: vec![] },
                    selection: mapping::FailureSelection::WhenTrue { predicate: 8 },
                    message: Some(9),
                },
                mapping::FailureRule {
                    iteration: mapping::FailureIteration::Source { collection: vec![] },
                    selection: mapping::FailureSelection::All,
                    message: Some(10),
                },
                mapping::FailureRule {
                    iteration: mapping::FailureIteration::Source { collection: vec![] },
                    selection: mapping::FailureSelection::All,
                    message: Some(11),
                },
            ],
            user_functions: [].into_iter().collect(),
            graph: Graph {
                nodes: [
                    (
                        1,
                        Node::Const {
                            value: Value::String("main untouched".into()),
                        },
                    ),
                    (
                        2,
                        Node::Const {
                            value: Value::String("chosen complete".into()),
                        },
                    ),
                    (3, Node::Raise { message: Some(4) }),
                    (
                        4,
                        Node::Const {
                            value: Value::String("unselected boom".into()),
                        },
                    ),
                    (
                        8,
                        Node::Const {
                            value: Value::Bool(false),
                        },
                    ),
                    (9, Node::Raise { message: None }),
                    (
                        10,
                        Node::Const {
                            value: Value::String("guard first".into()),
                        },
                    ),
                    (11, Node::Raise { message: None }),
                    (20, Node::Raise { message: None }),
                ]
                .into_iter()
                .collect(),
            },
            root: Scope {
                target_field: "".into(),
                iteration: ScopeIteration::None,
                construction: mapping::ScopeConstruction::Constructed,
                filter: None,
                post_group_filter: None,
                group_by: None,
                group_adjacent_by: None,
                group_starting_with: None,
                group_ending_with: None,
                group_into_blocks: None,
                sort_by: None,
                sort_descending: false,
                sort_then_by: vec![],
                sort_filter_order: mapping::SortFilterOrder::SortThenFilter,
                windows: vec![],
                iteration_output: mapping::IterationOutput::Repeated,
                bindings: vec![Binding {
                    target_field: "Value".into(),
                    node: 20,
                }],
                dynamic_bindings: vec![],
                children: vec![],
                dynamic_children: vec![],
                merge_dynamic_fields: false,
            },
        },
        Project {
            source: SchemaNode::group(
                "Files",
                vec![SchemaNode::scalar("File", ScalarType::String).repeating()],
            ),
            target: SchemaNode::group(
                "Primary",
                vec![SchemaNode::scalar("Value", ScalarType::String)],
            ),
            source_path: None,
            target_path: None,
            source_options: Default::default(),
            target_options: Default::default(),
            extra_sources: vec![
                mapping::NamedSource {
                    name: "unused_static".into(),
                    path: "unused-static.fixture".into(),
                    schema: SchemaNode::group(
                        "Unused",
                        vec![SchemaNode::scalar("Value", ScalarType::String)],
                    ),
                    options: Default::default(),
                    dynamic_path: None,
                },
                mapping::NamedSource {
                    name: "catalog".into(),
                    path: "".into(),
                    schema: SchemaNode::group(
                        "catalog",
                        vec![
                            SchemaNode::group(
                                "Rows",
                                vec![SchemaNode::scalar("Value", ScalarType::String)],
                            )
                            .repeating(),
                        ],
                    ),
                    options: Default::default(),
                    dynamic_path: Some(mapping::DynamicSourcePath {
                        node: 5,
                        iteration: vec!["File".into()],
                    }),
                },
                mapping::NamedSource {
                    name: "poison".into(),
                    path: "".into(),
                    schema: SchemaNode::group(
                        "poison",
                        vec![
                            SchemaNode::group(
                                "Rows",
                                vec![SchemaNode::scalar("Value", ScalarType::String)],
                            )
                            .repeating(),
                        ],
                    ),
                    options: Default::default(),
                    dynamic_path: Some(mapping::DynamicSourcePath {
                        node: 5,
                        iteration: vec!["File".into()],
                    }),
                },
            ],
            extra_targets: vec![
                mapping::NamedTarget {
                    name: "loaded".into(),
                    path: None,
                    schema: SchemaNode::group(
                        "Loaded",
                        vec![
                            SchemaNode::group(
                                "Row",
                                vec![SchemaNode::scalar("Value", ScalarType::String)],
                            )
                            .repeating(),
                        ],
                    ),
                    options: Default::default(),
                    root: Scope {
                        target_field: "".into(),
                        iteration: ScopeIteration::None,
                        construction: mapping::ScopeConstruction::Constructed,
                        filter: None,
                        post_group_filter: None,
                        group_by: None,
                        group_adjacent_by: None,
                        group_starting_with: None,
                        group_ending_with: None,
                        group_into_blocks: None,
                        sort_by: None,
                        sort_descending: false,
                        sort_then_by: vec![],
                        sort_filter_order: mapping::SortFilterOrder::SortThenFilter,
                        windows: vec![],
                        iteration_output: mapping::IterationOutput::Repeated,
                        bindings: vec![],
                        dynamic_bindings: vec![],
                        children: vec![Scope {
                            target_field: "Row".into(),
                            iteration: ScopeIteration::Source(vec![
                                "catalog".into(),
                                "Rows".into(),
                            ]),
                            construction: mapping::ScopeConstruction::Constructed,
                            filter: None,
                            post_group_filter: None,
                            group_by: None,
                            group_adjacent_by: None,
                            group_starting_with: None,
                            group_ending_with: None,
                            group_into_blocks: None,
                            sort_by: None,
                            sort_descending: false,
                            sort_then_by: vec![],
                            sort_filter_order: mapping::SortFilterOrder::SortThenFilter,
                            windows: vec![],
                            iteration_output: mapping::IterationOutput::Repeated,
                            bindings: vec![Binding {
                                target_field: "Value".into(),
                                node: 6,
                            }],
                            dynamic_bindings: vec![],
                            children: vec![],
                            dynamic_children: vec![],
                            merge_dynamic_fields: false,
                        }],
                        dynamic_children: vec![],
                        merge_dynamic_fields: false,
                    },
                },
                mapping::NamedTarget {
                    name: "poisoned".into(),
                    path: None,
                    schema: SchemaNode::group(
                        "Loaded",
                        vec![
                            SchemaNode::group(
                                "Row",
                                vec![SchemaNode::scalar("Value", ScalarType::String)],
                            )
                            .repeating(),
                        ],
                    ),
                    options: Default::default(),
                    root: Scope {
                        target_field: "".into(),
                        iteration: ScopeIteration::None,
                        construction: mapping::ScopeConstruction::Constructed,
                        filter: None,
                        post_group_filter: None,
                        group_by: None,
                        group_adjacent_by: None,
                        group_starting_with: None,
                        group_ending_with: None,
                        group_into_blocks: None,
                        sort_by: None,
                        sort_descending: false,
                        sort_then_by: vec![],
                        sort_filter_order: mapping::SortFilterOrder::SortThenFilter,
                        windows: vec![],
                        iteration_output: mapping::IterationOutput::Repeated,
                        bindings: vec![],
                        dynamic_bindings: vec![],
                        children: vec![Scope {
                            target_field: "Row".into(),
                            iteration: ScopeIteration::Source(vec!["poison".into(), "Rows".into()]),
                            construction: mapping::ScopeConstruction::Constructed,
                            filter: None,
                            post_group_filter: None,
                            group_by: None,
                            group_adjacent_by: None,
                            group_starting_with: None,
                            group_ending_with: None,
                            group_into_blocks: None,
                            sort_by: None,
                            sort_descending: false,
                            sort_then_by: vec![],
                            sort_filter_order: mapping::SortFilterOrder::SortThenFilter,
                            windows: vec![],
                            iteration_output: mapping::IterationOutput::Repeated,
                            bindings: vec![Binding {
                                target_field: "Value".into(),
                                node: 7,
                            }],
                            dynamic_bindings: vec![],
                            children: vec![],
                            dynamic_children: vec![],
                            merge_dynamic_fields: false,
                        }],
                        dynamic_children: vec![],
                        merge_dynamic_fields: false,
                    },
                },
            ],
            failure_rules: vec![],
            user_functions: [].into_iter().collect(),
            graph: Graph {
                nodes: [
                    (
                        1,
                        Node::Const {
                            value: Value::String("no dynamic request".into()),
                        },
                    ),
                    (
                        5,
                        Node::SourceField {
                            path: vec![],
                            frame: Some(vec!["File".into()]),
                        },
                    ),
                    (
                        6,
                        Node::SourceField {
                            path: vec!["Value".into()],
                            frame: Some(vec!["catalog".into(), "Rows".into()]),
                        },
                    ),
                    (
                        7,
                        Node::SourceField {
                            path: vec!["Value".into()],
                            frame: Some(vec!["poison".into(), "Rows".into()]),
                        },
                    ),
                ]
                .into_iter()
                .collect(),
            },
            root: Scope {
                target_field: "".into(),
                iteration: ScopeIteration::None,
                construction: mapping::ScopeConstruction::Constructed,
                filter: None,
                post_group_filter: None,
                group_by: None,
                group_adjacent_by: None,
                group_starting_with: None,
                group_ending_with: None,
                group_into_blocks: None,
                sort_by: None,
                sort_descending: false,
                sort_then_by: vec![],
                sort_filter_order: mapping::SortFilterOrder::SortThenFilter,
                windows: vec![],
                iteration_output: mapping::IterationOutput::Repeated,
                bindings: vec![Binding {
                    target_field: "Value".into(),
                    node: 1,
                }],
                dynamic_bindings: vec![],
                children: vec![],
                dynamic_children: vec![],
                merge_dynamic_fields: false,
            },
        },
        Project {
            source: SchemaNode::group("Input", vec![]),
            target: SchemaNode::group(
                "Primary",
                vec![
                    SchemaNode::group("A", vec![SchemaNode::scalar("Value", ScalarType::Int)])
                        .repeating(),
                    SchemaNode::group("B", vec![SchemaNode::scalar("Value", ScalarType::Int)])
                        .repeating(),
                ],
            ),
            source_path: None,
            target_path: None,
            source_options: Default::default(),
            target_options: Default::default(),
            extra_sources: vec![],
            extra_targets: vec![mapping::NamedTarget {
                name: "chosen".into(),
                path: None,
                schema: SchemaNode::group(
                    "Chosen",
                    vec![
                        SchemaNode::group("A", vec![SchemaNode::scalar("Value", ScalarType::Int)])
                            .repeating(),
                        SchemaNode::group("B", vec![SchemaNode::scalar("Value", ScalarType::Int)])
                            .repeating(),
                    ],
                ),
                options: Default::default(),
                root: Scope {
                    target_field: "".into(),
                    iteration: ScopeIteration::None,
                    construction: mapping::ScopeConstruction::Constructed,
                    filter: None,
                    post_group_filter: None,
                    group_by: None,
                    group_adjacent_by: None,
                    group_starting_with: None,
                    group_ending_with: None,
                    group_into_blocks: None,
                    sort_by: None,
                    sort_descending: false,
                    sort_then_by: vec![],
                    sort_filter_order: mapping::SortFilterOrder::SortThenFilter,
                    windows: vec![],
                    iteration_output: mapping::IterationOutput::Repeated,
                    bindings: vec![],
                    dynamic_bindings: vec![],
                    children: vec![
                        Scope {
                            target_field: "A".into(),
                            iteration: ScopeIteration::Sequence(
                                mapping::SequenceExpr::FilterMapV1(mapping::FilterMapV1 {
                                    source: Box::new(mapping::SequenceExpr::Generate {
                                        from: Some(1),
                                        to: 2,
                                        item: 30,
                                    }),
                                    item: 31,
                                    predicate: FunctionId::new(100),
                                    mapper: FunctionId::new(101),
                                    output_type: ScalarType::Int,
                                    captures: vec![],
                                }),
                            ),
                            construction: mapping::ScopeConstruction::Constructed,
                            filter: None,
                            post_group_filter: None,
                            group_by: None,
                            group_adjacent_by: None,
                            group_starting_with: None,
                            group_ending_with: None,
                            group_into_blocks: None,
                            sort_by: None,
                            sort_descending: false,
                            sort_then_by: vec![],
                            sort_filter_order: mapping::SortFilterOrder::SortThenFilter,
                            windows: vec![],
                            iteration_output: mapping::IterationOutput::Repeated,
                            bindings: vec![Binding {
                                target_field: "Value".into(),
                                node: 31,
                            }],
                            dynamic_bindings: vec![],
                            children: vec![],
                            dynamic_children: vec![],
                            merge_dynamic_fields: false,
                        },
                        Scope {
                            target_field: "B".into(),
                            iteration: ScopeIteration::Sequence(
                                mapping::SequenceExpr::FilterMapV1(mapping::FilterMapV1 {
                                    source: Box::new(mapping::SequenceExpr::Generate {
                                        from: Some(1),
                                        to: 2,
                                        item: 40,
                                    }),
                                    item: 41,
                                    predicate: FunctionId::new(100),
                                    mapper: FunctionId::new(101),
                                    output_type: ScalarType::Int,
                                    captures: vec![],
                                }),
                            ),
                            construction: mapping::ScopeConstruction::Constructed,
                            filter: None,
                            post_group_filter: None,
                            group_by: None,
                            group_adjacent_by: None,
                            group_starting_with: None,
                            group_ending_with: None,
                            group_into_blocks: None,
                            sort_by: None,
                            sort_descending: false,
                            sort_then_by: vec![],
                            sort_filter_order: mapping::SortFilterOrder::SortThenFilter,
                            windows: vec![],
                            iteration_output: mapping::IterationOutput::Repeated,
                            bindings: vec![Binding {
                                target_field: "Value".into(),
                                node: 41,
                            }],
                            dynamic_bindings: vec![],
                            children: vec![],
                            dynamic_children: vec![],
                            merge_dynamic_fields: false,
                        },
                    ],
                    dynamic_children: vec![],
                    merge_dynamic_fields: false,
                },
            }],
            failure_rules: vec![mapping::FailureRule {
                iteration: mapping::FailureIteration::Source { collection: vec![] },
                selection: mapping::FailureSelection::WhenTrue { predicate: 60 },
                message: Some(94),
            }],
            user_functions: [
                (
                    FunctionId::new(100),
                    UserFunction {
                        library: "selection-control".into(),
                        name: "keep".into(),
                        description: None,
                        parameters: vec![
                            mapping::FunctionParameter {
                                id: mapping::FunctionParameterId::new(1),
                                name: "item".into(),
                                ty: ScalarType::Int,
                            },
                            mapping::FunctionParameter {
                                id: mapping::FunctionParameterId::new(2),
                                name: "source_position".into(),
                                ty: ScalarType::Int,
                            },
                        ],
                        output_name: "value".into(),
                        output_type: ScalarType::Bool,
                        body: Graph {
                            nodes: [(
                                1,
                                Node::Const {
                                    value: Value::Bool(true),
                                },
                            )]
                            .into_iter()
                            .collect(),
                        },
                        output: 1,
                    },
                ),
                (
                    FunctionId::new(101),
                    UserFunction {
                        library: "selection-control".into(),
                        name: "identity".into(),
                        description: None,
                        parameters: vec![
                            mapping::FunctionParameter {
                                id: mapping::FunctionParameterId::new(1),
                                name: "item".into(),
                                ty: ScalarType::Int,
                            },
                            mapping::FunctionParameter {
                                id: mapping::FunctionParameterId::new(2),
                                name: "source_position".into(),
                                ty: ScalarType::Int,
                            },
                        ],
                        output_name: "value".into(),
                        output_type: ScalarType::Int,
                        body: Graph {
                            nodes: [(
                                1,
                                Node::FunctionParameter {
                                    parameter: mapping::FunctionParameterId::new(1),
                                },
                            )]
                            .into_iter()
                            .collect(),
                        },
                        output: 1,
                    },
                ),
            ]
            .into_iter()
            .collect(),
            graph: Graph {
                nodes: [
                    (
                        1,
                        Node::Const {
                            value: Value::Int(1),
                        },
                    ),
                    (
                        2,
                        Node::Const {
                            value: Value::Int(2),
                        },
                    ),
                    (
                        10,
                        Node::SourceField {
                            path: vec![],
                            frame: None,
                        },
                    ),
                    (
                        11,
                        Node::SourceField {
                            path: vec![],
                            frame: None,
                        },
                    ),
                    (
                        20,
                        Node::SourceField {
                            path: vec![],
                            frame: None,
                        },
                    ),
                    (
                        21,
                        Node::SourceField {
                            path: vec![],
                            frame: None,
                        },
                    ),
                    (
                        30,
                        Node::SourceField {
                            path: vec![],
                            frame: None,
                        },
                    ),
                    (
                        31,
                        Node::SourceField {
                            path: vec![],
                            frame: None,
                        },
                    ),
                    (
                        40,
                        Node::SourceField {
                            path: vec![],
                            frame: None,
                        },
                    ),
                    (
                        41,
                        Node::SourceField {
                            path: vec![],
                            frame: None,
                        },
                    ),
                    (
                        50,
                        Node::SourceField {
                            path: vec![],
                            frame: None,
                        },
                    ),
                    (
                        51,
                        Node::SourceField {
                            path: vec![],
                            frame: None,
                        },
                    ),
                    (
                        60,
                        Node::SequenceExists {
                            sequence: mapping::SequenceExpr::FilterMapV1(mapping::FilterMapV1 {
                                source: Box::new(mapping::SequenceExpr::Generate {
                                    from: Some(1),
                                    to: 2,
                                    item: 50,
                                }),
                                item: 51,
                                predicate: FunctionId::new(100),
                                mapper: FunctionId::new(101),
                                output_type: ScalarType::Int,
                                captures: vec![],
                            }),
                            predicate: 99,
                        },
                    ),
                    (94, Node::Raise { message: None }),
                    (
                        99,
                        Node::Const {
                            value: Value::Bool(false),
                        },
                    ),
                ]
                .into_iter()
                .collect(),
            },
            root: Scope {
                target_field: "".into(),
                iteration: ScopeIteration::None,
                construction: mapping::ScopeConstruction::Constructed,
                filter: None,
                post_group_filter: None,
                group_by: None,
                group_adjacent_by: None,
                group_starting_with: None,
                group_ending_with: None,
                group_into_blocks: None,
                sort_by: None,
                sort_descending: false,
                sort_then_by: vec![],
                sort_filter_order: mapping::SortFilterOrder::SortThenFilter,
                windows: vec![],
                iteration_output: mapping::IterationOutput::Repeated,
                bindings: vec![],
                dynamic_bindings: vec![],
                children: vec![
                    Scope {
                        target_field: "A".into(),
                        iteration: ScopeIteration::Sequence(mapping::SequenceExpr::FilterMapV1(
                            mapping::FilterMapV1 {
                                source: Box::new(mapping::SequenceExpr::Generate {
                                    from: Some(1),
                                    to: 2,
                                    item: 10,
                                }),
                                item: 11,
                                predicate: FunctionId::new(100),
                                mapper: FunctionId::new(101),
                                output_type: ScalarType::Int,
                                captures: vec![],
                            },
                        )),
                        construction: mapping::ScopeConstruction::Constructed,
                        filter: None,
                        post_group_filter: None,
                        group_by: None,
                        group_adjacent_by: None,
                        group_starting_with: None,
                        group_ending_with: None,
                        group_into_blocks: None,
                        sort_by: None,
                        sort_descending: false,
                        sort_then_by: vec![],
                        sort_filter_order: mapping::SortFilterOrder::SortThenFilter,
                        windows: vec![],
                        iteration_output: mapping::IterationOutput::Repeated,
                        bindings: vec![Binding {
                            target_field: "Value".into(),
                            node: 11,
                        }],
                        dynamic_bindings: vec![],
                        children: vec![],
                        dynamic_children: vec![],
                        merge_dynamic_fields: false,
                    },
                    Scope {
                        target_field: "B".into(),
                        iteration: ScopeIteration::Sequence(mapping::SequenceExpr::FilterMapV1(
                            mapping::FilterMapV1 {
                                source: Box::new(mapping::SequenceExpr::Generate {
                                    from: Some(1),
                                    to: 2,
                                    item: 20,
                                }),
                                item: 21,
                                predicate: FunctionId::new(100),
                                mapper: FunctionId::new(101),
                                output_type: ScalarType::Int,
                                captures: vec![],
                            },
                        )),
                        construction: mapping::ScopeConstruction::Constructed,
                        filter: None,
                        post_group_filter: None,
                        group_by: None,
                        group_adjacent_by: None,
                        group_starting_with: None,
                        group_ending_with: None,
                        group_into_blocks: None,
                        sort_by: None,
                        sort_descending: false,
                        sort_then_by: vec![],
                        sort_filter_order: mapping::SortFilterOrder::SortThenFilter,
                        windows: vec![],
                        iteration_output: mapping::IterationOutput::Repeated,
                        bindings: vec![Binding {
                            target_field: "Value".into(),
                            node: 21,
                        }],
                        dynamic_bindings: vec![],
                        children: vec![],
                        dynamic_children: vec![],
                        merge_dynamic_fields: false,
                    },
                ],
                dynamic_children: vec![],
                merge_dynamic_fields: false,
            },
        },
    ]
}
