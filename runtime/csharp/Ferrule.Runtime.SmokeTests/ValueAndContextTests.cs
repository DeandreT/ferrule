using Ferrule.Runtime;

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private static void ValueMaps()
    {
        IsolatedValueMapInputConversion();
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

        Equal(
            FerruleValue.FromInt64(42),
            FerruleUserFunctions.Adapt(
                Text(" 42 "),
                FerruleScalarType.Int64,
                7,
                3));
        Equal(
            FerruleValue.FromDouble(42),
            FerruleUserFunctions.Adapt(
                FerruleValue.FromInt64(42),
                FerruleScalarType.Double,
                7,
                null));
        Equal(
            FerruleValue.Null,
            FerruleUserFunctions.Adapt(
                FerruleValue.Null,
                FerruleScalarType.Bool,
                7,
                3));
        var functionType = Error(
            FerruleRuntimeError.UserFunctionType,
            () => FerruleUserFunctions.Adapt(
                FerruleValue.FromDouble(1.5),
                FerruleScalarType.Int64,
                7,
                3));
        Equal((ulong?)7, functionType.UserFunction);
        Equal((ulong?)3, functionType.FunctionParameter);
        Equal(FerruleScalarType.Int64, functionType.ExpectedScalarType);
    }

    private static void IsolatedValueMapInputConversion()
    {
        foreach (var (integer, rounded, exact) in new (long, double, bool)[]
        {
            (9_007_199_254_740_991, 9_007_199_254_740_991.0, true),
            (9_007_199_254_740_992, 9_007_199_254_740_992.0, true),
            (9_007_199_254_740_993, 9_007_199_254_740_992.0, false),
            (9_007_199_254_740_994, 9_007_199_254_740_994.0, true),
            (-9_007_199_254_740_991, -9_007_199_254_740_991.0, true),
            (-9_007_199_254_740_992, -9_007_199_254_740_992.0, true),
            (-9_007_199_254_740_993, -9_007_199_254_740_992.0, false),
            (long.MinValue, -9_223_372_036_854_775_808.0, true),
            (long.MinValue + 1, -9_223_372_036_854_775_808.0, false),
            (long.MaxValue, 9_223_372_036_854_775_808.0, false),
        })
        {
            var table = new[]
            {
                new FerruleValueMapEntry(FerruleValue.FromInt64(integer), Text("original")),
                new FerruleValueMapEntry(FerruleValue.FromInt64(integer), Text("later duplicate")),
                new FerruleValueMapEntry(FerruleValue.FromDouble(rounded), Text("converted")),
            };
            Equal(Text("converted"), FerruleValueMaps.Apply(
                FerruleValue.FromInt64(integer), FerruleScalarType.Double, table));
            Equal(Text(exact ? "converted" : "original"), FerruleValueMaps.ApplyUserFunction(
                FerruleValue.FromInt64(integer), FerruleScalarType.Double, table));
        }
        foreach (var input in new[] { FerruleValue.Null, FerruleValue.JsonNull, FerruleValue.XmlNil,
            Bool(true), Text("not-a-number"), FerruleValue.FromDouble(double.PositiveInfinity) })
        {
            Equal(Text("first"), FerruleValueMaps.ApplyUserFunction(input, FerruleScalarType.Double,
                new[] { new FerruleValueMapEntry(input, Text("first")) }));
        }
        Equal(Text("first zero"), FerruleValueMaps.ApplyUserFunction(
            FerruleValue.FromDouble(-0.0), FerruleScalarType.Double,
            new[]
            {
                new FerruleValueMapEntry(FerruleValue.FromDouble(0.0), Text("first zero")),
                new FerruleValueMapEntry(FerruleValue.FromDouble(-0.0), Text("second zero")),
            }));
        Equal(Text("miss"), FerruleValueMaps.ApplyUserFunction(
            FerruleValue.FromDouble(double.NaN), FerruleScalarType.Double,
            new[] { new FerruleValueMapEntry(FerruleValue.FromDouble(double.NaN), Text("unreachable")) },
            Text("miss")));
        Equal(FerruleValue.Null, FerruleValueMaps.ApplyUserFunction(FerruleValue.FromInt64(1), null,
            new[] { new FerruleValueMapEntry(FerruleValue.FromDouble(1), Text("wrong tag")) }));
        Equal(FerruleValue.Null, FerruleValueMaps.ApplyUserFunction(
            FerruleValue.FromInt64(1), FerruleScalarType.Double, Array.Empty<FerruleValueMapEntry>()));
        Equal(FerruleValue.Null, FerruleValueMaps.ApplyUserFunction(
            FerruleValue.FromInt64(1), FerruleScalarType.Double, Array.Empty<FerruleValueMapEntry>(),
            FerruleValue.Null));
        Equal(Text(string.Empty), FerruleValueMaps.ApplyUserFunction(
            FerruleValue.FromInt64(1), FerruleScalarType.Double, Array.Empty<FerruleValueMapEntry>(),
            Text(string.Empty)));
        Throws<ArgumentNullException>(() => FerruleValueMaps.ApplyUserFunction(Text("input"), null, null!));
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

        var unicodeClock = new FerruleExecutionContext("active", "main", "clock-\U0001F600");
        Equal(
            Text("clock-\U0001F600"),
            ScopeContext.FromSource(Group(), unicodeClock)
                .ResolveRuntimeValue(FerruleRuntimeValue.CurrentDateTime));
        Throws<ArgumentException>(() =>
            new FerruleExecutionContext("active", "main", "clock-\uD800"));
        Throws<ArgumentException>(() =>
            new FerruleExecutionContext("active", "main", "clock-\uDC00"));

        var malformedPaths = new FerruleExecutionContext("active-\uD800", "main-\uDC00");
        Equal("active-\uD800", malformedPaths.MappingFilePath);
        Equal("main-\uDC00", malformedPaths.MainMappingFilePath);
        var lossyPaths = ScopeContext.FromSource(Group(), malformedPaths);
        Equal(
            Text("active-\uFFFD"),
            lossyPaths.ResolveRuntimeValue(FerruleRuntimeValue.MappingFilePath));
        Equal(
            Text("main-\uFFFD"),
            lossyPaths.ResolveRuntimeValue(FerruleRuntimeValue.MainMappingFilePath));
    }

    private static void RuntimeParameters()
    {
        Equal(Text(string.Empty), FerruleValue.FromString(string.Empty));
        Equal(Text("\U0001F600"), FerruleValue.FromString("\uD83D\uDE00"));
        Throws<ArgumentNullException>(() => FerruleValue.FromString(null!));
        Throws<ArgumentException>(() => FerruleValue.FromString("before\uD800after"));
        Throws<ArgumentException>(() => FerruleValue.FromString("before\uDC00after"));
        var unicodeField = Field("\U0001F600", Scalar(Text("value")));
        Equal("\U0001F600", unicodeField.Name);
        Throws<ArgumentException>(() => Field("key\uD800", Scalar(Text("value"))));
        Throws<ArgumentException>(() => Field("key\uDC00", Scalar(Text("value"))));

        var parameters = new FerruleRuntimeParameters(new[]
        {
            KeyValuePair.Create("control_number", Text(" 42 ")),
            KeyValuePair.Create("test_mode", FerruleValue.FromBoolean(false)),
        });
        Equal(2, parameters.Count);
        var context = ScopeContext.FromSource(
            Group(),
            FerruleExecutionContext.WithParameters("mapping.ferrule", parameters));
        Equal(
            FerruleValue.FromInt64(42),
            context.ResolveRuntimeParameter(7, "control_number", FerruleScalarType.Int64));

        var missing = Error(
            FerruleRuntimeError.MissingRuntimeParameter,
            () => context.ResolveRuntimeParameter(8, "missing", FerruleScalarType.String));
        Equal((uint?)8, missing.Node);
        Equal("missing", missing.RuntimeParameter);

        var wrongType = Error(
            FerruleRuntimeError.RuntimeParameterType,
            () => context.ResolveRuntimeParameter(9, "test_mode", FerruleScalarType.Int64));
        Equal((uint?)9, wrongType.Node);
        Equal("test_mode", wrongType.RuntimeParameter);
        Equal(FerruleScalarType.Int64, wrongType.ExpectedScalarType);
        Equal(FerruleValueKind.Bool, wrongType.FoundKind);

        Throws<ArgumentException>(() => new FerruleRuntimeParameters(new[]
        {
            KeyValuePair.Create(string.Empty, FerruleValue.Null),
        }));
        Throws<ArgumentException>(() => new FerruleRuntimeParameters(new[]
        {
            KeyValuePair.Create("bad\0name", FerruleValue.Null),
        }));
        Throws<ArgumentException>(() => new FerruleRuntimeParameters(new[]
        {
            KeyValuePair.Create(new string('\u00e9', 129), FerruleValue.Null),
        }));
        Throws<ArgumentException>(() => new FerruleRuntimeParameters(new[]
        {
            KeyValuePair.Create("x", FerruleValue.Null),
            KeyValuePair.Create("x", FerruleValue.Null),
        }));
        Throws<ArgumentException>(() => new FerruleRuntimeParameters(
            Enumerable.Range(0, FerruleRuntimeParameters.MaximumCount + 1)
                .Select(index => KeyValuePair.Create($"p{index}", FerruleValue.Null))));
        Throws<ArgumentException>(() => new FerruleRuntimeParameters(new[]
        {
            KeyValuePair.Create(
                "large",
                FerruleValue.FromString(
                    new string('x', FerruleRuntimeParameters.MaximumStringUtf8Bytes + 1))),
        }));
    }

    private static void SourceDocumentPath()
    {
        var documents = new FerruleDocumentSet(new[]
        {
            new FerruleDocument(
                "portable/first.xml",
                Group(Field(
                    "Rows",
                    new FerruleRepeated(new[]
                    {
                        Group(Field("Name", Scalar(Text("first")))),
                    }))),
                "/inputs/first.xml"),
            new FerruleDocument(
                "portable/second.xml",
                Group(Field(
                    "Rows",
                    new FerruleRepeated(new[]
                    {
                        Group(Field("Name", Scalar(Text("second")))),
                    })))),
        });
        var root = ScopeContext.FromSource(documents);

        Equal(Text("/inputs/first.xml"), root.ResolveSourceDocumentPath());
        var documentContexts = root.IterateSource();
        Equal(Text("/inputs/first.xml"), documentContexts[0].ResolveSourceDocumentPath());
        Equal(
            Text("portable/second.xml"),
            documentContexts[1].ResolveSourceDocumentPath());
        Equal(
            Text("portable/second.xml"),
            documentContexts[1].IterateSource("Rows")[0].ResolveSourceDocumentPath());
        var groupedContexts = root.GroupBy(
            documentContexts,
            Array.Empty<string>(),
            context => context.ResolveSourceDocumentPath());
        Equal(Text("/inputs/first.xml"), groupedContexts[0].ResolveSourceDocumentPath());
        Equal(
            Text("portable/second.xml"),
            groupedContexts[1].ResolveSourceDocumentPath());

        var missing = Error(
            FerruleRuntimeError.MissingSourceField,
            () => ScopeContext.FromSource(Group()).ResolveSourceDocumentPath());
        Equal("<document-path>", missing.Detail);

        var unicodeDocument = new FerruleDocument(
            "portable/\U0001F600.xml",
            Group(),
            "/inputs/\U0001F600.xml");
        Equal(
            Text("/inputs/\U0001F600.xml"),
            ScopeContext.FromSource(new FerruleDocumentSet(new[] { unicodeDocument }))
                .ResolveSourceDocumentPath());
        Error(
            FerruleRuntimeError.InvalidDocumentPath,
            () => new FerruleDocument("portable/\uD800.xml", Group()));
        Error(
            FerruleRuntimeError.InvalidDocumentPath,
            () => new FerruleDocument("portable/ok.xml", Group(), "/inputs/\uDC00.xml"));
        Error(
            FerruleRuntimeError.InvalidDocumentPath,
            () => new FerruleDocument(string.Empty, Group()));
        Error(
            FerruleRuntimeError.NestedDocumentSet,
            () => new FerruleDocument("portable/ok.xml", new FerruleDocumentSet([])));
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
