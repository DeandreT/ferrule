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
                     lowRange.Replace(EmbeddedV2Prefix, "FERRULE-EMBEDDED-SCHEMA/3\n", StringComparison.Ordinal),
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
}
