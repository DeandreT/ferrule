using Ferrule.Runtime;

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private static void DecimalMappingOperations()
    {
        static FerruleValue Convert(string text) => FerruleFunctions.Call("decimal_from_nullable_text", new[] { FerruleValue.FromString(text) });
        foreach (var item in new[] { (" 0.1\t", "0.1"), ("9007199254740993", "9007199254740993"), ("2.5000", "2.5"), ("1e2", "100"), ("9007199254740993e0", "9007199254740993"), ("1234567890123456789012345678e-28", "0.1234567890123456789012345678"), ("5e-29", "0"), ("15e-29", "0.0000000000000000000000000002"), ("-15e-29", "-0.0000000000000000000000000002"), ("1e-99999999", "0"), ("0e99999999", "0") })
            Equal(FerruleValue.FromDecimal(FerruleDecimal96.ParseExact(item.Item2)), Convert(item.Item1));
        foreach (var value in new[] { FerruleValue.Null, FerruleValue.JsonNull })
            Equal(FerruleValue.FromDecimal(FerruleDecimal96.Zero), FerruleFunctions.Call("decimal_from_nullable_text", new[] { value }));
        foreach (var text in new[] { "", " ", "abc", "1,000", "NaN", "INF", "1e9999", "79228162514264337593543950336" })
        {
            var error = Error(FerruleRuntimeError.DecimalConversion, () => Convert(text));
            Equal(true, error.InnerException is FerruleDecimalException);
        }
        foreach (var pair in new[] { (new[] { "0.1", "0.2" }, "0.3"), (new[] { "9007199254740993", "1" }, "9007199254740994"), (new[] { "0.00000000000000000000000000005", "0.00000000000000000000000000005" }, "0"), (new[] { "-0.1", "0.1" }, "0"), (Array.Empty<string>(), "0") })
            Equal(Convert(pair.Item2), FerruleAggregates.Apply(FerruleAggregateOperation.DecimalSum, pair.Item1.Select(Convert).ToArray()));
        var overflow = Error(FerruleRuntimeError.AggregateDecimal, () => FerruleAggregates.Apply(FerruleAggregateOperation.DecimalSum, new[] { Convert("79228162514264337593543950335"), Convert("1"), Convert("-1") }));
        Equal(true, overflow.InnerException is FerruleDecimalException { Error: FerruleDecimalError.Overflow });
        var collected = new List<FerruleValue>();
        FerruleAggregates.CollectValue(FerruleAggregateOperation.DecimalSum, collected, Convert("79228162514264337593543950335"));
        Error(FerruleRuntimeError.AggregateDecimal, () => FerruleAggregates.CollectValue(FerruleAggregateOperation.DecimalSum, collected, Convert("1")));
        Equal(1, collected.Count);
        Equal(Convert("79228162514264337593543950335"), collected[0]);
        FerruleAggregates.CollectValue(FerruleAggregateOperation.DecimalSum, collected, Convert("-1"));
        Equal(Convert("79228162514264337593543950334"), FerruleAggregates.Apply(FerruleAggregateOperation.DecimalSum, collected));
        foreach (var invalid in new[] { FerruleValue.Null, FerruleValue.JsonNull, FerruleValue.XmlNil, FerruleValue.FromInt64(1), FerruleValue.FromDouble(0.1), FerruleValue.FromBoolean(false), FerruleValue.FromString("0.1") })
            for (var index = 0; index < 3; index++)
            {
                var values = new[] { Convert("0.1"), Convert("0.2"), Convert("0.3") };
                values[index] = invalid;
                Error(FerruleRuntimeError.AggregateUnsupportedScalar, () => FerruleAggregates.Apply(FerruleAggregateOperation.DecimalSum, values));
                Equal(Convert("0.3"), FerruleAggregates.Apply(FerruleAggregateOperation.DecimalSum, new[] { Convert("0.1"), Convert("0.2") }));
            }
    }
}
