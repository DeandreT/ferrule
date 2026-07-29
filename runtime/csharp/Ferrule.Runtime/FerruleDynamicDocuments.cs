namespace Ferrule.Runtime;

/// <summary>Constructs portable, mapping-produced document-set members.</summary>
public static class FerruleDynamicDocuments
{
    public static FerruleDocument Create(
        uint node,
        FerruleValue path,
        FerruleInstance value)
    {
        ArgumentNullException.ThrowIfNull(value);
        if (path.Kind != FerruleValueKind.String)
        {
            throw new FerruleRuntimeException(
                FerruleRuntimeError.DynamicTargetPath,
                $"Node {node}: dynamic target path expected a string, got {path.Kind}.",
                node: node,
                foundKind: path.Kind);
        }

        if (string.IsNullOrWhiteSpace(path.StringValue))
        {
            throw new FerruleRuntimeException(
                FerruleRuntimeError.EmptyDynamicTargetPath,
                $"Node {node}: dynamic target path must not be empty.",
                node: node);
        }

        try
        {
            return new FerruleDocument(path.StringValue, value);
        }
        catch (FerruleRuntimeException error)
            when (error.Error is FerruleRuntimeError.InvalidDocumentPath
                or FerruleRuntimeError.NestedDocumentSet)
        {
            throw new FerruleRuntimeException(
                FerruleRuntimeError.EmptyDynamicTargetPath,
                $"Node {node}: dynamic target path must not be empty.",
                error,
                node: node);
        }
    }
}
