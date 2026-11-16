use std::collections::BTreeMap;

use ::mapping::{FunctionId, FunctionParameterId, NodeId};
use codegen::{
    AggregateFunction, AggregateValue, Binding, CandidateIteration, Expression, GeneratedSequence,
    GroupingPlan, InnerJoin, IterationKind, IterationOutput, IterationPlan, IterationSource,
    JoinSource, JoinSourceCardinality, Program, ProgramValidationError, ScalarTargetDomain,
    SequenceWindow, SortFilterOrder, TargetConstruction, TargetScope, UserFunctionProgram,
};
use ir::ScalarType;

use crate::composition::ClassPart;
use crate::{EmitError, literal};

mod expressions;
use expressions::*;
pub(super) use expressions::{
    render_basic_expression, render_legacy_basic_expression, render_public_basic_expression,
};
mod iteration;
use iteration::*;
mod scopes;
use scopes::*;
pub(super) use scopes::{
    owned_dynamic_loading_paths, render_basic_scopes, render_basic_scopes_profile,
};
mod literals;
use literals::*;
pub(super) use literals::{
    aggregate_function_name, basic_path, basic_selected_output_types, render_optional_path,
};

mod failures;
mod filter_map;
mod owned_dynamic_loading;
mod owned_group_by;
mod owned_join;
mod owned_json_api;
#[cfg(test)]
mod owned_json_api_tests;
mod owned_recursive_collect;
mod owned_recursive_filter;
mod selected_json_api;
mod source_contract;
mod xml_api;
pub(crate) use xml_api::render_owned_document_sets;

/// Shared JSON document input/output records of the JSON companions.
const JSON_RECORDS: &str = "public sealed record NamedJsonInput(\n    string Name,\n    string Document);\n\npublic sealed record NamedJsonBytesInput(\n    string Name,\n    byte[] Document);\n\npublic sealed record NamedJsonOutput(\n    string Name,\n    string Document);\n\npublic sealed record NamedJsonBytesOutput(\n    string Name,\n    byte[] Document);\n\npublic sealed record JsonExecutionOutputs(\n    string Primary,\n    global::System.Collections.Generic.IReadOnlyList<NamedJsonOutput> Extras);\n\npublic sealed record JsonBytesExecutionOutputs(\n    byte[] Primary,\n    global::System.Collections.Generic.IReadOnlyList<NamedJsonBytesOutput> Extras);\n\n";

struct ScopePlan<'a> {
    preserve_scalars: Option<ir::XmlScalarPreservation<'a>>,
    repeating: bool,
    iteration: Option<&'a IterationPlan>,
    construction: &'a TargetConstruction,
    evaluations: Vec<u32>,
    bindings: Vec<BindingPlan<'a>>,
    children: Vec<(&'a str, usize)>,
    dynamic_children: Vec<(u32, usize)>,
    segments: Vec<usize>,
}

struct BindingPlan<'a> {
    target_field: &'a str,
    target_domain: ScalarTargetDomain,
    repeating: bool,
    values: Vec<usize>,
}

/// Renders under the complete companion selection, preparing its plan.
#[cfg(test)]
pub(crate) fn render_complete(program: &Program) -> Result<ClassPart, EmitError> {
    render(
        program,
        &codegen::prepare_boundary_emission(program, codegen::CompanionSelection::Complete)?,
    )
}

/// Renders the typed entry points alone, preparing their plan.
#[cfg(test)]
pub(crate) fn render_typed(program: &Program) -> Result<ClassPart, EmitError> {
    render(
        program,
        &codegen::prepare_boundary_emission(program, codegen::CompanionSelection::TypedOnly)?,
    )
}

pub(crate) fn append_basic_owned_json_api(
    program: &Program,
    output: &mut String,
) -> Result<(), EmitError> {
    owned_json_api::render(program, output)
}

/// Whether the shared JSON document input/output records are declared.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum JsonRecords {
    Declared,
    Omitted,
}

/// Renders the mapping and the companions `plan` selects. The XML adapter
/// renders from the plan's proved classification; typed-only libraries
/// declare no JSON type.
pub(crate) fn render(
    program: &Program,
    plan: &codegen::BoundaryEmissionPlan<'_>,
) -> Result<ClassPart, EmitError> {
    let records = if plan.json_companions() {
        JsonRecords::Declared
    } else {
        JsonRecords::Omitted
    };
    render_mapping(program, plan, records)
}

/// [`render`] that keeps the JSON records for the CSV adapters' typed
/// libraries, whose artifacts predate typed-only selection.
pub(crate) fn render_with_json_records(
    program: &Program,
    plan: &codegen::BoundaryEmissionPlan<'_>,
) -> Result<ClassPart, EmitError> {
    render_mapping(program, plan, JsonRecords::Declared)
}

