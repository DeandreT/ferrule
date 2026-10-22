using Ferrule.Runtime;

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private static void DecimalValueIdentity()
    {
        Equal(6, (int)FerruleValueKind.String);
        foreach (var text in new[] { "0", "0.1", "-1.25", "9007199254740993", "79228162514264337593543950335", "0.0000000000000000000000000001" })
        {
            var number = FerruleDecimal96.ParseExact(text);
            var value = FerruleValue.FromDecimal(number);
            Equal(FerruleValueKind.Decimal, value.Kind);
            Equal(number, value.DecimalValue);
            Equal(text, value.ToString());
            Equal(value, FerruleValue.FromDecimal(FerruleDecimal96.ParseExact(text)));
            Equal(value.GetHashCode(), FerruleValue.FromDecimal(number).GetHashCode());
            NotEqual(value, FerruleValue.FromString(text));
            Error(FerruleRuntimeError.ValueKindMismatch, () => { _ = value.DoubleValue; });
        }
        NotEqual(FerruleValue.FromDecimal(FerruleDecimal96.ParseExact("1")), FerruleValue.FromInt64(1));
        NotEqual(FerruleValue.FromDecimal(FerruleDecimal96.ParseExact("0.1")), FerruleValue.FromDouble(0.1));
        Equal(FerruleValueKind.Null, default(FerruleValue).Kind);
        var exact = FerruleValue.FromDecimal(FerruleDecimal96.ParseExact("9007199254740993.1"));
        Equal(FerruleValue.FromString("9007199254740993.1"), FerruleFunctions.Call("string", new[] { exact }));
        Equal(FerruleValue.FromBoolean(true), FerruleFunctions.Call("is_numeric", new[] { exact }));
        foreach (var function in new[] { "to_number", "boolean", "positive", "floor" })
        {
            Error(FerruleRuntimeError.FunctionType, () => FerruleFunctions.Call(function, new[] { exact }));
        }
        foreach (var function in new[] { "add", "subtract", "multiply", "divide", "equal", "not_equal", "less_than", "greater_than" })
        {
            Error(FerruleRuntimeError.FunctionType, () => FerruleFunctions.Call(function, new[] { exact, FerruleValue.FromInt64(1) }));
            Error(FerruleRuntimeError.FunctionType, () => FerruleFunctions.Call(function, new[] { FerruleValue.FromInt64(1), exact }));
        }
        foreach (var operation in new[] { FerruleAggregateOperation.Sum, FerruleAggregateOperation.Avg, FerruleAggregateOperation.Min, FerruleAggregateOperation.Max })
        {
            for (var index = 0; index < 3; index++)
            {
                var values = new[] { FerruleValue.FromInt64(1), FerruleValue.FromInt64(2), FerruleValue.FromInt64(3) };
                values[index] = exact;
                Error(FerruleRuntimeError.AggregateUnsupportedScalar, () => FerruleAggregates.Apply(operation, values));
            }
        }
        Equal(exact, FerruleAggregates.Apply(FerruleAggregateOperation.ItemAt, new[] { exact }, FerruleValue.FromInt64(1)));
        Equal(FerruleValue.FromInt64(1), FerruleAggregates.Apply(FerruleAggregateOperation.Count, new[] { exact }));
        Equal(FerruleValue.FromString("9007199254740993.1"), FerruleAggregates.Apply(FerruleAggregateOperation.Join, new[] { exact }));
        Error(FerruleRuntimeError.AggregateUnsupportedScalar, () => FerruleAggregates.Apply(FerruleAggregateOperation.ItemAt, new[] { exact }, exact));
        var sortedValues = new[] { FerruleValue.FromDecimal(FerruleDecimal96.ParseExact("9007199254740993")), FerruleValue.FromDecimal(FerruleDecimal96.ParseExact("9007199254740992")), FerruleValue.FromInt64(1), FerruleValue.FromDecimal(FerruleDecimal96.ParseExact("9007199254740992")) };
        Equal("2,1,3,0", string.Join(",", FerruleSequences.StableSort(new[] { 0, 1, 2, 3 }, new[] { new FerruleSortKey<int>(index => sortedValues[index]) })));
        Equal("0,1,3,2", string.Join(",", FerruleSequences.StableSort(new[] { 0, 1, 2, 3 }, new[] { new FerruleSortKey<int>(index => sortedValues[index], true) })));
        foreach (var absent in new[] { FerruleValue.Null, FerruleValue.JsonNull })
        {
            Error(FerruleRuntimeError.FunctionType, () => FerruleFunctions.Call("sqlite_multiply", new[] { exact, absent }));
            Error(FerruleRuntimeError.FunctionType, () => FerruleFunctions.Call("sqlite_multiply", new[] { absent, exact }));
        }
        foreach (var scalarType in new[] { FerruleScalarType.String, FerruleScalarType.Int64, FerruleScalarType.Double, FerruleScalarType.Bool })
        {
            Error(FerruleRuntimeError.UserFunctionType, () => FerruleUserFunctions.Adapt(exact, scalarType, 1, null));
            var row = new FerruleGroup(new[] { new FerruleField("Amount", new FerruleScalar(exact)) });
            try
            {
                _ = FerruleCsv.Serialize(new FerruleRepeated(new[] { row }), new[] { new FerruleCsvField("Amount", scalarType) });
                throw new InvalidOperationException("A decimal must not be admitted by a legacy CSV scalar type.");
            }
            catch (FerruleCsvException error)
            {
                Equal(FerruleCsvError.ValueType, error.Error);
                Equal("decimal", error.Got);
            }
        }
    }
}
