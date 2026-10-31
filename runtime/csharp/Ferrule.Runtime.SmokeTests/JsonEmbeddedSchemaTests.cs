using System.Text;
using Ferrule.Runtime;

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private const string EmbeddedV2Prefix = "FERRULE-EMBEDDED-SCHEMA/2\n";
    private const string LowBits = "\"FERRULE-F64-BITS:0031fa182c40c60d\"";
    private const string HighBits = "\"FERRULE-F64-BITS:0031fa182c40c60e\"";

    private static void GeneratedEmbeddedSchemaDescriptors()
    {
        var low = BitConverter.Int64BitsToDouble(0x0031fa182c40c60d);
        var high = BitConverter.Int64BitsToDouble(0x0031fa182c40c60e);
        var lowRange = EmbeddedV2Prefix +
            """{"name":"Value","numeric_range":{"kind":"number","bounds":{"minimum":{"value":__LOW__},"maximum":{"value":__LOW__}}},"kind":{"kind":"scalar","ty":"float"}}"""
                .Replace("__LOW__", LowBits, StringComparison.Ordinal);
        var highRange = lowRange.Replace(LowBits, HighBits, StringComparison.Ordinal);

        Equal("1e-307\n", FerruleJson.SerializeEmbedded(
            lowRange, Scalar(FerruleValue.FromDouble(low))));
        Equal(true,
            Encoding.UTF8.GetBytes("1e-307\n").SequenceEqual(
                FerruleJson.SerializeEmbeddedBytes(
                    lowRange, Scalar(FerruleValue.FromDouble(low)))));
        Error(FerruleRuntimeError.JsonBoundary, () => FerruleJson.SerializeEmbedded(
            lowRange, Scalar(FerruleValue.FromDouble(high))));
        _ = FerruleJson.ParseEmbedded(lowRange, "1e-307");
        _ = FerruleJson.ParseEmbeddedBytes(lowRange, Encoding.UTF8.GetBytes("1e-307"));
        Error(FerruleRuntimeError.JsonBoundary, () => FerruleJson.ParseEmbedded(
            highRange, "1e-307"));
        _ = FerruleJson.ParseEmbedded(highRange, "1.0000000000000001e-307");
        _ = FerruleJson.ParseEmbeddedBytes(
            highRange, Encoding.UTF8.GetBytes("1.0000000000000001e-307"));
        Error(FerruleRuntimeError.JsonBoundary, () => FerruleJson.ParseEmbedded(
            lowRange, "1.0000000000000001e-307"));

        var allowed = EmbeddedV2Prefix +
            """{"name":"Value","json_allowed_values":[{"type":"float","value":__LOW__},{"type":"float","value":__HIGH__}],"kind":{"kind":"scalar","ty":"float"}}"""
                .Replace("__LOW__", LowBits, StringComparison.Ordinal)
                .Replace("__HIGH__", HighBits, StringComparison.Ordinal);
        _ = FerruleJson.SerializeEmbedded(allowed, Scalar(FerruleValue.FromDouble(low)));
        _ = FerruleJson.SerializeEmbedded(allowed, Scalar(FerruleValue.FromDouble(high)));

        var alternative = EmbeddedV2Prefix +
            """{"name":"Root","kind":{"kind":"group","children":[{"name":"X","kind":{"kind":"scalar","ty":"float"}}],"alternatives":[{"members":["X"],"required":["X"],"constraints":[{"member":"X","value":{"type":"float","value":__LOW__}}]}]}}"""
                .Replace("__LOW__", LowBits, StringComparison.Ordinal);
        _ = FerruleJson.SerializeEmbedded(alternative,
            Group(Field("X", Scalar(FerruleValue.FromDouble(low)))));
        Error(FerruleRuntimeError.JsonBoundary, () => FerruleJson.SerializeEmbedded(
            alternative, Group(Field("X", Scalar(FerruleValue.FromDouble(high))))));

        var contains = EmbeddedV2Prefix +
            """{"name":"Values","repeating":true,"json_contains":[{"predicate":{"kind":"schema","schema":{"name":"item","numeric_range":{"kind":"number","bounds":{"minimum":{"value":__LOW__},"maximum":{"value":__LOW__}}},"kind":{"kind":"scalar","ty":"float"}}},"range":{"minimum":1}}],"kind":{"kind":"scalar","ty":"float"}}"""
                .Replace("__LOW__", LowBits, StringComparison.Ordinal);
        _ = FerruleJson.SerializeEmbedded(contains,
            new FerruleRepeated(new[] { Scalar(FerruleValue.FromDouble(low)) }));
        Error(FerruleRuntimeError.JsonBoundary, () => FerruleJson.SerializeEmbedded(
            contains, new FerruleRepeated(new[] { Scalar(FerruleValue.FromDouble(high)) })));

        var dynamic = EmbeddedV2Prefix +
            """{"name":"Root","kind":{"kind":"group","children":[],"dynamic":{"name":"*","numeric_range":{"kind":"number","bounds":{"minimum":{"value":__LOW__},"maximum":{"value":__LOW__}}},"kind":{"kind":"scalar","ty":"float"}}}}"""
                .Replace("__LOW__", LowBits, StringComparison.Ordinal);
        _ = FerruleJson.SerializeEmbedded(dynamic,
            Group(Field("arbitrary", Scalar(FerruleValue.FromDouble(low)))));
        Error(FerruleRuntimeError.JsonBoundary, () => FerruleJson.SerializeEmbedded(
            dynamic, Group(Field("arbitrary", Scalar(FerruleValue.FromDouble(high))))));

        var dependent = EmbeddedV2Prefix +
            """{"name":"Root","json_dependent_schemas":[{"trigger":"Trigger","predicate":{"kind":"schema","schema":{"name":"predicate","kind":{"kind":"group","children":[{"name":"Trigger","kind":{"kind":"scalar","ty":"bool"}},{"name":"Value","numeric_range":{"kind":"number","bounds":{"minimum":{"value":__LOW__},"maximum":{"value":__LOW__}}},"kind":{"kind":"scalar","ty":"float"}}],"required":["Value"]}}}}],"kind":{"kind":"group","children":[{"name":"Trigger","kind":{"kind":"scalar","ty":"bool"}},{"name":"Value","kind":{"kind":"scalar","ty":"float"}}]}}"""
                .Replace("__LOW__", LowBits, StringComparison.Ordinal);
        _ = FerruleJson.SerializeEmbedded(dependent, Group(
            Field("Trigger", Scalar(FerruleValue.FromBoolean(true))),
            Field("Value", Scalar(FerruleValue.FromDouble(low)))));
        var invalidDependent = Group(
            Field("Trigger", Scalar(FerruleValue.FromBoolean(true))),
            Field("Value", Scalar(FerruleValue.FromDouble(high))));
        Error(FerruleRuntimeError.JsonBoundary, () =>
            FerruleJson.SerializeEmbedded(dependent, invalidDependent));

        var xml = EmbeddedV2Prefix +
            """{"name":"Root","kind":{"kind":"group","children":[{"name":"Amount","numeric_range":{"kind":"number","bounds":{"minimum":{"value":__LOW__}}},"kind":{"kind":"scalar","ty":"float"}}]}}"""
                .Replace("__LOW__", LowBits, StringComparison.Ordinal);
        var xmlInstance = Group(Field("Amount", Scalar(FerruleValue.FromDouble(low))));
        var xmlValue = FerruleXml.SerializeEmbedded(
            7, xml, xmlInstance, false, false, null);
        Equal(FerruleXml.Serialize(7, xml[EmbeddedV2Prefix.Length..],
            xmlInstance, false, false, null), xmlValue);

        foreach (var malformed in new[]
                 {
                     lowRange.Replace("0031fa182c40c60d", "0031FA182C40C60D", StringComparison.Ordinal),
                     lowRange.Replace("0031fa182c40c60d", "0031fa182c40c60", StringComparison.Ordinal),
                     lowRange.Replace("0031fa182c40c60d", "7ff0000000000000", StringComparison.Ordinal),
                     lowRange.Replace(LowBits, "1e-307", StringComparison.Ordinal),
                     lowRange.Replace(EmbeddedV2Prefix, "FERRULE-EMBEDDED-SCHEMA/6\n", StringComparison.Ordinal),
                 })
        {
            Error(FerruleRuntimeError.JsonBoundary, () => FerruleJson.ParseEmbedded(malformed, "1e-307"));
        }
        Error(FerruleRuntimeError.JsonBoundary, () => FerruleJson.Parse(
            lowRange, "1e-307"));
        Error(FerruleRuntimeError.JsonBoundary, () => FerruleJson.Serialize(
            lowRange, Scalar(FerruleValue.FromDouble(low))));
        Error(FerruleRuntimeError.JsonBoundary, () => FerruleJson.ParseEmbedded(
            lowRange[EmbeddedV2Prefix.Length..], "1e-307"));
        var markerName = EmbeddedV2Prefix +
            """{"name":"FERRULE-F64-BITS:0031fa182c40c60d","kind":{"kind":"scalar","ty":"string"}}""";
        Equal(Text("text"), ((FerruleScalar)FerruleJson.ParseEmbedded(
            markerName, "\"text\"")).Value);
        var atLimit = lowRange + new string(' ',
            FerruleJson.MaximumSchemaBytes - Encoding.UTF8.GetByteCount(lowRange));
        _ = FerruleJson.SerializeEmbedded(atLimit,
            Scalar(FerruleValue.FromDouble(low)));
        var overLimit = atLimit + " ";
        Error(FerruleRuntimeError.JsonBoundary, () => FerruleJson.ParseEmbedded(
            overLimit, "1e-307"));
        Error(FerruleRuntimeError.XmlSerialization,
            () => FerruleXml.Serialize(7, xml,
                Group(Field("Amount", Scalar(FerruleValue.FromDouble(low)))),
                false, false, null));
        var malformedXml = EmbeddedV2Prefix +
            """{"name":"Root","kind":{"kind":"group","children":[],"alternatives":["invalid"]}}""";
        Error(FerruleRuntimeError.XmlSerialization,
            () => FerruleXml.SerializeEmbedded(7, malformedXml,
                Group(), false, false, null));
    }

    private const string EmbeddedV5Prefix = "FERRULE-EMBEDDED-SCHEMA/5\n";

    // An open [string, integer] tuple with a string tail and length 0..4.
    private const string TupleFrame =
        """{"name":"line","json_tuple":{"length":{"maximum":4},"tail":"open"},"kind":{"kind":"group","children":[{"name":"item1","kind":{"kind":"scalar","ty":"string"}},{"name":"item2","kind":{"kind":"scalar","ty":"int"}},{"name":"rest","repeating":true,"kind":{"kind":"scalar","ty":"string"}}]}}""";

    private static void GeneratedEmbeddedTupleFrames()
    {
        var descriptor = EmbeddedV5Prefix + TupleFrame;
        var line = FerruleJson.ParseEmbedded(descriptor, """["a",1,"b"]""");
        Equal(Text("a"), ScalarPathResolver.Resolve(line, "item1"));
        Equal(FerruleValue.FromInt64(1), ScalarPathResolver.Resolve(line, "item2"));
        Equal("[\n  \"a\",\n  1,\n  \"b\"\n]\n", FerruleJson.SerializeEmbedded(descriptor, line));
        var shortLine = FerruleJson.ParseEmbedded(descriptor, """["a"]""");
        Equal(FerruleValue.Null, ScalarPathResolver.Resolve(shortLine, "item2"));
        Equal("[\"a\"]", FerruleJson.SerializeEmbeddedCompact(descriptor, shortLine));

        Equal(
            "JSON tuple 'line' requires at most 4 items, got 5.",
            Error(FerruleRuntimeError.JsonBoundary,
                () => FerruleJson.ParseEmbedded(descriptor, """["a",1,"b","c","d"]""")).Message);
        var position = Error(FerruleRuntimeError.JsonBoundary,
            () => FerruleJson.ParseEmbedded(descriptor, """["a","x"]"""));
        Equal("JSON tuple 'line' position 2: JSON field 'item2' expected integer, got String.", position.Message);
        Equal(true, position.InnerException is FerruleRuntimeException);
        Equal(
            "JSON tuple 'line' cannot write position 1 as absent before a later present item.",
            Error(FerruleRuntimeError.JsonBoundary, () => FerruleJson.SerializeEmbedded(descriptor, Group(
                Field("item2", Scalar(FerruleValue.FromInt64(1)))))).Message);
        Equal(
            "JSON tuple 'line' does not declare field 'item3'.",
            Error(FerruleRuntimeError.JsonBoundary, () => FerruleJson.SerializeEmbedded(descriptor, Group(
                Field("item3", Scalar(Text("x")))))).Message);
        Equal(
            "Non-repeating JSON tuple frame root 'line' cannot hold root rows or JSON Lines records.",
            Error(FerruleRuntimeError.JsonBoundary, () => FerruleJson.SerializeEmbedded(
                descriptor, new FerruleRepeated(new[] { line }))).Message);

        // A missing single frame property is absent and stays omitted.
        var document = EmbeddedV5Prefix +
            """{"name":"Doc","kind":{"kind":"group","children":[__FRAME__,{"name":"id","kind":{"kind":"scalar","ty":"int"}}]}}"""
                .Replace("__FRAME__", TupleFrame, StringComparison.Ordinal);
        var withoutLine = FerruleJson.ParseEmbedded(document, """{"id":1}""");
        Equal(FerruleValue.Null, ((FerruleScalar)((FerruleGroup)withoutLine).Fields[0].Value).Value);
        Equal("{\"id\":1}", FerruleJson.SerializeEmbeddedCompact(document, withoutLine));
        Equal("{\"line\":[],\"id\":1}", FerruleJson.SerializeEmbeddedCompact(
            document, FerruleJson.ParseEmbedded(document, """{"line":[],"id":1}""")));

        // A member's own array assertions apply inside its position.
        const string uniqueMember =
            """{"name":"Row","json_tuple":{"length":{"maximum":2},"tail":"closed"},"kind":{"kind":"group","children":[{"name":"item1","kind":{"kind":"scalar","ty":"string"}},{"name":"item2","kind":{"kind":"group","children":[{"name":"codes","repeating":true,"json_unique_items":true,"kind":{"kind":"scalar","ty":"int"}}]}}]}}""";
        _ = FerruleJson.ParseEmbedded(EmbeddedV5Prefix + uniqueMember, """["a",{"codes":[1,2]}]""");
        foreach (var duplicate in new[] { """["a",{"codes":[1,1]}]""", """[0,{"codes":[1,1]}]""" })
        {
            Equal("JSON array 'codes' violates uniqueItems at item 2.",
                Error(FerruleRuntimeError.JsonBoundary, () => FerruleJson.ParseEmbedded(
                    EmbeddedV5Prefix + uniqueMember, duplicate)).Message);
        }
        Equal("JSON tuple 'Row' position 1: JSON field 'item1' expected string, got Number.",
            Error(FerruleRuntimeError.JsonBoundary, () => FerruleJson.ParseEmbedded(
                EmbeddedV5Prefix + uniqueMember, """[0,{"codes":[1,2]}]""")).Message);

        const string recursiveWithoutUnique =
            """{"name":"Root","kind":{"kind":"group","children":[{"name":"point","json_tuple":{"length":{"maximum":2},"tail":"closed"},"kind":{"kind":"group","children":[{"name":"item1","kind":{"kind":"scalar","ty":"string"}},{"name":"item2","kind":{"kind":"scalar","ty":"int"}}]}},{"name":"next","recursive_ref":"Root","container_nullable":true,"kind":{"kind":"group","children":[]}}]}}""";
        var deepNext = "null";
        for (var index = 0; index < 65; index++) deepNext = "{\"next\":" + deepNext + "}";
        Equal("JSON tuple 'point' position 1: JSON field 'item1' expected string, got Number.",
            Error(FerruleRuntimeError.JsonBoundary, () => FerruleJson.ParseEmbedded(
                EmbeddedV5Prefix + recursiveWithoutUnique,
                "{\"point\":[0],\"next\":" + deepNext + "}")).Message);
        var unusedPredicate = recursiveWithoutUnique.Replace(
            "{\"name\":\"Root\",",
            """{"name":"Root","json_dependent_schemas":[{"trigger":"unused","predicate":{"kind":"schema","schema":{"name":"Predicate","kind":{"kind":"group","children":[{"name":"codes","repeating":true,"json_unique_items":true,"kind":{"kind":"scalar","ty":"int"}}]}}}}],""",
            StringComparison.Ordinal);
        Equal("JSON tuple 'point' position 1: JSON field 'item1' expected string, got Number.",
            Error(FerruleRuntimeError.JsonBoundary, () => FerruleJson.ParseEmbedded(
                EmbeddedV5Prefix + unusedPredicate,
                "{\"point\":[0],\"next\":" + deepNext + "}")).Message);

        foreach (var malformed in new[]
                 {
                     // Identical members and tail describe a homogeneous array.
                     TupleFrame.Replace("\"ty\":\"int\"", "\"ty\":\"string\"", StringComparison.Ordinal),
                     // A closed frame's maximum is its member count.
                     TupleFrame.Replace("\"tail\":\"open\"", "\"tail\":\"closed\"", StringComparison.Ordinal),
                     TupleFrame.Replace("\"name\":\"item2\"", "\"name\":\"second\"", StringComparison.Ordinal),
                     TupleFrame.Replace("\"name\":\"rest\",\"repeating\":true", "\"name\":\"rest\"", StringComparison.Ordinal),
                     // A repeating frame owns no uniqueItems, and must state each tuple's null.
                     TupleFrame.Replace("\"json_tuple\":{", "\"repeating\":true,\"json_object_item_nullable\":false,\"json_unique_items\":true,\"json_tuple\":{", StringComparison.Ordinal),
                     TupleFrame.Replace("\"json_tuple\":{", "\"repeating\":true,\"json_tuple\":{", StringComparison.Ordinal),
                     TupleFrame.Replace("\"tail\":\"open\"", "\"tail\":\"open\",\"extra\":1", StringComparison.Ordinal),
                 })
        {
            var message = Error(FerruleRuntimeError.JsonBoundary,
                () => FerruleJson.ParseEmbedded(EmbeddedV5Prefix + malformed, "[]")).Message;
            if (!message.Contains("has invalid JSON tuple frame metadata", StringComparison.Ordinal))
            {
                throw new InvalidOperationException($"Expected a tuple frame refusal, got {message}");
            }
        }
        const string recursive =
            """{"name":"T","json_tuple":{"length":{"maximum":2},"tail":"closed"},"kind":{"kind":"group","children":[{"name":"item1","kind":{"kind":"scalar","ty":"string"}},{"name":"item2","recursive_ref":"T","kind":{"kind":"group","children":[]}}]}}""";
        Equal(
            "Embedded JSON recursive reference 'item2' expands tuple frame 'T' through one of its own positions; recursive tuple frames are not supported.",
            Error(FerruleRuntimeError.JsonBoundary,
                () => FerruleJson.ParseEmbedded(EmbeddedV5Prefix + recursive, """["a"]""")).Message);

        // Graph-level JSON functions take plain descriptors without a version,
        // so they refuse frames before reading their input.
        var functionSchema =
            """{"name":"Payload","kind":{"kind":"group","children":[__FRAME__,{"name":"Rows","repeating":true,"kind":{"kind":"group","children":[{"name":"Qty","kind":{"kind":"scalar","ty":"int"}}]}}]}}"""
                .Replace("__FRAME__", TupleFrame, StringComparison.Ordinal);
        foreach (var (function, arguments) in new (string, FerruleValue[])[]
                 {
                     ("json_parse_field", new[] { Text("not JSON"), Text(functionSchema), Text("""["line","item1"]""") }),
                     ("json_parse_object_exists", new[] { Text("not JSON"), Text(functionSchema), Text("[]") }),
                     ("json_parse_array_length", new[] { Text("not JSON"), Text(functionSchema), Text("""["Rows"]""") }),
                     ("json_parse_array_field", new[] { Text("not JSON"), Text(functionSchema), Text("""["Rows"]"""), Text("""["Qty"]"""), FerruleValue.FromInt64(1) }),
                 })
        {
            var error = Error(FerruleRuntimeError.FunctionInvalidArgument,
                () => FerruleFunctions.Call(function, arguments));
            Equal(function, error.Function);
            Equal("schema descriptor contains a JSON tuple frame", error.Detail);
        }
    }
}
