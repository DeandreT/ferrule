use std::collections::BTreeMap;

use codegen::{Binding, Expression, Program, TargetScope};
use ir::ScalarType;

use crate::{EmitError, literal};

struct ScopePlan<'a> {
    repeating: bool,
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
        if expressions.insert(node.id, &node.expression).is_some() {
            return Err(EmitError::DuplicateNode { node: node.id });
        }
        if let Expression::Const { value } = &node.expression {
            literal::value(node.id, value)?;
        }
    }

    let mut scopes = Vec::new();
    add_scope(&program.root, &expressions, &mut scopes)?;

    let mut output =
        String::from("namespace Ferrule.Generated;\n\npublic static class GeneratedMapping\n{\n");
    output.push_str(
        "    public static global::Ferrule.Runtime.FerruleInstance Execute(\n        global::Ferrule.Runtime.FerruleInstance source)\n    {\n        global::System.ArgumentNullException.ThrowIfNull(source);\n        return Scope_0(source);\n    }\n",
    );

    for (node, expression) in expressions {
        output.push('\n');
        output.push_str(&format!(
            "    private static global::Ferrule.Runtime.FerruleValue Node_{node}(\n        global::Ferrule.Runtime.FerruleInstance source) =>\n        ",
        ));
        match expression {
            Expression::SourceField { path } => {
                output.push_str("global::Ferrule.Runtime.ScalarPathResolver.Resolve(source, ");
                render_path(path, &mut output);
                output.push_str(");\n");
            }
            Expression::Const { value } => {
                output.push_str(&literal::value(node, value)?);
                output.push_str(";\n");
            }
        }
    }

    for (scope_index, scope) in scopes.iter().enumerate() {
        output.push('\n');
        output.push_str(&format!(
            "    private static global::Ferrule.Runtime.FerruleInstance Scope_{scope_index}(\n        global::Ferrule.Runtime.FerruleInstance source)\n    {{\n"
        ));
        for (binding_index, expression) in scope.evaluations.iter().enumerate() {
            output.push_str(&format!(
                "        var value_{scope_index}_{binding_index} = Node_{expression}(source);\n"
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
            output.push_str(&format!(", Scope_{child_index}(source)),\n"));
        }
        output.push_str("        });\n");
        if scope.repeating {
            output.push_str(&format!(
                "        return new global::Ferrule.Runtime.FerruleRepeated(new global::Ferrule.Runtime.FerruleInstance[] {{ group_{scope_index} }});\n"
            ));
        } else {
            output.push_str(&format!("        return group_{scope_index};\n"));
        }
        output.push_str("    }\n");
    }
    output.push_str("}\n");
    Ok(output)
}

fn add_scope<'a>(
    scope: &'a TargetScope,
    expressions: &BTreeMap<u32, &'a Expression>,
    scopes: &mut Vec<ScopePlan<'a>>,
) -> Result<usize, EmitError> {
    let scope_index = scopes.len();
    scopes.push(ScopePlan {
        repeating: false,
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
        if !expressions.contains_key(&binding.expression) {
            return Err(EmitError::MissingExpression {
                node: binding.expression,
            });
        }
        if let Some(&existing) = first_binding.get(binding.target_field.as_str()) {
            let plan = &mut bindings[existing];
            if !plan.repeating || !binding.repeating || plan.target_type != binding.target_type {
                return Err(EmitError::InvalidDuplicateBinding {
                    scope: scope_index,
                    binding: binding_index,
                });
            }
            plan.values.push(binding_index);
        } else {
            first_binding.insert(binding.target_field.as_str(), bindings.len());
            bindings.push(binding_plan(binding, binding_index));
        }
    }

    let mut children = Vec::with_capacity(scope.children.len());
    for child in &scope.children {
        let child_index = add_scope(child, expressions, scopes)?;
        children.push((child.target_field.as_str(), child_index));
    }
    scopes[scope_index] = ScopePlan {
        repeating: scope.repeating,
        evaluations,
        bindings,
        children,
    };
    Ok(scope_index)
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
