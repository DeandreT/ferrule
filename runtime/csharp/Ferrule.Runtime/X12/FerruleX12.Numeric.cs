namespace Ferrule.Runtime;

public static partial class FerruleX12
{
    // Input-only: writer fixed-value comparisons keep their original lexical
    // values, and every admitted leaf is visited once during reader traversal.
    private static FerruleValue ApplyImpliedDecimals(Node node, FerruleValue value, string[] path)
    {
        if (node.ImpliedPlaces is not { } places
            || value.Kind is FerruleValueKind.Null or FerruleValueKind.JsonNull or FerruleValueKind.XmlNil) return value;
        if (value.Kind != FerruleValueKind.Double || !double.IsFinite(value.DoubleValue))
            throw Failure(FerruleX12Error.Value, "X12 scalar has an invalid numeric representation.", path: string.Join('/', path));
        double scaled = value.DoubleValue / Math.Pow(10, places);
        if (!double.IsFinite(scaled))
            throw Failure(FerruleX12Error.Value, "X12 scalar has an invalid numeric representation.", path: string.Join('/', path));
        return FerruleValue.FromDouble(scaled);
    }
}
