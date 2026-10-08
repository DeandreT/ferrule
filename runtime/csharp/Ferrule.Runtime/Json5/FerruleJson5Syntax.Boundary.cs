namespace Ferrule.Runtime;

public static partial class FerruleJson5Syntax
{
    // Optional entry points split the existing encoding census from grammar so
    // both descriptors are checked after encoding and before any syntax work.
    internal static long CountBoundaryBytes(string source) => CountOriginalBytes(source);

    internal static void ValidateBoundaryText(string source)
    {
        var bytes = CountOriginalBytes(source);
        if (bytes > MaximumOriginalBytes)
        {
            throw FerruleJson5SyntaxException.Limit(
                FerruleJson5SyntaxResource.OriginalDocumentBytes, 0, bytes, MaximumOriginalBytes);
        }
    }

    internal static string NormalizeBoundaryText(string source) =>
        new Parser(source, MaximumNormalizedBytes, MaximumContainerDepth, MaximumSyntaxWork).Run();
}
