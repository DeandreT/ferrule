using System.Text;

namespace Ferrule.Runtime;

/// <summary>Explicit singular closed-object JSON5 input and strict JSON output.</summary>
public static class FerruleJson5
{
    private static readonly UTF8Encoding StrictUtf8 = new(false, true);

    public static string Execute(
        string sourceDescriptor, string targetDescriptor, string source,
        Func<FerruleInstance, FerruleInstance> mapping)
    {
        CheckArguments(sourceDescriptor, targetDescriptor, source, mapping);
        try { FerruleJson5Syntax.ValidateBoundaryText(source); }
        catch (FerruleJson5SyntaxException error)
        { throw new FerruleJson5BoundaryException(FerruleJson5Stage.Encoding, error); }
        return FerruleJson.ExecuteJson5Accepted(sourceDescriptor, targetDescriptor, source, mapping);
    }

    public static byte[] ExecuteBytes(
        string sourceDescriptor, string targetDescriptor, byte[] source,
        Func<FerruleInstance, FerruleInstance> mapping)
    {
        CheckArguments(sourceDescriptor, targetDescriptor, source, mapping);
        if (source.LongLength > FerruleJson5Syntax.MaximumOriginalBytes)
        {
            throw new FerruleJson5BoundaryException(FerruleJson5Stage.Encoding,
                new FerruleJson5ResourceException(FerruleJson5Resource.OriginalDocumentBytes,
                    source.LongLength, FerruleJson5Syntax.MaximumOriginalBytes));
        }
        string decoded;
        try { decoded = StrictUtf8.GetString(source); }
        catch (DecoderFallbackException error)
        { throw new FerruleJson5BoundaryException(FerruleJson5Stage.Encoding, error); }
        // Valid decoded text has the same UTF-8 size. BOM remains input trivia.
        var output = FerruleJson.ExecuteJson5Accepted(sourceDescriptor, targetDescriptor, decoded, mapping);
        return StrictUtf8.GetBytes(output);
    }

    private static void CheckArguments(
        string sourceDescriptor, string targetDescriptor, object source,
        Func<FerruleInstance, FerruleInstance> mapping)
    {
        ArgumentNullException.ThrowIfNull(sourceDescriptor);
        ArgumentNullException.ThrowIfNull(targetDescriptor);
        ArgumentNullException.ThrowIfNull(source);
        ArgumentNullException.ThrowIfNull(mapping);
    }
}
