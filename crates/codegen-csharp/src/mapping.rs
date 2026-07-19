use std::collections::BTreeMap;

use codegen::{Binding, Expression, Program, TargetScope};
use ir::ScalarType;

use crate::{EmitError, literal};

struct ScopePlan<'a> {
    repeating: bool,
    iteration: Option<&'a [String]>,
    filter: Option<u32>,
    evaluations: Vec<u32>,
    bindings: Vec<BindingPlan<'a>>,
    children: Vec<(&'a str, usize)>,
}

struct BindingPlan<'a> {
    target_field: &'a str,
    target_type: ScalarType,
    repeating: bool,
    values: Vec<usize>,
}

pub(crate) fn render(program: &Program) -> Result<String, EmitError> {
    let mut expressions = BTreeMap::new();
    for node in &program.expressions {
        expressions.insert(node.id, &node.expression);
    }

    let mut scopes = Vec::new();
    add_scope(&program.root, &mut scopes);

    let mut output =
        String::from("namespace Ferrule.Generated;\n\npublic static class GeneratedMapping\n{\n");
    output.push_str(
        "    public static global::Ferrule.Runtime.FerruleInstance Execute(\n        global::Ferrule.Runtime.FerruleInstance source)\n    {\n        global::System.ArgumentNullException.ThrowIfNull(source);\n        return Scope_0(global::Ferrule.Runtime.ScopeContext.FromSource(source));\n    }\n",
    );

    for (node, expression) in expressions {
        output.push('\n');
        output.push_str(&format!(
            "    private static global::Ferrule.Runtime.FerruleValue Node_{node}(\n        global::Ferrule.Runtime.ScopeContext context)",
        ));
        match expression {
            Expression::SourceField { frame, path } => {
                output.push_str(" =>\n        ");
                if let Some(frame) = frame {
                    output.push_str("context.ResolveScalarInFrame(");
                    render_path(frame, &mut output);
                    output.push_str(", ");
                    render_path(path, &mut output);
                } else {
                    output.push_str("context.ResolveScalar(");
                    render_path(path, &mut output);
                }
                output.push_str(");\n");
            }
            Expression::Position { collection } => {
                output.push_str(
                    " =>\n        global::Ferrule.Runtime.FerruleValue.FromInt64(context.Position(",
                );
                render_path(collection, &mut output);
                output.push_str("));\n");
            }
            Expression::Const { value } => {
                output.push_str(" =>\n        ");
                output.push_str(&literal::value(node, value)?);
                output.push_str(";\n");
            }
            Expression::Call { function, args } => {
                output.push_str(" =>\n        global::Ferrule.Runtime.FerruleFunctions.Call(");
                output.push_str(&literal::string(function.as_str()));
                output.push_str(", new global::Ferrule.Runtime.FerruleValue[] { ");
                for (index, argument) in args.iter().enumerate() {
                    if index != 0 {
                        output.push_str(", ");
                    }
                    output.push_str(&format!("Node_{argument}(context)"));
                }
                output.push_str(" });\n");
            }
            Expression::If {
                condition,
                then,
                else_,
            } => {
                output.push_str("\n    {\n");
                output.push_str(&format!(
                    "        var condition_{node} = Node_{condition}(context);\n"
                ));
                output.push_str(&format!(
                    "        if (global::Ferrule.Runtime.FerruleFunctions.RequireBoolean(condition_{node}, {condition}U))\n        {{\n            return Node_{then}(context);\n        }}\n"
                ));
                output.push_str(&format!("        return Node_{else_}(context);\n    }}\n"));
            }
        }
    }

    for (scope_index, scope) in scopes.iter().enumerate() {
        output.push('\n');
        output.push_str(&format!(
            "    private static global::Ferrule.Runtime.FerruleInstance Scope_{scope_index}(\n        global::Ferrule.Runtime.ScopeContext context)\n    {{\n"
        ));
        if let Some(path) = scope.iteration {
            output.push_str(&format!(
                "        var items_{scope_index} = new global::System.Collections.Generic.List<global::Ferrule.Runtime.FerruleInstance>();\n        foreach (var item_context_{scope_index} in context.IterateSource("
            ));
            render_path(path, &mut output);
            output.push_str("))\n        {\n");
            if let Some(filter) = scope.filter {
                output.push_str(&format!(
                    "            var filter_{scope_index} = Node_{filter}(item_context_{scope_index});\n            if (!global::Ferrule.Runtime.FerruleFunctions.RequireBoolean(filter_{scope_index}, {filter}U))\n            {{\n                continue;\n            }}\n            var output_context_{scope_index} = item_context_{scope_index}.WithCompactedPosition(items_{scope_index}.Count + 1);\n            items_{scope_index}.Add(ScopeItem_{scope_index}(output_context_{scope_index}));\n"
                ));
            } else {
                output.push_str(&format!(
                    "            items_{scope_index}.Add(ScopeItem_{scope_index}(item_context_{scope_index}));\n"
                ));
            }
            output.push_str(&format!(
                "        }}\n        return new global::Ferrule.Runtime.FerruleRepeated(items_{scope_index});\n"
            ));
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
            "    private static global::Ferrule.Runtime.FerruleGroup ScopeItem_{scope_index}(\n        global::Ferrule.Runtime.ScopeContext context)\n    {{\n"
        ));
        for (binding_index, expression) in scope.evaluations.iter().enumerate() {
            output.push_str(&format!(
                "        var value_{scope_index}_{binding_index} = Node_{expression}(context);\n"
            ));
        }
        output.push_str(
            &format!(
                "        var group_{scope_index} = new global::Ferrule.Runtime.FerruleGroup(new global::Ferrule.Runtime.FerruleField[]\n        {{\n"
            ),
        );
        for binding in &scope.bindings {
            output.push_str("            new global::Ferrule.Runtime.FerruleField(");
            output.push_str(&literal::string(binding.target_field));
            output.push_str(", ");
            if binding.repeating {
                output.push_str(
                    "TargetBuilder.RepeatedScalar(new global::Ferrule.Runtime.FerruleValue[] { ",
                );
                for (index, binding_index) in binding.values.iter().enumerate() {
                    if index != 0 {
                        output.push_str(", ");
                    }
                    output.push_str(&format!("value_{scope_index}_{binding_index}"));
                }
                output.push_str(" }, ");
            } else {
                output.push_str(&format!(
                    "TargetBuilder.Scalar(value_{scope_index}_{}, ",
                    binding.values[0]
                ));
            }
            output.push_str(target_type(binding.target_type));
            output.push_str(")),\n");
        }
        for (target_field, child_index) in &scope.children {
            output.push_str("            new global::Ferrule.Runtime.FerruleField(");
            output.push_str(&literal::string(target_field));
            output.push_str(&format!(", Scope_{child_index}(context)),\n"));
        }
        output.push_str("        });\n");
        output.push_str(&format!("        return group_{scope_index};\n"));
        output.push_str("    }\n");
    }
    output.push_str("}\n");
    Ok(output)
}

