//! Runtime primitives used by generated Rust mappings.
//!
//! This crate intentionally contains only construction, source-context
//! traversal, and scalar builtin operations. Scope orchestration and expression
//! control flow belong in generated code, not in a second mapping interpreter.

#![forbid(unsafe_code)]

use std::fmt;

pub use functions::FunctionError;
pub use ir::{Instance, ScalarType, Value};

/// Failure produced while executing generated mapping code.
#[derive(Debug, PartialEq)]
pub enum RuntimeError {
    SourcePath(SourcePathError),
    Function(FunctionError),
    NotABool { node: u32, found: &'static str },
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SourcePath(error) => error.fmt(formatter),
            Self::Function(error) => error.fmt(formatter),
            Self::NotABool { node, found } => {
                write!(formatter, "node {node}: expected a bool, got {found}")
            }
        }
    }
}

impl std::error::Error for RuntimeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::SourcePath(error) => Some(error),
            Self::Function(error) => Some(error),
            Self::NotABool { .. } => None,
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

/// The structural kind encountered while resolving a scalar source path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InstanceKind {
    Scalar,
    Group,
    Repeated,
    DocumentSet,
    MappedSequence,
}

impl InstanceKind {
    fn of(instance: &Instance) -> Self {
        match instance {
            Instance::Scalar(_) => Self::Scalar,
            Instance::Group(_) => Self::Group,
            Instance::Repeated(_) => Self::Repeated,
            Instance::DocumentSet(_) => Self::DocumentSet,
            Instance::MappedSequence(_) => Self::MappedSequence,
        }
    }
}

impl fmt::Display for InstanceKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Scalar => "scalar",
            Self::Group => "group",
            Self::Repeated => "repeated value",
            Self::DocumentSet => "document set",
            Self::MappedSequence => "mapped sequence",
        })
    }
}

/// Ordered source frames visible to one generated target scope.
///
/// Cloning a context clones only its frame metadata; source instances remain
/// borrowed from the input. Source iteration appends one frame for every
/// repeated or document collection it crosses, plus a plain terminal frame
/// when the selected value is not itself a collection.
#[derive(Clone)]
pub struct ScopeContext<'a> {
    frames: Vec<ScopeFrame<'a>>,
}

#[derive(Clone)]
struct ScopeFrame<'a> {
    instance: &'a Instance,
    collection: Option<CollectionIdentity>,
}

#[derive(Clone)]
enum CollectionIdentity {
    Repeated(Vec<String>),
    Document(Vec<String>),
}

impl CollectionIdentity {
    fn path(&self) -> &[String] {
        match self {
            Self::Repeated(path) | Self::Document(path) => path,
        }
    }
}

impl<'a> ScopeContext<'a> {
    /// Creates the root context for one generated mapping execution.
    pub fn new(source: &'a Instance) -> Self {
        Self {
            frames: vec![ScopeFrame {
                instance: source,
                collection: None,
            }],
        }
    }

    /// Produces one child context for every item selected by `path`.
    ///
    /// The path is evaluated from the innermost frame that owns its first
    /// field, falling back to the current frame. An empty path iterates the
    /// current repeated/document value, or selects one ordinary current
    /// value. Repetition crossed at any depth branches in source order.
    pub fn walk_source(&self, path: &[&str]) -> Vec<Self> {
        let Some(base) = self
            .frames
            .iter()
            .rev()
            .find(|frame| match path.first() {
                Some(first) => frame.instance.field(first).is_some(),
                None => true,
            })
            .or_else(|| self.frames.last())
        else {
            return Vec::new();
        };

        walk_source_frames(base.instance, path, &[], &[])
            .into_iter()
            .map(|extension| {
                let mut frames = self.frames.clone();
                frames.extend(extension);
                Self { frames }
            })
            .collect()
    }

    /// Resolves a scalar using active collection identity before ordinary
    /// innermost-to-outermost fallback.
    ///
    /// Uniterated repetitions contribute their first item. An empty
    /// repetition resolves to `Null` immediately and therefore shadows an
    /// outer field with the same path.
    pub fn resolve_scalar(&self, path: &[&str]) -> Result<Value, SourcePathError> {
        let owned_path = owned_path(path);
        let mut first_error = None;

        for frame in self.frames.iter().rev() {
            let Some(collection) = &frame.collection else {
                continue;
            };
            let prefix = collection.path();
            if prefix.is_empty() || !has_prefix(path, prefix) {
                continue;
            }
            match resolve_scalar_in(
                frame.instance,
                &path[prefix.len()..],
                &owned_path,
                prefix.len(),
            ) {
                Ok(value) => return Ok(value),
                Err(error) => first_error.get_or_insert(error),
            };
        }

        for frame in self.frames.iter().rev() {
            match resolve_scalar_in(frame.instance, path, &owned_path, 0) {
                Ok(value) => return Ok(value),
                Err(error) => first_error.get_or_insert(error),
            };
        }

        Err(first_error.unwrap_or(SourcePathError::ExpectedScalar {
            path: owned_path,
            found: InstanceKind::Group,
        }))
    }
}

