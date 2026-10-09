use codegen::{GeneratedFilterMapV1, Program, UserFunctionProgram};

use super::scalar_type_name;

pub(super) fn render_values(
    identifier: &str,
    composition: &GeneratedFilterMapV1,
    output: &mut String,
) {
    output.push_str(&format!(
        "        sequence_values_{identifier} = global::Ferrule.Runtime.FerruleFilterMap.Evaluate(\n            context, {}U,\n            ",
        composition.item,
    ));
    if let Some(from) = composition.source.from {
        output.push_str(&format!(
            "new({from}U, sourceContext => Node_{from}(sourceContext))"
        ));
    } else {
        output.push_str("null");
    }
    output.push_str(&format!(
        ",\n            new({}U, sourceContext => Node_{}(sourceContext)),\n            new global::Ferrule.Runtime.FerruleFilterMapCapture[]\n            {{\n",
        composition.source.to, composition.source.to,
    ));
    for capture in &composition.captures {
        output.push_str(&format!(
            "                new({}U, global::Ferrule.Runtime.FerruleScalarType.{}, captureContext => Node_{}(captureContext)),\n",
            capture.node, scalar_type_name(capture.ty), capture.node,
        ));
    }
    output.push_str(&format!(
        "            }},\n            FilterMapStage_{}, FilterMapStage_{},\n            global::Ferrule.Runtime.FerruleScalarType.{});\n",
        composition.predicate.get(), composition.mapper.get(), scalar_type_name(composition.output_type),
    ));
}

pub(super) fn render_stage_entry(function: &UserFunctionProgram, output: &mut String) {
    let id = function.id.get();
    output.push_str(&format!(
        "\n    private static readonly global::Ferrule.Runtime.FerruleFilterMapStage FilterMapStage_{id} = new(\n        {id}UL, {}U, InvokeFilterMapStage_{id});\n\n    private static global::Ferrule.Runtime.FerruleValue InvokeFilterMapStage_{id}(\n        global::Ferrule.Runtime.ScopeContext context, global::Ferrule.Runtime.FerruleValue[] arguments)\n    {{\n",
        function.output,
    ));
    // Runtime stage dispatch has charged/checkered call entry. The complete
    // supplied argument vector exists before ordinary ordered adaptations.
    for (index, parameter) in function.parameters.iter().enumerate() {
        output.push_str(&format!(
            "        var argument_{index} = global::Ferrule.Runtime.FerruleUserFunctions.Adapt(\n            arguments[{index}], global::Ferrule.Runtime.FerruleScalarType.{}, {id}UL, {}UL);\n",
            scalar_type_name(parameter.ty), parameter.id.get(),
        ));
    }
    output.push_str(&format!(
        "        return UserFunction_{id}(context, new global::Ferrule.Runtime.FerruleValue[] {{ "
    ));
    for index in 0..function.parameters.len() {
        if index != 0 {
            output.push_str(", ");
        }
        output.push_str(&format!("argument_{index}"));
    }
    output.push_str(" });\n    }\n");
}

pub(super) fn add_scoped_node_wrappers(program: &Program, output: &mut String) {
    if program.filter_map_v1_sequences().is_empty() {
        return;
    }
    // Replace declarations only. The required literal newline cannot occur
    // inside escaped project string literals. Dependencies keep calling Node_.
    let declaration = "\n    private static global::Ferrule.Runtime.FerruleValue Node_";
    *output = output.replace(
        declaration,
        "\n    private static global::Ferrule.Runtime.FerruleValue NodeCore_",
    );
    for node in &program.expressions {
        output.push_str(&format!(
            "\n    private static global::Ferrule.Runtime.FerruleValue Node_{}(global::Ferrule.Runtime.ScopeContext context) =>\n        context.EvaluateFilterMapNode({}U, null, () => NodeCore_{}(context));\n",
            node.id, node.id, node.id,
        ));
    }
    for function in &program.user_functions {
        let id = function.id.get();
        let declaration = format!(
            "\n    private static global::Ferrule.Runtime.FerruleValue UserFunction_{id}_Node_"
        );
        let replacement = format!(
            "\n    private static global::Ferrule.Runtime.FerruleValue UserFunctionCore_{id}_Node_"
        );
        *output = output.replace(&declaration, &replacement);
        for node in &function.expressions {
            output.push_str(&format!(
                "\n    private static global::Ferrule.Runtime.FerruleValue UserFunction_{id}_Node_{}(\n        global::Ferrule.Runtime.ScopeContext context, global::System.Collections.Generic.IReadOnlyList<global::Ferrule.Runtime.FerruleValue> parameters) =>\n        context.EvaluateFilterMapNode({}U, {id}UL, () => UserFunctionCore_{id}_Node_{}(context, parameters));\n",
                node.id, node.id, node.id,
            ));
        }
    }
}
