//! Test-only private typed prototype. Public emitters remain gated by #516.
//! Caller-prepared declarations must match every Program source name and original
//! ordinal; acquisition/supplied-input normalization belongs to #527/#519.
//! Five limits are required and cover qualified runtime primitive metadata, not
//! output trees, caller query literals or other externally held metadata.

use crate::{EmitError, literal, mapping};
use ::mapping::{NodeId, SourceOwner, SourceScopeLocation, SourceScopeStep, SourceScopeTarget};
use codegen::{
    Expression, IterationOutput, IterationSource, OwnedSourceCapabilityOrigin as Origin,
    OwnedSourceCapabilityReason as Reason, OwnedSourceDiagnostic as Diagnostic,
    OwnedSourceDiagnosticLocation as Location, OwnedSourceDiagnosticRequest as Request,
    OwnedSourceNamespace as Namespace, Program, ProgramValidationError, TargetConstruction,
    TargetScope,
};
use std::collections::BTreeSet;

pub(crate) fn render_basic_owned_mapping(program: &Program) -> Result<String, EmitError> {
    codegen::validate_program(program)?;
    validate_private_inventory(program)?;
    let mut body = String::new();
    let (primary, extras) = mapping::render_basic_scopes(program, &mut body)?;
    let mut output = String::from(
        "namespace Ferrule.Generated;\n\npublic sealed record NamedOutput(string Name, global::Ferrule.Runtime.FerruleInstance Instance);\npublic sealed record ExecutionOutputs(global::Ferrule.Runtime.FerruleInstance Primary, global::System.Collections.Generic.IReadOnlyList<NamedOutput> Extras);\n\n",
    );
    output.push_str(mapping::basic_selected_output_types());
    output.push_str("public static partial class GeneratedMapping\n{\n    // No implementation in the prototype: C# erases calls outside the private test host.\n    static partial void ObserveOwnedBasicNode(uint node, global::Ferrule.Runtime.FerruleBasicScopeContext context);\n");
    output.push_str("    // Private prototype: exact prepared declarations, no input normalization or policy defaults.\n    private static ExecutionOutputs ExecuteOwnedBasic(\n        global::Ferrule.Runtime.FerruleInstance source,\n        global::System.Collections.Generic.IReadOnlyList<global::Ferrule.Runtime.FerruleDeclaredNamedSourceRoot> declared,\n        global::Ferrule.Runtime.FerruleOwnedSourceLimits limits,\n        global::Ferrule.Runtime.FerruleExecutionContext? execution = null)\n    {\n        global::System.ArgumentNullException.ThrowIfNull(source);\n        var roots = global::Ferrule.Runtime.FerruleBasicSourceRoots.Create(source, declared, limits);\n        var context = roots.RootContext(execution);\n");
    output.push_str(&format!(
        "        var primary = Scope_{primary}(context);\n"
    ));
    for (index, (_, scope)) in extras.iter().enumerate() {
        output.push_str(&format!(
            "        var extra_{index} = Scope_{scope}(context);\n"
        ));
    }
    output.push_str(
        "        var result = new ExecutionOutputs(primary, new NamedOutput[]\n        {\n",
    );
    for (index, (name, _)) in extras.iter().enumerate() {
        output.push_str(&format!(
            "            new({}, extra_{index}),\n",
            literal::string(name)
        ));
    }
    output.push_str(
        "        });\n        global::System.GC.KeepAlive(roots);\n        return result;\n    }\n",
    );
    output.push_str("\n    private static SelectedTargetOutput ExecuteSelectedOwnedBasic(\n        global::Ferrule.Runtime.FerruleInstance source,\n        global::Ferrule.Runtime.FerruleTargetSelection selection,\n        global::System.Collections.Generic.IReadOnlyList<global::Ferrule.Runtime.FerruleDeclaredNamedSourceRoot> declared,\n        global::Ferrule.Runtime.FerruleOwnedSourceLimits limits,\n        global::Ferrule.Runtime.FerruleExecutionContext? execution = null)\n    {\n        global::System.ArgumentNullException.ThrowIfNull(source);\n        global::System.ArgumentNullException.ThrowIfNull(selection);\n        var selected = ResolveOwnedBasicTarget(selection);\n        var roots = global::Ferrule.Runtime.FerruleBasicSourceRoots.Create(source, declared, limits);\n        var context = roots.RootContext(execution);\n        SelectedTargetOutput result;\n        switch (selected)\n        {\n");
    output.push_str(&format!("            case 0: result = new SelectedTargetOutput.Primary(Scope_{primary}(context)); break;\n"));
    for (index, (name, scope)) in extras.iter().enumerate() {
        output.push_str(&format!("            case {}: result = new SelectedTargetOutput.Named(new NamedOutput({}, Scope_{scope}(context))); break;\n", index + 1, literal::string(name)));
    }
    output.push_str("            default: throw new global::System.InvalidOperationException(\"Resolved target is invalid.\");\n        }\n        global::System.GC.KeepAlive(roots);\n        return result;\n    }\n\n    private static int ResolveOwnedBasicTarget(global::Ferrule.Runtime.FerruleTargetSelection selection)\n    {\n        if (selection.IsPrimary) return 0;\n        var name = selection.Name!;\n");
    for (index, (name, _)) in extras.iter().enumerate() {
        output.push_str(&format!("        if (global::System.String.Equals(name, {}, global::System.StringComparison.Ordinal)) return {};\n", literal::string(name), index + 1));
    }
    output.push_str("        throw new global::Ferrule.Runtime.FerruleRuntimeException(\n            global::Ferrule.Runtime.FerruleRuntimeError.UnknownTarget,\n            $\"project has no named target `{name}`\",\n            detail: name);\n    }\n");
    let mut nodes = program.expressions.iter().collect::<Vec<_>>();
    nodes.sort_by_key(|node| node.id);
    for node in nodes {
        mapping::render_basic_expression(node.id, &node.expression, &mut output)?;
    }
    output.push_str(&body);
    output.push_str("}\n");
    Ok(output)
}

