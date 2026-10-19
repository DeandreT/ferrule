namespace Ferrule.Generated;

[global::System.Flags]
internal enum TargetScalarDomain
{
    String = 1 << 0,
    Int64 = 1 << 1,
    Double = 1 << 2,
    Bool = 1 << 3,
}

internal static class TargetBuilder
{
    internal static global::Ferrule.Runtime.FerruleInstance Scalar(
        global::Ferrule.Runtime.FerruleValue value,
        TargetScalarDomain targetDomain) =>
        new global::Ferrule.Runtime.FerruleScalar(Adapt(value, targetDomain));

    internal static global::Ferrule.Runtime.FerruleInstance RepeatedScalar(
        global::System.Collections.Generic.IEnumerable<global::Ferrule.Runtime.FerruleValue> values,
        TargetScalarDomain targetDomain)
    {
        global::System.ArgumentNullException.ThrowIfNull(values);
        var items = new global::System.Collections.Generic.List<global::Ferrule.Runtime.FerruleInstance>();
        foreach (var sourceValue in values)
        {
            var value = Adapt(sourceValue, targetDomain);
            if (value.Kind != global::Ferrule.Runtime.FerruleValueKind.Null)
            {
                items.Add(new global::Ferrule.Runtime.FerruleScalar(value));
            }
        }

        return new global::Ferrule.Runtime.FerruleRepeated(items);
    }

    private static global::Ferrule.Runtime.FerruleValue Adapt(
        global::Ferrule.Runtime.FerruleValue value,
        TargetScalarDomain targetDomain)
    {
        var actualDomain = ValueDomain(value.Kind);
        if (actualDomain.HasValue &&
            (targetDomain & actualDomain.Value) != 0)
        {
            return value;
        }

        if ((targetDomain & TargetScalarDomain.Int64) != 0 &&
            value.Kind == global::Ferrule.Runtime.FerruleValueKind.Double)
        {
            var number = value.DoubleValue;
            if (global::System.Math.Truncate(number) == number &&
                number >= (double)long.MinValue &&
                number < -(double)long.MinValue)
            {
                return global::Ferrule.Runtime.FerruleValue.FromInt64((long)number);
            }
        }
        else if ((targetDomain & TargetScalarDomain.Double) != 0 &&
                 value.Kind == global::Ferrule.Runtime.FerruleValueKind.Int64)
        {
            var integer = value.Int64Value;
            var number = (double)integer;
            if (number >= (double)long.MinValue &&
                number < -(double)long.MinValue &&
                (long)number == integer)
            {
                return global::Ferrule.Runtime.FerruleValue.FromDouble(number);
            }
        }

        return value;
    }

    private static TargetScalarDomain? ValueDomain(
        global::Ferrule.Runtime.FerruleValueKind kind) =>
        kind switch
        {
            global::Ferrule.Runtime.FerruleValueKind.String => TargetScalarDomain.String,
            global::Ferrule.Runtime.FerruleValueKind.Int64 => TargetScalarDomain.Int64,
            global::Ferrule.Runtime.FerruleValueKind.Double => TargetScalarDomain.Double,
            global::Ferrule.Runtime.FerruleValueKind.Bool => TargetScalarDomain.Bool,
            _ => null,
        };
}
