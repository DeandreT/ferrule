using Ferrule.Runtime;

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private static void DecimalNumberText()
    {
        foreach (var (text, expected) in new (string, string)[] {
            (" 1 ","1"),("\t.1E+2\n","10"),("1.e2","100"),("0e9999","0"),("1e-9999","0"),
            ("-1.20E+2","-120"),("0.1234567890123456789012345678","0.1234567890123456789012345678"),
            ("0.1234567890123456789012345678e0","0.123456789012346"),
            ("79228162514264337593543950335","79228162514264337593543950335") })
            Equal(expected, FerruleDecimal96.ParseNumber(text).ToString());
        void Failure(string text, FerruleDecimalError expected)
        {
            try { FerruleDecimal96.ParseNumber(text); throw new Exception("Expected numeric text failure"); }
            catch (FerruleDecimalException error) { Equal(expected, error.Error); }
        }
        foreach (var text in new[] {"NaN"," INF ","-INF","1e9999","-1e9999","7.9228162514264337593543950335e28"})
            Failure(text, FerruleDecimalError.Overflow);
        foreach (var text in new[] {"","\t","e0","1e","++1","1e++1","1,000","1_0","Infinity","+INF","nan","٠","1\u00a00","\u180e1","\u200b1","\ufeff1","\ud800"})
            Failure(text, FerruleDecimalError.InvalidNumberLexical);
        Failure(null!, FerruleDecimalError.InvalidNumberLexical);
        Equal("1", FerruleDecimal96.ParseNumber(new string(' ',4095)+"1").ToString());
        Failure(new string(' ',4096)+"1", FerruleDecimalError.InputTooLong);
        Failure(new string('\u00a0',2048)+"1", FerruleDecimalError.InputTooLong);
    }
}