fn capability(
    location: Location,
    request: Option<Request>,
    reason: Reason,
    origin: Origin,
) -> EmitError {
    ProgramValidationError::OwnedSource {
        diagnostic: Box::new(Diagnostic::Capability {
            location,
            request,
            reason,
            origin,
        }),
    }
    .into()
}

fn expression_refusal(node: NodeId, expression: &Expression, reason: Reason) -> EmitError {
    let request = match expression {
        Expression::OwnedSourceField(field) => Some(Request::Field {
            owner: (&field.source.owner).into(),
            path: field.source.path.clone(),
            frame: field.frame.clone(),
        }),
        Expression::OwnedPosition(position) => Some(Request::Position {
            owner: (&position.owner).into(),
            collection: position.collection.clone(),
        }),
        _ => None,
    };
    capability(
        Location::Expression {
            namespace: Namespace::Main,
            node,
            scope: None,
        },
        request,
        reason,
        Origin::RetainedExpression {
            namespace: Namespace::Main,
            node,
        },
    )
}

fn scope_error(scope: &TargetScope, route: SourceScopeLocation, reason: Reason) -> EmitError {
    let request = scope
        .iteration
        .as_ref()
        .and_then(|iteration| iteration.owned_source_iteration())
        .map(|source| Request::Iteration {
            owner: (&source.source().owner).into(),
            path: source.source().path.clone(),
        });
    capability(
        Location::Iteration {
            scope: route.clone(),
        },
        request,
        reason,
        Origin::Scope { scope: route },
    )
}

