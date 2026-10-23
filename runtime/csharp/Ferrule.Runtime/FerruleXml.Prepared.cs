using System.Runtime.CompilerServices;
using System.Text.Json;

namespace Ferrule.Runtime;

public static partial class FerruleXml
{
    // Generated mappings pass their schema descriptors as string constants,
    // which the runtime interns, so each descriptor is prepared once per role
    // and process and shared by repeated and concurrent calls. The tables
    // compare keys by reference and do not keep them alive: a descriptor built
    // at run time is prepared once per string object and released with it.
    //
    // Prepared schemas are never modified after parsing. Every call keeps its
    // own input byte check, parser and projection budgets, writer and output
    // byte check. Only successful preparations are retained; a rejected
    // descriptor is parsed again by every call and throws its original error
    // at its original point.
    private static readonly ConditionalWeakTable<string, XmlSchemaNode> PreparedFlatInputSchemas = new();
    private static readonly ConditionalWeakTable<string, StructuredSchema> PreparedStructuredSchemas = new();
    private static readonly ConditionalWeakTable<string, PreparedDocumentSchema> PreparedDocumentSchemas = new();
    private static readonly ConditionalWeakTable<string, PreparedNodeSchema> PreparedNodeSchemas = new();

    /// <summary>A document output schema with its hint-independent checks already passed.</summary>
    private sealed record PreparedDocumentSchema(string Payload, XmlSchemaNode Schema, bool HasSupplementaryName);

    /// <summary>An expression-node serializer schema and the payload it was parsed from.</summary>
    private sealed record PreparedNodeSchema(string Payload, XmlSchemaNode Schema);

    private static T Prepared<T>(ConditionalWeakTable<string, T> table, string descriptor, Func<string, T> prepare)
        where T : class
    {
        if (descriptor is null) return prepare(descriptor!);
        if (table.TryGetValue(descriptor, out var prepared)) return prepared;
        var parsed = prepare(descriptor);
        // A concurrent first call may have prepared the same descriptor; keep
        // the first published value.
        return table.GetValue(descriptor, _ => parsed);
    }

    private static PreparedDocumentSchema DocumentSchema(string descriptor) =>
        Prepared(PreparedDocumentSchemas, descriptor, descriptor =>
        {
            var payload = FerruleEmbeddedSchema.Unwrap(descriptor, MaximumEmbeddedSchemaBytes).Payload;
            var schema = ParseSerializerSchema(payload, documentBoundary: true);
            RequireDocumentNamespaceMetadata(schema, root: true);
            return new PreparedDocumentSchema(payload, schema, HasSupplementaryDocumentName(schema));
        });

    private static XmlSchemaNode ParseSerializerSchema(string schemaJson, bool documentBoundary)
    {
        using var document = JsonDocument.Parse(
            schemaJson,
            new JsonDocumentOptions { MaxDepth = MaximumSchemaDepth });
        return XmlSchemaNode.Parse(document.RootElement, 0, documentBoundary);
    }
}
