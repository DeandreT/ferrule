using Ferrule.Runtime;

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private static void Decimal96Domain()
    {
        foreach (var (text, expected) in new[] {
            ("0", "0"), ("-0.000", "0"), ("0012.5000", "12.5"), (".25", "0.25"), ("3.", "3"),
            ("9007199254740993", "9007199254740993"),
            ("0.1234567890123456789012345678", "0.1234567890123456789012345678"),
            ("79228162514264337593543950335", "79228162514264337593543950335"),
        })
        {
            var value = FerruleDecimal96.ParseExact(text);
            Equal(expected, value.ToString());
            Equal(value, FerruleDecimal96.ParseRounded(text));
            Equal(value, FerruleDecimal96.FromParts(value.IsNegative, value.Coefficient, value.Scale));
            Equal(value, FerruleDecimal96.ParseExact(value.ToString()));
        }
        Equal(FerruleDecimal96.Zero, FerruleDecimal96.FromParts(true, 0, 28));
        Equal(FerruleDecimal96.ParseExact("2.5"), FerruleDecimal96.FromParts(false, 25000, 4));
        foreach (var (text, expected) in new[] {
            ("0.12345678901234567890123456785", "0.1234567890123456789012345678"),
            ("0.12345678901234567890123456795", "0.123456789012345678901234568"),
            ("0.00000000000000000000000000005", "0"),
            ("0.00000000000000000000000000015", "0.0000000000000000000000000002"),
        })
        {
            Equal(expected, FerruleDecimal96.ParseRounded(text).ToString());
            DecimalError(FerruleDecimalError.PrecisionLoss, () => FerruleDecimal96.ParseExact(text));
            Equal(FerruleDecimal96.ParseRounded(text).Negated(), FerruleDecimal96.ParseRounded("-" + text));
        }
        foreach (var text in new[] { "", "+", "-", ".", "1..0", "1e0", "NaN", " 1", "1 ", "1,000", "1\0", "١", "\ud800" })
            DecimalError(FerruleDecimalError.InvalidLexical, () => FerruleDecimal96.ParseRounded(text));
        DecimalError(FerruleDecimalError.InputTooLong, () => FerruleDecimal96.ParseExact(new string('0', 4097)));
        Equal(FerruleDecimal96.Zero, FerruleDecimal96.ParseExact(new string('0', 4096)));
        DecimalError(FerruleDecimalError.InvalidScale, () => FerruleDecimal96.FromParts(false, 1, 29));
        DecimalError(FerruleDecimalError.Overflow, () => FerruleDecimal96.FromParts(false, FerruleDecimal96.MaximumCoefficient + 1, 0));
        DecimalError(FerruleDecimalError.Overflow, () => FerruleDecimal96.ParseRounded("79228162514264337593543950336"));
    }

    private static void Decimal96Arithmetic()
    {
        foreach (var (left, right, expected) in new[] {
            ("0.1", "0.2", "0.3"), ("-0.1", "0.1", "0"),
            ("9007199254740993", "0.1", "9007199254740993.1"),
            ("7922816251426433759354395033.5", "-7922816251426433759354395033", "0.5"),
            ("79228162514264337593543950335", "0.0000000000000000000000000001", "79228162514264337593543950335"),
            ("79228162514264337593543950335", "-0.5", "79228162514264337593543950334"),
            ("7922816251426433759354395033.5", "0.05", "7922816251426433759354395034"),
        })
        {
            var a = FerruleDecimal96.ParseExact(left);
            var b = FerruleDecimal96.ParseExact(right);
            var result = FerruleDecimal96.ParseExact(expected);
            Equal(result, a.AddRounded(b));
            Equal(result, b.AddRounded(a));
            Equal(result.Negated(), a.Negated().AddRounded(b.Negated()));
            Equal(result, a.SubtractRounded(b.Negated()));
            Equal(FerruleDecimal96.Zero, a.SubtractRounded(a));
            Equal(true, a >= FerruleDecimal96.ParseExact(left));
        }
        var maximum = FerruleDecimal96.FromParts(false, FerruleDecimal96.MaximumCoefficient, 0);
        DecimalError(FerruleDecimalError.Overflow, () => maximum.AddRounded(FerruleDecimal96.ParseExact("1")));
        Equal(true, FerruleDecimal96.ParseExact("-10") < FerruleDecimal96.ParseExact("-0.01"));
        Equal(FerruleDecimal96.ParseExact("10").GetHashCode(), FerruleDecimal96.ParseExact("10.00").GetHashCode());
        var half = FerruleDecimal96.ParseRounded("0.00000000000000000000000000005");
        Equal(FerruleDecimal96.Zero, half.AddRounded(half));
    }

    private static void DecimalError(FerruleDecimalError error, Action action)
    {
        try { action(); }
        catch (FerruleDecimalException caught)
        {
            Equal(error, caught.Error);
            Equal<Exception?>(null, caught.InnerException);
            return;
        }
        throw new Exception("Expected decimal refusal: " + error);
    }
}
