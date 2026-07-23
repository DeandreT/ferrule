mod grouping;
mod join;
mod recursive;
mod resolve;
mod walk;

#[cfg(test)]
mod collection_find_tests;
#[cfg(test)]
mod grouping_tests;
#[cfg(test)]
mod join_tests;
#[cfg(test)]
mod named_tests;

pub use grouping::GroupedItems;
pub use join::{InnerJoinKey, InnerJoinStage};
pub use resolve::{InstanceKind, SourcePathError, clone_scalar, resolve_scalar};

use crate::{ExecutionContext, Instance, RuntimeError, RuntimeValue, Value};

/// One borrowed, statically declared secondary input.
#[derive(Clone, Copy, Debug)]
pub struct NamedInput<'a> {
    pub name: &'a str,
    pub instance: &'a Instance,
}

/// Owns scalar instances materialized by one generated sequence while its
/// borrowed candidate contexts are evaluated.
pub struct GeneratedItems {
    items: Vec<Instance>,
}

impl GeneratedItems {
    pub fn new(values: Vec<Value>) -> Self {
        Self {
            items: values.into_iter().map(Instance::Scalar).collect(),
        }
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
    named_inputs: &'a [NamedInput<'a>],
    execution: Option<ExecutionContext<'a>>,
}

#[derive(Clone)]
struct ScopeFrame<'a> {
    instance: &'a Instance,
    collection: Option<CollectionIdentity>,
    document_path: Option<&'a str>,
    join: Option<u64>,
    join_position: Option<(u64, usize)>,
}

#[derive(Clone)]
enum CollectionIdentity {
    Repeated { path: Vec<String>, index: usize },
    Document { path: Vec<String>, index: usize },
    Grouped { path: Vec<String>, index: usize },
}

impl CollectionIdentity {
    fn path(&self) -> &[String] {
        match self {
            Self::Repeated { path, .. }
            | Self::Document { path, .. }
            | Self::Grouped { path, .. } => path,
        }
    }

    const fn index(&self) -> usize {
        match self {
            Self::Repeated { index, .. }
            | Self::Document { index, .. }
            | Self::Grouped { index, .. } => *index,
        }
    }

    const fn set_index(&mut self, compact_index: usize) {
        match self {
            Self::Repeated { index, .. }
            | Self::Document { index, .. }
            | Self::Grouped { index, .. } => {
                *index = compact_index;
            }
        }
    }

    const fn is_grouped(&self) -> bool {
        matches!(self, Self::Grouped { .. })
    }
}

impl<'a> ScopeContext<'a> {
    /// Creates the root context for one generated mapping execution.
    pub fn new(source: &'a Instance) -> Self {
        Self {
            frames: vec![ScopeFrame {
                instance: source,
                collection: None,
                document_path: None,
                join: None,
                join_position: None,
            }],
            named_inputs: &[],
            execution: None,
        }
    }

    /// Creates a root context with borrowed secondary inputs available as
    /// one outer named frame.
    pub fn with_named_inputs(source: &'a Instance, inputs: &'a [NamedInput<'a>]) -> Self {
        Self {
            frames: vec![ScopeFrame {
                instance: source,
                collection: None,
                document_path: None,
                join: None,
                join_position: None,
            }],
            named_inputs: inputs,
            execution: None,
        }
    }

    /// Creates the root context with host values available to runtime-value
    /// expressions. The supplied metadata is copied while its path and text
    /// values remain borrowed for the mapping execution.
    pub fn with_execution_context(source: &'a Instance, execution: &ExecutionContext<'a>) -> Self {
        Self {
            frames: vec![ScopeFrame {
                instance: source,
                collection: None,
                document_path: None,
                join: None,
                join_position: None,
            }],
            named_inputs: &[],
            execution: Some(*execution),
        }
    }

    /// Creates a root context with secondary inputs and host values.
    pub fn with_named_inputs_and_execution_context(
        source: &'a Instance,
        inputs: &'a [NamedInput<'a>],
        execution: &ExecutionContext<'a>,
    ) -> Self {
        Self {
            frames: vec![ScopeFrame {
                instance: source,
                collection: None,
                document_path: None,
                join: None,
                join_position: None,
            }],
            named_inputs: inputs,
            execution: Some(*execution),
        }
    }

