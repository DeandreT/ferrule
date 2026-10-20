//! Paired Basic ItemAt over an explicitly owned recursive collection walk.

use ::mapping::NodeId;
use codegen::GeneratedSequence;

/// Paired Basic owned recursive walk: prefix, then separator, then the
/// exact-owner root selection. Owned access failures carry the consumer node.
pub(super) fn render_values(
    node: NodeId,
    identifier: &str,
    sequence: &GeneratedSequence,
    output: &mut String,
) {
    let GeneratedSequence::OwnedRecursiveCollect {
        collection,
        children,
        descent_value,
        values,
        value,
        prefix,
        separator,
        ..
    } = sequence
    else {
        return;
    };
    output.push_str(&format!(
        "        global::System.Collections.Generic.IReadOnlyList<global::Ferrule.Runtime.FerruleValue> sequence_values_{identifier};\n        var sequence_prefix_{identifier} = global::Ferrule.Runtime.FerruleSequences.RecursiveCollectArgumentText(Node_{prefix}(context));\n        var sequence_separator_{identifier} = global::Ferrule.Runtime.FerruleSequences.RecursiveCollectArgumentText(Node_{separator}(context));\n        try\n        {{\n            sequence_values_{identifier} = global::Ferrule.Runtime.FerruleSequences.RecursiveCollectOwned(\n                context,\n                "
    ));
    crate::owned_basic::render_owner(&collection.owner, output);
    for path in [&collection.path, children, descent_value, values, value] {
        output.push_str(",\n                ");
        super::render_path(path, output);
    }
    output.push_str(&format!(
        ",\n                sequence_prefix_{identifier},\n                sequence_separator_{identifier});\n        }}\n        catch (global::Ferrule.Runtime.FerruleRuntimeException error)\n        {{\n            error.WithOwnedSourceNode({node}U);\n            throw;\n        }}\n"
    ));
}
