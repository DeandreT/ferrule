use serde::{Deserialize, Serialize};

use crate::{Index, NodeId, Scalar};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Format {
    V1,
}

impl Serialize for Format {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str("ferrule.edit.v1")
    }
}

impl<'de> Deserialize<'de> for Format {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        match String::deserialize(deserializer)?.as_str() {
            "ferrule.edit.v1" => Ok(Self::V1),
            _ => Err(serde::de::Error::custom("expected ferrule.edit.v1")),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EditScript {
    pub format: Format,
    pub operations: Vec<EditOperation>,
}

/// Target names and user functions are not editable in v1, but existing named
/// targets can be addressed without a positional identity.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Target {
    Primary {},
    Named { name: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScopeRef {
    pub target: Target,
    pub children: Vec<Index>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SourceCollection {
    Legacy { path: Vec<String> },
    Primary { path: Vec<String> },
    Named { name: String, path: Vec<String> },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SourceOwner {
    Primary {},
    Named { name: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BindingRef {
    pub scope: ScopeRef,
    pub field: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Expression {
    Existing {
        node: NodeId,
    },
    Source {
        id: NodeId,
        path: Vec<String>,
        frame: Option<Vec<String>>,
    },
    OwnedSource {
        id: NodeId,
        owner: SourceOwner,
        path: Vec<String>,
        frame: Option<Vec<String>>,
    },
    Constant {
        id: NodeId,
        value: Scalar,
    },
    Call {
        id: NodeId,
        function: String,
        args: Vec<NodeId>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ScalarType {
    String {},
    Int {},
    Float {},
    Bool {},
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ValueMap {
    pub input_type: Option<ScalarType>,
    pub table: Vec<ValueMapEntry>,
    /// None disables the default; a typed Null enables an absent default.
    pub default: Option<Scalar>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ValueMapEntry {
    pub from: Scalar,
    pub to: Scalar,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Grouping {
    None {},
    By { node: NodeId },
    AdjacentBy { node: NodeId },
    StartingWith { node: NodeId },
    EndingWith { node: NodeId },
    IntoBlocks { node: NodeId },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SortKey {
    pub node: NodeId,
    pub descending: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum FailureSelection {
    All {},
    WhenTrue { expression: Expression },
    WhenFalse { expression: Expression },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum GeneratedSequence {
    Generate {
        from: Option<Expression>,
        to: Expression,
        item: NodeId,
    },
    Tokenize {
        input: Expression,
        delimiter: Expression,
        item: NodeId,
    },
    TokenizeByLength {
        input: Expression,
        length: Expression,
        item: NodeId,
    },
    TokenizeRegex {
        input: Expression,
        pattern: Expression,
        flags: Option<Expression>,
        item: NodeId,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum FailureIteration {
    Source { collection: SourceCollection },
    Generated { sequence: GeneratedSequence },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FailureRule {
    pub iteration: FailureIteration,
    pub selection: FailureSelection,
    pub message: Option<Expression>,
}

/// Closed intent vocabulary; no native model objects or arbitrary patches.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum EditOperation {
    Bind {
        binding: BindingRef,
        expression: Expression,
    },
    RetargetBinding {
        binding: BindingRef,
        to: BindingRef,
        node: NodeId,
    },
    RemoveBinding {
        binding: BindingRef,
    },
    Connect {
        node: NodeId,
        input: Index,
        expression: Expression,
    },
    Disconnect {
        node: NodeId,
        input: Index,
        replacement: Option<NodeId>,
    },
    EditConstant {
        node: NodeId,
        value: Scalar,
    },
    EditCall {
        node: NodeId,
        function: String,
        args: Vec<NodeId>,
    },
    EditValueMap {
        node: NodeId,
        value: ValueMap,
    },
    RemoveNodes {
        scope: ScopeRef,
        nodes: Vec<NodeId>,
        replacements: Vec<NodeId>,
    },
    AddScope {
        parent: ScopeRef,
        field: String,
    },
    RemoveScope {
        scope: ScopeRef,
    },
    SetIteration {
        scope: ScopeRef,
        source: Option<SourceCollection>,
    },
    SetFilter {
        scope: ScopeRef,
        node: Option<NodeId>,
    },
    SetSort {
        scope: ScopeRef,
        keys: Vec<SortKey>,
    },
    SetGrouping {
        scope: ScopeRef,
        grouping: Grouping,
    },
    AddFailureRule {
        rule: FailureRule,
    },
    RemoveFailureRule {
        index: Index,
    },
    MoveFailureRule {
        from: Index,
        to: Index,
    },
    EditFailureRule {
        index: Index,
        rule: FailureRule,
    },
}

impl EditOperation {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Bind { .. } => "Map target field",
            Self::RetargetBinding { .. } => "Retarget binding",
            Self::RemoveBinding { .. } => "Remove binding",
            Self::Connect { .. } => "Connect input",
            Self::Disconnect { .. } => "Disconnect input",
            Self::EditConstant { .. } => "Edit constant",
            Self::EditCall { .. } => "Edit function call",
            Self::EditValueMap { .. } => "Edit value map",
            Self::RemoveNodes { .. } => "Remove nodes",
            Self::AddScope { .. } => "Add child scope",
            Self::RemoveScope { .. } => "Remove child scope",
            Self::SetIteration { .. } => "Change scope iteration",
            Self::SetFilter { .. } => "Change scope filter",
            Self::SetSort { .. } => "Change scope sort",
            Self::SetGrouping { .. } => "Change scope grouping",
            Self::AddFailureRule { .. } => "Add failure rule",
            Self::RemoveFailureRule { .. } => "Remove failure rule",
            Self::MoveFailureRule { .. } => "Move failure rule",
            Self::EditFailureRule { .. } => "Edit failure rule",
        }
    }
}