    /// Resolves one host-supplied scalar or returns the same typed missing
    /// value error as the interpreter.
    pub fn runtime_value(&self, value: RuntimeValue) -> Result<Value, RuntimeError> {
        self.execution
            .and_then(|execution| execution.value(value))
            .ok_or(RuntimeError::MissingRuntimeValue { value })
    }

    /// Returns the resolved path of the nearest active source document.
    pub fn source_document_path(&self) -> Result<Value, SourcePathError> {
        self.frames
            .iter()
            .rev()
            .find_map(|frame| frame.document_path)
            .or_else(|| {
                self.frames.iter().rev().find_map(|frame| {
                    frame
                        .instance
                        .as_document_set()?
                        .first()
                        .map(ir::DocumentMember::source_path)
                })
            })
            .map(|path| Value::String(path.to_string()))
            .ok_or(SourcePathError::MissingDocumentPath)
    }

    /// Clones the innermost source group for independent target ownership.
    pub fn copy_current_group(&self) -> Result<Instance, crate::RuntimeError> {
        match self.frames.last().map(|frame| frame.instance) {
            Some(current @ Instance::Group(_)) => Ok(current.clone()),
            Some(Instance::Scalar(_)) => {
                Err(crate::RuntimeError::CopyCurrentSourceRequiresGroup { found: "scalar" })
            }
            Some(Instance::Repeated(_)) => {
                Err(crate::RuntimeError::CopyCurrentSourceRequiresGroup {
                    found: "repeated collection",
                })
            }
            Some(Instance::MappedSequence(_)) => {
                Err(crate::RuntimeError::CopyCurrentSourceRequiresGroup {
                    found: "mapped sequence",
                })
            }
            Some(Instance::DocumentSet(_)) => {
                Err(crate::RuntimeError::CopyCurrentSourceRequiresGroup {
                    found: "document set",
                })
            }
            None => Err(crate::RuntimeError::CopyCurrentSourceRequiresGroup {
                found: "missing context",
            }),
        }
    }

    /// Returns the 1-based position of the innermost active collection whose
    /// path ends with `collection`. An empty path selects the innermost active
    /// collection. Missing collection context has the engine's default value
    /// of `1`.
    pub fn position(&self, collection: &[&str]) -> usize {
        self.frames
            .iter()
            .rev()
            .filter_map(|frame| frame.collection.as_ref())
            .find(|identity| {
                collection.is_empty() || string_path_has_suffix(identity.path(), collection)
            })
            .map(CollectionIdentity::index)
            .unwrap_or(1)
    }

    /// Clones this context and replaces only its innermost collection's
    /// position. Source instances remain borrowed and unchanged.
    ///
    /// Generated filtered scopes use this view for output expressions after
    /// evaluating the predicate against the original candidate context.
    pub fn with_compact_last_position(&self, compact_index: usize) -> Self {
        let mut compact = self.clone();
        if let Some((_, position)) = compact
            .frames
            .iter_mut()
            .rev()
            .find_map(|frame| frame.join_position.as_mut())
        {
            *position = compact_index.max(1);
            return compact;
        }
        if let Some(collection) = compact
            .frames
            .iter_mut()
            .rev()
            .find_map(|frame| frame.collection.as_mut())
        {
            collection.set_index(compact_index.max(1));
        }
        compact
    }

