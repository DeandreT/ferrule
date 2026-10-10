//! Complete public constructors transcribed from the frozen seven recipes.
use ir::{ScalarType, SchemaNode, Value};
use mapping::{Binding, FunctionId, Graph, Node, Project, Scope, ScopeIteration};

pub(super) fn projects() -> Vec<Project> {
    vec![
        Project {
            source: SchemaNode::group(
                "Input",
                vec![SchemaNode::scalar("message", ScalarType::String)],
            )
            .with_required_fields(vec!["message".into()])
            .unwrap(),
            target: SchemaNode::group(
                "Primary",
                vec![SchemaNode::scalar("message", ScalarType::String)],
            ),
            source_path: None,
            target_path: None,
            source_options: Default::default(),
            target_options: Default::default(),
            extra_sources: vec![],
            extra_targets: vec![
                mapping::NamedTarget {
                    name: "résumé📦".into(),
                    path: None,
                    schema: SchemaNode::group(
                        "Résumé",
                        vec![
                            SchemaNode::scalar("text", ScalarType::String),
                            SchemaNode::scalar("n", ScalarType::Int),
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
                        bindings: vec![
                            Binding {
                                target_field: "text".into(),
                                node: 1,
                            },
                            Binding {
                                target_field: "n".into(),
                                node: 2,
                            },
                        ],
                        dynamic_bindings: vec![],
                        children: vec![],
                        dynamic_children: vec![],
                        merge_dynamic_fields: false,
                    },
                },
                mapping::NamedTarget {
                    name: "boom".into(),
                    path: None,
                    schema: SchemaNode::group(
                        "Boom",
                        vec![SchemaNode::scalar("message", ScalarType::String)],
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
                            target_field: "message".into(),
                            node: 3,
                        }],
                        dynamic_bindings: vec![],
                        children: vec![],
                        dynamic_children: vec![],
                        merge_dynamic_fields: false,
                    },
                },
                mapping::NamedTarget {
                    name: "rows".into(),
                    path: None,
                    schema: SchemaNode::group(
                        "Rows",
                        vec![SchemaNode::scalar("n", ScalarType::Int)],
                    )
                    .repeating(),
                    options: Default::default(),
                    root: Scope {
                        target_field: "".into(),
                        iteration: ScopeIteration::Sequence(mapping::SequenceExpr::Generate {
                            from: Some(30),
                            to: 31,
                            item: 32,
                        }),
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
                            target_field: "n".into(),
                            node: 32,
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
                        Node::SourceField {
                            path: vec!["message".into()],
                            frame: None,
                        },
                    ),
                    (
                        2,
                        Node::Const {
                            value: Value::Int(7),
                        },
                    ),
                    (3, Node::Raise { message: Some(4) }),
                    (
                        4,
                        Node::Const {
                            value: Value::String("218 selected boom".into()),
                        },
                    ),
                    (
                        30,
                        Node::Const {
                            value: Value::Int(1),
                        },
                    ),
                    (
                        31,
                        Node::Const {
                            value: Value::Int(2),
                        },
                    ),
                    (
                        32,
                        Node::SourceField {
                            path: vec![],
                            frame: None,
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
                    target_field: "message".into(),
                    node: 1,
                }],
                dynamic_bindings: vec![],
                children: vec![],
                dynamic_children: vec![],
                merge_dynamic_fields: false,
            },
        },
        Project {
            source: SchemaNode::group(
                "Input",
                vec![SchemaNode::scalar("message", ScalarType::String)],
            )
            .with_required_fields(vec!["message".into()])
            .unwrap(),
            target: SchemaNode::group(
                "Primary",
                vec![SchemaNode::scalar("value", ScalarType::String)],
            ),
            source_path: None,
            target_path: None,
            source_options: Default::default(),
            target_options: Default::default(),
            extra_sources: vec![
                mapping::NamedSource {
                    name: "alpha".into(),
                    path: "alpha.json".into(),
                    schema: SchemaNode::group(
                        "Alpha",
                        vec![SchemaNode::scalar("code", ScalarType::String)],
                    )
                    .with_required_fields(vec!["code".into()])
                    .unwrap(),
                    options: Default::default(),
                    dynamic_path: None,
                },
                mapping::NamedSource {
                    name: "beta".into(),
                    path: "beta.json".into(),
                    schema: SchemaNode::group(
                        "Beta",
                        vec![SchemaNode::scalar("code", ScalarType::String)],
                    )
                    .with_required_fields(vec!["code".into()])
                    .unwrap(),
                    options: Default::default(),
                    dynamic_path: None,
                },
            ],
            extra_targets: vec![mapping::NamedTarget {
                name: "ref".into(),
                path: None,
                schema: SchemaNode::group(
                    "Ref",
                    vec![SchemaNode::scalar("code", ScalarType::String)],
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
                        target_field: "code".into(),
                        node: 2,
                    }],
                    dynamic_bindings: vec![],
                    children: vec![],
                    dynamic_children: vec![],
                    merge_dynamic_fields: false,
                },
            }],
            failure_rules: vec![],
            user_functions: [].into_iter().collect(),
            graph: Graph {
                nodes: [
                    (
                        1,
                        Node::Const {
                            value: Value::String("静".into()),
                        },
                    ),
                    (
                        2,
                        Node::SourceField {
                            path: vec!["alpha".into(), "code".into()],
                            frame: None,
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
                    target_field: "value".into(),
                    node: 1,
                }],
                dynamic_bindings: vec![],
                children: vec![],
                dynamic_children: vec![],
                merge_dynamic_fields: false,
            },
        },
        Project {
            source: SchemaNode::group(
                "Input",
                vec![SchemaNode::scalar("Files", ScalarType::String).repeating()],
            )
            .with_required_fields(vec!["Files".into()])
            .unwrap(),
            target: SchemaNode::group(
                "Primary",
                vec![SchemaNode::scalar("message", ScalarType::String)],
            ),
            source_path: None,
            target_path: None,
            source_options: Default::default(),
            target_options: Default::default(),
            extra_sources: vec![
                mapping::NamedSource {
                    name: "unused".into(),
                    path: "unused.json".into(),
                    schema: SchemaNode::group(
                        "Unused",
                        vec![SchemaNode::scalar("code", ScalarType::String)],
                    )
                    .with_required_fields(vec!["code".into()])
                    .unwrap(),
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
                                vec![SchemaNode::scalar("value", ScalarType::String)],
                            )
                            .repeating(),
                        ],
                    ),
                    options: Default::default(),
                    dynamic_path: Some(mapping::DynamicSourcePath {
                        node: 5,
                        iteration: vec!["Files".into()],
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
                                vec![SchemaNode::scalar("value", ScalarType::String)],
                            )
                            .repeating(),
                        ],
                    ),
                    options: Default::default(),
                    dynamic_path: Some(mapping::DynamicSourcePath {
                        node: 5,
                        iteration: vec!["Files".into()],
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
                                "rows",
                                vec![SchemaNode::scalar("value", ScalarType::String)],
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
                            target_field: "rows".into(),
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
                                target_field: "value".into(),
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
                        "Poisoned",
                        vec![
                            SchemaNode::group(
                                "rows",
                                vec![SchemaNode::scalar("value", ScalarType::String)],
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
                            target_field: "rows".into(),
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
                                target_field: "value".into(),
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
                            value: Value::String("skip dynamic".into()),
                        },
                    ),
                    (
                        5,
                        Node::SourceField {
                            path: vec![],
                            frame: Some(vec!["Files".into()]),
                        },
                    ),
                    (
                        6,
                        Node::SourceField {
                            path: vec!["value".into()],
                            frame: Some(vec!["catalog".into(), "Rows".into()]),
                        },
                    ),
                    (
                        7,
                        Node::SourceField {
                            path: vec!["value".into()],
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
                    target_field: "message".into(),
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
                vec![SchemaNode::scalar("message", ScalarType::String)],
            ),
            source_path: None,
            target_path: None,
            source_options: Default::default(),
            target_options: Default::default(),
            extra_sources: vec![],
            extra_targets: vec![mapping::NamedTarget {
                name: "blocked".into(),
                path: None,
                schema: SchemaNode::group(
                    "Blocked",
                    vec![SchemaNode::scalar("message", ScalarType::String)],
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
                        target_field: "message".into(),
                        node: 2,
                    }],
                    dynamic_bindings: vec![],
                    children: vec![],
                    dynamic_children: vec![],
                    merge_dynamic_fields: false,
                },
            }],
            failure_rules: vec![mapping::FailureRule {
                iteration: mapping::FailureIteration::Source { collection: vec![] },
                selection: mapping::FailureSelection::WhenTrue { predicate: 80 },
                message: Some(81),
            }],
            user_functions: [].into_iter().collect(),
            graph: Graph {
                nodes: [
                    (
                        1,
                        Node::Const {
                            value: Value::String("primary untouched".into()),
                        },
                    ),
                    (
                        2,
                        Node::Const {
                            value: Value::String("named untouched".into()),
                        },
                    ),
                    (
                        80,
                        Node::Const {
                            value: Value::Bool(true),
                        },
                    ),
                    (
                        81,
                        Node::Const {
                            value: Value::String("218 global failure".into()),
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
                    target_field: "message".into(),
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
            target: SchemaNode::scalar("PrimaryText", ScalarType::String),
            source_path: None,
            target_path: None,
            source_options: Default::default(),
            target_options: Default::default(),
            extra_sources: vec![],
            extra_targets: vec![
                mapping::NamedTarget {
                    name: "parameter".into(),
                    path: None,
                    schema: SchemaNode::scalar("ParameterText", ScalarType::String),
                    options: Default::default(),
                    root: Scope {
                        target_field: "".into(),
                        iteration: ScopeIteration::None,
                        construction: mapping::ScopeConstruction::Scalar { value: 1 },
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
                        children: vec![],
                        dynamic_children: vec![],
                        merge_dynamic_fields: false,
                    },
                },
                mapping::NamedTarget {
                    name: "expanded".into(),
                    path: None,
                    schema: SchemaNode::scalar("ExpandedText", ScalarType::String),
                    options: Default::default(),
                    root: Scope {
                        target_field: "".into(),
                        iteration: ScopeIteration::None,
                        construction: mapping::ScopeConstruction::Scalar { value: 5 },
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
                        children: vec![],
                        dynamic_children: vec![],
                        merge_dynamic_fields: false,
                    },
                },
                mapping::NamedTarget {
                    name: "expanded-over".into(),
                    path: None,
                    schema: SchemaNode::scalar("ExpandedText", ScalarType::String),
                    options: Default::default(),
                    root: Scope {
                        target_field: "".into(),
                        iteration: ScopeIteration::None,
                        construction: mapping::ScopeConstruction::Scalar { value: 6 },
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
                        Node::RuntimeParameter {
                            name: "greeting".into(),
                            ty: ScalarType::String,
                            preview: None,
                        },
                    ),
                    (
                        2,
                        Node::Const {
                            value: Value::String("ok".into()),
                        },
                    ),
                    (
                        3,
                        Node::Const {
                            value: Value::String("xxxxx".into()),
                        },
                    ),
                    (
                        4,
                        Node::Const {
                            value: Value::String("xxxxxx".into()),
                        },
                    ),
                    (
                        5,
                        Node::Call {
                            function: "concat".into(),
                            args: vec![1, 1, 1, 1, 1, 1, 1, 1, 3],
                        },
                    ),
                    (
                        6,
                        Node::Call {
                            function: "concat".into(),
                            args: vec![1, 1, 1, 1, 1, 1, 1, 1, 4],
                        },
                    ),
                ]
                .into_iter()
                .collect(),
            },
            root: Scope {
                target_field: "".into(),
                iteration: ScopeIteration::None,
                construction: mapping::ScopeConstruction::Scalar { value: 2 },
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
                children: vec![],
                dynamic_children: vec![],
                merge_dynamic_fields: false,
            },
        },
        Project {
            source: SchemaNode::group("Input", vec![]),
            target: SchemaNode::group(
                "Primary",
                vec![SchemaNode::scalar("message", ScalarType::String)],
            ),
            source_path: None,
            target_path: None,
            source_options: Default::default(),
            target_options: Default::default(),
            extra_sources: vec![],
            extra_targets: vec![mapping::NamedTarget {
                name: "filtered".into(),
                path: None,
                schema: SchemaNode::group(
                    "Filtered",
                    vec![
                        SchemaNode::group("A", vec![SchemaNode::scalar("n", ScalarType::Int)])
                            .repeating(),
                        SchemaNode::group("B", vec![SchemaNode::scalar("n", ScalarType::Int)])
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
                                        item: 10,
                                    }),
                                    item: 11,
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
                                target_field: "n".into(),
                                node: 11,
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
                                        item: 20,
                                    }),
                                    item: 21,
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
                                target_field: "n".into(),
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
            }],
            failure_rules: vec![],
            user_functions: [
                (
                    FunctionId::new(100),
                    mapping::UserFunction {
                        library: "selected-json218".into(),
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
                                name: "position".into(),
                                ty: ScalarType::Int,
                            },
                        ],
                        output_name: "result".into(),
                        output_type: ScalarType::Bool,
                        body: Graph {
                            nodes: [(
                                3,
                                Node::Const {
                                    value: Value::Bool(true),
                                },
                            )]
                            .into_iter()
                            .collect(),
                        },
                        output: 3,
                    },
                ),
                (
                    FunctionId::new(101),
                    mapping::UserFunction {
                        library: "selected-json218".into(),
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
                                name: "position".into(),
                                ty: ScalarType::Int,
                            },
                        ],
                        output_name: "result".into(),
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
                        3,
                        Node::Const {
                            value: Value::String("no stages".into()),
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
                    target_field: "message".into(),
                    node: 3,
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
                vec![SchemaNode::scalar("message", ScalarType::String)],
            ),
            source_path: None,
            target_path: None,
            source_options: Default::default(),
            target_options: Default::default(),
            extra_sources: vec![],
            extra_targets: vec![mapping::NamedTarget {
                name: "one-scalar".into(),
                path: None,
                schema: SchemaNode::scalar("OneScalar", ScalarType::String)
                    .with_string_length_range(ir::StringLengthRange::new(1, Some(1)).unwrap())
                    .unwrap(),
                options: Default::default(),
                root: Scope {
                    target_field: "".into(),
                    iteration: ScopeIteration::None,
                    construction: mapping::ScopeConstruction::Scalar { value: 2 },
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
                    children: vec![],
                    dynamic_children: vec![],
                    merge_dynamic_fields: false,
                },
            }],
            failure_rules: vec![],
            user_functions: [].into_iter().collect(),
            graph: Graph {
                nodes: [
                    (
                        1,
                        Node::Const {
                            value: Value::String("safe".into()),
                        },
                    ),
                    (
                        2,
                        Node::Const {
                            value: Value::String("é".into()),
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
                    target_field: "message".into(),
                    node: 1,
                }],
                dynamic_bindings: vec![],
                children: vec![],
                dynamic_children: vec![],
                merge_dynamic_fields: false,
            },
        },
    ]
}
