use std::fmt;

use mapping::NodeId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SequenceExpressionRole {
    Input(usize),
    Item,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecursiveSequencePathRole {
    Collection,
    Children,
    DescentValue,
    Values,
    Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JoinKeySide {
    Left,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupingExpressionRole {
    Key,
    AdjacentKey,
    StartingPredicate,
    EndingPredicate,
    BlockSize,
}

/// Lexical owner of one private generated-sequence item expression.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum SequenceOwner {
    /// A scope under the primary target.
    Scope(Vec<String>),
    /// A scope under one named target.
    NamedTargetScope {
        target: String,
        path: Vec<String>,
    },
    /// A generated sequence owned by a one-based failure-rule index.
    FailureRule(usize),
    Expression(NodeId),
}

impl fmt::Display for SequenceExpressionRole {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Input(index) => write!(formatter, "input {}", index + 1),
            Self::Item => formatter.write_str("item"),
        }
    }
}

impl fmt::Display for JoinKeySide {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Left => "left",
            Self::Right => "right",
        })
    }
}

impl fmt::Display for GroupingExpressionRole {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Key => "key",
            Self::AdjacentKey => "adjacent key",
            Self::StartingPredicate => "starting predicate",
            Self::EndingPredicate => "ending predicate",
            Self::BlockSize => "block size",
        })
    }
}
