using System.Runtime.CompilerServices;

namespace Ferrule.Runtime;

public static partial class FerruleJson
{
    // Generated mappings pass their schema descriptors as string constants,
    // which the runtime interns, so each descriptor object is prepared once per
    // process and shared by repeated and concurrent calls. The tables compare
    // keys by reference and do not keep them alive: a descriptor built at run
    // time is prepared once per string object and released with it.
    //
    // A prepared schema graph is never modified after BindRecursiveReferences.
    // Every call keeps its own NodeBudget, so depth, recursion and pattern work
    // are accounted per document. Only successful preparations are retained; a
    // rejected schema is parsed again by every call and throws its original
    // error at its original point.
    private static readonly ConditionalWeakTable<string, JsonSchemaNode> PreparedPlainSchemas = new();
    private static readonly ConditionalWeakTable<string, JsonSchemaNode> PreparedEmbeddedSchemas = new();

    private static JsonSchemaNode PreparedSchema(string schemaJson, bool embedded)
    {
        var prepared = embedded ? PreparedEmbeddedSchemas : PreparedPlainSchemas;
        if (prepared.TryGetValue(schemaJson, out var schema))
        {
            return schema;
        }

        var parsed = ParseSchema(schemaJson, embedded);
        // A concurrent first call may have prepared the same descriptor; keep
        // the first published graph.
        return prepared.GetValue(schemaJson, _ => parsed);
    }
}
