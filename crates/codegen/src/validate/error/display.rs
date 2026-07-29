use std::fmt;

use mapping::{FunctionId, NodeId};

use super::super::{RecursiveSequencePathRole, SequenceOwner};
use super::ProgramValidationError;

impl fmt::Display for ProgramValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidSchemaMetadata { boundary, path } => write!(
                formatter,
                "compiled mapping {boundary} schema {} has invalid metadata",
                display_path(path)
            ),
            Self::EmptyExtraSourceName { index } => write!(
                formatter,
                "compiled mapping extra source {} has an empty name",
                index + 1
            ),
            Self::DuplicateExtraSourceName {
                name,
                first,
                duplicate,
            } => write!(
                formatter,
                "compiled mapping extra sources {} and {} share name {name:?}",
                first + 1,
                duplicate + 1
            ),
            Self::DuplicateExpression { node } => {
                write!(
                    formatter,
                    "compiled mapping contains duplicate expression {node}"
                )
            }
            Self::MissingDependency { node, dependency } => write!(
                formatter,
                "compiled mapping expression {node} references missing expression {dependency}"
            ),
            Self::ExpressionCycle { cycle } => write!(
                formatter,
                "compiled mapping expressions contain a cycle: {}",
                display_cycle(cycle)
            ),
            Self::DuplicateUserFunction {
                function,
                first,
                duplicate,
            } => write!(
                formatter,
                "compiled mapping user functions {} and {} share id {}",
                first + 1,
                duplicate + 1,
                function.get()
            ),
            Self::UserFunction { function, error } => {
                write!(formatter, "user function {}: {error}", function.get())
            }
            Self::MissingUserFunctionOutput { function, output } => write!(
                formatter,
                "user function {} output references missing expression {output}",
                function.get()
            ),
            Self::DuplicateUserFunctionParameter {
                function,
                parameter,
            } => write!(
                formatter,
                "user function {} declares parameter {} more than once",
                function.get(),
                parameter.get()
            ),
            Self::FunctionParameterInMain { node, parameter } => write!(
                formatter,
                "compiled mapping expression {node} reads user-function parameter {} outside a function",
                parameter.get()
            ),
            Self::UnknownFunctionParameter {
                function,
                node,
                parameter,
            } => write!(
                formatter,
                "user function {} expression {node} reads undeclared parameter {}",
                function.get(),
                parameter.get()
            ),
            Self::UnsupportedUserFunctionExpression { function, node } => write!(
                formatter,
                "user function {} expression {node} depends on mapping context",
                function.get()
            ),
            Self::MissingUserFunction {
                owner,
                node,
                function,
            } => write!(
                formatter,
                "{} expression {node} calls missing user function {}",
                display_function_owner(*owner),
                function.get()
            ),
            Self::UserFunctionArity {
                owner,
                node,
                function,
                expected,
                actual,
            } => write!(
                formatter,
                "{} expression {node} calls user function {} with {actual} arguments; expected {expected}",
                display_function_owner(*owner),
                function.get()
            ),
            Self::UserFunctionCycle { cycle } => write!(
                formatter,
                "compiled mapping user functions contain a cycle: {}",
                cycle
                    .iter()
                    .map(|function| function.get().to_string())
                    .collect::<Vec<_>>()
                    .join(" -> ")
            ),
            Self::UserFunctionDepth { function, limit } => write!(
                formatter,
                "user function {} exceeds the maximum call depth of {limit}",
                function.get()
            ),
            Self::InvalidAggregateCollection { node, collection } => write!(
                formatter,
                "compiled mapping aggregate expression {node} collection {} matches no source path",
                display_path(collection)
            ),
            Self::InvalidAggregateValuePath {
                node,
                collection,
                value,
            } => write!(
                formatter,
                "compiled mapping aggregate expression {node} value {} is not a scalar under collection {}",
                display_path(value),
                display_path(collection)
            ),
            Self::InvalidCollectionFindCollection { node, collection } => write!(
                formatter,
                "compiled mapping collection-find expression {node} collection {} matches no source path",
                display_path(collection)
            ),
            Self::InvalidLookupCollection { node, collection } => write!(
                formatter,
                "compiled mapping lookup expression {node} collection {} is not a repeating source collection",
                display_path(collection)
            ),
            Self::InvalidLookupKeyPath {
                node,
                collection,
                key,
            } => write!(
                formatter,
                "compiled mapping lookup expression {node} key {} is not a scalar under collection {}",
                display_path(key),
                display_path(collection)
            ),
            Self::InvalidLookupValuePath {
                node,
                collection,
                value,
            } => write!(
                formatter,
                "compiled mapping lookup expression {node} value {} is not a scalar under collection {}",
                display_path(value),
                display_path(collection)
            ),
            Self::InvalidXmlSerializeSource { node, path, schema } => write!(
                formatter,
                "compiled mapping XML serializer expression {node} source {} does not match schema {schema:?}",
                display_path(path)
            ),
            Self::RepeatingXmlSerializeSchema { node, schema } => write!(
                formatter,
                "compiled mapping XML serializer expression {node} schema {schema:?} must describe one document element"
            ),
            Self::EmptyXmlSerializeNamespace { node } => write!(
                formatter,
                "compiled mapping XML serializer expression {node} default namespace cannot be empty"
            ),
            Self::UnsupportedXmlSerializeSchema {
                node,
                schema,
                feature,
            } => write!(
                formatter,
                "compiled mapping XML serializer expression {node} schema {schema:?} uses unsupported {feature}"
            ),
            Self::InvalidXmlMixedContentSource { node, path } => write!(
                formatter,
                "compiled mapping XML mixed-content expression {node} source {} is not a mixed-content group",
                display_path(path)
            ),
            Self::EmptyXmlMixedContentElement { node, replacement } => write!(
                formatter,
                "compiled mapping XML mixed-content expression {node} replacement {} has an empty element name",
                replacement + 1
            ),
            Self::DuplicateXmlMixedContentElement { node, element } => write!(
                formatter,
                "compiled mapping XML mixed-content expression {node} replaces element {element:?} more than once"
            ),
            Self::InvalidXmlMixedContentCollection {
                node,
                replacement,
                collection,
            } => write!(
                formatter,
                "compiled mapping XML mixed-content expression {node} replacement {} collection {} is not repeating",
                replacement + 1,
                display_path(collection)
            ),
            Self::DuplicateJoinOwner { join } => write!(
                formatter,
                "compiled mapping join id {} has more than one owning scope",
                join.get()
            ),
            Self::JoinRequiresRootContext { target_path, join } => write!(
                formatter,
                "target scope {} join {} requires a root source context",
                display_path(target_path),
                join.get()
            ),
            Self::JoinAggregateRequiresRootContext { node, join } => write!(
                formatter,
                "compiled mapping join-aggregate expression {node} for join {} requires a root source context",
                join.get()
            ),
            Self::InvalidJoinSource {
                join,
                collection,
                cardinality,
            } => write!(
                formatter,
                "compiled mapping join {} source {} is not a valid {cardinality:?} source",
                join.get(),
                display_path(collection)
            ),
            Self::InvalidJoinKey {
                join,
                side,
                collection,
                path,
            } => write!(
                formatter,
                "compiled mapping join {} {side} key {} is not a scalar under source {}",
                join.get(),
                display_path(path),
                display_path(collection)
            ),
            Self::InactiveJoinExpression { node, join } => write!(
                formatter,
                "compiled mapping expression {node} references inactive join {}",
                join.get()
            ),
            Self::InvalidJoinFieldCollection {
                node,
                join,
                collection,
            } => write!(
                formatter,
                "compiled mapping join-field expression {node} collection {} does not belong to join {}",
                display_path(collection),
                join.get()
            ),
            Self::InvalidJoinFieldPath {
                node,
                join,
                collection,
                path,
            } => write!(
                formatter,
                "compiled mapping join-field expression {node} path {} is not a scalar under join {} source {}",
                display_path(path),
                join.get(),
                display_path(collection)
            ),
            Self::InvalidSourceIteration {
                target_path,
                source_path,
            } => write!(
                formatter,
                "target scope {} source iteration {} matches no source path",
                display_path(target_path),
                display_path(source_path)
            ),
            Self::MissingDynamicSourcePathExpression { source, expression } => write!(
                formatter,
                "dynamic extra source {source:?} references missing path expression {expression}"
            ),
            Self::InvalidDynamicSourceDriver { source, driver } => write!(
                formatter,
                "dynamic extra source {source:?} driver {} matches no available static source path",
                display_path(driver)
            ),
            Self::DynamicDocumentsRequireRoot { target_path } => write!(
                formatter,
                "target scope {} dynamic document iteration is valid only at a target root",
                display_path(target_path)
            ),
            Self::MissingDynamicTargetPathExpression {
                target_path,
                expression,
            } => write!(
                formatter,
                "target scope {} dynamic target path references missing expression {expression}",
                display_path(target_path)
            ),
            Self::MissingGroupingExpression {
                target_path,
                role,
                expression,
            } => write!(
                formatter,
                "target scope {} grouping {role} references missing expression {expression}",
                display_path(target_path)
            ),
            Self::JoinGroupingUnsupported { target_path, join } => write!(
                formatter,
                "target scope {} join {} cannot use grouping",
                display_path(target_path),
                join.get()
            ),
            Self::InvalidFailureSourceIteration { rule, source_path } => write!(
                formatter,
                "failure rule {rule} source iteration {} matches no repeating source path",
                display_path(source_path)
            ),
            Self::MissingFailurePredicate { rule, expression } => write!(
                formatter,
                "failure rule {rule} selection predicate references missing expression {expression}"
            ),
            Self::MissingFailureMessage { rule, expression } => write!(
                formatter,
                "failure rule {rule} message references missing expression {expression}"
            ),
            Self::MissingTargetScope { target_path } => write!(
                formatter,
                "target scope {} matches no target schema path",
                display_path(target_path)
            ),
            Self::TargetCardinalityMismatch {
                target_path,
                scope_repeating,
                target_repeating,
            } => write!(
                formatter,
                "target scope {} repeating flag {scope_repeating} does not match target schema cardinality {target_repeating}",
                display_path(target_path)
            ),
            Self::ScopeSequenceRequiresGroupTarget { target_path } => write!(
                formatter,
                "target scope {} concatenation requires a group target",
                display_path(target_path)
            ),
            Self::InvalidScopeSequenceWrapper { target_path } => write!(
                formatter,
                "target scope {} concatenation wrapper has unsupported content, controls, or output",
                display_path(target_path)
            ),
            Self::InvalidScopeSequenceSegment {
                target_path,
                segment,
            } => write!(
                formatter,
                "target scope {} concatenation segment {} has a target field or output kind that does not match its wrapper",
                display_path(target_path),
                segment + 1
            ),
            Self::MissingSequenceExpression {
                owner,
                role,
                expression,
            } => write!(
                formatter,
                "{} generated sequence {} references missing expression {expression}",
                display_owner(owner),
                role
            ),
            Self::InvalidSequenceItem { owner, expression } => write!(
                formatter,
                "{} generated sequence item expression {expression} is not an unframed empty-path source field",
                display_owner(owner)
            ),
            Self::InvalidRecursiveSequencePath { owner, role, path } => write!(
                formatter,
                "{} recursive sequence {} path {} does not match its source schema",
                display_owner(owner),
                display_recursive_path_role(*role),
                display_path(path)
            ),
            Self::DuplicateSequenceItem {
                owner,
                first_owner,
                expression,
            } => write!(
                formatter,
                "{} generated sequence item expression {expression} is already owned by {}",
                display_owner(owner),
                display_owner(first_owner)
            ),
            Self::SequenceItemOutOfContext {
                owner,
                expression,
                item,
            } => write!(
                formatter,
                "{} expression {expression} references generated sequence item {item} outside its owning context",
                display_owner(owner)
            ),
            Self::MissingBindingExpression {
                target_path,
                target_field,
                expression,
            } => write!(
                formatter,
                "target scope {} field {target_field:?} references missing expression {expression}",
                display_path(target_path)
            ),
            Self::MissingScalarExpression {
                target_path,
                expression,
            } => write!(
                formatter,
                "target scope {} scalar construction references missing expression {expression}",
                display_path(target_path)
            ),
            Self::ScalarConstructionRequiresScalarTarget { target_path } => write!(
                formatter,
                "target scope {} scalar construction requires a scalar target",
                display_path(target_path)
            ),
            Self::GroupConstructionRequiresGroupTarget { target_path } => write!(
                formatter,
                "target scope {} group construction requires a group target",
                display_path(target_path)
            ),
            Self::XmlMixedContentConstructionRequiresGroupSource { target_path } => write!(
                formatter,
                "target scope {} XML mixed-content construction requires a group source item",
                display_path(target_path)
            ),
            Self::XmlMixedContentConstructionRequiresMixedTarget { target_path } => write!(
                formatter,
                "target scope {} XML mixed-content construction requires a group target with a text field",
                display_path(target_path)
            ),
            Self::EmptyXmlMixedContentConstruction { target_path } => write!(
                formatter,
                "target scope {} XML mixed-content construction requires at least one child mapping",
                display_path(target_path)
            ),
            Self::InvalidXmlMixedContentConstructionElement {
                target_path,
                element,
            } => write!(
                formatter,
                "target scope {} XML mixed-content child mapping {} requires a unique non-empty source name and a non-empty target name",
                display_path(target_path),
                element + 1
            ),
            Self::InvalidXmlMixedContentConstructionTarget {
                target_path,
                element,
                target_field,
            } => write!(
                formatter,
                "target scope {} XML mixed-content child mapping {} target {target_field:?} must be a repeating scalar field",
                display_path(target_path),
                element + 1
            ),
            Self::InvalidRecursiveFilterConstruction { target_path } => write!(
                formatter,
                "target scope {} recursive-filter construction requires distinct non-empty child and item collection names",
                display_path(target_path)
            ),
            Self::RecursiveFilterConstructionRequiresGroupSource { target_path } => write!(
                formatter,
                "target scope {} recursive-filter construction requires a group source item",
                display_path(target_path)
            ),
            Self::RecursiveFilterConstructionRequiresGroupTarget { target_path } => write!(
                formatter,
                "target scope {} recursive-filter construction requires a group target",
                display_path(target_path)
            ),
            Self::RecursiveFilterConstructionRequiresMatchingGroups { target_path } => write!(
                formatter,
                "target scope {} recursive-filter construction requires matching source and target group fields",
                display_path(target_path)
            ),
            Self::InvalidRecursiveFilterChildren { target_path, field } => write!(
                formatter,
                "target scope {} recursive-filter child field {field:?} must be a repeating recursive group",
                display_path(target_path)
            ),
            Self::InvalidRecursiveFilterItems { target_path, field } => write!(
                formatter,
                "target scope {} recursive-filter item field {field:?} must be a repeating group",
                display_path(target_path)
            ),
            Self::MissingRecursiveFilterPredicate {
                target_path,
                expression,
            } => write!(
                formatter,
                "target scope {} recursive-filter predicate references missing expression {expression}",
                display_path(target_path)
            ),
            Self::RecursiveFilterConstructionHasContent { target_path } => write!(
                formatter,
                "target scope {} recursive-filter construction cannot contain bindings or child scopes",
                display_path(target_path)
            ),
            Self::RecursiveFilterConstructionHasControls { target_path } => write!(
                formatter,
                "target scope {} recursive-filter construction cannot use scope controls",
                display_path(target_path)
            ),
            Self::RecursiveFilterConstructionHasInvalidIteration { target_path } => write!(
                formatter,
                "target scope {} recursive-filter construction cannot iterate a generated sequence or inner join",
                display_path(target_path)
            ),
            Self::InvalidPathHierarchyConstruction { target_path } => write!(
                formatter,
                "target scope {} path-hierarchy construction requires a non-empty collection, separator, name, and distinct directory/file fields",
                display_path(target_path)
            ),
            Self::InvalidPathHierarchyCollection {
                target_path,
                collection,
            } => write!(
                formatter,
                "target scope {} path-hierarchy collection {} must be a repeating scalar",
                display_path(target_path),
                display_path(collection)
            ),
            Self::PathHierarchyConstructionRequiresGroupTarget { target_path } => write!(
                formatter,
                "target scope {} path-hierarchy construction requires a group target",
                display_path(target_path)
            ),
            Self::InvalidPathHierarchyName { target_path, field } => write!(
                formatter,
                "target scope {} path-hierarchy name field {field:?} must be a non-repeating scalar",
                display_path(target_path)
            ),
            Self::InvalidPathHierarchyFiles {
                target_path,
                field,
                name,
            } => write!(
                formatter,
                "target scope {} path-hierarchy file field {field:?} must be a repeating group with scalar {name:?}",
                display_path(target_path)
            ),
            Self::InvalidPathHierarchyDirectories { target_path, field } => write!(
                formatter,
                "target scope {} path-hierarchy directory field {field:?} must be a repeating recursive reference to the target group",
                display_path(target_path)
            ),
            Self::PathHierarchyConstructionHasContent { target_path } => write!(
                formatter,
                "target scope {} path-hierarchy construction cannot contain bindings or child scopes",
                display_path(target_path)
            ),
            Self::PathHierarchyConstructionHasIteration { target_path } => write!(
                formatter,
                "target scope {} path-hierarchy construction cannot use scope iteration",
                display_path(target_path)
            ),
            Self::InvalidAdjacencyTreeConstruction { target_path } => write!(
                formatter,
                "target scope {} adjacency-tree construction requires distinct non-empty collection/key/parent paths and target key/child fields",
                display_path(target_path)
            ),
            Self::InvalidAdjacencyTreeCollection {
                target_path,
                collection,
            } => write!(
                formatter,
                "target scope {} adjacency-tree collection {} must be a repeating group",
                display_path(target_path),
                display_path(collection)
            ),
            Self::InvalidAdjacencyTreeField {
                target_path,
                role,
                path,
            } => write!(
                formatter,
                "target scope {} adjacency-tree {role} field {} must be a non-repeating string",
                display_path(target_path),
                display_path(path)
            ),
            Self::AdjacencyTreeConstructionRequiresGroupTarget { target_path } => write!(
                formatter,
                "target scope {} adjacency-tree construction requires a group target",
                display_path(target_path)
            ),
            Self::InvalidAdjacencyTreeTargetKey { target_path, field } => write!(
                formatter,
                "target scope {} adjacency-tree target key {field:?} must be a non-repeating string",
                display_path(target_path)
            ),
            Self::InvalidAdjacencyTreeTargetChildren { target_path, field } => write!(
                formatter,
                "target scope {} adjacency-tree child field {field:?} must be a repeating recursive reference to the target group",
                display_path(target_path)
            ),
            Self::MissingAdjacencyTreeRoot {
                target_path,
                expression,
            } => write!(
                formatter,
                "target scope {} adjacency-tree root references missing expression {expression}",
                display_path(target_path)
            ),
            Self::AdjacencyTreeConstructionHasContent { target_path } => write!(
                formatter,
                "target scope {} adjacency-tree construction cannot contain bindings or child scopes",
                display_path(target_path)
            ),
            Self::AdjacencyTreeConstructionHasIteration { target_path } => write!(
                formatter,
                "target scope {} adjacency-tree construction cannot use scope iteration",
                display_path(target_path)
            ),
            Self::CopyConstructionRequiresGroupSource { target_path } => write!(
                formatter,
                "target scope {} copy-current-source construction requires a group source item",
                display_path(target_path)
            ),
            Self::CopyConstructionRequiresGroupTarget { target_path } => write!(
                formatter,
                "target scope {} copy-current-source construction requires a group target",
                display_path(target_path)
            ),
            Self::CopyConstructionRequiresMatchingGroups { target_path } => write!(
                formatter,
                "target scope {} copy-current-source construction requires matching source and target group fields",
                display_path(target_path)
            ),
            Self::CopyConstructionHasContent { target_path } => write!(
                formatter,
                "target scope {} copy-current-source construction cannot contain bindings or child scopes",
                display_path(target_path)
            ),
            Self::CopyConstructionHasGrouping { target_path } => write!(
                formatter,
                "target scope {} copy-current-source construction cannot use grouping",
                display_path(target_path)
            ),
            Self::ScalarConstructionHasContent { target_path } => write!(
                formatter,
                "target scope {} scalar construction cannot contain bindings or child scopes",
                display_path(target_path)
            ),
            Self::MissingFilterExpression {
                target_path,
                expression,
            } => write!(
                formatter,
                "target scope {} filter references missing expression {expression}",
                display_path(target_path)
            ),
            Self::MissingPostGroupFilterExpression {
                target_path,
                expression,
            } => write!(
                formatter,
                "target scope {} post-group filter references missing expression {expression}",
                display_path(target_path)
            ),
            Self::MissingSortExpression {
                target_path,
                key,
                expression,
            } => write!(
                formatter,
                "target scope {} sort key {} references missing expression {expression}",
                display_path(target_path),
                key + 1
            ),
            Self::MissingWindowExpression {
                target_path,
                window,
                bound,
                expression,
            } => write!(
                formatter,
                "target scope {} sequence window {} bound {} references missing expression {expression}",
                display_path(target_path),
                window + 1,
                bound + 1
            ),
            Self::InvalidIterationOutput {
                target_path,
                output,
            } => write!(
                formatter,
                "target scope {} cannot use {output:?} iteration output with its target cardinality or location",
                display_path(target_path)
            ),
            Self::InvalidDynamicTarget {
                target_path,
                reason,
            } => write!(
                formatter,
                "target scope {} has an invalid computed-property construction: {reason}",
                display_path(target_path)
            ),
            Self::MissingDynamicPropertyExpression {
                target_path,
                property,
                role,
                expression,
            } => write!(
                formatter,
                "target scope {} computed property {} {role} references missing expression {expression}",
                display_path(target_path),
                property + 1
            ),
            Self::DynamicChild {
                target_path,
                child,
                error,
            } => write!(
                formatter,
                "target scope {} computed child {}: {error}",
                display_path(target_path),
                child + 1
            ),
            Self::InvalidDuplicateBinding {
                target_path,
                target_field,
                first_binding,
                duplicate_binding,
            } => write!(
                formatter,
                "target scope {} bindings {first_binding} and {duplicate_binding} conflict for field {target_field:?}",
                display_path(target_path)
            ),
            Self::DuplicateChildTarget {
                target_path,
                target_field,
                first_child,
                duplicate_child,
            } => write!(
                formatter,
                "target scope {} children {first_child} and {duplicate_child} both construct field {target_field:?}",
                display_path(target_path)
            ),
            Self::BindingChildCollision {
                target_path,
                target_field,
                binding,
                child,
            } => write!(
                formatter,
                "target scope {} binding {binding} and child {child} both construct field {target_field:?}",
                display_path(target_path)
            ),
            Self::InvalidBindingTarget {
                target_path,
                target_field,
                binding,
            } => write!(
                formatter,
                "target scope {} binding {binding} does not match scalar field {target_field:?}",
                display_path(target_path)
            ),
            Self::InvalidScalarTargetDomain { target_path } => write!(
                formatter,
                "target scope {} scalar construction domain does not match its schema",
                display_path(target_path)
            ),
            Self::NamedTarget { target, error } => {
                write!(formatter, "named target `{target}`: {error}")
            }
        }
    }
}

