namespace Ferrule.Runtime;

/// <summary>Constructs portable, mapping-produced document-set members.</summary>
public static class FerruleDynamicDocuments
{
    public static FerruleDocumentSet Collect(IEnumerable<FerruleInstance> collections)
    {
        ArgumentNullException.ThrowIfNull(collections);
        var documents = new List<FerruleDocument>();
        foreach (var collection in collections)
        {
            ArgumentNullException.ThrowIfNull(collection);
            if (collection is not FerruleDocumentSet set)
            {
                var found = collection switch
                {
                    FerruleScalar => "a scalar",
                    FerruleGroup => "a group",
                    FerruleRepeated => "a repeated value",
                    FerruleMappedSequence => "a mapped sequence",
                    _ => "an unknown value",
                };
                throw new FerruleRuntimeException(
                    FerruleRuntimeError.InvalidDocumentCollectionItem,
                    $"document collection requires document sets, found {found}",
                    foundInstance: found);
            }
            documents.AddRange(set.Documents);
        }
        return new FerruleDocumentSet(documents);
    }

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
