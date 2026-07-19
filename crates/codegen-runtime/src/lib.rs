//! Runtime primitives used by generated Rust mappings.
//!
//! This crate intentionally contains only construction, source-context
//! traversal, and scalar builtin operations. Scope orchestration and expression
//! control flow belong in generated code, not in a second mapping interpreter.

#![forbid(unsafe_code)]

mod aggregate;
mod context;
mod generated_sequence;
mod iteration;

use std::fmt;

pub use aggregate::{AggregateFunction, aggregate};
pub use context::{
    GeneratedItems, InstanceKind, ScopeContext, SourcePathError, clone_scalar, resolve_scalar,
};
pub use functions::FunctionError;
pub use generated_sequence::{
    MAX_GENERATED_SEQUENCE_ITEMS, generate_sequence, tokenize, tokenize_by_length,
};
pub use ir::{Instance, ScalarType, Value};
pub use iteration::{
    SequenceWindow, SortDirection, apply_sequence_windows, item_count, sort_candidates,
};

/// Failure produced while executing generated mapping code.
#[derive(Debug, PartialEq)]
pub enum RuntimeError {
    SourcePath(SourcePathError),
    Function(FunctionError),
    AggregateIntegerOverflow { function: AggregateFunction },
    AggregateNonFinite { function: AggregateFunction },
    GeneratedSequenceTooLarge { requested: u128, max: u128 },
    NotABool { node: u32, found: &'static str },
    NotAnItemCount { node: u32, found: &'static str },
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SourcePath(error) => error.fmt(formatter),
            Self::Function(error) => error.fmt(formatter),
            Self::AggregateIntegerOverflow { function } => {
                write!(
                    formatter,
                    "{function:?} aggregate overflowed the integer range"
                )
            }
            Self::AggregateNonFinite { function } => write!(
                formatter,
                "{function:?} aggregate encountered or produced a non-finite number"
            ),
            Self::GeneratedSequenceTooLarge { requested, max } => write!(
                formatter,
                "generate-sequence requested {requested} items; maximum is {max}"
            ),
            Self::NotABool { node, found } => {
                write!(formatter, "node {node}: expected a bool, got {found}")
            }
            Self::NotAnItemCount { node, found } => {
                write!(
                    formatter,
                    "node {node}: expected an item count, got {found}"
                )
            }
        }
    }
}

impl std::error::Error for RuntimeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::SourcePath(error) => Some(error),
            Self::Function(error) => Some(error),
            Self::AggregateIntegerOverflow { .. }
            | Self::AggregateNonFinite { .. }
            | Self::GeneratedSequenceTooLarge { .. }
            | Self::NotABool { .. }
            | Self::NotAnItemCount { .. } => None,
        }
    }
}

impl From<SourcePathError> for RuntimeError {
    fn from(error: SourcePathError) -> Self {
        Self::SourcePath(error)
    }
}

impl From<FunctionError> for RuntimeError {
    fn from(error: FunctionError) -> Self {
        Self::Function(error)
    }
}

/// Dispatches one scalar builtin while retaining its typed failure.
pub fn call(function: &str, args: &[Value]) -> Result<Value, RuntimeError> {
    functions::call(function, args).map_err(RuntimeError::from)
}

/// Extracts the boolean condition required by a generated conditional.
pub fn require_bool(node: u32, value: Value) -> Result<bool, RuntimeError> {
    match value {
        Value::Bool(value) => Ok(value),
        value => Err(RuntimeError::NotABool {
            node,
            found: value.type_name(),
        }),
    }
}

/// One ordered field supplied to [`group`].
pub type GroupField = (String, Instance);

/// Creates one named group field.
pub fn field(name: impl Into<String>, value: Instance) -> GroupField {
    (name.into(), value)
}

/// Creates a group while retaining the input iterator's field order.
pub fn group(fields: impl IntoIterator<Item = GroupField>) -> Instance {
    Instance::Group(fields.into_iter().collect())
}

/// Creates a scalar instance.
pub fn scalar(value: Value) -> Instance {
    Instance::Scalar(value)
}

/// Creates a repeated instance while retaining item order.
pub fn repeated(items: impl IntoIterator<Item = Instance>) -> Instance {
    Instance::Repeated(items.into_iter().collect())
}

