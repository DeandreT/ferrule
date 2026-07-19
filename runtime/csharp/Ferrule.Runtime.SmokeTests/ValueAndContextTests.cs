using Ferrule.Runtime;

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private static void ValueMaps()
    {
        var duplicateRows = new[]
        {
            new FerruleValueMapEntry(Text("A"), Text("first")),
            new FerruleValueMapEntry(Text("A"), Text("second")),
        };
        Equal(
            Text("first"),
            FerruleValueMaps.Apply(Text("A"), null, duplicateRows));
        Equal(
            Text("fallback"),
            FerruleValueMaps.Apply(
                Text("missing"),
                null,
                duplicateRows,
                Text("fallback")));
        Equal(
            FerruleValue.Null,
            FerruleValueMaps.Apply(Text("missing"), null, duplicateRows));
        Equal(
            FerruleValue.XmlNil,
            FerruleValueMaps.Apply(
                Text("missing"),
                null,
                duplicateRows,
                FerruleValue.XmlNil));

        MapEquals(Text("bool"), Bool(true), FerruleScalarType.String, Text("true"));
        MapEquals(Text("int"), FerruleValue.FromInt64(-17), FerruleScalarType.String, Text("-17"));
        MapEquals(
            FerruleValue.FromInt64(7),
            Text("  +7  "),
            FerruleScalarType.Int64,
            FerruleValue.FromInt64(7));
        MapEquals(
            FerruleValue.FromInt64(long.MinValue),
            FerruleValue.FromDouble((double)long.MinValue),
            FerruleScalarType.Int64,
            FerruleValue.FromInt64(long.MinValue));
        MapEquals(
            FerruleValue.FromDouble(12.5),
            Text(" 12.5 "),
            FerruleScalarType.Double,
            FerruleValue.FromDouble(12.5));
        MapEquals(Bool(true), Text(" 1 "), FerruleScalarType.Bool, Bool(true));
        MapEquals(Bool(false), Text(" false "), FerruleScalarType.Bool, Bool(false));

        // Failed coercion retains the original tagged value before matching.
        MapEquals(Text("original bool"), Bool(true), FerruleScalarType.Int64, Bool(true));
        MapEquals(
            Text("original text"),
            Text("not-an-int"),
            FerruleScalarType.Int64,
            Text("not-an-int"));
        MapEquals(
            Text("positive infinity"),
            FerruleValue.FromDouble(double.PositiveInfinity),
            FerruleScalarType.String,
            FerruleValue.FromDouble(double.PositiveInfinity));
        MapEquals(
            Text("upper bound remains double"),
            FerruleValue.FromDouble(-(double)long.MinValue),
            FerruleScalarType.Int64,
            FerruleValue.FromDouble(-(double)long.MinValue));

        // Integer-to-double coercion intentionally follows f64 precision loss.
        MapEquals(
            Text("rounded"),
            FerruleValue.FromInt64(9_007_199_254_740_993),
            FerruleScalarType.Double,
            FerruleValue.FromDouble(9_007_199_254_740_992));
        Equal(
            FerruleValue.Null,
            FerruleValueMaps.Apply(
                FerruleValue.FromInt64(1),
                null,
                new[]
                {
                    new FerruleValueMapEntry(FerruleValue.FromDouble(1), Text("wrong tag")),
                }));

        MapEquals(Text("null"), FerruleValue.Null, FerruleScalarType.Bool, FerruleValue.Null);
        MapEquals(Text("nil"), FerruleValue.XmlNil, FerruleScalarType.Int64, FerruleValue.XmlNil);
        Equal(
            Text("NaN misses"),
            FerruleValueMaps.Apply(
                FerruleValue.FromDouble(double.NaN),
                FerruleScalarType.Double,
                new[]
                {
                    new FerruleValueMapEntry(
                        FerruleValue.FromDouble(double.NaN),
                        Text("NaN matched")),
                },
                Text("NaN misses")));

        FloatStringMapEquals(-0.0, "-0");
        FloatStringMapEquals(1e-7, "0.0000001");
        FloatStringMapEquals(1e20, "100000000000000000000");
        FloatStringMapEquals(
            double.Epsilon,
            "0." + new string('0', 323) + "5");
        FloatStringMapEquals(
            BitConverter.Int64BitsToDouble(0x0010000000000000),
            "0." + new string('0', 307) + "22250738585072014");
        FloatStringMapEquals(
            double.MaxValue,
            "17976931348623157" + new string('0', 292));

        Throws<ArgumentNullException>(() => FerruleValueMaps.Apply(
            Text("input"),
            null,
            null!));
    }

    private static void RuntimeExecutionContext()
    {
        const string ActivePath = " ./maps/active.ferrule.json ";
        const string MainPath = "../main.ferrule.json";
        const string CurrentDateTime = "2031-08-17T06:07:08.900-07:00";
        var execution = new FerruleExecutionContext(
            ActivePath,
            MainPath,
            CurrentDateTime);
        var source = Group(Field(
            "Rows",
            new FerruleRepeated(new[]
            {
                Group(Field("Value", Scalar(Text("first")))),
                Group(Field("Value", Scalar(Text("second")))),
            })));
        var root = ScopeContext.FromSource(source, execution);

        RuntimeValuesEqual(root, ActivePath, MainPath, CurrentDateTime);
        RuntimeValuesEqual(
            root.IterateSource("Rows")[0],
            ActivePath,
            MainPath,
            CurrentDateTime);
        RuntimeValuesEqual(
            root.IterateGenerated(new[] { Text("generated") })[0],
            ActivePath,
            MainPath,
            CurrentDateTime);
        RuntimeValuesEqual(
            root.EnumerateGenerated(new[] { Text("lazy") }).Single(),
            ActivePath,
            MainPath,
            CurrentDateTime);
        RuntimeValuesEqual(
            root.AggregateItems("Rows")[1],
            ActivePath,
            MainPath,
            CurrentDateTime);
        RuntimeValuesEqual(
            root.IterateSource("Rows")[1].WithCompactedPosition(1),
            ActivePath,
            MainPath,
            CurrentDateTime);

        var sameMapping = ScopeContext.FromSource(
            Group(),
            new FerruleExecutionContext(string.Empty));
        Equal(
            Text(string.Empty),
            sameMapping.ResolveRuntimeValue(FerruleRuntimeValue.MappingFilePath));
        Equal(
            Text(string.Empty),
            sameMapping.ResolveRuntimeValue(FerruleRuntimeValue.MainMappingFilePath));

        var emptyDateTime = ScopeContext.FromSource(
            Group(),
            new FerruleExecutionContext("active", "main", string.Empty));
        Equal(
            Text(string.Empty),
            emptyDateTime.ResolveRuntimeValue(FerruleRuntimeValue.CurrentDateTime));

        var missingContext = Error(
            FerruleRuntimeError.MissingRuntimeValue,
            () => ScopeContext.FromSource(Group()).ResolveRuntimeValue(
                FerruleRuntimeValue.MappingFilePath));
        Equal(FerruleRuntimeValue.MappingFilePath, missingContext.RuntimeValue);

        var missingDateTime = Error(
            FerruleRuntimeError.MissingRuntimeValue,
            () => ScopeContext.FromSource(
                Group(),
                new FerruleExecutionContext("mapping.ferrule.json"))
                .ResolveRuntimeValue(FerruleRuntimeValue.CurrentDateTime));
        Equal(FerruleRuntimeValue.CurrentDateTime, missingDateTime.RuntimeValue);

        Throws<ArgumentNullException>(() => new FerruleExecutionContext(null!));
        Throws<ArgumentNullException>(() => new FerruleExecutionContext("active", null!));
    }

    private static void MapEquals(
        FerruleValue expected,
        FerruleValue input,
        FerruleScalarType inputType,
        FerruleValue from) =>
        Equal(
            expected,
            FerruleValueMaps.Apply(
                input,
                inputType,
                new[] { new FerruleValueMapEntry(from, expected) }));

    private static void FloatStringMapEquals(double input, string expected) =>
        MapEquals(
            Text("matched"),
            FerruleValue.FromDouble(input),
            FerruleScalarType.String,
            Text(expected));

    private static void RuntimeValuesEqual(
        ScopeContext context,
        string activePath,
        string mainPath,
        string currentDateTime)
    {
        Equal(
            Text(activePath),
            context.ResolveRuntimeValue(FerruleRuntimeValue.MappingFilePath));
        Equal(
            Text(mainPath),
            context.ResolveRuntimeValue(FerruleRuntimeValue.MainMappingFilePath));
        Equal(
            Text(currentDateTime),
            context.ResolveRuntimeValue(FerruleRuntimeValue.CurrentDateTime));
    }
}