fn add_scope<'a>(scope: &'a TargetScope, scopes: &mut Vec<ScopePlan<'a>>) -> usize {
    let scope_index = scopes.len();
    scopes.push(ScopePlan {
        repeating: false,
        iteration: None,
        filter: None,
        evaluations: Vec::new(),
        bindings: Vec::new(),
        children: Vec::new(),
    });

    let mut bindings = Vec::<BindingPlan<'a>>::new();
    let evaluations = scope
        .bindings
        .iter()
        .map(|binding| binding.expression)
        .collect();
    let mut first_binding = BTreeMap::<&str, usize>::new();
    for (binding_index, binding) in scope.bindings.iter().enumerate() {
        if let Some(&existing) = first_binding.get(binding.target_field.as_str()) {
            bindings[existing].values.push(binding_index);
        } else {
            first_binding.insert(binding.target_field.as_str(), bindings.len());
            bindings.push(binding_plan(binding, binding_index));
        }
    }

    let mut children = Vec::with_capacity(scope.children.len());
    for child in &scope.children {
        let child_index = add_scope(child, scopes);
        children.push((child.target_field.as_str(), child_index));
    }
    scopes[scope_index] = ScopePlan {
        repeating: scope.repeating,
        iteration: scope.iteration.as_ref().map(codegen::SourceIteration::path),
        filter: scope.filter,
        evaluations,
        bindings,
        children,
    };
    scope_index
}

fn binding_plan(binding: &Binding, binding_index: usize) -> BindingPlan<'_> {
    BindingPlan {
        target_field: &binding.target_field,
        target_type: binding.target_type,
        repeating: binding.repeating,
        values: vec![binding_index],
    }
}

fn render_path(path: &[String], output: &mut String) {
    if path.is_empty() {
        output.push_str("global::System.Array.Empty<string>()");
        return;
    }
    output.push_str("new string[] { ");
    for (index, segment) in path.iter().enumerate() {
        if index != 0 {
            output.push_str(", ");
        }
        output.push_str(&literal::string(segment));
    }
    output.push_str(" }");
}

fn target_type(target_type: ScalarType) -> &'static str {
    match target_type {
        ScalarType::String => "TargetScalarType.String",
        ScalarType::Int => "TargetScalarType.Int64",
        ScalarType::Float => "TargetScalarType.Double",
        ScalarType::Bool => "TargetScalarType.Bool",
    }
}