impl std::error::Error for ProgramValidationError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::NamedTarget { error, .. }
            | Self::UserFunction { error, .. }
            | Self::DynamicChild { error, .. } => Some(error.as_ref()),
            _ => None,
        }
    }
}

fn display_function_owner(owner: Option<FunctionId>) -> String {
    owner.map_or_else(
        || "compiled mapping".into(),
        |function| format!("user function {}", function.get()),
    )
}

fn display_path(path: &[String]) -> String {
    if path.is_empty() {
        "<root>".into()
    } else {
        format!("`{}`", path.join("/"))
    }
}

fn display_owner(owner: &SequenceOwner) -> String {
    match owner {
        SequenceOwner::Scope(path) => format!("target scope {}", display_path(path)),
        SequenceOwner::NamedTargetScope { target, path } => {
            format!("named target `{target}` scope {}", display_path(path))
        }
        SequenceOwner::FailureRule(rule) => format!("failure rule {rule}"),
        SequenceOwner::DynamicSource(source) => {
            format!("dynamic extra source {source:?}")
        }
        SequenceOwner::Expression(node) => format!("compiled mapping expression {node}"),
    }
}

fn display_recursive_path_role(role: RecursiveSequencePathRole) -> &'static str {
    match role {
        RecursiveSequencePathRole::Collection => "collection",
        RecursiveSequencePathRole::Children => "children",
        RecursiveSequencePathRole::DescentValue => "descent-value",
        RecursiveSequencePathRole::Values => "values",
        RecursiveSequencePathRole::Value => "value",
    }
}

fn display_cycle(cycle: &[NodeId]) -> String {
    cycle
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(" -> ")
}
