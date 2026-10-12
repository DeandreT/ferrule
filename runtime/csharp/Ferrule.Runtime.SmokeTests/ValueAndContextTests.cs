using Ferrule.Runtime;

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private static void ValueMaps()
    {
        IsolatedValueMapInputConversion();
        DeclaredValueMapNullNormalization();
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

    private static void DeclaredValueMapNullNormalization()
    {
        var table = new[]
        {
            new FerruleValueMapEntry(FerruleValue.Null, Text("null-first")),
            new FerruleValueMapEntry(FerruleValue.Null, Text("null-second")),
            new FerruleValueMapEntry(FerruleValue.JsonNull, Text("json-first")),
            new FerruleValueMapEntry(FerruleValue.JsonNull, Text("json-second")),
            new FerruleValueMapEntry(FerruleValue.XmlNil, Text("nil-first")),
            new FerruleValueMapEntry(FerruleValue.XmlNil, Text("nil-second")),
        };
        foreach (var target in new[] { FerruleScalarType.String, FerruleScalarType.Int64, FerruleScalarType.Double, FerruleScalarType.Bool })
        {
            foreach (var (input, ordinary, preserved) in new (FerruleValue, string, string)[]
            {
                (FerruleValue.Null, "null-first", "null-first"),
                (FerruleValue.JsonNull, "null-first", "json-first"),
                (FerruleValue.XmlNil, "nil-first", "nil-first"),
            })
            {
                var declared = FerruleValueMaps.Apply(input, target, table, Text("miss"));
                var untyped = FerruleValueMaps.Apply(input, null, table, Text("miss"));
                var isolated = FerruleValueMaps.ApplyUserFunction(input, target, table, Text("miss"));
                Console.WriteLine($"marker {input.Kind}: {input}, target {target}: declared={declared.Kind}:{declared}, untyped={untyped.Kind}:{untyped}, isolated={isolated.Kind}:{isolated}");
                Equal(Text(ordinary), declared);
                Equal(Text(preserved), untyped);
                Equal(Text(preserved), isolated);
            }
            var onlyJson = new[]
            {
                new FerruleValueMapEntry(FerruleValue.JsonNull, Text("json-first")),
                new FerruleValueMapEntry(FerruleValue.JsonNull, Text("json-second")),
            };
            var declaredMiss = FerruleValueMaps.Apply(FerruleValue.JsonNull, target, onlyJson, Text("miss"));
            var untypedMatch = FerruleValueMaps.Apply(FerruleValue.JsonNull, null, onlyJson, Text("miss"));
            var isolatedMatch = FerruleValueMaps.ApplyUserFunction(FerruleValue.JsonNull, target, onlyJson, Text("miss"));
            Console.WriteLine($"only-json target {target}: declared={declaredMiss.Kind}:{declaredMiss}, untyped={untypedMatch.Kind}:{untypedMatch}, isolated={isolatedMatch.Kind}:{isolatedMatch}");
            Equal(Text("miss"), declaredMiss);
            Equal(Text("json-first"), untypedMatch);
            Equal(Text("json-first"), isolatedMatch);
            foreach (var defaultValue in new FerruleValue?[] { null, FerruleValue.Null, FerruleValue.JsonNull, FerruleValue.XmlNil, Text(string.Empty) })
            {
                var expected = defaultValue ?? FerruleValue.Null;
                var declared = FerruleValueMaps.Apply(FerruleValue.JsonNull, target, Array.Empty<FerruleValueMapEntry>(), defaultValue);
                var untyped = FerruleValueMaps.Apply(FerruleValue.JsonNull, null, Array.Empty<FerruleValueMapEntry>(), defaultValue);
                var isolated = FerruleValueMaps.ApplyUserFunction(FerruleValue.JsonNull, target, Array.Empty<FerruleValueMapEntry>(), defaultValue);
                Console.WriteLine($"empty-table target {target}, default {(defaultValue.HasValue ? defaultValue.Value.Kind.ToString() : "None")}: declared={declared.Kind}:{declared}, untyped={untyped.Kind}:{untyped}, isolated={isolated.Kind}:{isolated}");
                Equal(expected, declared);
                Equal(expected, untyped);
                Equal(expected, isolated);
            }
            foreach (var output in new[] { FerruleValue.JsonNull, FerruleValue.XmlNil })
            {
                var actual = FerruleValueMaps.Apply(FerruleValue.JsonNull, target,
                    new[] { new FerruleValueMapEntry(FerruleValue.Null, output) });
                Console.WriteLine($"selected-output target {target}: {actual.Kind}:{actual}");
                Equal(output, actual);
            }
        }
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

internal static partial class Program
{
    private static FerruleOwnedScopeContext OwnedSimple(FerruleInstance primary, FerruleOwnedSourceLimits? limits = null) =>
        FerruleOwnedSourceRoots.Create(primary, Array.Empty<FerruleDeclaredNamedSourceRoot>(), limits ?? OwnedLimits).RootContext();
    private static FerruleOwnedScopeContext OwnedMarkers() => OwnedSimple(Group(Field("Rows", OwnedRepeated(
        Group(), Group(Field("Details", Scalar(FerruleValue.Null))),
        Group(Field("Details", Scalar(FerruleValue.JsonNull))), Group(Field("Details", OwnedRepeated())),
        Group(Field("Details", Group(Field("Value", Scalar(FerruleValue.XmlNil))))),
        Group(Field("Details", Group(Field("Value", Scalar(FerruleValue.JsonNull))))),
        Group(Field("Details", Scalar(FerruleValue.FromInt64(9)))), Group(Field("Details", Group()))))));
    private static void OwnedRuntimeFieldAndWalk()
    {
        var c = OwnedCollision();
        var parent = OwnedRows("R08.parents", c.IterateSource(OwnedPrimaryOwner, new[] { "Parents" }))[0];
        var children = OwnedRows("R08.children", parent.IterateSource(OwnedPrimaryOwner, new[] { "Parents", "Children" }));
        var child = children[0];
        OwnedValue("R08.child", Text("child-a"), child.ResolveField(OwnedPrimaryOwner, new[] { "Parents", "Children", "Value" }));
        OwnedValue("R08.parent", Text("parent-1"), child.ResolveField(OwnedPrimaryOwner, new[] { "Parents", "Value" }));
        OwnedValue("R09", FerruleValue.Null, children[1].ResolveField(OwnedPrimaryOwner, new[] { "Parents", "Children", "Value" }));
        var pin = OwnedRows("R11.setup", c.IterateSource(OwnedPrimaryOwner, new[] { "Parents", "Children" }))[0];
        OwnedValue("R11.parent", Text("parent-1"), pin.ResolveField(OwnedPrimaryOwner, new[] { "Parents", "Value" }, new[] { "Parents" }));
        OwnedValue("R11.child", Text("child-a"), pin.ResolveField(OwnedPrimaryOwner, new[] { "Parents", "Children", "Value" }, new[] { "Parents", "Children" }));
        var incompatible = OwnedError("R12", FerruleOwnedSourceErrorKind.IncompatibleFramePath,
            () => pin.ResolveField(OwnedPrimaryOwner, new[] { "Parents", "Children", "Value" }, new[] { "Children" }));
        OwnedPath(new[] { "Children" }, incompatible.Frame); OwnedPath(new[] { "Parents", "Children", "Value" }, incompatible.Path);
        var incompatibleOuter = OwnedError("R13", FerruleOwnedSourceErrorKind.IncompatibleFramePath,
            () => pin.ResolveField(OwnedPrimaryOwner, new[] { "Value" }, new[] { "Parents" }));
        OwnedPath(new[] { "Parents" }, incompatibleOuter.Frame); OwnedPath(new[] { "Value" }, incompatibleOuter.Path);
        var markers = OwnedRows("R22.setup", OwnedMarkers().IterateSource(OwnedPrimaryOwner, new[] { "Rows" }));
        Equal(8, markers.Count); var field = new[] { "Rows", "Details", "Value" };
        for (var i = 0; i < 3; i++) OwnedValue("R22.null" + i, FerruleValue.Null, markers[i].ResolveField(OwnedPrimaryOwner, field));
        var empty = OwnedError("R22.empty", FerruleOwnedSourceErrorKind.UniteratedRepetition, () => markers[3].ResolveField(OwnedPrimaryOwner, field));
        Equal<nuint?>(2, empty.Segment); OwnedPath(field, empty.Path);
        OwnedValue("R22.nil", FerruleValue.XmlNil, markers[4].ResolveField(OwnedPrimaryOwner, field));
        OwnedValue("R22.jsonNull", FerruleValue.JsonNull, markers[5].ResolveField(OwnedPrimaryOwner, field));
        var malformed = OwnedError("R23", FerruleOwnedSourceErrorKind.Traversal, () => markers[6].ResolveField(OwnedPrimaryOwner, field));
        var cause = (FerruleOwnedSourceTraversalException)malformed.InnerException!;
        Equal(FerruleOwnedSourceTraversalKind.CannotTraverse, cause.Error); Equal<nuint?>(2, cause.Segment);
        Equal(FerruleOwnedSourceInstanceKind.Scalar, cause.Found); OwnedPath(field, cause.Path);
        var structural = OwnedError("R24", FerruleOwnedSourceErrorKind.Traversal,
            () => markers[7].ResolveField(OwnedPrimaryOwner, new[] { "Rows", "Details" }));
        var terminal = (FerruleOwnedSourceTraversalException)structural.InnerException!;
        Equal(FerruleOwnedSourceTraversalKind.ExpectedScalar, terminal.Error);
        Equal(FerruleOwnedSourceInstanceKind.Group, terminal.Found); OwnedPath(new[] { "Rows", "Details" }, terminal.Path);
        var uniterated = OwnedSimple(Group(Field("Rows", OwnedRepeated(Group(Field("Value", Scalar(FerruleValue.FromInt64(7)))),
            Group(Field("Value", Scalar(FerruleValue.FromInt64(8)))))), Field("Empty", OwnedRepeated())));
        foreach (var collection in new[] { "Rows", "Empty" })
        {
            var e = OwnedError("R25." + collection, FerruleOwnedSourceErrorKind.UniteratedRepetition,
                () => uniterated.ResolveField(OwnedPrimaryOwner, new[] { collection, "Value" }));
            Equal<nuint?>(1, e.Segment); OwnedPath(new[] { collection, "Value" }, e.Path);
        }
        var absentChild = OwnedRows("R26.setup", c.IterateSource(OwnedPrimaryOwner, new[] { "Parents", "Children" }))[1];
        Equal(0, OwnedRows("R26", absentChild.IterateSource(OwnedPrimaryOwner, new[] { "Parents", "Children", "Missing" })).Count);
        var rows = OwnedRows("R27", c.IterateSource(OwnedPrimaryOwner, new[] { "Parents", "Children" })); Equal(3, rows.Count);
        var values = new[] { Text("child-a"), FerruleValue.Null, Text("child-c") };
        var outer = new nuint[] { 1, 1, 2 }; var inner = new nuint[] { 1, 2, 1 };
        for (var i = 0; i < 3; i++)
        {
            OwnedValue("R27.value" + i, values[i], rows[i].ResolveField(OwnedPrimaryOwner, new[] { "Parents", "Children", "Value" }));
            OwnedIndex("R27.outer" + i, outer[i], rows[i].Position(OwnedPrimaryOwner, new[] { "Parents" }));
            OwnedIndex("R27.inner" + i, inner[i], rows[i].Position(OwnedPrimaryOwner, new[] { "Parents", "Children" }));
            Equal(2, rows[i].FrameCount); OwnedPath(new[] { "Parents" }, rows[i].FrameAt(0).Path);
            OwnedPath(new[] { "Parents", "Children" }, rows[i].FrameAt(1).Path);
        }
        var scalar = OwnedSimple(Scalar(Text("scalar-root")));
        var scalarIdentity = OwnedRows("R28.walk", scalar.IterateSource(OwnedPrimaryOwner, Array.Empty<string>()));
        Equal(1, scalarIdentity.Count); Equal(0, scalarIdentity[0].FrameCount);
        OwnedValue("R28.value", Text("scalar-root"), scalar.ResolveField(OwnedPrimaryOwner, Array.Empty<string>()));
        OwnedError("R28.position", FerruleOwnedSourceErrorKind.InactiveSourceFrame, () => scalar.Position(OwnedPrimaryOwner));
        var unsupported = OwnedSimple(Group(Field("Good", Scalar(Text("good"))),
            Field("Documents", new FerruleDocumentSet(new[] { new FerruleDocument("one.xml", Group(Field("Value", Scalar(Text("document"))))) })),
            Field("Generated", new FerruleMappedSequence(new[] { Group(Field("Value", Scalar(Text("generated")))) }))));
        OwnedValue("R29.good", Text("good"), unsupported.ResolveField(OwnedPrimaryOwner, new[] { "Good" }));
        foreach (var name in new[] { "Documents", "Generated" })
        {
            var kind = name == "Documents" ? FerruleOwnedSourceInstanceKind.DocumentSet : FerruleOwnedSourceInstanceKind.MappedSequence;
            Equal<FerruleOwnedSourceInstanceKind?>(kind, OwnedError("R29.field." + name,
                FerruleOwnedSourceErrorKind.UnsupportedSourceKind, () => unsupported.ResolveField(OwnedPrimaryOwner, new[] { name, "Value" })).FoundInstance);
            Equal<FerruleOwnedSourceInstanceKind?>(kind, OwnedError("R29.walk." + name,
                FerruleOwnedSourceErrorKind.UnsupportedSourceKind, () => unsupported.IterateSource(OwnedPrimaryOwner, new[] { name })).FoundInstance);
        }
    }
    private static void OwnedRuntimePositionsAndCopies()
    {
        var c = OwnedCollision();
        var rootPin = OwnedError("R14.pin", FerruleOwnedSourceErrorKind.InactiveSourceFrame,
            () => c.ResolveField(OwnedPrimaryOwner, new[] { "Value" }, Array.Empty<string>()));
        OwnedPath(Array.Empty<string>(), rootPin.Collection);
        Equal(true, OwnedError("R14.position", FerruleOwnedSourceErrorKind.InactiveSourceFrame,
            () => c.Position(OwnedPrimaryOwner)).Collection is null);
        var flat = OwnedSimple(OwnedRepeated(Group(Field("Value", Scalar(Text("flat-a")))), Group(Field("Value", Scalar(Text("flat-b"))))));
        var flatRows = OwnedRows("R15.setup", flat.IterateSource(OwnedPrimaryOwner, Array.Empty<string>())); Equal(2, flatRows.Count);
        OwnedValue("R15.field", Text("flat-b"), flatRows[1].ResolveField(OwnedPrimaryOwner, new[] { "Value" }, Array.Empty<string>()));
        OwnedIndex("R15.explicit", 2, flatRows[1].Position(OwnedPrimaryOwner, Array.Empty<string>()));
        OwnedIndex("R15.deepest", 2, flatRows[1].Position(OwnedPrimaryOwner));
        var group = OwnedSimple(Group(Field("Value", Scalar(Text("single")))));
        var identityGroup = OwnedRows("R16.identity", group.IterateSource(OwnedPrimaryOwner, Array.Empty<string>()));
        Equal(1, identityGroup.Count); Equal(0, identityGroup[0].FrameCount);
        OwnedValue("R16.value", Text("single"), group.ResolveField(OwnedPrimaryOwner, new[] { "Value" }));
        OwnedPath(Array.Empty<string>(), OwnedError("R16.position", FerruleOwnedSourceErrorKind.InactiveSourceFrame,
            () => group.Position(OwnedPrimaryOwner, Array.Empty<string>())).Collection);
        var rows = OwnedRows("R18.setup", c.IterateSource(OwnedPrimaryOwner, new[] { "Parents", "Children" })); var child = rows[1];
        OwnedIndex("R18.deepest", 2, child.Position(OwnedPrimaryOwner)); OwnedIndex("R18.parent", 1, child.Position(OwnedPrimaryOwner, new[] { "Parents" }));
        OwnedIndex("R18.child", 2, child.Position(OwnedPrimaryOwner, new[] { "Parents", "Children" }));
        OwnedPath(new[] { "Children" }, OwnedError("R18.suffix", FerruleOwnedSourceErrorKind.InactiveSourceFrame,
            () => child.Position(OwnedPrimaryOwner, new[] { "Children" })).Collection);
        var identity = OwnedRows("R19.identity", child.IterateSource(OwnedPrimaryOwner, new[] { "Parents" })); Equal(1, identity.Count);
        Equal(2, identity[0].FrameCount);
        for (var i = 0; i < 2; i++)
        {
            OwnedRaw("R19.item" + i, new { same = ReferenceEquals(child.FrameAt(i).Item, identity[0].FrameAt(i).Item) });
            Equal(true, ReferenceEquals(child.FrameAt(i).Item, identity[0].FrameAt(i).Item));
            Equal(child.FrameAt(i).Owner, identity[0].FrameAt(i).Owner);
            OwnedPath(child.FrameAt(i).Path, identity[0].FrameAt(i).Path);
        }
        OwnedIndex("R19.deepest", 2, identity[0].Position(OwnedPrimaryOwner)); OwnedIndex("R19.outer", 1, identity[0].Position(OwnedPrimaryOwner, new[] { "Parents" }));
        OwnedValue("R19.value", FerruleValue.Null, identity[0].ResolveField(OwnedPrimaryOwner, new[] { "Parents", "Children", "Value" }));
        var copy = child.WithPosition(OwnedPrimaryOwner, null, 7);
        OwnedRows("R20.copy", new[] { copy });
        OwnedRaw("R20.registry", new { same = ReferenceEquals(child.Roots, copy.Roots) });
        Equal(true, ReferenceEquals(child.Roots, copy.Roots)); Equal(child.FrameCount, copy.FrameCount);
        for (var i = 0; i < copy.FrameCount; i++)
        { Equal(true, ReferenceEquals(child.FrameAt(i).Item, copy.FrameAt(i).Item));
          Equal(child.FrameAt(i).Owner, copy.FrameAt(i).Owner); OwnedPath(child.FrameAt(i).Path, copy.FrameAt(i).Path); }
        OwnedIndex("R20.original", 2, child.Position(OwnedPrimaryOwner)); OwnedIndex("R20.copy", 7, copy.Position(OwnedPrimaryOwner));
        OwnedIndex("R20.outer", 1, copy.Position(OwnedPrimaryOwner, new[] { "Parents" }));
        OwnedValue("R20.value", FerruleValue.Null, copy.ResolveField(OwnedPrimaryOwner, new[] { "Parents", "Children", "Value" }));
        var outer = OwnedRows("R21.setup", c.IterateSource(OwnedPrimaryOwner, new[] { "Parents" }))[0];
        Equal<nuint?>(0, OwnedError("R21", FerruleOwnedSourceErrorKind.InvalidPosition, () => outer.WithPosition(OwnedPrimaryOwner, null, 0)).RequestedIndex);
        var clone = child.CloneView(); OwnedRows("R31.clone", new[] { clone });
        OwnedRaw("R31.registry", new { same = ReferenceEquals(child.Roots, clone.Roots) });
        Equal(true, ReferenceEquals(child.Roots, clone.Roots)); Equal(child.FrameCount, clone.FrameCount);
        for (var i = 0; i < child.FrameCount; i++)
        { Equal(true, ReferenceEquals(child.FrameAt(i).Item, clone.FrameAt(i).Item));
          Equal(child.FrameAt(i).Owner, clone.FrameAt(i).Owner); OwnedPath(child.FrameAt(i).Path, clone.FrameAt(i).Path);
          Equal(child.FrameAt(i).Index, clone.FrameAt(i).Index); }
        OwnedIndex("R31.position", 2, child.Position(OwnedPrimaryOwner));
        OwnedValue("R31.primary", Text("primary"), child.ResolveField(OwnedPrimaryOwner, new[] { "Catalog", "Label" }));
        var request = new[] { "Rows" };
        var saved = OwnedRows("CS02.setup", OwnedSimple(Group(Field("Rows", OwnedRepeated(Group(Field("Value", Scalar(Text("row")))))))).IterateSource(OwnedPrimaryOwner, request))[0];
        request[0] = "Else";
        OwnedIndex("CS02.position", 1, saved.Position(OwnedPrimaryOwner, new[] { "Rows" }));
        OwnedValue("CS02.value", Text("row"), saved.ResolveField(OwnedPrimaryOwner, new[] { "Rows", "Value" }, new[] { "Rows" }));
    }
    private static void OwnedLimitError(string id, FerruleOwnedSourceLimitKind expected, Action operation)
    { Equal<FerruleOwnedSourceLimitKind?>(expected, OwnedError(id, FerruleOwnedSourceErrorKind.MetadataLimit, operation).LimitKind); }
    private static void OwnedRuntimeLimits()
    {
        var l = new FerruleOwnedSourceLimits(6, 2, 3, 4096, 3);
        var root = OwnedCollision(l);
        OwnedRaw("B01", new { roots = (ulong)root.Roots.RegistryCount }); Equal<nuint>(6, root.Roots.RegistryCount);
        OwnedLimitError("B02", FerruleOwnedSourceLimitKind.Roots, () => OwnedCollision(l with { MaxRoots = 5 }));
        var names = OwnedCollision(l with { MaxMetadataBytes = 35 });
        OwnedRaw("B03", new { utf8 = (ulong)names.Roots.RegistryText }); Equal<nuint>(35, names.Roots.RegistryText);
        OwnedLimitError("B04", FerruleOwnedSourceLimitKind.MetadataBytes, () => OwnedCollision(l with { MaxMetadataBytes = 34 }));
        Equal(3, OwnedRows("B05", root.IterateSource(OwnedPrimaryOwner, new[] { "Parents", "Children" })).Count);
        OwnedLimitError("B06", FerruleOwnedSourceLimitKind.Frames,
            () => OwnedCollision(l with { MaxActiveFrames = 1 }).IterateSource(OwnedPrimaryOwner, new[] { "Parents", "Children" }));
        var admitted = OwnedRows("B07.setup", root.IterateSource(OwnedPrimaryOwner, new[] { "Parents", "Children" }))[0];
        OwnedValue("B07", Text("child-a"), admitted.ResolveField(OwnedPrimaryOwner, new[] { "Parents", "Children", "Value" }));
        var narrow = OwnedCollision(l with { MaxPathSegments = 2 });
        var staged = OwnedRows("B08.setup", narrow.IterateSource(OwnedPrimaryOwner, new[] { "Parents", "Children" }))[0];
        OwnedLimitError("B08", FerruleOwnedSourceLimitKind.PathSegments,
            () => staged.ResolveField(OwnedPrimaryOwner, new[] { "Parents", "Children", "Value" }));
        Equal(3, OwnedRows("B09", root.IterateSource(OwnedPrimaryOwner, new[] { "Parents", "Children" })).Count);
        OwnedLimitError("B10", FerruleOwnedSourceLimitKind.WalkContexts,
            () => OwnedCollision(l with { MaxWalkContexts = 2 }).IterateSource(OwnedPrimaryOwner, new[] { "Parents", "Children" }));
        var overflow = OwnedError("B11", FerruleOwnedSourceErrorKind.MetadataArithmeticOverflow,
            () => OwnedSourceAccounting.Add(nuint.MaxValue, 1, FerruleOwnedSourceLimitKind.MetadataBytes));
        Equal<FerruleOwnedSourceLimitKind?>(FerruleOwnedSourceLimitKind.MetadataBytes, overflow.LimitKind);
        var byteCounter = new OwnedCounter(8, FerruleOwnedSourceLimitKind.MetadataBytes); byteCounter.Charge(7); byteCounter.Charge(1);
        OwnedIndex("B12.equal", 8, byteCounter.Used);
        OwnedLimitError("B12.over", FerruleOwnedSourceLimitKind.MetadataBytes, () => byteCounter.Charge(1));
        OwnedIndex("B12.unchanged", 8, byteCounter.Used);
        var minimal = new FerruleOwnedSourceLimits(1, 0, 0, 0, 0);
        OwnedIndex("K01", 7, OwnedSourceAccounting.CoreSlots(minimal));
        OwnedIndex("K01.csharp", 9, OwnedSourceAccounting.CSharpSlots(minimal));
        var slots = new FerruleOwnedSourceLimits(2, 2, 3, 0, 3);
        OwnedIndex("K02", 62, OwnedSourceAccounting.CoreSlots(slots));
        OwnedIndex("K02.csharp", 82, OwnedSourceAccounting.CSharpSlots(slots));
        Equal<FerruleOwnedSourceLimitKind?>(FerruleOwnedSourceLimitKind.MetadataSlots,
            OwnedError("K03", FerruleOwnedSourceErrorKind.MetadataArithmeticOverflow,
                () => FerruleOwnedSourceRoots.Create(Group(), Array.Empty<FerruleDeclaredNamedSourceRoot>(),
                    l with { MaxWalkContexts = nuint.MaxValue })).LimitKind);
        var counter = new OwnedCounter(3, FerruleOwnedSourceLimitKind.MetadataSlots);
        counter.Charge(2); OwnedIndex("K04.2", 2, counter.Used); counter.Charge(1); OwnedIndex("K04.3", 3, counter.Used);
        var refusal = OwnedError("K04.refusal", FerruleOwnedSourceErrorKind.MetadataLimit, () => counter.Charge(1));
        Equal<nuint?>(3, refusal.Used); Equal<nuint?>(1, refusal.Requested); Equal<nuint?>(3, refusal.Maximum);
        counter.Release(1); OwnedIndex("K04.release", 2, counter.Used); counter.Charge(1); OwnedIndex("K04.recharge", 3, counter.Used);
        var zero = new OwnedCounter(3, FerruleOwnedSourceLimitKind.MetadataSlots);
        OwnedError("K05", FerruleOwnedSourceErrorKind.MetadataArithmeticOverflow, () => zero.Release(1));
        OwnedIndex("K05.unchanged", 0, zero.Used);
    }
}
