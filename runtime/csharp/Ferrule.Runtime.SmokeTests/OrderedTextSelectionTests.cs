using Ferrule.Runtime;

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private static void OrderedTextSelection()
    {
        static FerruleValue Text(string value) => FerruleValue.FromString(value);
        var cases = new (FerruleValue[] Values, FerruleValue Last, FerruleValue Distinct)[]
        {
            (Array.Empty<FerruleValue>(), FerruleValue.Null, FerruleValue.Null),
            (new[] { FerruleValue.Null }, FerruleValue.Null, FerruleValue.Null),
            (new[] { Text("A"), Text("B"), Text("A") }, Text("A"), Text("B")),
            (new[] { Text("A"), FerruleValue.Null, Text("B"), FerruleValue.Null }, Text("B"), Text("B")),
            (new[] { Text("A"), Text(""), Text("A") }, Text("A"), Text("")),
            (new[] { Text(" X "), Text("X"), Text(" X ") }, Text(" X "), Text("X")),
            (new[] { Text("é"), Text("e\u0301"), Text("é") }, Text("é"), Text("e\u0301")),
            (new[] { Text("A"), Text("B"), Text("A"), Text("C"), Text("B") }, Text("B"), Text("C")),
        };
        foreach (var item in cases)
        {
            Equal(item.Last, FerruleAggregates.Apply(FerruleAggregateOperation.LastPresentText, item.Values));
            Equal(item.Distinct, FerruleAggregates.Apply(FerruleAggregateOperation.LastDistinctText, item.Values));
        }
        foreach (var operation in new[] { FerruleAggregateOperation.LastPresentText, FerruleAggregateOperation.LastDistinctText })
        {
            foreach (var invalid in new[] { FerruleValue.JsonNull, FerruleValue.XmlNil, FerruleValue.FromInt64(1), FerruleValue.FromDouble(0.25), FerruleValue.FromBoolean(true), FerruleValue.FromDecimal(FerruleDecimal96.Zero) })
                for (var position = 0; position < 3; position++)
                {
                    var values = new[] { Text("A"), Text("B"), Text("A") };
                    values[position] = invalid;
                    var error = Error(FerruleRuntimeError.AggregateUnsupportedScalar, () => FerruleAggregates.Apply(operation, values));
                    Equal(operation, error.AggregateOperation!.Value);
                    Equal(invalid.Kind, error.FoundKind!.Value);
                    Equal(operation == FerruleAggregateOperation.LastPresentText ? Text("A") : Text("B"), FerruleAggregates.Apply(operation, new[] { Text("A"), Text("B"), Text("A") }));
                }
            var argumentError = Error(FerruleRuntimeError.FunctionInvalidArgument, () => FerruleAggregates.Apply(operation, new[] { Text("A") }, FerruleValue.Null));
            Equal(operation == FerruleAggregateOperation.LastPresentText ? "last_present_text" : "last_distinct_text", argumentError.Function!);
        }
    }
}
