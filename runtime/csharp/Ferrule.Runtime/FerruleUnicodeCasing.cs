using System.Buffers;
using System.Text;

namespace Ferrule.Runtime;

/// <summary>
/// Pinned Unicode 17.0.0 language-neutral full casing shared row by row with
/// the Rust runtime. Host globalization and .NET's own Unicode version never
/// participate; see docs/unicode-casing.md.
/// </summary>
internal static partial class FerruleUnicodeCasing
{
    private const int CapitalSigma = 0x03A3;
    private const int FinalSigma = 0x03C2;

    private readonly record struct Run(int First, int Last, int Stride, int Delta);
    private readonly record struct Expansion(int Scalar, int First, int Second, int Third);
    private readonly record struct ScalarRange(int First, int Last);

    internal static string ToUpper(string value) => Map(value, upper: true);

    internal static string ToLower(string value) => Map(value, upper: false);

    private static string Map(string value, bool upper)
    {
        var builder = new StringBuilder(value.Length);
        var index = 0;
        while (index < value.Length)
        {
            var character = value[index];
            if (character < 0x80)
            {
                builder.Append(upper ? char.ToUpperInvariant(character) : char.ToLowerInvariant(character));
                index++;
                continue;
            }
            if (Rune.DecodeFromUtf16(value.AsSpan(index), out var rune, out var consumed) != OperationStatus.Done)
            {
                // Runtime strings are well formed; retain any other code unit.
                builder.Append(character);
                index++;
                continue;
            }
            var scalar = rune.Value;
            if (!upper && scalar == CapitalSigma && IsFinalSigma(value, index, consumed))
            {
                builder.Append((char)FinalSigma);
            }
            else if (upper)
            {
                AppendMapping(builder, scalar, UpperRuns, UpperExpansions);
            }
            else
            {
                AppendMapping(builder, scalar, LowerRuns, LowerExpansions);
            }
            index += consumed;
        }
        return builder.ToString();
    }

    // Unicode Final_Sigma: a cased scalar precedes the sigma and none follows
    // it, skipping case-ignorable scalars of the original text on both sides.
    // A scalar that is both cased and case-ignorable is skipped.
    private static bool IsFinalSigma(string value, int index, int length) =>
        CasedAfterIgnorable(value.AsSpan(0, index), backward: true)
        && !CasedAfterIgnorable(value.AsSpan(index + length), backward: false);

    private static bool CasedAfterIgnorable(ReadOnlySpan<char> text, bool backward)
    {
        while (!text.IsEmpty)
        {
            var status = backward
                ? Rune.DecodeLastFromUtf16(text, out var rune, out var consumed)
                : Rune.DecodeFromUtf16(text, out rune, out consumed);
            var scalar = status == OperationStatus.Done ? rune.Value : Rune.ReplacementChar.Value;
            if (!InRanges(scalar, CaseIgnorable)) { return InRanges(scalar, Cased); }
            text = backward ? text[..^consumed] : text[consumed..];
        }
        return false;
    }

    private static bool InRanges(int scalar, ScalarRange[] ranges)
    {
        var low = 0;
        var high = ranges.Length;
        while (low < high)
        {
            var middle = low + (high - low) / 2;
            var range = ranges[middle];
            if (scalar < range.First) { high = middle; }
            else if (scalar > range.Last) { low = middle + 1; }
            else { return true; }
        }
        return false;
    }

    private static void AppendMapping(StringBuilder builder, int scalar, Run[] runs, Expansion[] expansions)
    {
        var low = 0;
        var high = expansions.Length;
        while (low < high)
        {
            var middle = low + (high - low) / 2;
            var expansion = expansions[middle];
            if (scalar < expansion.Scalar) { high = middle; }
            else if (scalar > expansion.Scalar) { low = middle + 1; }
            else
            {
                // Zero pads shorter expansions; U+0000 never occurs in a mapping.
                AppendScalar(builder, expansion.First);
                if (expansion.Second != 0) { AppendScalar(builder, expansion.Second); }
                if (expansion.Third != 0) { AppendScalar(builder, expansion.Third); }
                return;
            }
        }
        AppendScalar(builder, RunMapping(scalar, runs));
    }

    private static int RunMapping(int scalar, Run[] runs)
    {
        var low = 0;
        var high = runs.Length;
        while (low < high)
        {
            var middle = low + (high - low) / 2;
            var run = runs[middle];
            if (scalar < run.First) { high = middle; }
            else if (scalar > run.Last) { low = middle + 1; }
            else { return (scalar - run.First) % run.Stride == 0 ? scalar + run.Delta : scalar; }
        }
        return scalar;
    }

    private static void AppendScalar(StringBuilder builder, int scalar)
    {
        if (scalar < 0x10000)
        {
            builder.Append((char)scalar);
            return;
        }
        Span<char> pair = stackalloc char[2];
        builder.Append(pair[..new Rune(scalar).EncodeToUtf16(pair)]);
    }
}
