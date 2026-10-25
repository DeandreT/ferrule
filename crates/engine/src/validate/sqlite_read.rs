//! Placement and runtime-parameter checks for declared SQLite query sources.

use ir::{ScalarType, SchemaNode};
use mapping::{FormatOptions, Node, Project, validate_sqlite_read_profile};

use super::{ValidationEndpoint, ValidationIssue, ValidationOwner};

pub(super) fn validate_sqlite_read(project: &Project, issues: &mut Vec<ValidationIssue>) {
    let target_only = "`sqlite_read` is valid only for primary and static named mapping sources";
    if project.target_options.sqlite_read.is_some() {
        issues.push(
            ValidationIssue::new("target format options", target_only)
                .with_owner(ValidationOwner::Endpoint(ValidationEndpoint::Target)),
        );
    }
    for (index, target) in project.extra_targets.iter().enumerate() {
        if target.options.sqlite_read.is_some() {
            issues.push(
                ValidationIssue::new(
                    format!("extra target `{}` format options", target.name),
                    target_only,
                )
                .with_owner(ValidationOwner::Endpoint(
                    ValidationEndpoint::NamedTarget {
                        index,
                        name: target.name.clone(),
                    },
                )),
            );
        }
    }
    validate_source(
        project,
        "source format options",
        &project.source,
        &project.source_options,
        false,
        ValidationEndpoint::Source,
        issues,
    );
    for (index, source) in project.extra_sources.iter().enumerate() {
        validate_source(
            project,
            &format!("extra source `{}` format options", source.name),
            &source.schema,
            &source.options,
            source.dynamic_path.is_some(),
            ValidationEndpoint::NamedSource {
                index,
                name: source.name.clone(),
            },
            issues,
        );
    }
}

fn validate_source(
    project: &Project,
    location: &str,
    schema: &SchemaNode,
    options: &FormatOptions,
    dynamic: bool,
    endpoint: ValidationEndpoint,
    issues: &mut Vec<ValidationIssue>,
) {
    let Some(profile) = &options.sqlite_read else {
        return;
    };
    let mut push = |message: String| {
        issues.push(
            ValidationIssue::new(location, message)
                .with_owner(ValidationOwner::Endpoint(endpoint.clone())),
        );
    };
    if dynamic {
        push("per-item dynamic sources cannot use `sqlite_read`".into());
    }
    let exact = FormatOptions {
        sqlite_read: Some(profile.clone()),
        ..FormatOptions::default()
    };
    if *options != exact {
        push("`sqlite_read` cannot be combined with another format's options".into());
    }
    let plan = match validate_sqlite_read_profile(schema, profile) {
        Ok(plan) => plan,
        Err(error) => return push(error.to_string()),
    };
    for parameter in plan.parameters() {
        if let Some(problem) =
            runtime_declaration_problem(project, &parameter.runtime_parameter, parameter.ty)
        {
            push(format!(
                "query parameter `{}` binds runtime parameter `{}`, which {problem}",
                parameter.name, parameter.runtime_parameter
            ));
        }
    }
}

/// A query binding needs a required runtime parameter declared, anywhere in
/// the project graph or its functions, with exactly the binding's type.
/// Reachability does not matter: the declaration is a host input contract.
fn runtime_declaration_problem(project: &Project, name: &str, ty: ScalarType) -> Option<String> {
    let mut declared = false;
    let nodes = project.graph.nodes.values().chain(
        project
            .user_functions
            .values()
            .flat_map(|function| function.body.nodes.values()),
    );
    for node in nodes {
        match node {
            Node::RuntimeParameter {
                name: declared_name,
                ty: declared_ty,
                ..
            } if declared_name == name => {
                if *declared_ty != ty {
                    return Some(format!("is declared as {declared_ty:?}, not {ty:?}"));
                }
                declared = true;
            }
            Node::RuntimeParameterDefault {
                name: declared_name,
                ..
            } if declared_name == name => {
                return Some("has a default; query bindings require a required parameter".into());
            }
            _ => {}
        }
    }
    (!declared).then(|| "is not declared by any runtime parameter node".into())
}
