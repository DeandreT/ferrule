use mapping::{FunctionId, NodeId};

/// An endpoint in the project that produced a validation result.
///
/// Named endpoints retain their declaration index as well as their name so
/// even invalid duplicate or empty names can be addressed without ambiguity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidationEndpoint {
    Source,
    Target,
    NamedSource { index: usize, name: String },
    NamedTarget { index: usize, name: String },
}

/// One structural step from a target's root scope. All indexes are zero-based.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidationScopeStep {
    Child(usize),
    DynamicChild(usize),
    Segment(usize),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationScopeLocation {
    pub target: ValidationEndpoint,
    pub path: Vec<ValidationScopeStep>,
}

impl ValidationScopeLocation {
    pub(super) fn root(target: ValidationEndpoint) -> Self {
        Self {
            target,
            path: Vec::new(),
        }
    }

    pub(super) fn descendant(&self, step: ValidationScopeStep) -> Self {
        let mut location = self.clone();
        location.path.push(step);
        location
    }
}

/// One structural step from a boundary's schema root.
///
/// Structural indexes distinguish duplicate child names and schema predicates
/// from ordinary fields whose names resemble the human-readable location text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidationSchemaStep {
    Child(usize),
    DynamicField,
    Contains(usize),
    DependentSchema(usize),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationSchemaLocation {
    pub endpoint: ValidationEndpoint,
    pub path: Vec<ValidationSchemaStep>,
}

impl ValidationSchemaLocation {
    pub(super) fn root(endpoint: ValidationEndpoint) -> Self {
        Self {
            endpoint,
            path: Vec::new(),
        }
    }

    pub(super) fn descendant(&self, step: ValidationSchemaStep) -> Self {
        let mut location = self.clone();
        location.path.push(step);
        location
    }
}

/// Machine-readable ownership, independent of validation display text.
///
/// Locations refer to the project snapshot that was validated. Hosts must
/// revalidate after structural edits before navigating an old result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidationOwner {
    GraphNode {
        function: Option<FunctionId>,
        node: NodeId,
    },
    UserFunction(FunctionId),
    Endpoint(ValidationEndpoint),
    SchemaNode(ValidationSchemaLocation),
    Scope(ValidationScopeLocation),
    FailureRule {
        index: usize,
    },
}
