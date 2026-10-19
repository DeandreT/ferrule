using System.Diagnostics.CodeAnalysis;

namespace Ferrule.Runtime;

/// <summary>Strict scalar lookup for statically scoped Ferrule instance trees.</summary>
public static class ScalarPathResolver
{
    public static FerruleValue Resolve(FerruleInstance root, params string[] path) =>
        Resolve(root, (IReadOnlyList<string>)path);

    public static FerruleValue Resolve(FerruleInstance root, IReadOnlyList<string> path)
    {
        ArgumentNullException.ThrowIfNull(root);
        ArgumentNullException.ThrowIfNull(path);

        FerruleInstance current = root;
        for (var index = 0; index < path.Count; index++)
        {
            var segment = path[index];
            if (current is FerruleRepeated repeated)
            {
                if (repeated.Items.Count == 0)
                {
                    return FerruleValue.Null;
                }

                current = repeated.Items[0];
            }

            if (!TryGetField(current, segment, out var next))
            {
                if (current is FerruleScalar)
                {
                    throw new FerruleRuntimeException(
                        FerruleRuntimeError.MissingSourceField,
                        $"Cannot traverse field '{segment}' through a scalar at '{DisplayPrefix(path, index)}'.");
                }

                throw new FerruleRuntimeException(
                    FerruleRuntimeError.MissingSourceField,
                    $"Field '{segment}' does not exist at '{DisplayPrefix(path, index)}'.");
            }

            current = next;
        }

        if (current is FerruleRepeated terminalRepeated)
        {
            if (terminalRepeated.Items.Count == 0)
            {
                return FerruleValue.Null;
            }

            current = terminalRepeated.Items[0];
        }

        if (current is FerruleScalar scalar)
        {
            return scalar.Value;
        }

        throw new FerruleRuntimeException(
            FerruleRuntimeError.MissingSourceField,
            $"Scalar path '{DisplayPrefix(path, path.Count)}' ends at {KindName(current)}.");
    }

    private static bool TryGetField(
        FerruleInstance current,
        string name,
        [NotNullWhen(true)] out FerruleInstance? value)
    {
        switch (current)
        {
            case FerruleGroup group:
                return group.TryGetField(name, out value);
            case FerruleDocumentSet { Documents.Count: > 0 } documents
                when documents.Documents[0].Value is FerruleGroup document:
                return document.TryGetField(name, out value);
            default:
                value = null;
                return false;
        }
    }

    private static string DisplayPrefix(IReadOnlyList<string> path, int count) =>
        count == 0 ? "<root>" : string.Join('/', path.Take(count));

    private static string KindName(FerruleInstance instance) => instance switch
    {
        FerruleScalar => "a scalar",
        FerruleGroup => "a group",
        FerruleRepeated => "a repeated value",
        FerruleMappedSequence => "a mapped sequence",
        FerruleDocumentSet => "a document set",
        _ => "an unknown instance",
    };
}