fn check_scope(
    program: &Program,
    scope: &TargetScope,
    route: SourceScopeLocation,
) -> Result<(), EmitError> {
    if let Some(iteration) = &scope.iteration {
        let reason = match iteration.input() {
            IterationSource::OwnedSource(_) | IterationSource::Source(_) => None,
            IterationSource::Generated(_) => Some(Reason::GeneratedItem),
            IterationSource::InnerJoin(_) => Some(Reason::Join),
            IterationSource::Concatenate(_) => Some(Reason::Concatenate),
            IterationSource::DynamicDocuments(_) => Some(Reason::Document),
        };
        if let Some(reason) = reason {
            return Err(scope_error(scope, route, reason));
        }
        if iteration.filter().is_some()
            || iteration.sort().is_some()
            || iteration.grouping().is_some()
            || iteration.post_group_filter().is_some()
            || !iteration.windows().is_empty()
            || iteration.output() != IterationOutput::Repeated
        {
            return Err(scope_error(scope, route, Reason::ScopeControl));
        }
    }
    if !matches!(
        scope.construction,
        TargetConstruction::Group | TargetConstruction::Scalar { .. }
    ) {
        let reason = match scope.construction {
            TargetConstruction::XmlMixedContent { .. } => Reason::XmlOperator,
            TargetConstruction::DynamicGroup { .. } => Reason::DynamicTarget,
            TargetConstruction::RecursiveFilter { .. }
            | TargetConstruction::PathHierarchy { .. }
            | TargetConstruction::AdjacencyTree { .. } => Reason::TreeOperator,
            _ => Reason::SpecialConstruction,
        };
        return Err(scope_error(scope, route, reason));
    }
    if let Some(iteration) = &scope.iteration
        && let IterationSource::Source(source) = iteration.input()
        && dynamic_legacy_path(program, source.path())
    {
        return Err(scope_error(scope, route, Reason::DynamicLoader));
    }
    for (index, child) in scope.children.iter().enumerate() {
        check_scope(
            program,
            child,
            route.clone().descendant(SourceScopeStep::Child(index)),
        )?;
    }
    Ok(())
}

// A legacy frame replaces its path, including an explicitly empty frame.
// Metadata matching is exact and does not infer runtime source ownership.
fn dynamic_legacy_path(program: &Program, path: &[String]) -> bool {
    path.first().is_some_and(|name| {
        program
            .extra_sources
            .iter()
            .any(|source| source.name == *name && source.dynamic.is_some())
    })
}

// Only potential represented target roots retain a legacy leaf. Both If arms
// and parameter defaults remain potential demands; unused declarations and
// dead legacy leaves do not activate a dynamic source. Earlier private checks
// close functions, failures and specialized scopes; the following graph
// inventory continues to refuse unsupported operators.
fn retained_expressions(program: &Program) -> BTreeSet<NodeId> {
    fn roots(scope: &TargetScope, pending: &mut Vec<NodeId>) {
        if let TargetConstruction::Scalar { expression, .. } = &scope.construction {
            pending.push(*expression);
        }
        pending.extend(scope.bindings.iter().map(|binding| binding.expression));
        for child in &scope.children {
            roots(child, pending);
        }
    }
    let mut pending = Vec::new();
    roots(&program.root, &mut pending);
    for target in &program.extra_targets {
        roots(&target.root, &mut pending);
    }
    let mut retained = BTreeSet::new();
    while let Some(id) = pending.pop() {
        if !retained.insert(id) {
            continue;
        }
        let Some(node) = program.expressions.iter().find(|node| node.id == id) else {
            continue;
        };
        match &node.expression {
            Expression::Call { args, .. } => pending.extend(args.iter().copied()),
            Expression::If {
                condition,
                then,
                else_,
            } => pending.extend([*condition, *then, *else_]),
            Expression::RuntimeParameterDefault { default, .. } => pending.push(*default),
            Expression::Raise { message } => pending.extend(message.iter().copied()),
            _ => {}
        }
    }
    retained
}