fn render_mapping(
    program: &Program,
    plan: &codegen::BoundaryEmissionPlan<'_>,
    records: JsonRecords,
) -> Result<ClassPart, EmitError> {
    codegen::validate_owned_emission(program)?;
    let has_filter_map = !program.filter_map_v1_sequences().is_empty();
    let mut expressions = BTreeMap::new();
    for node in &program.expressions {
        expressions.insert(node.id, &node.expression);
    }
    let functions = program
        .user_functions
        .iter()
        .map(|function| (function.id, function))
        .collect::<BTreeMap<_, _>>();

    let mut scopes = Vec::new();
    let primary_scope = add_scope(&program.root, &mut scopes);
    // Named targets are refused with a policy, so every scope is primary.
    match program.xml_scalar_preservation() {
        Some(preservation @ ir::XmlScalarPreservation::Document) => {
            scopes
                .iter_mut()
                .for_each(|scope| scope.preserve_scalars = Some(preservation));
        }
        preservation => scopes[primary_scope].preserve_scalars = preservation,
    }
    let extra_scopes = program
        .extra_targets
        .iter()
        .map(|target| (target.name.as_str(), add_scope(&target.root, &mut scopes)))
        .collect::<Vec<_>>();

    let mut types = String::from(
        "public sealed record NamedInput(\n    string Name,\n    global::Ferrule.Runtime.FerruleInstance Instance);\n\npublic sealed record NamedOutput(\n    string Name,\n    global::Ferrule.Runtime.FerruleInstance Instance);\n\npublic sealed record ExecutionOutputs(\n    global::Ferrule.Runtime.FerruleInstance Primary,\n    global::System.Collections.Generic.IReadOnlyList<NamedOutput> Extras);\n\n",
    );
    if records == JsonRecords::Declared {
        types.push_str(JSON_RECORDS);
    }
    types.push_str(xml_api::types(plan.xml()));
    if plan.json_companions() {
        types.push_str(selected_json_api::OUTPUT_TYPES);
    }
    types.push_str(SELECTED_TARGET_OUTPUT_TYPES);
    let mut output = String::new();
    source_contract::render(program, &mut output)?;
    let selected_xml = plan
        .xml()
        .is_some_and(|xml| xml.selected_inputs().is_some());
    render_entry_points(
        program,
        primary_scope,
        &extra_scopes,
        selected_xml,
        &mut output,
    );
    if plan.json_companions() {
        render_json_entry_points(program, &mut output)?;
        selected_json_api::render(program, &mut output);
    }
    if let Some(xml) = plan.xml() {
        xml_api::render_plan(program, xml, &mut output)?;
    }
    failures::render(&program.failure_rules, &mut output)?;
    for function in &program.user_functions {
        render_user_function(function, &functions, has_filter_map, &mut output)?;
    }

    for (node, expression) in expressions {
        render_expression(
            node,
            expression,
            &functions,
            has_filter_map,
            ExpressionContext::Legacy,
            &mut output,
        )?;
    }

    for (scope_index, scope) in scopes.iter().enumerate() {
        output.push('\n');
        output.push_str(&format!(
            "    private static global::Ferrule.Runtime.FerruleInstance Scope_{scope_index}(\n        global::Ferrule.Runtime.ScopeContext context)\n    {{\n"
        ));
        if matches!(scope.construction, TargetConstruction::CopyCurrentRows) {
            output.push_str("        context.ValidateFlatRowsSource();\n");
        }
        if let Some(iteration) = scope.iteration {
            match iteration.kind() {
                IterationKind::Concatenate(_) => {
                    render_concatenated_scope(program, iteration, &scope.segments, &mut output)?;
                }
                IterationKind::OptionalPresence(presence) => {
                    // The condition and the constructed group share the parent context.
                    let condition = presence.condition();
                    output.push_str(&format!(
                        "        if (!global::Ferrule.Runtime.FerruleFunctions.RequireBoolean(Node_{condition}(context), {condition}U))\n        {{\n            return new global::Ferrule.Runtime.FerruleRepeated(global::System.Array.Empty<global::Ferrule.Runtime.FerruleInstance>());\n        }}\n        var item_{scope_index} = ScopeItem_{scope_index}(context);\n        return new global::Ferrule.Runtime.FerruleRepeated(new global::Ferrule.Runtime.FerruleInstance[] {{ item_{scope_index} }});\n"
                    ));
                }
                IterationKind::Candidates(candidates) => {
                    render_iteration_scope(
                        program,
                        scope_index,
                        iteration,
                        candidates,
                        matches!(
                            scope.construction,
                            TargetConstruction::DynamicGroup { merge: true, .. }
                        ),
                        matches!(scope.construction, TargetConstruction::CollectDocuments),
                        &mut output,
                    )?;
                }
                IterationKind::Owned(_) => {
                    return Err(codegen::unrendered_iteration(program, iteration).into());
                }
            }
        } else {
            output.push_str(&format!(
                "        var item_{scope_index} = ScopeItem_{scope_index}(context);\n"
            ));
            if scope.repeating {
                output.push_str(&format!(
                    "        return new global::Ferrule.Runtime.FerruleRepeated(new global::Ferrule.Runtime.FerruleInstance[] {{ item_{scope_index} }});\n"
                ));
            } else {
                output.push_str(&format!("        return item_{scope_index};\n"));
            }
        }
        output.push_str("    }\n\n");
        output.push_str(&format!(
            "    private static global::Ferrule.Runtime.FerruleInstance ScopeItem_{scope_index}(\n        global::Ferrule.Runtime.ScopeContext context)\n    {{\n"
        ));
        if let TargetConstruction::Scalar {
            expression,
            target_domain,
        } = scope.construction
        {
            output.push_str(&format!(
                "        return TargetBuilder.Scalar(Node_{expression}(context), {});\n    }}\n",
                render_target_domain(*target_domain)
            ));
            continue;
        }
        if let TargetConstruction::RecursiveFilter {
            children,
            items,
            predicate,
        } = scope.construction
        {
            output.push_str(
                "        return global::Ferrule.Runtime.FerruleRecursiveFilter.Apply(\n            context,\n            ",
            );
            output.push_str(&literal::string(children));
            output.push_str(",\n            ");
            output.push_str(&literal::string(items));
            output.push_str(&format!(
                ",\n            {predicate}U,\n            Node_{predicate});\n    }}\n"
            ));
            continue;
        }
        if let TargetConstruction::PathHierarchy {
            collection,
            separator,
            directories,
            files,
            name,
        } = scope.construction
        {
            output.push_str(
                "        return global::Ferrule.Runtime.FerrulePathHierarchy.Build(\n            context,\n            ",
            );
            render_path(collection, &mut output);
            output.push_str(",\n            ");
            output.push_str(&literal::string(separator));
            output.push_str(",\n            ");
            output.push_str(&literal::string(directories));
            output.push_str(",\n            ");
            output.push_str(&literal::string(files));
            output.push_str(",\n            ");
            output.push_str(&literal::string(name));
            output.push_str(");\n    }\n");
            continue;
        }
        if let TargetConstruction::AdjacencyTree {
            collection,
            key,
            parent,
            target_key,
            target_children,
            root,
        } = scope.construction
        {
            output.push_str(
                "        return global::Ferrule.Runtime.FerruleAdjacencyTree.Build(\n            context,\n            ",
            );
            render_path(collection, &mut output);
            output.push_str(",\n            ");
            render_path(key, &mut output);
            output.push_str(",\n            ");
            render_path(parent, &mut output);
            output.push_str(",\n            ");
            output.push_str(&literal::string(target_key));
            output.push_str(",\n            ");
            output.push_str(&literal::string(target_children));
            output.push_str(",\n            ");
            match root {
                Some(root) => output.push_str(&format!("Node_{root}")),
                None => output.push_str("null"),
            }
            output.push_str(");\n    }\n");
            continue;
        }
        if matches!(scope.construction, TargetConstruction::CopyCurrentSource) {
            output.push_str("        return context.CopyCurrentGroup();\n    }\n");
            continue;
        }
        if matches!(scope.construction, TargetConstruction::CopyCurrentRows) {
            output.push_str("        return context.CopyCurrentRows();\n    }\n");
            continue;
        }
        render_bound_group(scope_index, scope, &mut output);
    }
    filter_map::add_scoped_node_wrappers(program, &mut output);
    Ok(ClassPart {
        types,
        members: output,
    })
}

const SELECTED_TARGET_OUTPUT_TYPES: &str = r#"public abstract class SelectedTargetOutput
{
    private protected SelectedTargetOutput() { }

    public sealed class Primary : SelectedTargetOutput
    {
        internal Primary(global::Ferrule.Runtime.FerruleInstance instance) { Instance = instance; }
        public global::Ferrule.Runtime.FerruleInstance Instance { get; }
    }

    public sealed class Named : SelectedTargetOutput
    {
        internal Named(NamedOutput output) { Output = output; }
        public NamedOutput Output { get; }
    }
}

"#;

