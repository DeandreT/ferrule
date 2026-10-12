//! Private test prototype for the paired, control-free Basic source context.
//! Public emitters keep their ordinary closed gate. The prepared declaration
//! slice is trusted complete program metadata, not an arbitrary-input API.

use super::*;
use codegen::{
    OwnedSourceCapabilityOrigin as Origin, OwnedSourceCapabilityReason as Capability,
    OwnedSourceDiagnostic as Diagnostic, OwnedSourceDiagnosticLocation as Location,
    OwnedSourceDiagnosticOwner as Owner, OwnedSourceDiagnosticRequest as Request,
};
use mapping::{SourceOwner, SourceScopeLocation, SourceScopeStep, SourceScopeTarget};

pub(crate) fn render_basic_owned_source(program: &Program) -> Result<String, EmitError> {
    validate_program(program)?;
    check_inventory(program)?;
    let mut scopes = Vec::new();
    collect_scopes(&program.root, "scope_root".into(), &mut scopes);
    for (index, target) in program.extra_targets.iter().enumerate() {
        collect_scopes(&target.root, format!("scope_extra_{index}"), &mut scopes);
    }
    let retained = expression_usage(program, &scopes);
    let functions = BTreeMap::new();
    let mut source = String::from(
        "// Private Basic prototype. Public emission remains closed.\n\
         #[allow(unused_imports)]\n\
         use codegen_runtime::{BasicScopeContext, BasicSourceRoots, DeclaredNamedSourceRoot,\n\
             ExecutionContext, Instance, OwnedSourceLimits, RuntimeError, RuntimeValue,\n\
             ScalarType, SourceOwnerRef, TargetSelection, Value, adapt_target_value,\n\
             adapt_union_target_value, call, field, group, repeated, require_bool, scalar, value_map};\n\n",
    );
    source.push_str(&render_entries(program));
    for node in &program.expressions {
        if retained.contains(&node.id) {
            source.push_str(&render_expression_in_context(
                node.id,
                &node.expression,
                "expression_",
                None,
                &functions,
                ExpressionEmission {
                    filter_map: false,
                    owner_function: None,
                    basic: true,
                },
            )?);
        }
    }
    if !program.failure_rules.is_empty() {
        source.push_str(&failure::render_basic(program)?);
    }
    for (name, scope, children, _) in scopes {
        source.push_str(&render_basic_scope(&name, scope, &children));
    }
    Ok(source)
}

fn scope_refusal(
    route: SourceScopeLocation,
    reason: Capability,
    request: Option<Request>,
) -> EmitError {
    ProgramValidationError::OwnedSource {
        diagnostic: Box::new(Diagnostic::Capability {
            location: Location::Iteration {
                scope: route.clone(),
            },
            request,
            reason,
            origin: Origin::Scope { scope: route },
        }),
    }
    .into()
}

fn iteration_request(scope: &TargetScope) -> Option<Request> {
    scope
        .iteration
        .as_ref()?
        .owned_source_iteration()
        .map(|source| Request::Iteration {
            owner: Owner::from(&source.source().owner),
            path: source.source().path.clone(),
        })
}

fn check_scope(
    program: &Program,
    scope: &TargetScope,
    route: SourceScopeLocation,
) -> Result<(), EmitError> {
    if !matches!(
        &scope.construction,
        TargetConstruction::Group | TargetConstruction::Scalar { .. }
    ) {
        return Err(scope_refusal(
            route,
            Capability::SpecialConstruction,
            iteration_request(scope),
        ));
    }
    if let Some(iteration) = &scope.iteration {
        if iteration.filter().is_some()
            || iteration.sort().is_some()
            || iteration.grouping().is_some()
            || iteration.post_group_filter().is_some()
            || !iteration.windows().is_empty()
            || iteration.output() != IterationOutput::Repeated
        {
            return Err(scope_refusal(
                route,
                Capability::ScopeControl,
                iteration_request(scope),
            ));
        }
        match iteration.input() {
            IterationSource::Source(source) => {
                if let Some(name) = source.path().first()
                    && program
                        .extra_sources
                        .iter()
                        .any(|s| s.name == *name && s.dynamic.is_some())
                {
                    return Err(scope_refusal(route, Capability::DynamicLoader, None));
                }
            }
            IterationSource::OwnedSource(_) => {}
            IterationSource::Generated(_) => {
                return Err(scope_refusal(route, Capability::GeneratedItem, None));
            }
            IterationSource::InnerJoin(_) => {
                return Err(scope_refusal(route, Capability::Join, None));
            }
            IterationSource::Concatenate(_) => {
                return Err(scope_refusal(route, Capability::Concatenate, None));
            }
            IterationSource::DynamicDocuments(_) => {
                return Err(scope_refusal(route, Capability::Document, None));
            }
        }
    }
    for (index, child) in scope.children.iter().enumerate() {
        check_scope(
            program,
            child,
            route.descendant(SourceScopeStep::Child(index)),
        )?;
    }
    Ok(())
}