/// Creates an absent scalar value.
pub const fn null() -> Value {
    Value::Null
}

/// Creates a boolean scalar value.
pub const fn boolean(value: bool) -> Value {
    Value::Bool(value)
}

/// Creates a signed integer scalar value.
pub const fn integer(value: i64) -> Value {
    Value::Int(value)
}

/// Creates a finite or non-finite floating-point scalar value without coercion.
pub const fn float(value: f64) -> Value {
    Value::Float(value)
}

/// Creates a string scalar value.
pub fn string(value: impl Into<String>) -> Value {
    Value::String(value.into())
}

/// Creates a present XML `xsi:nil` scalar value.
pub fn xml_nil() -> Value {
    Value::xml_nil()
}

/// Applies the scalar coercions performed when an expression is bound to a
/// numeric target field. Values that cannot be converted exactly are kept in
/// their original representation.
pub fn adapt_target_value(value: Value, expected: ScalarType) -> Value {
    match (expected, value) {
        (ScalarType::Int, Value::Float(value))
            if value.is_finite()
                && value.fract() == 0.0
                && value >= i64::MIN as f64
                && value < -(i64::MIN as f64) =>
        {
            Value::Int(value as i64)
        }
        (ScalarType::Float, Value::Int(value)) => {
            let converted = value as f64;
            if (converted as i128) == i128::from(value) {
                Value::Float(converted)
            } else {
                Value::Int(value)
            }
        }
        (_, value) => value,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ir::DocumentMember;

    #[test]
    fn resolves_nested_group_fields_and_empty_scalar_path() {
        let source = group([field("Order", group([field("Id", scalar(integer(42)))]))]);

        assert_eq!(
            resolve_scalar(&source, &["Order", "Id"]),
            Ok(Value::Int(42))
        );
        assert_eq!(
            resolve_scalar(&scalar(boolean(true)), &[]),
            Ok(Value::Bool(true))
        );
    }

    #[test]
    fn reads_first_item_when_crossing_uniterated_repetition() {
        let source = group([field(
            "Rows",
            repeated([
                group([field("Name", scalar(string("first")))]),
                group([field("Name", scalar(string("second")))]),
            ]),
        )]);

        assert_eq!(
            clone_scalar(&source, &["Rows", "Name"]),
            Ok(Value::String("first".to_string()))
        );
    }

    #[test]
    fn source_walk_handles_empty_nonrepeating_and_document_collections() {
        let row = |name: &str| group([field("Name", scalar(string(name)))]);
        let rows = repeated([row("first"), row("second")]);
        let empty_context = ScopeContext::new(&rows).walk_source(&[]);
        assert_eq!(empty_context.len(), 2);
        assert_eq!(
            empty_context[1].resolve_scalar(&["Name"]),
            Ok(string("second"))
        );

        let profile_source = group([field("Profile", row("only"))]);
        let profile_context = ScopeContext::new(&profile_source).walk_source(&["Profile"]);
        assert_eq!(profile_context.len(), 1);
        assert_eq!(
            profile_context[0].resolve_scalar(&["Name"]),
            Ok(string("only"))
        );

        let Some(first) = DocumentMember::new("first.xml", row("document one")) else {
            panic!("valid first document member")
        };
        let Some(second) = DocumentMember::new("second.xml", row("document two")) else {
            panic!("valid second document member")
        };
        let documents = Instance::DocumentSet(vec![first, second]);
        let document_contexts = ScopeContext::new(&documents).walk_source(&[]);
        assert_eq!(document_contexts.len(), 2);
        assert_eq!(
            document_contexts[1].resolve_scalar(&["Name"]),
            Ok(string("document two"))
        );
    }

    #[test]
    fn generated_items_retain_parent_fallback_and_independent_positions() {
        let source = group([field(
            "Rows",
            repeated([
                group([field("Name", scalar(string("first")))]),
                group([field("Name", scalar(string("second")))]),
            ]),
        )]);
        let rows = ScopeContext::new(&source).walk_source(&["Rows"]);
        let generated = GeneratedItems::new(vec![integer(10), integer(20)]);
        let items = rows[1].generated_items(&generated);

        assert_eq!(items.len(), 2);
        assert_eq!(items[1].resolve_scalar(&[]), Ok(integer(20)));
        assert_eq!(items[1].resolve_scalar(&["Name"]), Ok(string("second")));
        assert_eq!(items[1].position(&[]), 2);
        assert_eq!(items[1].position(&["Rows"]), 2);

        let compact = items[1].with_compact_last_position(7);
        assert_eq!(compact.resolve_scalar(&[]), Ok(integer(20)));
        assert_eq!(compact.position(&[]), 7);
        assert_eq!(compact.position(&["Rows"]), 2);
    }

    #[test]
    fn active_collection_prefixes_select_current_multi_hop_items() {
        let child = |name: &str| group([field("Name", scalar(string(name)))]);
        let parent = |id: i64, children: Vec<Instance>| {
            group([
                field("Id", scalar(integer(id))),
                field("Children", repeated(children)),
            ])
        };
        let source = group([field(
            "Parents",
            repeated([
                parent(1, vec![child("a"), child("b")]),
                parent(2, vec![child("c")]),
            ]),
        )]);

        let contexts = ScopeContext::new(&source).walk_source(&["Parents", "Children"]);

        assert_eq!(contexts.len(), 3);
        assert_eq!(
            contexts[1].resolve_scalar(&["Parents", "Id"]),
            Ok(integer(1))
        );
        assert_eq!(
            contexts[1].resolve_scalar(&["Parents", "Children", "Name"]),
            Ok(string("b"))
        );
        assert_eq!(
            contexts[2].resolve_scalar(&["Parents", "Id"]),
            Ok(integer(2))
        );
    }

    #[test]
    fn pinned_fields_positions_and_compact_views_select_exact_collection_frames() {
        let child = |name: &str| group([field("Name", scalar(string(name)))]);
        let parent = |id: i64, children: Vec<Instance>| {
            group([
                field("Id", scalar(integer(id))),
                field("Children", repeated(children)),
            ])
        };
        let source = group([field(
            "Parents",
            repeated([
                parent(1, vec![child("a"), child("b")]),
                parent(2, vec![child("c")]),
            ]),
        )]);

        let parents = ScopeContext::new(&source).walk_source(&["Parents"]);
        let children = parents[0].walk_source(&["Children"]);
        let second = &children[1];

        assert_eq!(
            second.resolve_scalar_in_frame(&["Parents"], &["Id"]),
            Ok(integer(1))
        );
        assert_eq!(
            second.resolve_scalar_in_frame(&["Parents", "Children"], &["Name"]),
            Ok(string("b"))
        );
        assert_eq!(second.position(&["Parents"]), 1);
        assert_eq!(second.position(&["Children"]), 2);
        assert_eq!(second.position(&[]), 2);
        assert_eq!(second.position(&["Inactive"]), 1);

        let compact = second.with_compact_last_position(7);
        assert_eq!(second.position(&["Children"]), 2);
        assert_eq!(compact.position(&["Children"]), 7);
        assert_eq!(compact.position(&["Parents"]), 1);

        assert_eq!(
            second.resolve_scalar_in_frame(&["Inactive"], &["Name"]),
            Err(SourcePathError::MissingFrame {
                frame: vec!["Inactive".to_string()],
                path: vec!["Name".to_string()],
            })
        );
    }

    #[test]
    fn innermost_fallback_and_empty_repetition_shadowing_match_the_engine() {
        let source = group([
            field(
                "Rows",
                repeated([group([field("Name", scalar(string("outer")))])]),
            ),
            field(
                "Inner",
                group([
                    field("Rows", repeated(Vec::<Instance>::new())),
                    field("Local", scalar(string("inner"))),
                ]),
            ),
        ]);
        let contexts = ScopeContext::new(&source).walk_source(&["Inner"]);

        assert_eq!(contexts.len(), 1);
        assert_eq!(contexts[0].resolve_scalar(&["Local"]), Ok(string("inner")));
        assert_eq!(
            contexts[0].resolve_scalar(&["Rows", "Name"]),
            Ok(Value::Null)
        );
        assert!(matches!(
            contexts[0].resolve_scalar(&["Missing"]),
            Err(SourcePathError::MissingField { field, .. }) if field == "Missing"
        ));
    }

    #[test]
    fn first_document_is_transparent_to_field_traversal() {
        let first = DocumentMember::new("first.xml", group([field("Code", scalar(string("A")))]));
        let second = DocumentMember::new("second.xml", group([field("Code", scalar(string("B")))]));
        let documents = Instance::DocumentSet([first, second].into_iter().flatten().collect());

        assert_eq!(clone_scalar(&documents, &["Code"]), Ok(string("A")));
    }

    #[test]
    fn missing_and_group_terminal_paths_are_typed_errors() {
        let source = group([field("Nested", group([]))]);

        assert_eq!(
            resolve_scalar(&source, &["Missing"]),
            Err(SourcePathError::MissingField {
                path: vec!["Missing".to_string()],
                segment: 0,
                field: "Missing".to_string(),
            })
        );
        assert_eq!(
            resolve_scalar(&source, &["Nested"]),
            Err(SourcePathError::ExpectedScalar {
                path: vec!["Nested".to_string()],
                found: InstanceKind::Group,
            })
        );
    }

    #[test]
    fn empty_repetition_resolves_to_null() {
        let source = group([field("Rows", repeated([]))]);

        assert_eq!(resolve_scalar(&source, &["Rows", "Name"]), Ok(Value::Null));
        assert_eq!(resolve_scalar(&repeated([]), &[]), Ok(Value::Null));
    }

    #[test]
    fn mapped_sequences_are_not_implicitly_crossed() {
        let source =
            Instance::MappedSequence(vec![group([field("Name", scalar(string("first")))])]);

        assert_eq!(
            resolve_scalar(&source, &["Name"]),
            Err(SourcePathError::CannotTraverse {
                path: vec!["Name".to_string()],
                segment: 0,
                found: InstanceKind::MappedSequence,
            })
        );
        assert_eq!(
            resolve_scalar(&source, &[]),
            Err(SourcePathError::ExpectedScalar {
                path: Vec::new(),
                found: InstanceKind::MappedSequence,
            })
        );
    }

    #[test]
    fn cloning_preserves_null_and_xml_nil() {
        let source = group([
            field("Absent", scalar(null())),
            field("Nil", scalar(xml_nil())),
        ]);

        assert_eq!(clone_scalar(&source, &["Absent"]), Ok(Value::Null));
        assert_eq!(clone_scalar(&source, &["Nil"]), Ok(Value::xml_nil()));
    }

    #[test]
    fn group_helper_preserves_declared_field_order() {
        let value = group([
            field("third", scalar(integer(3))),
            field("first", scalar(integer(1))),
            field("second", scalar(integer(2))),
        ]);
        let Instance::Group(fields) = value else {
            panic!("group helper must return a group");
        };

        assert_eq!(
            fields
                .iter()
                .map(|(name, _)| name.as_str())
                .collect::<Vec<_>>(),
            ["third", "first", "second"]
        );
    }

    #[test]
    fn target_numeric_adaptation_requires_exact_values() {
        assert_eq!(
            adapt_target_value(Value::Float(42.0), ScalarType::Int),
            Value::Int(42)
        );
        assert_eq!(
            adapt_target_value(Value::Float(42.5), ScalarType::Int),
            Value::Float(42.5)
        );
        assert_eq!(
            adapt_target_value(Value::Int(42), ScalarType::Float),
            Value::Float(42.0)
        );
        let imprecise = 9_007_199_254_740_993_i64;
        assert_eq!(
            adapt_target_value(Value::Int(imprecise), ScalarType::Float),
            Value::Int(imprecise)
        );
    }

    #[test]
    fn aggregate_items_flatten_multi_hop_collections_and_retain_positions() {
        let row = |value: i64| {
            group([field(
                "Payload",
                group([field("Value", scalar(integer(value)))]),
            )])
        };
        let bucket = |rows| group([field("Rows", repeated(rows))]);
        let department = |buckets| group([field("Buckets", repeated(buckets))]);
        let source = group([field(
            "Departments",
            repeated([
                department(vec![bucket(vec![row(10)])]),
                department(vec![bucket(vec![row(20), row(21)]), bucket(vec![row(22)])]),
            ]),
        )]);
        let departments = ScopeContext::new(&source).walk_source(&["Departments"]);

        let items = departments[1].aggregate_items(&["Buckets", "Rows"]);

        assert_eq!(items.len(), 3);
        assert_eq!(
            items
                .iter()
                .map(|item| item.aggregate_current_scalar(&["Payload", "Value"]))
                .collect::<Vec<_>>(),
            [integer(20), integer(21), integer(22)]
        );
        assert_eq!(
            items
                .iter()
                .map(|item| item.position(&["Departments"]))
                .collect::<Vec<_>>(),
            [2, 2, 2]
        );
        assert_eq!(
            items
                .iter()
                .map(|item| item.position(&["Buckets"]))
                .collect::<Vec<_>>(),
            [1, 1, 2]
        );
        assert_eq!(
            items
                .iter()
                .map(|item| item.position(&["Rows"]))
                .collect::<Vec<_>>(),
            [1, 2, 1]
        );
    }

    #[test]
    fn aggregate_items_support_empty_scalar_and_document_collections() {
        let values = repeated([scalar(integer(4)), scalar(integer(7))]);
        let items = ScopeContext::new(&values).aggregate_items(&[]);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].aggregate_current_scalar(&[]), integer(4));
        assert_eq!(items[1].aggregate_current_scalar(&[]), integer(7));
        assert_eq!(items[0].position(&[]), 1);
        assert_eq!(items[1].position(&[]), 2);

        let Some(first) =
            DocumentMember::new("first.xml", group([field("Value", scalar(integer(11)))]))
        else {
            panic!("valid first aggregate document")
        };
        let Some(second) =
            DocumentMember::new("second.xml", group([field("Value", scalar(integer(12)))]))
        else {
            panic!("valid second aggregate document")
        };
        let documents = Instance::DocumentSet(vec![first, second]);
        let items = ScopeContext::new(&documents).aggregate_items(&[]);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].aggregate_current_scalar(&["Value"]), integer(11));
        assert_eq!(items[1].aggregate_current_scalar(&["Value"]), integer(12));
        assert_eq!(items[1].position(&[]), 2);
    }

    #[test]
    fn aggregate_lookup_uses_the_innermost_owner_without_value_fallback() {
        let line = |value: i64| {
            group([
                field("Value", scalar(integer(value))),
                field("Structural", group(Vec::new())),
            ])
        };
        let source = group([
            field("OuterValue", scalar(integer(99))),
            field("Lines", repeated([line(1)])),
            field(
                "Containers",
                repeated([group([field("Lines", repeated([line(7), line(8)]))])]),
            ),
        ]);
        let containers = ScopeContext::new(&source).walk_source(&["Containers"]);

        let items = containers[0].aggregate_items(&["Lines"]);

        assert_eq!(items.len(), 2);
        assert_eq!(items[0].aggregate_current_scalar(&["Value"]), integer(7));
        assert_eq!(items[1].aggregate_current_scalar(&["Value"]), integer(8));
        assert_eq!(items[0].resolve_scalar(&["OuterValue"]), Ok(integer(99)));
        assert_eq!(
            items[0].aggregate_current_scalar(&["OuterValue"]),
            Value::Null
        );
        assert_eq!(
            items[0].aggregate_current_scalar(&["Structural"]),
            Value::Null
        );
        assert_eq!(items[0].aggregate_current_scalar(&["Missing"]), Value::Null);
        assert!(containers[0].aggregate_items(&["Missing"]).is_empty());
    }

    #[test]
    fn scalar_calls_preserve_typed_function_failures() {
        assert_eq!(
            call("add", &[Value::Int(4), Value::Int(5)]),
            Ok(Value::Int(9))
        );
        assert_eq!(
            call("divide", &[Value::Int(1), Value::Int(0)]),
            Err(RuntimeError::Function(FunctionError::DivideByZero))
        );
    }

    #[test]
    fn boolean_requirements_retain_the_condition_node() {
        assert_eq!(require_bool(12, Value::Bool(true)), Ok(true));
        assert_eq!(
            require_bool(12, Value::String("not a bool".to_string())),
            Err(RuntimeError::NotABool {
                node: 12,
                found: "string",
            })
        );
    }
}