fn render_selected_target_entry_points(
    program: &Program,
    primary_scope: usize,
    extra_scopes: &[(&str, usize)],
    selected_xml: bool,
    output: &mut String,
) {
    output.push_str(
        "\n    public static SelectedTargetOutput ExecuteSelectedTarget(\n        global::Ferrule.Runtime.FerruleInstance source,\n        global::Ferrule.Runtime.FerruleTargetSelection selection)\n    {\n        return ExecuteSelectedTargetWithHost(source, selection, global::System.Array.Empty<NamedInput>());\n    }\n\n    public static SelectedTargetOutput ExecuteSelectedTargetWithHost(\n        global::Ferrule.Runtime.FerruleInstance source,\n        global::Ferrule.Runtime.FerruleTargetSelection selection,\n        global::System.Collections.Generic.IReadOnlyList<NamedInput> extraSources,\n        global::Ferrule.Runtime.FerruleExecutionContext? executionContext = null,\n        global::Ferrule.Runtime.IFerruleDynamicSourceLoader? dynamicSourceLoader = null)\n    {\n        global::System.ArgumentNullException.ThrowIfNull(source);\n        global::System.ArgumentNullException.ThrowIfNull(selection);\n        var selected = ResolveTarget(selection);\n        var context = CreateContext(source, extraSources, executionContext, dynamicSourceLoader);\n",
    );
    // Only selected XML enters execution with an already-admitted context.
    // Other profiles retain their original inline typed entry-point body.
    if selected_xml {
        output.push_str("        return ExecuteSelectedTargetFromContext(context, selected);\n    }\n\n    private static SelectedTargetOutput ExecuteSelectedTargetFromContext(global::Ferrule.Runtime.ScopeContext context, int selected)\n    {\n");
    }
    if !program.failure_rules.is_empty() {
        output.push_str("        EvaluateFailureRules(context);\n");
    }
    output.push_str(&format!("        switch (selected)\n        {{\n            case 0: return new SelectedTargetOutput.Primary(Scope_{primary_scope}(context));\n"));
    for (index, (name, scope)) in extra_scopes.iter().enumerate() {
        output.push_str(&format!("            case {}: return new SelectedTargetOutput.Named(new NamedOutput({}, Scope_{scope}(context)));\n", index + 1, literal::string(name)));
    }
    output.push_str("            default: throw new global::System.InvalidOperationException(\"Resolved target is invalid.\");\n        }\n    }\n\n    private static int ResolveTarget(global::Ferrule.Runtime.FerruleTargetSelection selection)\n    {\n        if (selection.IsPrimary) return 0;\n        var name = selection.Name!;\n");
    for (index, (name, _)) in extra_scopes.iter().enumerate() {
        output.push_str(&format!("        if (global::System.String.Equals(name, {}, global::System.StringComparison.Ordinal)) return {};\n", literal::string(name), index + 1));
    }
    output.push_str("        throw new global::Ferrule.Runtime.FerruleRuntimeException(\n            global::Ferrule.Runtime.FerruleRuntimeError.UnknownTarget,\n            $\"project has no named target `{name}`\",\n            detail: name);\n    }\n");
}

fn render_entry_points(
    program: &Program,
    primary_scope: usize,
    extra_scopes: &[(&str, usize)],
    selected_xml: bool,
    output: &mut String,
) {
    output.push_str(
        "    public static global::Ferrule.Runtime.FerruleInstance Execute(\n        global::Ferrule.Runtime.FerruleInstance source)\n    {\n        return ExecuteWithSources(source, global::System.Array.Empty<NamedInput>());\n    }\n",
    );
    output.push_str(
        "\n    public static global::Ferrule.Runtime.FerruleInstance Execute(\n        global::Ferrule.Runtime.FerruleInstance source,\n        global::Ferrule.Runtime.FerruleExecutionContext executionContext)\n    {\n        return ExecuteWithSources(source, global::System.Array.Empty<NamedInput>(), executionContext);\n    }\n",
    );
    output.push_str(
        "\n    public static global::Ferrule.Runtime.FerruleInstance ExecuteWithSources(\n        global::Ferrule.Runtime.FerruleInstance source,\n        global::System.Collections.Generic.IReadOnlyList<NamedInput> extraSources)\n    {\n        return ExecuteOutputsWithSources(source, extraSources).Primary;\n    }\n",
    );
    output.push_str(
        "\n    public static global::Ferrule.Runtime.FerruleInstance ExecuteWithSources(\n        global::Ferrule.Runtime.FerruleInstance source,\n        global::System.Collections.Generic.IReadOnlyList<NamedInput> extraSources,\n        global::Ferrule.Runtime.FerruleExecutionContext executionContext)\n    {\n        return ExecuteOutputsWithSources(source, extraSources, executionContext).Primary;\n    }\n",
    );
    output.push_str(
        "\n    public static global::Ferrule.Runtime.FerruleInstance ExecuteWithDynamicSourceLoader(\n        global::Ferrule.Runtime.FerruleInstance source,\n        global::Ferrule.Runtime.IFerruleDynamicSourceLoader loader)\n    {\n        return ExecuteOutputsWithDynamicSourceLoader(source, loader).Primary;\n    }\n\
         \n    public static global::Ferrule.Runtime.FerruleInstance ExecuteWithSourcesAndDynamicSourceLoader(\n        global::Ferrule.Runtime.FerruleInstance source,\n        global::System.Collections.Generic.IReadOnlyList<NamedInput> extraSources,\n        global::Ferrule.Runtime.IFerruleDynamicSourceLoader loader)\n    {\n        return ExecuteOutputsWithSourcesAndDynamicSourceLoader(source, extraSources, loader).Primary;\n    }\n\
         \n    public static global::Ferrule.Runtime.FerruleInstance ExecuteWithSourcesContextAndDynamicSourceLoader(\n        global::Ferrule.Runtime.FerruleInstance source,\n        global::System.Collections.Generic.IReadOnlyList<NamedInput> extraSources,\n        global::Ferrule.Runtime.FerruleExecutionContext executionContext,\n        global::Ferrule.Runtime.IFerruleDynamicSourceLoader loader)\n    {\n        return ExecuteOutputsWithSourcesContextAndDynamicSourceLoader(\n            source,\n            extraSources,\n            executionContext,\n            loader).Primary;\n    }\n",
    );
    output.push_str(
        "\n    public static ExecutionOutputs ExecuteOutputs(\n        global::Ferrule.Runtime.FerruleInstance source)\n    {\n        return ExecuteOutputsWithSources(source, global::System.Array.Empty<NamedInput>());\n    }\n",
    );
    output.push_str(
        "\n    public static ExecutionOutputs ExecuteOutputs(\n        global::Ferrule.Runtime.FerruleInstance source,\n        global::Ferrule.Runtime.FerruleExecutionContext executionContext)\n    {\n        return ExecuteOutputsWithSources(source, global::System.Array.Empty<NamedInput>(), executionContext);\n    }\n",
    );
    output.push_str(
        "\n    public static ExecutionOutputs ExecuteOutputsWithSources(\n        global::Ferrule.Runtime.FerruleInstance source,\n        global::System.Collections.Generic.IReadOnlyList<NamedInput> extraSources)\n    {\n        return ExecuteOutputs(CreateContext(source, extraSources, null));\n    }\n",
    );
    output.push_str(
        "\n    public static ExecutionOutputs ExecuteOutputsWithSources(\n        global::Ferrule.Runtime.FerruleInstance source,\n        global::System.Collections.Generic.IReadOnlyList<NamedInput> extraSources,\n        global::Ferrule.Runtime.FerruleExecutionContext executionContext)\n    {\n        global::System.ArgumentNullException.ThrowIfNull(executionContext);\n        return ExecuteOutputs(CreateContext(source, extraSources, executionContext));\n    }\n",
    );
    output.push_str(
        "\n    public static ExecutionOutputs ExecuteOutputsWithDynamicSourceLoader(\n        global::Ferrule.Runtime.FerruleInstance source,\n        global::Ferrule.Runtime.IFerruleDynamicSourceLoader loader)\n    {\n        return ExecuteOutputsWithSourcesAndDynamicSourceLoader(\n            source,\n            global::System.Array.Empty<NamedInput>(),\n            loader);\n    }\n\
         \n    public static ExecutionOutputs ExecuteOutputsWithSourcesAndDynamicSourceLoader(\n        global::Ferrule.Runtime.FerruleInstance source,\n        global::System.Collections.Generic.IReadOnlyList<NamedInput> extraSources,\n        global::Ferrule.Runtime.IFerruleDynamicSourceLoader loader)\n    {\n        global::System.ArgumentNullException.ThrowIfNull(loader);\n        return ExecuteOutputs(CreateContext(source, extraSources, null, loader));\n    }\n\
         \n    public static ExecutionOutputs ExecuteOutputsWithSourcesContextAndDynamicSourceLoader(\n        global::Ferrule.Runtime.FerruleInstance source,\n        global::System.Collections.Generic.IReadOnlyList<NamedInput> extraSources,\n        global::Ferrule.Runtime.FerruleExecutionContext executionContext,\n        global::Ferrule.Runtime.IFerruleDynamicSourceLoader loader)\n    {\n        global::System.ArgumentNullException.ThrowIfNull(executionContext);\n        global::System.ArgumentNullException.ThrowIfNull(loader);\n        return ExecuteOutputs(CreateContext(source, extraSources, executionContext, loader));\n    }\n",
    );
    render_selected_target_entry_points(program, primary_scope, extra_scopes, selected_xml, output);
    render_source_context(program, output);
    output.push_str(
        "\n    private static ExecutionOutputs ExecuteOutputs(\n        global::Ferrule.Runtime.ScopeContext context)\n    {\n",
    );
    if !program.failure_rules.is_empty() {
        output.push_str("        EvaluateFailureRules(context);\n");
    }
    output.push_str(&format!(
        "        var primary = Scope_{primary_scope}(context);\n"
    ));
    for (index, (_, scope)) in extra_scopes.iter().enumerate() {
        output.push_str(&format!(
            "        var extra_{index} = Scope_{scope}(context);\n"
        ));
    }
    output.push_str(
        "        return new ExecutionOutputs(\n            primary,\n            new NamedOutput[]\n            {\n",
    );
    for (index, (name, _)) in extra_scopes.iter().enumerate() {
        output.push_str("                new(");
        output.push_str(&literal::string(name));
        output.push_str(&format!(", extra_{index}),\n"));
    }
    output.push_str("            });\n    }\n");
}

