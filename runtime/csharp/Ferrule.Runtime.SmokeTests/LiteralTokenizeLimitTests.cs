using Ferrule.Runtime;

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private static FerruleValue LiteralTokenizeUnicodeInput(int delimiters)
    {
        // One owned UTF-16 string, with a two-scalar literal separator.
        return Text(string.Create(delimiters * 3 + 3, delimiters, static (characters, count) =>
        {
            for (var index = 0; index < count; index++)
            {
                var offset = index * 3;
                characters[offset] = '\uD83D';
                characters[offset + 1] = '\uDE42';
                characters[offset + 2] = '|';
            }
            var end = count * 3;
            characters[end] = 'e'; characters[end + 1] = 'n'; characters[end + 2] = 'd';
        }));
    }

    private static void LiteralTokenizeGeneratedItemCap()
    {
        var maximum = (int)FerruleSequences.MaximumGeneratedSequenceItems;
        {
            var original = FerruleSequences.Tokenize(LiteralTokenizeUnicodeInput(maximum - 1), Text("🙂|"));
            Console.WriteLine($"tokenize exact-cap original count={original.Count} last={original[^1]}");
            Equal(maximum, original.Count);
            for (var index = 0; index < maximum - 1; index++) Equal(Text(string.Empty), original[index]);
            Equal(Text("end"), original[^1]);
        }
        // No second million-item expected collection, forced GC, or RAM claim.
        LiteralTokenizeCapRefusal(LiteralTokenizeUnicodeInput(maximum), Text("🙂|"), (UInt128)maximum + 1);
        LiteralTokenizeCapRefusal(Text(new string('x', maximum * 2)), Text("x"), (UInt128)maximum * 2 + 1);
    }

    private static void LiteralTokenizeCapRefusal(FerruleValue input, FerruleValue delimiter, UInt128 requested)
    {
        FerruleRuntimeException original;
        try
        {
            var values = FerruleSequences.Tokenize(input, delimiter);
            throw new InvalidOperationException($"Over-cap original unexpectedly returned {values.Count} items.");
        }
        catch (FerruleRuntimeException error) { original = error; }
        Console.WriteLine($"tokenize over-cap original: {original}");
        Equal(FerruleRuntimeError.GeneratedSequenceTooLarge, original.Error);
        Equal(requested, original.RequestedItems);
        Equal((UInt128)FerruleSequences.MaximumGeneratedSequenceItems, original.MaximumItems);
        Equal<string?>(null, original.Function);
        Equal<FerruleValueKind?>(null, original.FoundKind);
        Equal<string?>(null, original.Detail);
        Equal($"generate-sequence requested {requested} items; maximum is {FerruleSequences.MaximumGeneratedSequenceItems}", original.Message);
    }

    private static void LiteralTokenizeDelimiterAndParentContexts()
    {
        foreach (var (input, delimiter, expected) in new (string, string, string[])[]
        {
            ("", ",", [""]),
            (",", ",", ["", ""]),
            ("a,,b,", ",", ["a", "", "b", ""]),
            ("🙂|a🙂|🙂|b🙂|", "🙂|", ["", "a", "", "b", ""]),
            ("ababa", "aba", ["", "ba"]),
            ("e\u0301🙂", "e\u0301", ["", "🙂"]),
            ("x", "🙂|longer", ["x"]),
        })
        {
            var original = FerruleSequences.Tokenize(Text(input), Text(delimiter));
            Equal(expected.Length, original.Count);
            for (var index = 0; index < expected.Length; index++) Equal(Text(expected[index]), original[index]);
        }
        foreach (var input in new[] { Text(string.Empty), LiteralTokenizeUnicodeInput((int)FerruleSequences.MaximumGeneratedSequenceItems) })
        {
            var original = Error(FerruleRuntimeError.FunctionInvalidArgument, () => FerruleSequences.Tokenize(input, Text(string.Empty)));
            Equal("tokenize", original.Function); Equal("requires a non-empty delimiter", original.Detail);
        }
        var inputType = Error(FerruleRuntimeError.FunctionType, () => FerruleSequences.Tokenize(FerruleValue.FromInt64(1), Bool(false)));
        Equal("tokenize", inputType.Function); Equal(FerruleValueKind.Int64, inputType.FoundKind);
        var delimiterType = Error(FerruleRuntimeError.FunctionType,
            () => FerruleSequences.Tokenize(LiteralTokenizeUnicodeInput((int)FerruleSequences.MaximumGeneratedSequenceItems), Bool(false)));
        Equal("tokenize", delimiterType.Function); Equal(FerruleValueKind.Bool, delimiterType.FoundKind);

        var parent = ScopeContext.FromSource(Group(Field("Parent", Scalar(Text("outer")))));
        var values = FerruleSequences.Tokenize(Text("🙂|a🙂|"), Text("🙂|"));
        var contexts = parent.IterateGenerated(values);
        Equal(3, contexts.Count);
        for (var index = 0; index < contexts.Count; index++)
        {
            Equal(values[index], contexts[index].ResolveScalar());
            Equal(Text("outer"), contexts[index].ResolveScalar("Parent"));
            Equal((long)index + 1, contexts[index].Position());
        }
    }
}