fn check_inventory(program: &Program) -> Result<(), EmitError> {
    let primary = SourceScopeLocation::root(SourceScopeTarget::Primary);
    check_scope(program, &program.root, primary.clone())?;
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
    // Ordinary source-backed rules share only the legacy view of the paired
    // context. Common validation keeps owned rule expressions closed; generated
    // and dynamic failure sources remain unsupported in this private bridge.
    for (index, rule) in program.failure_rules.iter().enumerate() {
        match &rule.iteration {
            codegen::FailureIteration::Generated(_) => {
                return Err(failure_refusal(index, Capability::FailureRule));
            }
            codegen::FailureIteration::Source(source) => {
                if source.path().first().is_some_and(|name| {
                    program
                        .extra_sources
                        .iter()
                        .any(|source| source.name == *name && source.dynamic.is_some())
                }) {
                    return Err(failure_refusal(index, Capability::DynamicLoader));
                }
            }
        }
    }
    let mut scopes = Vec::new();
    collect_scopes(&program.root, "scope_root".into(), &mut scopes);
    for (index, target) in program.extra_targets.iter().enumerate() {
        collect_scopes(&target.root, format!("scope_extra_{index}"), &mut scopes);
    }
    let retained = expression_usage(program, &scopes);
    for node in &program.expressions {
        if !retained.contains(&node.id) {
            continue;
        }
        if let Expression::UserFunctionCall { function, .. } = &node.expression {
            let body = program
                .user_functions
                .iter()
                .find(|body| body.id == *function)
                .expect("validated function reference");
            return Err(ProgramValidationError::OwnedSource {
                diagnostic: Box::new(Diagnostic::Capability {
                    location: Location::Expression {
                        namespace: codegen::OwnedSourceNamespace::Function(*function),
                        node: body.output,
                        scope: None,
                    },
                    request: None,
                    reason: Capability::IsolatedFunction,
                    origin: Origin::FunctionBody {
                        function: *function,
                    },
                }),
            }
            .into());
        }
        let supported = matches!(
            &node.expression,
            Expression::OwnedSourceField(_)
                | Expression::OwnedPosition(_)
                | Expression::SourceField { .. }
                | Expression::Position { .. }
                | Expression::Const { .. }
                | Expression::RuntimeValue { .. }
                | Expression::RuntimeParameter { .. }
                | Expression::RuntimeParameterDefault { .. }
                | Expression::Call { .. }
                | Expression::If { .. }
                | Expression::Raise { .. }
                | Expression::ValueMap { .. }
        );
        let dynamic_legacy = matches!(&node.expression,
            Expression::SourceField { path, frame } if frame.as_ref().unwrap_or(path).first()
                .is_some_and(|name| program.extra_sources.iter().any(|s| s.name == *name && s.dynamic.is_some())));
        if !supported || dynamic_legacy {
            return Err(codegen::owned_expression_emission_error(
                codegen::OwnedSourceNamespace::Main,
                node.id,
                &node.expression,
            )
            .into());
        }
    }
    Ok(())
}

// Failure routes have their own exact rule index; they are not target scopes.
// Reuse the existing diagnostic family and retain its primary iteration location.
pub(super) fn failure_refusal(index: usize, reason: Capability) -> EmitError {
    ProgramValidationError::OwnedSource {
        diagnostic: Box::new(Diagnostic::Capability {
            location: Location::Iteration {
                scope: SourceScopeLocation::root(SourceScopeTarget::Primary),
            },
            request: None,
            reason,
            origin: Origin::Failure { index },
        }),
    }
    .into()
}

fn owner_literal(owner: &SourceOwner) -> String {
    match owner {
        SourceOwner::Primary => "SourceOwnerRef::Primary".into(),
        SourceOwner::Named { name } => format!("SourceOwnerRef::Named({})", rust_string(name)),
    }
}

pub(super) fn render_owned_access(expression: &Expression) -> Option<String> {
    match expression {
        Expression::OwnedSourceField(field) => {
            let owner = owner_literal(&field.source.owner);
            let path = render_string_path(&field.source.path);
            let frame = field.frame.as_ref().map_or_else(
                || "None".into(),
                |frame| format!("Some(&[{}][..])", render_string_path(frame)),
            );
            Some(format!("context.owned_field({owner}, &[{path}], {frame})"))
        }
        Expression::OwnedPosition(position) => {
            let owner = owner_literal(&position.owner);
            let collection = position.collection.as_ref().map_or_else(
                || "None".into(),
                |path| format!("Some(&[{}][..])", render_string_path(path)),
            );
            // Generated roots and actual Vec-backed walks are the sole context
            // constructors. Their 1-based positions fit i64 on 32/64-bit targets.
            Some(format!(
                "{{ let position = context.owned_position({owner}, {collection})?; Ok(Value::Int(position as i64)) }}"
            ))
        }
        _ => None,
    }
}