fn render_json_entry_points(program: &Program, output: &mut String) -> Result<(), EmitError> {
    let static_sources = program
        .extra_sources
        .iter()
        .filter(|source| source.dynamic.is_none())
        .collect::<Vec<_>>();
    let dynamic_sources = program
        .extra_sources
        .iter()
        .filter(|source| source.dynamic.is_some())
        .collect::<Vec<_>>();
    let source_schema = codegen::serialize_embedded_schema(
        &program.source,
        codegen::MAX_EMBEDDED_JSON_SCHEMA_BYTES,
    )?;
    let target_schema = codegen::serialize_embedded_schema(
        &program.target,
        codegen::MAX_EMBEDDED_JSON_SCHEMA_BYTES,
    )?;
    let extra_source_schemas = program
        .extra_sources
        .iter()
        .filter(|source| source.dynamic.is_none())
        .map(|source| {
            codegen::serialize_embedded_schema(
                &source.source,
                codegen::MAX_EMBEDDED_JSON_SCHEMA_BYTES,
            )
            .map_err(EmitError::from)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let dynamic_source_schemas = dynamic_sources
        .iter()
        .map(|source| {
            codegen::serialize_embedded_schema(
                &source.source,
                codegen::MAX_EMBEDDED_JSON_SCHEMA_BYTES,
            )
            .map_err(EmitError::from)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let extra_target_schemas = program
        .extra_targets
        .iter()
        .map(|target| {
            codegen::serialize_embedded_schema(
                &target.target,
                codegen::MAX_EMBEDDED_JSON_SCHEMA_BYTES,
            )
            .map_err(EmitError::from)
        })
        .collect::<Result<Vec<_>, _>>()?;

    output.push_str("\n    private const string SourceJsonSchema = ");
    output.push_str(&literal::string(&source_schema));
    output.push_str(";\n    private const string TargetJsonSchema = ");
    output.push_str(&literal::string(&target_schema));
    output.push_str(";\n");
    if !static_sources.is_empty() {
        output.push_str(
            "    private static readonly string[] ExtraSourceJsonSchemas = new string[]\n    {\n",
        );
        for schema in &extra_source_schemas {
            output.push_str("        ");
            output.push_str(&literal::string(schema));
            output.push_str(",\n");
        }
        output.push_str("    };\n");
    }
    if !dynamic_sources.is_empty() {
        output.push_str(
            "    private static readonly string[] DynamicSourceJsonSchemas = new string[]\n    {\n",
        );
        for schema in &dynamic_source_schemas {
            output.push_str("        ");
            output.push_str(&literal::string(schema));
            output.push_str(",\n");
        }
        output.push_str("    };\n");
    }
    if program.extra_targets.is_empty() {
        output.push_str(
            "    private static readonly string[] ExtraTargetJsonSchemas = global::System.Array.Empty<string>();\n",
        );
    } else {
        output.push_str(
            "    private static readonly string[] ExtraTargetJsonSchemas = new string[]\n    {\n",
        );
        for schema in &extra_target_schemas {
            output.push_str("        ");
            output.push_str(&literal::string(schema));
            output.push_str(",\n");
        }
        output.push_str("    };\n");
    }

    output.push_str(
        "\n    public static string ExecuteJson(string source)\n    {\n        return ExecuteJsonOutputs(source).Primary;\n    }\n\
         \n    public static string ExecuteJson(\n        string source,\n        global::Ferrule.Runtime.FerruleExecutionContext executionContext)\n    {\n        return ExecuteJsonOutputs(source, executionContext).Primary;\n    }\n\
         \n    public static string ExecuteJsonWithSources(\n        string source,\n        global::System.Collections.Generic.IReadOnlyList<NamedJsonInput> extraSources)\n    {\n        return ExecuteJsonOutputsWithSources(source, extraSources).Primary;\n    }\n\
         \n    public static string ExecuteJsonWithSources(\n        string source,\n        global::System.Collections.Generic.IReadOnlyList<NamedJsonInput> extraSources,\n        global::Ferrule.Runtime.FerruleExecutionContext executionContext)\n    {\n        return ExecuteJsonOutputsWithSources(source, extraSources, executionContext).Primary;\n    }\n\
         \n    public static JsonExecutionOutputs ExecuteJsonOutputs(string source)\n    {\n        return ExecuteJsonOutputsWithSources(source, global::System.Array.Empty<NamedJsonInput>());\n    }\n\
         \n    public static JsonExecutionOutputs ExecuteJsonOutputs(\n        string source,\n        global::Ferrule.Runtime.FerruleExecutionContext executionContext)\n    {\n        return ExecuteJsonOutputsWithSources(\n            source,\n            global::System.Array.Empty<NamedJsonInput>(),\n            executionContext);\n    }\n\
         \n    public static JsonExecutionOutputs ExecuteJsonOutputsWithSources(\n        string source,\n        global::System.Collections.Generic.IReadOnlyList<NamedJsonInput> extraSources)\n    {\n        global::System.ArgumentNullException.ThrowIfNull(source);\n        global::System.ArgumentNullException.ThrowIfNull(extraSources);\n        ValidateNamedJsonInputNames(extraSources);\n        var parsedSource = global::Ferrule.Runtime.FerruleJson.ParseEmbedded(SourceJsonSchema, source);\n        var parsedInputs = ParseNamedJsonInputs(extraSources);\n        return SerializeJsonOutputs(ExecuteOutputsWithSources(parsedSource, parsedInputs));\n    }\n\
         \n    public static JsonExecutionOutputs ExecuteJsonOutputsWithSources(\n        string source,\n        global::System.Collections.Generic.IReadOnlyList<NamedJsonInput> extraSources,\n        global::Ferrule.Runtime.FerruleExecutionContext executionContext)\n    {\n        global::System.ArgumentNullException.ThrowIfNull(source);\n        global::System.ArgumentNullException.ThrowIfNull(extraSources);\n        global::System.ArgumentNullException.ThrowIfNull(executionContext);\n        ValidateNamedJsonInputNames(extraSources);\n        var parsedSource = global::Ferrule.Runtime.FerruleJson.ParseEmbedded(SourceJsonSchema, source);\n        var parsedInputs = ParseNamedJsonInputs(extraSources);\n        return SerializeJsonOutputs(ExecuteOutputsWithSources(\n            parsedSource,\n            parsedInputs,\n            executionContext));\n    }\n",
    );
    output.push_str(
        r#"
    public static byte[] ExecuteJsonBytes(byte[] source)
    {
        return ExecuteJsonBytesOutputs(source).Primary;
    }

    public static byte[] ExecuteJsonBytes(
        byte[] source,
        global::Ferrule.Runtime.FerruleExecutionContext executionContext)
    {
        return ExecuteJsonBytesOutputs(source, executionContext).Primary;
    }

    public static byte[] ExecuteJsonBytesWithSources(
        byte[] source,
        global::System.Collections.Generic.IReadOnlyList<NamedJsonBytesInput> extraSources)
    {
        return ExecuteJsonBytesOutputsWithSources(source, extraSources).Primary;
    }

    public static byte[] ExecuteJsonBytesWithSources(
        byte[] source,
        global::System.Collections.Generic.IReadOnlyList<NamedJsonBytesInput> extraSources,
        global::Ferrule.Runtime.FerruleExecutionContext executionContext)
    {
        return ExecuteJsonBytesOutputsWithSources(source, extraSources, executionContext).Primary;
    }

    public static JsonBytesExecutionOutputs ExecuteJsonBytesOutputs(byte[] source)
    {
        return ExecuteJsonBytesOutputsWithSources(
            source,
            global::System.Array.Empty<NamedJsonBytesInput>());
    }

    public static JsonBytesExecutionOutputs ExecuteJsonBytesOutputs(
        byte[] source,
        global::Ferrule.Runtime.FerruleExecutionContext executionContext)
    {
        return ExecuteJsonBytesOutputsWithSources(
            source,
            global::System.Array.Empty<NamedJsonBytesInput>(),
            executionContext);
    }

    public static JsonBytesExecutionOutputs ExecuteJsonBytesOutputsWithSources(
        byte[] source,
        global::System.Collections.Generic.IReadOnlyList<NamedJsonBytesInput> extraSources)
    {
        global::System.ArgumentNullException.ThrowIfNull(source);
        global::System.ArgumentNullException.ThrowIfNull(extraSources);
        ValidateNamedJsonBytesInputNames(extraSources);
        var parsedSource =
            global::Ferrule.Runtime.FerruleJson.ParseEmbeddedBytes(SourceJsonSchema, source);
        var parsedInputs = ParseNamedJsonBytesInputs(extraSources);
        return SerializeJsonBytesOutputs(
            ExecuteOutputsWithSources(parsedSource, parsedInputs));
    }

    public static JsonBytesExecutionOutputs ExecuteJsonBytesOutputsWithSources(
        byte[] source,
        global::System.Collections.Generic.IReadOnlyList<NamedJsonBytesInput> extraSources,
        global::Ferrule.Runtime.FerruleExecutionContext executionContext)
    {
        global::System.ArgumentNullException.ThrowIfNull(source);
        global::System.ArgumentNullException.ThrowIfNull(extraSources);
        global::System.ArgumentNullException.ThrowIfNull(executionContext);
        ValidateNamedJsonBytesInputNames(extraSources);
        var parsedSource =
            global::Ferrule.Runtime.FerruleJson.ParseEmbeddedBytes(SourceJsonSchema, source);
        var parsedInputs = ParseNamedJsonBytesInputs(extraSources);
        return SerializeJsonBytesOutputs(ExecuteOutputsWithSources(
            parsedSource,
            parsedInputs,
            executionContext));
    }
"#,
    );
    if !dynamic_sources.is_empty() {
        output.push_str(
            "\n    public static string ExecuteJsonWithDynamicSourceLoader(\n\
                 string source,\n\
                 global::Ferrule.Runtime.IFerruleDynamicJsonSourceLoader loader)\n\
             {\n\
                 return ExecuteJsonOutputsWithDynamicSourceLoader(source, loader).Primary;\n\
             }\n\
             \n    public static string ExecuteJsonWithSourcesAndDynamicSourceLoader(\n\
                 string source,\n\
                 global::System.Collections.Generic.IReadOnlyList<NamedJsonInput> extraSources,\n\
                 global::Ferrule.Runtime.IFerruleDynamicJsonSourceLoader loader)\n\
             {\n\
                 return ExecuteJsonOutputsWithSourcesAndDynamicSourceLoader(\n\
                     source,\n\
                     extraSources,\n\
                     loader).Primary;\n\
             }\n\
             \n    public static string ExecuteJsonWithSourcesContextAndDynamicSourceLoader(\n\
                 string source,\n\
                 global::System.Collections.Generic.IReadOnlyList<NamedJsonInput> extraSources,\n\
                 global::Ferrule.Runtime.FerruleExecutionContext executionContext,\n\
                 global::Ferrule.Runtime.IFerruleDynamicJsonSourceLoader loader)\n\
             {\n\
                 return ExecuteJsonOutputsWithSourcesContextAndDynamicSourceLoader(\n\
                     source,\n\
                     extraSources,\n\
                     executionContext,\n\
                     loader).Primary;\n\
             }\n\
             \n    public static JsonExecutionOutputs ExecuteJsonOutputsWithDynamicSourceLoader(\n\
                 string source,\n\
                 global::Ferrule.Runtime.IFerruleDynamicJsonSourceLoader loader)\n\
             {\n\
                 return ExecuteJsonOutputsWithJsonLoader(\n\
                     source,\n\
                     global::System.Array.Empty<NamedJsonInput>(),\n\
                     null,\n\
                     loader);\n\
             }\n\
             \n    public static JsonExecutionOutputs ExecuteJsonOutputsWithSourcesAndDynamicSourceLoader(\n\
                 string source,\n\
                 global::System.Collections.Generic.IReadOnlyList<NamedJsonInput> extraSources,\n\
                 global::Ferrule.Runtime.IFerruleDynamicJsonSourceLoader loader)\n\
             {\n\
                 return ExecuteJsonOutputsWithJsonLoader(source, extraSources, null, loader);\n\
             }\n\
             \n    public static JsonExecutionOutputs ExecuteJsonOutputsWithSourcesContextAndDynamicSourceLoader(\n\
                 string source,\n\
                 global::System.Collections.Generic.IReadOnlyList<NamedJsonInput> extraSources,\n\
                 global::Ferrule.Runtime.FerruleExecutionContext executionContext,\n\
                 global::Ferrule.Runtime.IFerruleDynamicJsonSourceLoader loader)\n\
             {\n\
                 global::System.ArgumentNullException.ThrowIfNull(executionContext);\n\
                 return ExecuteJsonOutputsWithJsonLoader(\n\
                     source,\n\
                     extraSources,\n\
                     executionContext,\n\
                     loader);\n\
             }\n\
             \n    private static JsonExecutionOutputs ExecuteJsonOutputsWithJsonLoader(\n\
                 string source,\n\
                 global::System.Collections.Generic.IReadOnlyList<NamedJsonInput> extraSources,\n\
                 global::Ferrule.Runtime.FerruleExecutionContext? executionContext,\n\
                 global::Ferrule.Runtime.IFerruleDynamicJsonSourceLoader loader)\n\
             {\n\
                 global::System.ArgumentNullException.ThrowIfNull(source);\n\
                 global::System.ArgumentNullException.ThrowIfNull(extraSources);\n\
                 global::System.ArgumentNullException.ThrowIfNull(loader);\n\
                 ValidateNamedJsonInputNames(extraSources);\n\
                 var parsedSource = global::Ferrule.Runtime.FerruleJson.ParseEmbedded(SourceJsonSchema, source);\n\
                 var parsedInputs = ParseNamedJsonInputs(extraSources);\n\
                 var adapter = new global::Ferrule.Runtime.FerruleJsonDynamicSourceAdapter(\n\
                     loader,\n\
                     DynamicSourceJsonSchema);\n\
                 var outputs = executionContext is null\n\
                     ? ExecuteOutputsWithSourcesAndDynamicSourceLoader(\n\
                         parsedSource,\n\
                         parsedInputs,\n\
                         adapter)\n\
                     : ExecuteOutputsWithSourcesContextAndDynamicSourceLoader(\n\
                         parsedSource,\n\
                         parsedInputs,\n\
                         executionContext,\n\
                         adapter);\n\
                 return SerializeJsonOutputs(outputs);\n\
             }\n\
             \n    private static string? DynamicSourceJsonSchema(string sourceName) => sourceName switch\n    {\n",
        );
        for (index, source) in dynamic_sources.iter().enumerate() {
            output.push_str("        ");
            output.push_str(&literal::string(&source.name));
            output.push_str(&format!(" => DynamicSourceJsonSchemas[{index}],\n"));
        }
        output.push_str("        _ => null,\n    };\n");
    }

    output.push_str(
        "\n    private static void ValidateNamedJsonInputNames(\n        global::System.Collections.Generic.IReadOnlyList<NamedJsonInput> extraSources)\n    {\n",
    );
    if program.execution_contract.is_some() {
        output.push_str(
            "        ValidateClosedSourceBindings(extraSources, extraSources.Count, null);\n",
        );
    } else if static_sources.is_empty() {
        output.push_str(
            "        foreach (var extraSource in extraSources)\n        {\n            global::System.ArgumentNullException.ThrowIfNull(extraSource);\n            global::System.ArgumentNullException.ThrowIfNull(extraSource.Name);\n            throw new global::Ferrule.Runtime.FerruleRuntimeException(\n                global::Ferrule.Runtime.FerruleRuntimeError.UnexpectedNamedSource,\n                $\"named source '{extraSource.Name}' is not declared by this mapping\",\n                detail: extraSource.Name);\n        }\n",
        );
    } else {
        output.push_str(
            "        var matched = new global::System.Collections.Generic.HashSet<string>(global::System.StringComparer.Ordinal);\n        foreach (var extraSource in extraSources)\n        {\n            global::System.ArgumentNullException.ThrowIfNull(extraSource);\n            global::System.ArgumentNullException.ThrowIfNull(extraSource.Name);\n            global::System.ArgumentNullException.ThrowIfNull(extraSource.Document);\n",
        );
        output.push_str("            if (extraSource.Name is not (");
        for (index, source) in static_sources.iter().enumerate() {
            if index != 0 {
                output.push_str(" or ");
            }
            output.push_str(&literal::string(&source.name));
        }
        output.push_str(
            "))\n            {\n                throw new global::Ferrule.Runtime.FerruleRuntimeException(\n                    global::Ferrule.Runtime.FerruleRuntimeError.UnexpectedNamedSource,\n                    $\"named source '{extraSource.Name}' is not declared by this mapping\",\n                    detail: extraSource.Name);\n            }\n            if (!matched.Add(extraSource.Name))\n            {\n                throw new global::Ferrule.Runtime.FerruleRuntimeException(\n                    global::Ferrule.Runtime.FerruleRuntimeError.DuplicateNamedSource,\n                    $\"named source '{extraSource.Name}' was supplied more than once\",\n                    detail: extraSource.Name);\n            }\n        }\n",
        );
        for source in &static_sources {
            let name = literal::string(&source.name);
            output.push_str(&format!(
                "        if (!matched.Contains({name}))\n        {{\n            throw new global::Ferrule.Runtime.FerruleRuntimeException(\n                global::Ferrule.Runtime.FerruleRuntimeError.MissingNamedSource,\n                \"named source \" + {name} + \" is required by this mapping\",\n                detail: {name});\n        }}\n"
            ));
        }
    }
    output.push_str("    }\n");

    output.push_str(
        "\n    private static void ValidateNamedJsonBytesInputNames(\n        global::System.Collections.Generic.IReadOnlyList<NamedJsonBytesInput> extraSources)\n    {\n",
    );
    if program.execution_contract.is_some() {
        output.push_str(
            "        ValidateClosedSourceBindings(extraSources, extraSources.Count, null);\n",
        );
    } else if static_sources.is_empty() {
        output.push_str(
            "        foreach (var extraSource in extraSources)\n        {\n            global::System.ArgumentNullException.ThrowIfNull(extraSource);\n            global::System.ArgumentNullException.ThrowIfNull(extraSource.Name);\n            throw new global::Ferrule.Runtime.FerruleRuntimeException(\n                global::Ferrule.Runtime.FerruleRuntimeError.UnexpectedNamedSource,\n                $\"named source '{extraSource.Name}' is not declared by this mapping\",\n                detail: extraSource.Name);\n        }\n",
        );
    } else {
        output.push_str(
            "        var matched = new global::System.Collections.Generic.HashSet<string>(global::System.StringComparer.Ordinal);\n        foreach (var extraSource in extraSources)\n        {\n            global::System.ArgumentNullException.ThrowIfNull(extraSource);\n            global::System.ArgumentNullException.ThrowIfNull(extraSource.Name);\n",
        );
        output.push_str("            if (extraSource.Name is not (");
        for (index, source) in static_sources.iter().enumerate() {
            if index != 0 {
                output.push_str(" or ");
            }
            output.push_str(&literal::string(&source.name));
        }
        output.push_str(
            "))\n            {\n                throw new global::Ferrule.Runtime.FerruleRuntimeException(\n                    global::Ferrule.Runtime.FerruleRuntimeError.UnexpectedNamedSource,\n                    $\"named source '{extraSource.Name}' is not declared by this mapping\",\n                    detail: extraSource.Name);\n            }\n            if (!matched.Add(extraSource.Name))\n            {\n                throw new global::Ferrule.Runtime.FerruleRuntimeException(\n                    global::Ferrule.Runtime.FerruleRuntimeError.DuplicateNamedSource,\n                    $\"named source '{extraSource.Name}' was supplied more than once\",\n                    detail: extraSource.Name);\n            }\n        }\n",
        );
        for source in &static_sources {
            let name = literal::string(&source.name);
            output.push_str(&format!(
                "        if (!matched.Contains({name}))\n        {{\n            throw new global::Ferrule.Runtime.FerruleRuntimeException(\n                global::Ferrule.Runtime.FerruleRuntimeError.MissingNamedSource,\n                \"named source \" + {name} + \" is required by this mapping\",\n                detail: {name});\n        }}\n"
            ));
        }
    }
    output.push_str("    }\n");

    output.push_str(
        "\n    private static global::System.Collections.Generic.IReadOnlyList<NamedInput> ParseNamedJsonInputs(\n        global::System.Collections.Generic.IReadOnlyList<NamedJsonInput> extraSources)\n    {\n",
    );
    if program.execution_contract.is_some() {
        output.push_str("        return global::System.Array.Empty<NamedInput>();\n");
    } else if static_sources.is_empty() {
        output.push_str(
            "        foreach (var extraSource in extraSources)\n        {\n            global::System.ArgumentNullException.ThrowIfNull(extraSource);\n            global::System.ArgumentNullException.ThrowIfNull(extraSource.Name);\n            throw new global::Ferrule.Runtime.FerruleRuntimeException(\n                global::Ferrule.Runtime.FerruleRuntimeError.UnexpectedNamedSource,\n                $\"named source '{extraSource.Name}' is not declared by this mapping\",\n                detail: extraSource.Name);\n        }\n        return global::System.Array.Empty<NamedInput>();\n",
        );
    } else {
        output.push_str(
            "        var parsed = new global::System.Collections.Generic.List<NamedInput>(extraSources.Count);\n        foreach (var extraSource in extraSources)\n        {\n            global::System.ArgumentNullException.ThrowIfNull(extraSource);\n            global::System.ArgumentNullException.ThrowIfNull(extraSource.Name);\n            global::System.ArgumentNullException.ThrowIfNull(extraSource.Document);\n            var schema = extraSource.Name switch\n            {\n",
        );
        for (index, source) in static_sources.iter().enumerate() {
            output.push_str("                ");
            output.push_str(&literal::string(&source.name));
            output.push_str(&format!(" => ExtraSourceJsonSchemas[{index}],\n"));
        }
        output.push_str(
            "                _ => throw new global::Ferrule.Runtime.FerruleRuntimeException(\n                    global::Ferrule.Runtime.FerruleRuntimeError.UnexpectedNamedSource,\n                    $\"named source '{extraSource.Name}' is not declared by this mapping\",\n                    detail: extraSource.Name),\n            };\n            parsed.Add(new NamedInput(\n                extraSource.Name,\n                global::Ferrule.Runtime.FerruleJson.ParseEmbedded(schema, extraSource.Document)));\n        }\n        return parsed;\n",
        );
    }
    output.push_str("    }\n");

    output.push_str(
        "\n    private static global::System.Collections.Generic.IReadOnlyList<NamedInput> ParseNamedJsonBytesInputs(\n        global::System.Collections.Generic.IReadOnlyList<NamedJsonBytesInput> extraSources)\n    {\n",
    );
    if program.execution_contract.is_some() {
        output.push_str("        return global::System.Array.Empty<NamedInput>();\n");
    } else if static_sources.is_empty() {
        output.push_str(
            "        foreach (var extraSource in extraSources)\n        {\n            global::System.ArgumentNullException.ThrowIfNull(extraSource);\n            global::System.ArgumentNullException.ThrowIfNull(extraSource.Name);\n            throw new global::Ferrule.Runtime.FerruleRuntimeException(\n                global::Ferrule.Runtime.FerruleRuntimeError.UnexpectedNamedSource,\n                $\"named source '{extraSource.Name}' is not declared by this mapping\",\n                detail: extraSource.Name);\n        }\n        return global::System.Array.Empty<NamedInput>();\n",
        );
    } else {
        output.push_str(
            "        var parsed = new global::System.Collections.Generic.List<NamedInput>(extraSources.Count);\n        foreach (var extraSource in extraSources)\n        {\n            global::System.ArgumentNullException.ThrowIfNull(extraSource);\n            global::System.ArgumentNullException.ThrowIfNull(extraSource.Name);\n            global::System.ArgumentNullException.ThrowIfNull(extraSource.Document);\n            var schema = extraSource.Name switch\n            {\n",
        );
        for (index, source) in static_sources.iter().enumerate() {
            output.push_str("                ");
            output.push_str(&literal::string(&source.name));
            output.push_str(&format!(" => ExtraSourceJsonSchemas[{index}],\n"));
        }
        output.push_str(
            "                _ => throw new global::Ferrule.Runtime.FerruleRuntimeException(\n                    global::Ferrule.Runtime.FerruleRuntimeError.UnexpectedNamedSource,\n                    $\"named source '{extraSource.Name}' is not declared by this mapping\",\n                    detail: extraSource.Name),\n            };\n            parsed.Add(new NamedInput(\n                extraSource.Name,\n                global::Ferrule.Runtime.FerruleJson.ParseEmbeddedBytes(schema, extraSource.Document)));\n        }\n        return parsed;\n",
        );
    }
    output.push_str("    }\n");

    output.push_str(
        "\n    private static JsonExecutionOutputs SerializeJsonOutputs(ExecutionOutputs outputs)\n    {\n        if (outputs.Extras.Count != ExtraTargetJsonSchemas.Length)\n        {\n            throw new global::Ferrule.Runtime.FerruleRuntimeException(\n                global::Ferrule.Runtime.FerruleRuntimeError.JsonBoundary,\n                \"generated mapping returned an unexpected number of named targets\",\n                detail: \"named target count\");\n        }\n        var extras = new global::System.Collections.Generic.List<NamedJsonOutput>(outputs.Extras.Count);\n        for (var index = 0; index < outputs.Extras.Count; index++)\n        {\n            var extra = outputs.Extras[index];\n            extras.Add(new NamedJsonOutput(\n                extra.Name,\n                global::Ferrule.Runtime.FerruleJson.SerializeEmbedded(\n                    ExtraTargetJsonSchemas[index],\n                    extra.Instance)));\n        }\n        return new JsonExecutionOutputs(\n            global::Ferrule.Runtime.FerruleJson.SerializeEmbedded(TargetJsonSchema, outputs.Primary),\n            extras);\n    }\n",
    );
    output.push_str(
        "\n    private static JsonBytesExecutionOutputs SerializeJsonBytesOutputs(ExecutionOutputs outputs)\n    {\n        if (outputs.Extras.Count != ExtraTargetJsonSchemas.Length)\n        {\n            throw new global::Ferrule.Runtime.FerruleRuntimeException(\n                global::Ferrule.Runtime.FerruleRuntimeError.JsonBoundary,\n                \"generated mapping returned an unexpected number of named targets\",\n                detail: \"named target count\");\n        }\n        var extras = new global::System.Collections.Generic.List<NamedJsonBytesOutput>(outputs.Extras.Count);\n        for (var index = 0; index < outputs.Extras.Count; index++)\n        {\n            var extra = outputs.Extras[index];\n            extras.Add(new NamedJsonBytesOutput(\n                extra.Name,\n                global::Ferrule.Runtime.FerruleJson.SerializeEmbeddedBytes(\n                    ExtraTargetJsonSchemas[index],\n                    extra.Instance)));\n        }\n        return new JsonBytesExecutionOutputs(\n            global::Ferrule.Runtime.FerruleJson.SerializeEmbeddedBytes(\n                TargetJsonSchema,\n                outputs.Primary),\n            extras);\n    }\n",
    );
    Ok(())
}

fn render_source_context(program: &Program, output: &mut String) {
    if source_contract::render_context(program, output) {
        return;
    }
    let static_sources = program
        .extra_sources
        .iter()
        .filter(|source| source.dynamic.is_none())
        .collect::<Vec<_>>();
    output.push_str(
        "\n    private static global::Ferrule.Runtime.ScopeContext CreateContext(\n        global::Ferrule.Runtime.FerruleInstance source,\n        global::System.Collections.Generic.IReadOnlyList<NamedInput> extraSources,\n        global::Ferrule.Runtime.FerruleExecutionContext? executionContext,\n        global::Ferrule.Runtime.IFerruleDynamicSourceLoader? dynamicSourceLoader = null)\n    {\n        global::System.ArgumentNullException.ThrowIfNull(source);\n        global::System.ArgumentNullException.ThrowIfNull(extraSources);\n",
    );
    source_contract::input(
        program,
        output,
        "source",
        "extraSources.Count",
        "executionContext",
    );
    if static_sources.is_empty() {
        output.push_str(
            "        foreach (var extraSource in extraSources)\n        {\n            global::System.ArgumentNullException.ThrowIfNull(extraSource);\n            global::System.ArgumentNullException.ThrowIfNull(extraSource.Name);\n            global::System.ArgumentNullException.ThrowIfNull(extraSource.Instance);\n            throw new global::Ferrule.Runtime.FerruleRuntimeException(\n                global::Ferrule.Runtime.FerruleRuntimeError.UnexpectedNamedSource,\n                $\"named source '{extraSource.Name}' is not declared by this mapping\",\n                detail: extraSource.Name);\n        }\n",
        );
    } else {
        output.push_str(
            "        var namedSources = new global::System.Collections.Generic.Dictionary<string, global::Ferrule.Runtime.FerruleInstance>(global::System.StringComparer.Ordinal);\n        foreach (var extraSource in extraSources)\n        {\n            global::System.ArgumentNullException.ThrowIfNull(extraSource);\n            global::System.ArgumentNullException.ThrowIfNull(extraSource.Name);\n            global::System.ArgumentNullException.ThrowIfNull(extraSource.Instance);\n",
        );
        output.push_str("            if (extraSource.Name is not (");
        for (index, source) in static_sources.iter().enumerate() {
            if index != 0 {
                output.push_str(" or ");
            }
            output.push_str(&literal::string(&source.name));
        }
        output.push_str(
            "))\n            {\n                throw new global::Ferrule.Runtime.FerruleRuntimeException(\n                    global::Ferrule.Runtime.FerruleRuntimeError.UnexpectedNamedSource,\n                    $\"named source '{extraSource.Name}' is not declared by this mapping\",\n                    detail: extraSource.Name);\n            }\n            if (!namedSources.TryAdd(extraSource.Name, extraSource.Instance))\n            {\n                throw new global::Ferrule.Runtime.FerruleRuntimeException(\n                    global::Ferrule.Runtime.FerruleRuntimeError.DuplicateNamedSource,\n                    $\"named source '{extraSource.Name}' was supplied more than once\",\n                    detail: extraSource.Name);\n            }\n        }\n",
        );
    }
    for (index, source) in static_sources.iter().enumerate() {
        let name = literal::string(&source.name);
        output.push_str(&format!(
            "        if (!namedSources.TryGetValue({name}, out var namedSource_{index}))\n        {{\n            throw new global::Ferrule.Runtime.FerruleRuntimeException(\n                global::Ferrule.Runtime.FerruleRuntimeError.MissingNamedSource,\n                \"named source \" + {name} + \" is required by this mapping\",\n                detail: {name});\n        }}\n"
        ));
    }
    if static_sources.is_empty() {
        output.push_str(
            "        var context = global::Ferrule.Runtime.ScopeContext.FromSource(source, executionContext);\n        return dynamicSourceLoader is null\n            ? context\n            : context.WithDynamicSourceLoader(dynamicSourceLoader);\n    }\n",
        );
        return;
    }
    output.push_str(
        "        var context = global::Ferrule.Runtime.ScopeContext.FromSources(\n            source,\n            new global::Ferrule.Runtime.FerruleField[]\n            {\n",
    );
    for (index, source) in static_sources.iter().enumerate() {
        output.push_str("                new(");
        output.push_str(&literal::string(&source.name));
        output.push_str(&format!(", namedSource_{index}),\n"));
    }
    output.push_str(
        "            },\n            executionContext);\n        return dynamicSourceLoader is null\n            ? context\n            : context.WithDynamicSourceLoader(dynamicSourceLoader);\n    }\n",
    );
}