fn validate_private_inventory(program: &Program) -> Result<(), EmitError> {
    // General schema/structure and existing validation precedence already ran.
    check_scope(
        program,
        &program.root,
        SourceScopeLocation::root(SourceScopeTarget::Primary),
    )?;
    for (index, target) in program.extra_targets.iter().enumerate() {
        check_scope(
            program,
            &target.root,
            SourceScopeLocation::root(SourceScopeTarget::Named {
                index,
                name: target.name.clone(),
            }),
        )?;
    }
    if let Some(function) = program.user_functions.first() {
        return Err(capability(
            Location::Expression {
                namespace: Namespace::Function(function.id),
                node: function.output,
                scope: None,
            },
            None,
            Reason::IsolatedFunction,
            Origin::FunctionBody {
                function: function.id,
            },
        ));
    }
    if !program.failure_rules.is_empty() {
        // Global failure admission is a boundary of this private primary entry.
        return Err(capability(
            Location::Iteration {
                scope: SourceScopeLocation::root(SourceScopeTarget::Primary),
            },
            None,
            Reason::FailureRule,
            Origin::Failure { index: 0 },
        ));
    }
    let retained = retained_expressions(program);
    for node in &program.expressions {
        if let Expression::SourceField { path, frame } = &node.expression
            && retained.contains(&node.id)
            && dynamic_legacy_path(program, frame.as_ref().unwrap_or(path))
        {
            return Err(codegen::owned_expression_emission_error(
                Namespace::Main,
                node.id,
                &node.expression,
            )
            .into());
        }
        let reason = match &node.expression {
            Expression::OwnedSourceField(_)
            | Expression::OwnedPosition(_)
            | Expression::SourceField { .. }
            | Expression::Position { .. }
            | Expression::Const { .. }
            | Expression::Call { .. }
            | Expression::If { .. }
            | Expression::RuntimeValue { .. }
            | Expression::RuntimeParameter { .. }
            | Expression::RuntimeParameterDefault { .. }
            | Expression::Raise { .. } => continue,
            Expression::SourceRootXmlTypeEquals { .. }
            | Expression::XmlSerialize { .. }
            | Expression::XmlMixedContent { .. } => Reason::XmlOperator,
            Expression::SourceDocumentPath => Reason::Document,
            Expression::DynamicSourceField { .. } => Reason::DynamicLoader,
            Expression::JoinField { .. } | Expression::JoinPosition { .. } => Reason::Join,
            Expression::Lookup { .. }
            | Expression::CollectionFind { .. }
            | Expression::Aggregate { .. }
            | Expression::JoinAggregate { .. } => Reason::AggregateLookup,
            Expression::SequenceExists { .. }
            | Expression::SequenceItemAt { .. }
            | Expression::SequenceAggregate { .. } => Reason::GeneratedItem,
            Expression::UserFunctionCall { .. } | Expression::FunctionParameter { .. } => {
                Reason::IsolatedFunction
            }
            _ => Reason::UnsupportedBasicContext,
        };
        return Err(expression_refusal(node.id, &node.expression, reason));
    }
    Ok(())
}

pub(super) fn scope_refusal(program: &Program, scope_index: usize) -> EmitError {
    fn visit<'a>(
        scope: &'a TargetScope,
        route: SourceScopeLocation,
        all: &mut Vec<(&'a TargetScope, SourceScopeLocation)>,
    ) {
        all.push((scope, route.clone()));
        for (index, child) in scope.children.iter().enumerate() {
            visit(
                child,
                route.clone().descendant(SourceScopeStep::Child(index)),
                all,
            );
        }
    }
    let mut all = Vec::new();
    visit(
        &program.root,
        SourceScopeLocation::root(SourceScopeTarget::Primary),
        &mut all,
    );
    for (index, target) in program.extra_targets.iter().enumerate() {
        visit(
            &target.root,
            SourceScopeLocation::root(SourceScopeTarget::Named {
                index,
                name: target.name.clone(),
            }),
            &mut all,
        );
    }
    let (scope, route) = &all[scope_index];
    scope_error(scope, route.clone(), Reason::UnsupportedBasicContext)
}

pub(super) fn render_owner(owner: &SourceOwner, output: &mut String) {
    match owner {
        SourceOwner::Primary => {
            output.push_str("global::Ferrule.Runtime.FerruleOwnedSourceOwner.Primary")
        }
        SourceOwner::Named { name } => output.push_str(&format!(
            "global::Ferrule.Runtime.FerruleOwnedSourceOwner.Named({})",
            literal::string(name)
        )),
    }
}

pub(super) fn render_owned_leaf(
    _node: NodeId,
    expression: &Expression,
    output: &mut String,
) -> Result<(), EmitError> {
    match expression {
        Expression::OwnedSourceField(field) => {
            output.push_str(" =>\n        context.ResolveOwnedField(");
            render_owner(&field.source.owner, output);
            output.push_str(", ");
            mapping::basic_path(&field.source.path, output);
            output.push_str(", ");
            mapping::render_optional_path(field.frame.as_deref(), output);
            output.push_str(");\n");
        }
        Expression::OwnedPosition(position) => {
            // This private entry accepts no contexts or phase indices. All owned
            // frames come from int-sized actual walks; no arbitrary index can enter.
            output.push_str(" =>\n        global::Ferrule.Runtime.FerruleValue.FromInt64(checked((long)context.OwnedPosition(");
            render_owner(&position.owner, output);
            output.push_str(", ");
            mapping::render_optional_path(position.collection.as_deref(), output);
            output.push_str(")));\n");
        }
        _ => unreachable!("shared expression mode dispatches only two owned leaves"),
    }
    Ok(())
}
