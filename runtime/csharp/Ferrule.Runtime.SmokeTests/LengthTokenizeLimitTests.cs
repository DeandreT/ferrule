using Ferrule.Runtime;

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private static FerruleValue LengthTokenizeUnicodeInput(int ascii)
    {
        // One owned string; the final surrogate pair is one Unicode scalar.
        return Text(string.Create(ascii + 2, ascii, static (characters, count) =>
        {
            characters[..count].Fill('x');
            characters[count] = '\uD83D';
            characters[count + 1] = '\uDE42';
        }));
    }

    private static void LengthTokenizeGeneratedItemCap()
    {
        var maximum = (int)FerruleSequences.MaximumGeneratedSequenceItems;
        {
            var original = FerruleSequences.TokenizeByLength(
                LengthTokenizeUnicodeInput(maximum - 1), FerruleValue.FromInt64(1));
            Console.WriteLine($"tokenize-by-length exact-cap original count={original.Count} last={original[^1]}");
            Equal(maximum, original.Count);
            for (var index = 0; index < maximum - 1; index++) Equal(Text("x"), original[index]);
            Equal(Text("🙂"), original[^1]);
        }
        // Refusal cases reuse only an owned input string, without a second
        // million-item expected collection. No GC or RAM release is asserted.
        LengthTokenizeCapRefusal(LengthTokenizeUnicodeInput(maximum), FerruleValue.FromInt64(1));
        var overTwo = LengthTokenizeUnicodeInput(maximum * 2);
        foreach (var length in new[] { FerruleValue.FromInt64(2), FerruleValue.FromDouble(2.9), Text(" \u2003+2\u0085 ") })
            LengthTokenizeCapRefusal(overTwo, length);
    }

    private static void LengthTokenizeCapRefusal(FerruleValue input, FerruleValue length)
    {
        FerruleRuntimeException original;
        try
        {
            var values = FerruleSequences.TokenizeByLength(input, length);
            throw new InvalidOperationException($"Over-cap original unexpectedly returned {values.Count} items.");
        }
        catch (FerruleRuntimeException error) { original = error; }
        Console.WriteLine($"tokenize-by-length over-cap original: {original}");
        Equal(FerruleRuntimeError.GeneratedSequenceTooLarge, original.Error);
        Equal((UInt128)FerruleSequences.MaximumGeneratedSequenceItems + 1, original.RequestedItems);
        Equal((UInt128)FerruleSequences.MaximumGeneratedSequenceItems, original.MaximumItems);
        Equal<string?>(null, original.Function);
        Equal<FerruleValueKind?>(null, original.FoundKind);
        Equal<string?>(null, original.Detail);
        Equal($"generate-sequence requested {FerruleSequences.MaximumGeneratedSequenceItems + 1} items; maximum is {FerruleSequences.MaximumGeneratedSequenceItems}", original.Message);
    }

    private static void LengthTokenizeCoercionAndUnicode()
    {
        const string text = "e\u0301🙂z!";
        foreach (var length in new[] { FerruleValue.FromInt64(2), FerruleValue.FromDouble(2.9), Text(" \u2003+2\u0085 ") })
        {
            var original = FerruleSequences.TokenizeByLength(Text(text), length);
            Equal(3, original.Count);
            Equal(Text("e\u0301"), original[0]); Equal(Text("🙂z"), original[1]); Equal(Text("!"), original[2]);
        }
        foreach (var length in new[] { FerruleValue.FromInt64(long.MaxValue), FerruleValue.FromDouble(double.MaxValue), Text(long.MaxValue.ToString(System.Globalization.CultureInfo.InvariantCulture)) })
        {
            var original = FerruleSequences.TokenizeByLength(Text(text), length);
            Equal(1, original.Count); Equal(Text(text), original[0]);
        }
        Equal(0, FerruleSequences.TokenizeByLength(Text(string.Empty), FerruleValue.FromInt64(1)).Count);
        foreach (var length in new[]
        {
            FerruleValue.FromInt64(0), FerruleValue.FromInt64(-1), FerruleValue.FromDouble(0.9),
            FerruleValue.FromDouble(double.NaN), FerruleValue.FromDouble(double.PositiveInfinity), Text("2.0"),
            Bool(true), FerruleValue.Null, FerruleValue.JsonNull, FerruleValue.XmlNil,
        })
        {
            var original = Error(FerruleRuntimeError.FunctionInvalidArgument,
                () => FerruleSequences.TokenizeByLength(Text(string.Empty), length));
            Equal("tokenize-by-length", original.Function);
            Equal("requires a positive integer length", original.Detail);
        }
        var wrongType = Error(FerruleRuntimeError.FunctionType,
            () => FerruleSequences.TokenizeByLength(Bool(true), FerruleValue.FromInt64(0)));
        Equal("tokenize-by-length", wrongType.Function); Equal(FerruleValueKind.Bool, wrongType.FoundKind);
        var badWidth = Error(FerruleRuntimeError.FunctionInvalidArgument,
            () => FerruleSequences.TokenizeByLength(LengthTokenizeUnicodeInput((int)FerruleSequences.MaximumGeneratedSequenceItems), Text("2.0")));
        Equal("tokenize-by-length", badWidth.Function); Equal("requires a positive integer length", badWidth.Detail);
    }
}