    /// Reborrows the parent frames and appends each generated scalar as an
    /// empty-path collection frame with its raw one-based position.
    pub fn generated_item_contexts<'b>(
        &'b self,
        items: &'b GeneratedItems,
    ) -> impl Iterator<Item = ScopeContext<'b>> + 'b
    where
        'a: 'b,
    {
        items.items.iter().enumerate().map(move |(index, item)| {
            let mut frames = self
                .frames
                .iter()
                .map(|frame| ScopeFrame {
                    instance: frame.instance,
                    collection: frame.collection.clone(),
                    document_path: frame.document_path,
                    join: frame.join,
                    join_position: frame.join_position,
                })
                .collect::<Vec<_>>();
            frames.push(collection_frame(
                item,
                CollectionIdentity::Repeated {
                    path: Vec::new(),
                    index: index + 1,
                },
            ));
            ScopeContext {
                frames,
                named_inputs: self.named_inputs,
                execution: self.execution,
            }
        })
    }

    /// Materializes all generated-item contexts for scope sorting, filtering,
    /// and windowing while retaining the lazy iterator's frame semantics.
    pub fn generated_items<'b>(&'b self, items: &'b GeneratedItems) -> Vec<ScopeContext<'b>>
    where
        'a: 'b,
    {
        self.generated_item_contexts(items).collect()
    }

    pub(crate) fn current_instance(&self) -> Option<&Instance> {
        self.frames.last().map(|frame| frame.instance)
    }

    pub(crate) fn with_xml_mixed_content_value<'b>(
        &'b self,
        value: &'b Instance,
        collection: &[&str],
        index: usize,
    ) -> ScopeContext<'b>
    where
        'a: 'b,
    {
        let mut frames = self
            .frames
            .iter()
            .map(|frame| ScopeFrame {
                instance: frame.instance,
                collection: frame.collection.clone(),
                document_path: frame.document_path,
                join: frame.join,
                join_position: frame.join_position,
            })
            .collect::<Vec<_>>();
        frames.push(collection_frame(
            value,
            CollectionIdentity::Repeated {
                path: collection
                    .iter()
                    .map(|segment| (*segment).to_string())
                    .collect(),
                index,
            },
        ));
        ScopeContext {
            frames,
            named_inputs: self.named_inputs,
            execution: self.execution,
        }
    }

    pub(crate) fn with_recursive_filter_item<'b>(
        &'b self,
        item: &'b Instance,
        collection: &str,
        index: usize,
    ) -> ScopeContext<'b>
    where
        'a: 'b,
    {
        let mut frames = self
            .frames
            .iter()
            .map(|frame| ScopeFrame {
                instance: frame.instance,
                collection: frame.collection.clone(),
                document_path: frame.document_path,
                join: frame.join,
                join_position: frame.join_position,
            })
            .collect::<Vec<_>>();
        frames.push(collection_frame(
            item,
            CollectionIdentity::Repeated {
                path: vec![collection.to_string()],
                index,
            },
        ));
        ScopeContext {
            frames,
            named_inputs: self.named_inputs,
            execution: self.execution,
        }
    }

    fn named_input(&self, name: &str) -> Option<&'a Instance> {
        self.named_inputs
            .iter()
            .find(|input| input.name == name)
            .map(|input| input.instance)
    }
}

fn collection_frame<'a>(instance: &'a Instance, collection: CollectionIdentity) -> ScopeFrame<'a> {
    ScopeFrame {
        instance,
        collection: Some(collection),
        document_path: None,
        join: None,
        join_position: None,
    }
}

fn document_frame<'a>(
    instance: &'a Instance,
    collection: CollectionIdentity,
    source_path: &'a str,
) -> ScopeFrame<'a> {
    ScopeFrame {
        instance,
        collection: Some(collection),
        document_path: Some(source_path),
        join: None,
        join_position: None,
    }
}

fn has_prefix(path: &[&str], prefix: &[String]) -> bool {
    path.len() >= prefix.len()
        && path
            .iter()
            .zip(prefix)
            .all(|(segment, expected)| *segment == expected)
}

fn has_suffix(path: &[&str], suffix: &[String]) -> bool {
    path.len() >= suffix.len()
        && path[path.len() - suffix.len()..]
            .iter()
            .zip(suffix)
            .all(|(segment, expected)| *segment == expected)
}

fn same_path(path: &[&str], expected: &[String]) -> bool {
    path.len() == expected.len()
        && path
            .iter()
            .zip(expected)
            .all(|(segment, expected)| *segment == expected)
}

fn string_path_has_suffix(path: &[String], suffix: &[&str]) -> bool {
    path.len() >= suffix.len()
        && path[path.len() - suffix.len()..]
            .iter()
            .zip(suffix)
            .all(|(segment, expected)| segment == expected)
}