fn render_basic_scope(name: &str, scope: &TargetScope, children: &[String]) -> String {
    let mut source = format!(
        "fn {name}(context: &BasicScopeContext<'_>) -> Result<Instance, RuntimeError> {{\n"
    );
    if let Some(iteration) = &scope.iteration {
        let walk = match iteration.input() {
            IterationSource::Source(input) => format!(
                "context.walk_source_cohort(&[{}])?",
                render_string_path(input.path())
            ),
            IterationSource::OwnedSource(input) => format!(
                "context.walk_owned_source_cohort({}, &[{}])?",
                owner_literal(&input.source().owner),
                render_string_path(&input.source().path)
            ),
            _ => unreachable!("private Basic inventory checked before rendering"),
        };
        source.push_str(&format!("    let cohort = {walk};\n    let mut outputs = Vec::with_capacity(cohort.len());\n    for candidate in cohort.iter() {{\n        let item_context = candidate.context();\n"));
        source.push_str(&render_scope_item(
            scope,
            children,
            "        ",
            "item_context",
        ));
        source.push_str("        outputs.push(output);\n    }\n    Ok(repeated(outputs))\n");
    } else {
        source.push_str(&render_scope_item(scope, children, "    ", "context"));
        source.push_str(if scope.repeating {
            "    Ok(repeated([output]))\n"
        } else {
            "    Ok(output)\n"
        });
    }
    source.push_str("}\n\n");
    source
}

fn render_entries(program: &Program) -> String {
    let mut source = String::from(
        "#[derive(Clone, Debug, PartialEq)]\n\
         pub(crate) struct NamedOutput { pub name: String, pub instance: Instance }\n\
         #[derive(Clone, Debug, PartialEq)]\n\
         pub(crate) struct ExecutionOutputs { pub primary: Instance, pub extras: Vec<NamedOutput> }\n\
         #[derive(Clone, Debug, PartialEq)]\n\
         pub(crate) enum SelectedTargetOutput { Primary(Instance), Named(NamedOutput) }\n\n\
         // Trusted declaration precondition: exact complete Program name/order/count,\n\
         // optional bindings and original ordinals. This private route does not\n\
         // normalize arbitrary supplied inputs or attach a dynamic source loader.\n\
         pub(crate) fn execute_private_owned_basic<'src>(\n\
             source: &'src Instance, declared: &'src [DeclaredNamedSourceRoot<'src>],\n\
             execution: Option<&ExecutionContext<'src>>, limits: OwnedSourceLimits,\n\
         ) -> Result<ExecutionOutputs, RuntimeError> {\n\
             let roots = BasicSourceRoots::try_new(source, declared, limits)?;\n\
             let context = match execution { Some(e) => roots.root_context_with_execution(e)?, None => roots.root_context()? };\n\
",
    );
    if !program.failure_rules.is_empty() {
        source.push_str("    evaluate_basic_failure_rules(&context)?;\n");
    }
    source.push_str("let primary = scope_root(&context)?;\nlet extras = vec![\n");
    for (index, target) in program.extra_targets.iter().enumerate() {
        source.push_str(&format!("        NamedOutput {{ name: {}.to_string(), instance: scope_extra_{index}(&context)? }},\n", rust_string(&target.name)));
    }
    source.push_str("    ];\n    Ok(ExecutionOutputs { primary, extras })\n}\n\n");
    source.push_str("pub(crate) fn execute_selected_private_owned_basic<'src>(\n    source: &'src Instance, declared: &'src [DeclaredNamedSourceRoot<'src>],\n    execution: Option<&ExecutionContext<'src>>, limits: OwnedSourceLimits, selection: TargetSelection<'_>,\n) -> Result<SelectedTargetOutput, RuntimeError> {\n    let selected = match selection {\n        TargetSelection::Primary => 0,\n        TargetSelection::Named(name) => match name {\n");
    for (index, target) in program.extra_targets.iter().enumerate() {
        source.push_str(&format!(
            "            {} => {},\n",
            rust_string(&target.name),
            index + 1
        ));
    }
    source.push_str("            _ => return Err(RuntimeError::UnknownTarget { name: name.to_string() }),\n        },\n    };\n    let roots = BasicSourceRoots::try_new(source, declared, limits)?;\n    let context = match execution { Some(e) => roots.root_context_with_execution(e)?, None => roots.root_context()? };\n");
    if !program.failure_rules.is_empty() {
        source.push_str("    evaluate_basic_failure_rules(&context)?;\n");
    }
    source.push_str("    match selected {\n        0 => Ok(SelectedTargetOutput::Primary(scope_root(&context)?)),\n");
    for (index, target) in program.extra_targets.iter().enumerate() {
        source.push_str(&format!("        {} => Ok(SelectedTargetOutput::Named(NamedOutput {{ name: {}.to_string(), instance: scope_extra_{index}(&context)? }})),\n", index + 1, rust_string(&target.name)));
    }
    source.push_str("        _ => unreachable!(\"resolved private target\"),\n    }\n}\n\n");
    source
}
