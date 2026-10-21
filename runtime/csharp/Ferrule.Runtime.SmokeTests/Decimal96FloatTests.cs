using Ferrule.Runtime;

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private static void Decimal96FloatConversion()
    {
        foreach (var (input, expected) in new (double, string)[] {
            (0.0,"0"),(-0.0,"0"),(0.1,"0.1"),(-0.1,"-0.1"),
            (1.0/3.0,"0.333333333333333"),(1.2345678901234567,"1.23456789012346"),
            (9007199254740991.0,"9007199254740990"),(1e28,"10000000000000000000000000000"),
            (1e-28,"0.0000000000000000000000000001"),(5e-29,"0"),
            (1.5e-28,"0.0000000000000000000000000002"),(2.5e-28,"0.0000000000000000000000000002") })
        {
            var actual = FerruleDecimal96.FromDouble(input);
            Equal(expected, actual.ToString());
            Equal(FerruleDecimal96.ParseExact(expected), actual);
            Equal(actual.Negated(), FerruleDecimal96.FromDouble(-input));
        }
        Equal("79228162514264300000000000000", FerruleDecimal96.FromDouble(BitConverter.UInt64BitsToDouble(0x45ef_ffff_ffff_ffff)).ToString());
        Equal("0", FerruleDecimal96.FromDouble(BitConverter.UInt64BitsToDouble(0x3a0f_b0f6_be50_601aUL)).ToString());
        Equal("0.0000000000000000000000000001", FerruleDecimal96.FromDouble(BitConverter.UInt64BitsToDouble(0x3a0f_b0f6_be50_601bUL)).ToString());
        foreach (var input in new[] { double.NaN, double.PositiveInfinity, double.NegativeInfinity,
            double.MaxValue, -double.MaxValue, BitConverter.UInt64BitsToDouble(0x45f0_0000_0000_0000) })
        {
            try { FerruleDecimal96.FromDouble(input); throw new Exception("Expected decimal overflow"); }
            catch (FerruleDecimalException error) { Equal(FerruleDecimalError.Overflow, error.Error); }
        }
    }
}