fn walk_source_frames<'a>(
    base: &'a Instance,
    path: &[&str],
    prefix: &[String],
    acc: &[ScopeFrame<'a>],
) -> Vec<Vec<ScopeFrame<'a>>> {
    match base {
        Instance::Repeated(items) if !path.is_empty() => {
            return items
                .iter()
                .flat_map(|item| {
                    let mut next = acc.to_vec();
                    next.push(collection_frame(
                        item,
                        CollectionIdentity::Repeated(prefix.to_vec()),
                    ));
                    walk_source_frames(item, path, prefix, &next)
                })
                .collect();
        }
        _ => {}
    }

    match path.split_first() {
        None => match base {
            Instance::DocumentSet(documents) => documents
                .iter()
                .map(|document| {
                    let mut next = acc.to_vec();
                    next.push(collection_frame(
                        document.value(),
                        CollectionIdentity::Document(prefix.to_vec()),
                    ));
                    next
                })
                .collect(),
            Instance::Repeated(items) => items
                .iter()
                .map(|item| {
                    let mut next = acc.to_vec();
                    next.push(collection_frame(
                        item,
                        CollectionIdentity::Repeated(prefix.to_vec()),
                    ));
                    next
                })
                .collect(),
            _ => {
                let mut next = acc.to_vec();
                next.push(ScopeFrame {
                    instance: base,
                    collection: None,
                });
                vec![next]
            }
        },
        Some((segment, rest)) => {
            if let Instance::DocumentSet(documents) = base {
                return documents
                    .iter()
                    .flat_map(|document| {
                        let mut next = acc.to_vec();
                        next.push(collection_frame(
                            document.value(),
                            CollectionIdentity::Document(prefix.to_vec()),
                        ));
                        walk_source_frames(document.value(), path, prefix, &next)
                    })
                    .collect();
            }

            let mut collection_path = prefix.to_vec();
            collection_path.push((*segment).to_string());
            match base.field(segment) {
                None => Vec::new(),
                Some(Instance::Repeated(items)) => items
                    .iter()
                    .flat_map(|item| {
                        let mut next = acc.to_vec();
                        next.push(collection_frame(
                            item,
                            CollectionIdentity::Repeated(collection_path.clone()),
                        ));
                        if rest.is_empty() {
                            vec![next]
                        } else {
                            walk_source_frames(item, rest, &collection_path, &next)
                        }
                    })
                    .collect(),
                Some(other) => walk_source_frames(other, rest, &collection_path, acc),
            }
        }
    }
}

fn collection_frame<'a>(instance: &'a Instance, collection: CollectionIdentity) -> ScopeFrame<'a> {
    ScopeFrame {
        instance,
        collection: Some(collection),
    }
}

fn has_prefix(path: &[&str], prefix: &[String]) -> bool {
    path.len() >= prefix.len()
        && path
            .iter()
            .zip(prefix)
            .all(|(segment, expected)| *segment == expected)
}

/// Failure to resolve a generated mapping's static scalar source path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SourcePathError {
    /// A named field was absent from the current group or first document.
    MissingField {
        path: Vec<String>,
        segment: usize,
        field: String,
    },
    /// A path segment attempted to traverse a scalar or unsupported sequence.
    CannotTraverse {
        path: Vec<String>,
        segment: usize,
        found: InstanceKind,
    },
    /// The complete path selected a structural value instead of a scalar.
    ExpectedScalar {
        path: Vec<String>,
        found: InstanceKind,
    },
}

impl fmt::Display for SourcePathError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingField {
                path,
                segment,
                field,
            } => write!(
                formatter,
                "source path {} is missing field {field:?} at segment {segment}",
                display_path(path)
            ),
            Self::CannotTraverse {
                path,
                segment,
                found,
            } => write!(
                formatter,
                "source path {} cannot traverse {found} at segment {segment}",
                display_path(path)
            ),
            Self::ExpectedScalar { path, found } => write!(
                formatter,
                "source path {} resolved to {found}, expected scalar",
                display_path(path)
            ),
        }
    }
}

impl std::error::Error for SourcePathError {}

/// Resolves one scalar without outward context fallback or scalar coercion.
///
/// Every uniterated [`Instance::Repeated`] in the path contributes its first
/// item, matching engine behavior for the initial non-iterating codegen
/// subset. [`Instance::DocumentSet`] traversal remains transparent through
/// [`Instance::field`], which selects its first document. An empty path is
/// valid only when `source` itself is scalar.
pub fn resolve_scalar(source: &Instance, path: &[&str]) -> Result<Value, SourcePathError> {
    ScopeContext::new(source).resolve_scalar(path)
}

/// Resolves and clones one scalar value for independent target ownership.
pub fn clone_scalar(source: &Instance, path: &[&str]) -> Result<Value, SourcePathError> {
    resolve_scalar(source, path)
}

fn first_repeated(instance: &Instance) -> Option<&Instance> {
    match instance {
        Instance::Repeated(items) => items.first(),
        _ => Some(instance),
    }
}

fn resolve_scalar_in(
    source: &Instance,
    path: &[&str],
    owned_path: &[String],
    segment_offset: usize,
) -> Result<Value, SourcePathError> {
    let mut current = source;
    for (segment, field_name) in path.iter().enumerate() {
        let Some(next) = first_repeated(current) else {
            return Ok(Value::Null);
        };
        current = next;
        current = current.field(field_name).ok_or_else(|| {
            let found = InstanceKind::of(current);
            if matches!(found, InstanceKind::Group | InstanceKind::DocumentSet) {
                SourcePathError::MissingField {
                    path: owned_path.to_vec(),
                    segment: segment_offset + segment,
                    field: field_name.to_string(),
                }
            } else {
                SourcePathError::CannotTraverse {
                    path: owned_path.to_vec(),
                    segment: segment_offset + segment,
                    found,
                }
            }
        })?;
    }

    let Some(current) = first_repeated(current) else {
        return Ok(Value::Null);
    };
    current
        .as_scalar()
        .cloned()
        .ok_or_else(|| SourcePathError::ExpectedScalar {
            path: owned_path.to_vec(),
            found: InstanceKind::of(current),
        })
}

fn owned_path(path: &[&str]) -> Vec<String> {
    path.iter().map(|segment| (*segment).to_string()).collect()
}

fn display_path(path: &[String]) -> String {
    if path.is_empty() {
        "<current>".to_string()
    } else {
        path.join("/")
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
