//! The JSON schema carried by version 1 buffers.

use ferrule_host::{
    BoundaryId, BoundaryShape, DisplayText, Inspection, NativeLocation, ResourceKind,
    ValidationEndpoint, ValidationOwner, ValidationReport, ValidationSchemaStep,
    ValidationScopeStep,
};
use serde_json::{Value, json};

use crate::Failure;

pub(super) fn inspection(report: Inspection) -> Result<Value, Failure> {
    let boundaries = report
        .boundaries
        .into_iter()
        .map(|boundary| {
            let shape = match boundary.shape {
                BoundaryShape::Scalar => "scalar",
                BoundaryShape::ScalarUnion => "scalarUnion",
                BoundaryShape::Object => "object",
            };
            Ok(json!({
                "id": boundary_id(boundary.id),
                "name": text(boundary.name),
                "rootName": text(boundary.root_name),
                "shape": shape,
                "repeating": boundary.repeating,
                "nativeLocation": location(boundary.native_location)?,
            }))
        })
        .collect::<Result<Vec<_>, Failure>>()?;
    let resources = report
        .unresolved_resources
        .into_iter()
        .map(|resource| {
            Ok(json!({
                "boundaryId": boundary_id(resource.boundary_id),
                "kind": resource_kind(resource.kind)?,
                "nativeLocation": location(resource.native_location)?,
            }))
        })
        .collect::<Result<Vec<_>, Failure>>()?;
    Ok(json!({
        "boundaries": boundaries,
        "counts": {
            "schemaNodes": report.counts.schema_nodes, "graphNodes": report.counts.graph_nodes,
            "scopeNodes": report.counts.scope_nodes, "userFunctions": report.counts.user_functions,
            "failureRules": report.counts.failure_rules, "totalItems": report.counts.total_items,
        },
        "unresolvedResources": resources,
        "truncated": report.truncated,
    }))
}

pub(super) fn validation(report: ValidationReport) -> Result<Value, Failure> {
    let diagnostics = report
        .diagnostics
        .into_iter()
        .map(|diagnostic| {
            Ok(json!({
                "message": text(diagnostic.message),
                "location": text(diagnostic.location),
                "owner": diagnostic.owner.map(owner),
                "nativeLocation": location(diagnostic.native_location)?,
            }))
        })
        .collect::<Result<Vec<_>, Failure>>()?;
    Ok(json!({
        "isValid": report.is_valid,
        "totalIssueCount": report.total_issue_count,
        "diagnostics": diagnostics,
        "truncated": report.truncated,
    }))
}

fn text(value: DisplayText) -> Value {
    json!({ "text": value.text, "truncated": value.truncated })
}

fn boundary_id(id: BoundaryId) -> Value {
    match id {
        BoundaryId::Source => json!({ "kind": "source" }),
        BoundaryId::Target => json!({ "kind": "target" }),
        BoundaryId::NamedSource(index) => {
            json!({ "kind": "namedSource", "index": index.to_string() })
        }
        BoundaryId::NamedTarget(index) => {
            json!({ "kind": "namedTarget", "index": index.to_string() })
        }
    }
}

fn resource_kind(kind: ResourceKind) -> Result<&'static str, Failure> {
    Ok(match kind {
        ResourceKind::DesignTimeInput => "designTimeInput",
        ResourceKind::DesignTimeOutput => "designTimeOutput",
        ResourceKind::DynamicInput => "dynamicInput",
        ResourceKind::DynamicOutput => "dynamicOutput",
        ResourceKind::EdiConfiguration => "ediConfiguration",
        ResourceKind::ExternalSource => "externalSource",
        ResourceKind::XmlSchemaHint => "xmlSchemaHint",
        ResourceKind::SchemaReference => "schemaReference",
        _ => return Err(Failure::internal()),
    })
}

fn location(location: NativeLocation) -> Result<Value, Failure> {
    Ok(match location {
        NativeLocation::Document => json!({ "kind": "document" }),
        NativeLocation::JsonPointer(pointer) => {
            json!({ "kind": "jsonPointer", "pointer": pointer })
        }
        NativeLocation::GraphNode { function, node } => json!({
            "kind": "graphNode", "nodeId": node, "functionId": function.map(|id| id.get().to_string()),
        }),
        _ => return Err(Failure::internal()),
    })
}

fn owner(owner: ValidationOwner) -> Value {
    match owner {
        ValidationOwner::GraphNode { function, node } => json!({
            "kind": "graphNode", "nodeId": node, "functionId": function.map(|id| id.get().to_string()),
        }),
        ValidationOwner::UserFunction(function) => json!({
            "kind": "userFunction", "functionId": function.get().to_string(),
        }),
        ValidationOwner::Endpoint(endpoint) => {
            json!({ "kind": "endpoint", "endpoint": endpoint_id(endpoint) })
        }
        ValidationOwner::SchemaNode(schema) => json!({
            "kind": "schemaNode", "endpoint": endpoint_id(schema.endpoint),
            "path": schema.path.into_iter().map(schema_step).collect::<Vec<_>>(),
        }),
        ValidationOwner::Scope(scope) => json!({
            "kind": "scope", "target": endpoint_id(scope.target),
            "path": scope.path.into_iter().map(scope_step).collect::<Vec<_>>(),
        }),
        ValidationOwner::FailureRule { index } => {
            json!({ "kind": "failureRule", "index": index.to_string() })
        }
    }
}

fn endpoint_id(endpoint: ValidationEndpoint) -> Value {
    match endpoint {
        ValidationEndpoint::Source => json!({ "kind": "source" }),
        ValidationEndpoint::Target => json!({ "kind": "target" }),
        ValidationEndpoint::NamedSource { index, name } => {
            json!({ "kind": "namedSource", "index": index.to_string(), "name": name })
        }
        ValidationEndpoint::NamedTarget { index, name } => {
            json!({ "kind": "namedTarget", "index": index.to_string(), "name": name })
        }
    }
}

fn schema_step(step: ValidationSchemaStep) -> Value {
    match step {
        ValidationSchemaStep::Child(index) => {
            json!({ "kind": "child", "index": index.to_string() })
        }
        ValidationSchemaStep::DynamicField => json!({ "kind": "dynamicField" }),
        ValidationSchemaStep::Contains(index) => {
            json!({ "kind": "contains", "index": index.to_string() })
        }
        ValidationSchemaStep::DependentSchema(index) => {
            json!({ "kind": "dependentSchema", "index": index.to_string() })
        }
    }
}

fn scope_step(step: ValidationScopeStep) -> Value {
    let (kind, index) = match step {
        ValidationScopeStep::Child(index) => ("child", index),
        ValidationScopeStep::DynamicChild(index) => ("dynamicChild", index),
        ValidationScopeStep::Segment(index) => ("segment", index),
    };
    json!({ "kind": kind, "index": index.to_string() })
}
