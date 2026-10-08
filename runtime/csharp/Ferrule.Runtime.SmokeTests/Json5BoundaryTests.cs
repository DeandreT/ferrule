using System.Globalization;
using System.Text;
using System.Text.Json;
using System.Text.Json.Nodes;
using Ferrule.Runtime;

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private static string Json5Descriptor(string profile) => profile switch
    {
        "I" => "{\"name\":\"Root\",\"kind\":{\"kind\":\"group\",\"children\":[{\"name\":\"n\",\"kind\":{\"kind\":\"scalar\",\"ty\":\"int\"}}]}}",
        "F" => "{\"name\":\"Root\",\"kind\":{\"kind\":\"group\",\"children\":[{\"name\":\"n\",\"kind\":{\"kind\":\"scalar\",\"ty\":\"float\"}}]}}",
        "N" => "{\"name\":\"Root\",\"kind\":{\"kind\":\"group\",\"children\":[{\"name\":\"n\",\"nullable\":true,\"kind\":{\"kind\":\"scalar\",\"ty\":\"float\"}}]}}",
        "S" => "{\"name\":\"Root\",\"kind\":{\"kind\":\"group\",\"children\":[{\"name\":\"n\",\"kind\":{\"kind\":\"scalar\",\"ty\":\"string\"}}]}}",
        "B" => "{\"name\":\"Root\",\"kind\":{\"kind\":\"group\",\"children\":[{\"name\":\"n\",\"kind\":{\"kind\":\"scalar\",\"ty\":\"bool\"}}]}}",
        "U" => "{\"name\":\"Root\",\"kind\":{\"kind\":\"group\",\"children\":[{\"name\":\"雪\",\"kind\":{\"kind\":\"scalar\",\"ty\":\"int\"}}]}}",
        "K" => "{\"name\":\"Root\",\"kind\":{\"kind\":\"group\",\"children\":[{\"name\":\"n\",\"kind\":{\"kind\":\"scalar\",\"ty\":\"int\"}},{\"name\":\"m\",\"kind\":{\"kind\":\"scalar\",\"ty\":\"int\"}}]}}",
        _ => throw new InvalidOperationException("Unknown literal profile."),
    };

    private static object Json5Tree(FerruleInstance instance) => instance switch
    {
        FerruleGroup group => new { kind = "Group", fields = group.Fields.Select(field => new { name = field.Name, instance = Json5Tree(field.Value) }).ToArray() },
        FerruleScalar scalar => new { kind = "Scalar", value = Json5Value(scalar.Value) },
        _ => throw new InvalidOperationException("Unexpected instance outside closed profile."),
    };
    private static Dictionary<string, object> Json5Value(FerruleValue value)
    {
        var result = new Dictionary<string, object>();
        result["tag"] = value.Kind switch { FerruleValueKind.Int64 => "Int", FerruleValueKind.Double => "Float", _ => value.Kind.ToString() };
        switch (value.Kind)
        {
            case FerruleValueKind.Int64: result["decimal"] = value.Int64Value.ToString(CultureInfo.InvariantCulture); break;
            case FerruleValueKind.Double: result["bits_hex"] = unchecked((ulong)BitConverter.DoubleToInt64Bits(value.DoubleValue)).ToString("x16", CultureInfo.InvariantCulture); break;
            case FerruleValueKind.String: result["text"] = value.StringValue; break;
            case FerruleValueKind.Bool: result["value"] = value.BooleanValue; break;
        }
        return result;
    }
    private static object? Json5Cause(Exception? error) => error is null ? null : new
    {
        type = error.GetType().FullName, fullOriginal = error.ToString(),
        publicFields = error.GetType().GetProperties().Where(property => property.Name != "InnerException" && property.GetIndexParameters().Length == 0)
            .ToDictionary(property => property.Name, property => property.GetValue(error)?.ToString()),
        encoderIndex = (error as EncoderFallbackException)?.Index,
        encoderCodeUnit = error is EncoderFallbackException encoder ? (int?)encoder.CharUnknown : null,
        decoderIndex = (error as DecoderFallbackException)?.Index,
        inner = Json5Cause(error.InnerException),
    };
    private sealed record Json5BoundaryObserved(string? Text, byte[]? Bytes, FerruleInstance? Typed, Exception? Error, int MappingCalls);
    private static Json5BoundaryObserved Json5Observe(
        string id, string? original, byte[]? originalBytes, string sourceSchema, string targetSchema,
        bool bytes, Func<FerruleInstance, FerruleInstance>? mapping = null)
    {
        string? text = null; byte[]? output = null; FerruleInstance? typed = null; FerruleInstance? mapped = null; Exception? error = null; var calls = 0;
        FerruleInstance Apply(FerruleInstance input) { typed = input; calls++; mapped = mapping is null ? input : mapping(input); return mapped; }
        try
        {
            if (bytes) { output = FerruleJson5.ExecuteBytes(sourceSchema, targetSchema, originalBytes!, Apply); }
            else { text = FerruleJson5.Execute(sourceSchema, targetSchema, original!, Apply); }
        }
        catch (Exception caught) { error = caught; }
        Console.WriteLine(JsonSerializer.Serialize(new
        {
            id, originalUtf16Units = original?.Select(value => (int)value).ToArray(), originalHex = originalBytes is null ? null : Convert.ToHexString(originalBytes),
            sourceSchema, targetSchema, text, outputHex = output is null ? null : Convert.ToHexString(output),
            typed = typed is null ? null : Json5Tree(typed), mapped = mapped is null ? null : Json5Tree(mapped), calls, error = Json5Cause(error),
        }));
        return new(text, output, typed, error, calls);
    }
    private static FerruleJson5BoundaryException Json5Stage(Json5BoundaryObserved observed, FerruleJson5Stage expected)
    {
        Equal((string?)null, observed.Text); Equal((byte[]?)null, observed.Bytes);
        if (observed.Error is not FerruleJson5BoundaryException error)
        { throw new InvalidOperationException("Expected retained JSON5 boundary exception.", observed.Error); }
        Equal(expected, error.Stage); return error;
    }

    private static void Json5BoundaryCorpus()
    {
        foreach (var item in Json5BoundaryCases)
        {
            var descriptor = Json5Descriptor(item.Profile);
            for (var route = 0; route < 4; route++)
            {
                var context = new FerruleExecutionContext("/logical/active.json");
                var observed = Json5Observe(item.Id + "/" + route, item.Input, Encoding.UTF8.GetBytes(item.Input), descriptor, descriptor, route >= 2,
                    route % 2 == 0 ? null : input => { _ = context.MappingFilePath; return input; });
                Equal("/logical/active.json", context.MappingFilePath);
                if (item.Stage is { } stage)
                {
                    var error = Json5Stage(observed, stage); Equal(0, observed.MappingCalls);
                    Equal(item.Kind, error.SyntaxKind);
                    if (item.Utf8Offset is { } offset) { Equal<long?>(offset, error.Utf8Offset); }
                    if (item.InputKind == "RootObjectRequired")
                    {
                        if (error.InnerException is not FerruleJson5SchemaException { Kind: FerruleJson5SchemaKind.RootObjectRequired })
                        { throw new InvalidOperationException("Original root-object refusal missing."); }
                    }
                    else if (stage == FerruleJson5Stage.Input)
                    {
                        if (error.InnerException is not FerruleRuntimeException { Error: FerruleRuntimeError.JsonBoundary })
                        { throw new InvalidOperationException("Original typed projection cause missing."); }
                    }
                }
                else
                {
                    Equal((Exception?)null, observed.Error); Equal(1, observed.MappingCalls);
                    Equal(item.Output, observed.Text ?? Encoding.UTF8.GetString(observed.Bytes!));
                    if (observed.Bytes is { } output) { Equal(Convert.ToHexString(Encoding.UTF8.GetBytes(item.Output!)), Convert.ToHexString(output)); }
                    Equal(true, JsonNode.DeepEquals(JsonNode.Parse(item.Typed!), JsonSerializer.SerializeToNode(Json5Tree(observed.Typed!))));
                }
            }
        }
        Equal(75, Json5BoundaryCases.Length);
    }

    private static void Json5BoundaryEncodingAndPriority()
    {
        var descriptor = Json5Descriptor("I");
        foreach (var bytes in new byte[][] { [0x7b, 0x6e, 0x3a, 0xff, 0x7d], [0xff, 0xfe, 0x7b, 0], [0xed, 0xa0, 0x80] })
        {
            var observed = Json5Observe("invalid bytes", null, bytes, "bad schema", descriptor, true);
            var error = Json5Stage(observed, FerruleJson5Stage.Encoding);
            if (error.InnerException is not DecoderFallbackException decoder) { throw new InvalidOperationException("Original decoder cause missing."); }
            Equal<long?>(decoder.Index, error.Utf8Offset); Equal(0, observed.MappingCalls);
        }
        foreach (var (text, index, offset) in new[] { ("雪\ud800", 1, 3L), ("'😀'\udc00", 4, 6L) })
        {
            var observed = Json5Observe("invalid text", text, null, "bad schema", descriptor, false);
            var error = Json5Stage(observed, FerruleJson5Stage.Encoding);
            Equal<int?>(index, error.Utf16Index); Equal<long?>(offset, error.Utf8Offset);
            if (error.InnerException is not FerruleJson5SyntaxException { InnerException: EncoderFallbackException encoder })
            { throw new InvalidOperationException("Actual encoder chain missing."); }
            Equal(0, encoder.Index); Equal((int)text[index], (int)encoder.CharUnknown);
        }
        foreach (var bytes in new[] { false, true })
        {
            var nullInput = Json5Observe("null input", null, null, descriptor, descriptor, bytes);
            if (nullInput.Error is not ArgumentNullException argument) { throw new InvalidOperationException("Null argument wrapper changed."); }
            Equal("source", argument.ParamName); Equal(0, nullInput.MappingCalls);
            var source = Json5Observe("source schema before syntax", "{n:NaN}", Encoding.UTF8.GetBytes("{n:NaN}"), "invalid", descriptor, bytes);
            var sourceError = Json5Stage(source, FerruleJson5Stage.Schema); Equal<FerruleJson5SchemaSide?>(FerruleJson5SchemaSide.Source, sourceError.SchemaSide);
            if (sourceError.InnerException is not JsonException) { throw new InvalidOperationException("Initial strict descriptor parse cause missing."); }
            var target = Json5Observe("target schema before syntax", "{n:NaN}", Encoding.UTF8.GetBytes("{n:NaN}"), descriptor, "invalid", bytes);
            Equal<FerruleJson5SchemaSide?>(FerruleJson5SchemaSide.Target, Json5Stage(target, FerruleJson5Stage.Schema).SchemaSide);
        }
        var depth = "[" + new string('[', 127) + "0" + new string(']', 128);
        var depthError = Json5Stage(Json5Observe("depth128", depth, null, descriptor, descriptor, false), FerruleJson5Stage.Syntax);
        Equal<FerruleJson5Resource?>(FerruleJson5Resource.SyntaxDepth, depthError.Resource);
        Equal<long?>(128, depthError.Requested); Equal<long?>(127, depthError.Maximum);
    }

    private static void Json5BoundaryDescriptorIdentity()
    {
        var descriptor = Json5Descriptor("I");
        foreach (var prefix in new[] { "", "FERRULE-EMBEDDED-SCHEMA/2\n" })
        {
            foreach (var raw in new[]
            {
                "{\"name\":\"R\",\"na\\u006de\":\"R\",\"kind\":{\"kind\":\"group\",\"children\":[]}}",
                "{\"name\":\"R\",\"kind\":{\"kind\":\"group\",\"kind\":\"group\",\"children\":[]}}",
                "{\"name\":\"R\",\"future\":{\"x\":1,\"x\":2},\"kind\":{\"kind\":\"group\",\"children\":[]}}",
            })
            {
                var observed = Json5Observe("duplicate descriptor", "{n:NaN}", null, prefix + raw, descriptor, false);
                var error = Json5Stage(observed, FerruleJson5Stage.Schema);
                if (error.InnerException is not FerruleJson5SchemaException typed) { throw new InvalidOperationException("Typed profile refusal missing."); }
                Equal(FerruleJson5SchemaKind.DuplicateDescriptorField, typed.Kind);
            }
        }
        foreach (var (raw, field) in new (string, string)[]
        {
            ("{\"name\":\"R\",\"kind\":{\"kind\":\"group\",\"children\":[],\"future\":true},\"ki\\u006ed\":{\"kind\":\"group\",\"children\":[]}}", "kind"),
            ("{\"name\":\"R\",\"repeating\":true,\"repeating\":false,\"kind\":{\"kind\":\"group\",\"children\":[]}}", "repeating"),
            ("{\"name\":\"R\",\"雪\":0,\"\\u96ea\":1,\"kind\":{\"kind\":\"group\",\"children\":[]}}", "雪"),
        })
        {
            var error = Json5Stage(Json5Observe("hidden decoded field", "{}", null, raw, descriptor, false), FerruleJson5Stage.Schema);
            if (error.InnerException is not FerruleJson5SchemaException typed) { throw new InvalidOperationException("Typed duplicate cause missing."); }
            Equal(FerruleJson5SchemaKind.DuplicateDescriptorField, typed.Kind); Equal(field, typed.Field);
        }
        var malformed = "{\"name\":\"R\",\"name\":\"R\",\"kind\":";
        var first = Json5Stage(Json5Observe("parse before duplicates", "{}", null, malformed, descriptor, false), FerruleJson5Stage.Schema);
        if (first.InnerException is not JsonException) { throw new InvalidOperationException("Complete parse priority changed."); }
        foreach (var raw in new[]
        {
            "{\"name\":\"R\",\"name\":\"R\",\"ignored\":1e400,\"kind\":{\"kind\":\"group\",\"children\":[]}}",
            "{\"name\":\"R\",\"name\":\"R\",\"ignored\":\"\\uD800\",\"kind\":{\"kind\":\"group\",\"children\":[]}}",
        })
        {
            var error = Json5Stage(Json5Observe("complete scalar parse before duplicate", "{}", null, raw, descriptor, false), FerruleJson5Stage.Schema);
            if (error.InnerException is not JsonException) { throw new InvalidOperationException("Deferred scalar parse priority changed."); }
        }
        var defaultMetadata = descriptor.Replace("\"name\":\"Root\",", "\"name\":\"Root\",\"repeating\":false,\"json_formats\":[],\"fixed\":null,");
        var accepted = Json5Observe("ordinary defaults", "{n:7}", null, defaultMetadata, descriptor, false);
        Equal((Exception?)null, accepted.Error); Equal("{\n  \"n\": 7\n}\n", accepted.Text);
        var nestedDefaults = descriptor
            .Replace("\"name\":\"Root\",", "\"name\":\"Root\",\"xml_namespace\":null,\"recursive_ref\":null,\"numeric_range\":null,\"default\":null,")
            .Replace("\"name\":\"n\",", "\"name\":\"n\",\"fixed\":null,\"recursive_ref\":null,")
            .Replace("\"kind\":\"group\",", "\"kind\":\"group\",\"dynamic\":null,");
        foreach (var prefix in new[] { "", "FERRULE-EMBEDDED-SCHEMA/2\n" })
        {
            var defaults = Json5Observe("nested ordinary defaults " + prefix, "{n:7}", null,
                prefix + nestedDefaults, prefix + defaultMetadata, false);
            Equal((Exception?)null, defaults.Error); Equal(1, defaults.MappingCalls);
            Equal("{\n  \"n\": 7\n}\n", defaults.Text);
        }
        foreach (var raw in new[]
        {
            descriptor.Replace("\"name\":\"Root\",", "\"name\":\"Root\",\"future\":null,"),
            descriptor.Replace("\"name\":\"Root\",", "\"name\":\"Root\",\"repeating\":true,"),
            descriptor.Replace("\"children\":[", "\"required\":[\"n\",\"n\"],\"children\":["),
        }) { _ = Json5Stage(Json5Observe("unsupported profile", "{}", null, raw, descriptor, false), FerruleJson5Stage.Schema); }
        // Independent compact-size witness:251 nodes,1,024,004 name bytes.
        // A pretty reader projection would exceed1MiB despite valid admission.
        var wideChildren = string.Join(",", Enumerable.Range(0, 250).Select(index =>
            "{\"name\":\"" + index.ToString("D3", CultureInfo.InvariantCulture) + new string('x', 4093)
            + "\",\"kind\":{\"kind\":\"scalar\",\"ty\":\"int\"}}"));
        var compactNearLimit = "{\"name\":\"Root\",\"kind\":{\"kind\":\"group\",\"children\":[" + wideChildren + "]}}";
        Equal(1_036_052, Encoding.UTF8.GetByteCount(compactNearLimit));
        const string emptyTarget = "{\"name\":\"Root\",\"kind\":{\"kind\":\"group\",\"children\":[]}}";
        foreach (var prefix in new[] { "", "FERRULE-EMBEDDED-SCHEMA/2\n" })
        {
            foreach (var source in new[] { compactNearLimit,
                compactNearLimit.Replace("\"name\":\"Root\",", "\"name\":\"Root\",\"fixed\":null,") })
            {
                var nearLimit = Json5Observe("compact descriptor limit " + prefix, "{}", null,
                    prefix + source, prefix + emptyTarget, false, _ => new FerruleGroup([]));
                Equal((Exception?)null, nearLimit.Error); Equal(1, nearLimit.MappingCalls);
                Equal("{}\n", nearLimit.Text);
            }
        }
        var longName = descriptor.Replace("\"n\"", "\"" + new string('x', 4097) + "\"");
        var nameError = Json5Stage(Json5Observe("name bytes", "{}", null, longName, descriptor, false), FerruleJson5Stage.Schema);
        if (nameError.InnerException is not FerruleJson5SchemaException nameLimit) { throw new InvalidOperationException("Name limit cause missing."); }
        Equal<FerruleJson5SchemaResource?>(FerruleJson5SchemaResource.NameLength, nameLimit.Resource);
        Equal<long?>(4097, nameLimit.Requested); Equal<long?>(4096, nameLimit.Maximum);
        foreach (var count in new[] { 4095, 4096 })
        {
            var children = string.Join(",", Enumerable.Range(0, count).Select(index =>
                "{\"name\":\"n" + index.ToString(CultureInfo.InvariantCulture) + "\",\"kind\":{\"kind\":\"scalar\",\"ty\":\"int\"}}"));
            var source = "{\"name\":\"R\",\"kind\":{\"kind\":\"group\",\"children\":[" + children + "]}}";
            const string empty = "{\"name\":\"R\",\"kind\":{\"kind\":\"group\",\"children\":[]}}";
            var observed = Json5Observe("schema nodes " + count, "{}", null, source, empty, false, _ => new FerruleGroup([]));
            if (count == 4095) { Equal((Exception?)null, observed.Error); Equal(1, observed.MappingCalls); Equal("{}\n", observed.Text); }
            else
            {
                var error = Json5Stage(observed, FerruleJson5Stage.Schema);
                if (error.InnerException is not FerruleJson5SchemaException limit) { throw new InvalidOperationException("Schema-node cap cause missing."); }
                Equal<FerruleJson5SchemaResource?>(FerruleJson5SchemaResource.SchemaNodes, limit.Resource);
                Equal<long?>(4097, limit.Requested); Equal<long?>(4096, limit.Maximum); Equal(0, observed.MappingCalls);
            }
        }
        var huge = descriptor + new string(' ', 1024 * 1024 + 1 - Encoding.UTF8.GetByteCount(descriptor));
        var descriptorError = Json5Stage(Json5Observe("descriptor bytes", "{}", null, huge, descriptor, false), FerruleJson5Stage.Schema);
        Equal<FerruleJson5Resource?>(FerruleJson5Resource.EmbeddedSchemaBytes, descriptorError.Resource);
        Equal<long?>(1048577, descriptorError.Requested); Equal<long?>(1048576, descriptorError.Maximum);
    }

    private static void Json5BoundaryNestedMappingAndOutput()
    {
        const string nested = "{\"name\":\"R\",\"kind\":{\"kind\":\"group\",\"children\":[{\"name\":\"inner\",\"kind\":{\"kind\":\"group\",\"children\":[{\"name\":\"n\",\"kind\":{\"kind\":\"scalar\",\"ty\":\"int\"}},{\"name\":\"s\",\"nullable\":true,\"kind\":{\"kind\":\"scalar\",\"ty\":\"string\"}}],\"required\":[\"n\"]}}]}}";
        var good = Json5Observe("nested required nullable", "{inner:{s:null,n:'old',n:7}}", null, nested, nested, false);
        Equal((Exception?)null, good.Error); Equal("{\n  \"inner\": {\n    \"n\": 7,\n    \"s\": null\n  }\n}\n", good.Text);
        _ = Json5Stage(Json5Observe("missing nested required", "{inner:{}}", null, nested, nested, false), FerruleJson5Stage.Input);
        var cause = new FerruleRuntimeException(FerruleRuntimeError.MappingException, "stop", node: 3, mappingExceptionMessage: "");
        var mapping = Json5Observe("mapping actual cause", "{n:7}", null, Json5Descriptor("I"), Json5Descriptor("I"), false, _ => throw cause);
        Equal(true, ReferenceEquals(cause, Json5Stage(mapping, FerruleJson5Stage.Mapping).InnerException)); Equal(1, mapping.MappingCalls);
        var invalidOutput = Json5Observe("typed output failure", "{n:7}", null, Json5Descriptor("I"), Json5Descriptor("I"), false,
            _ => new FerruleGroup([new("n", new FerruleScalar(FerruleValue.FromString("bad")))]));
        var outputError = Json5Stage(invalidOutput, FerruleJson5Stage.Output);
        if (outputError.InnerException is not FerruleRuntimeException { Error: FerruleRuntimeError.JsonBoundary })
        { throw new InvalidOperationException("Typed strict output cause missing."); }
        Equal(1, invalidOutput.MappingCalls);
        foreach (var value in new[] { double.NaN, double.PositiveInfinity, double.NegativeInfinity })
        {
            var original = Json5Observe("nonfinite typed output", "{n:1}", null, Json5Descriptor("F"), Json5Descriptor("F"), false,
                _ => new FerruleGroup([new("n", new FerruleScalar(FerruleValue.FromDouble(value)))]));
            var error = Json5Stage(original, FerruleJson5Stage.Output);
            if (error.InnerException is not FerruleRuntimeException { Error: FerruleRuntimeError.JsonBoundary })
            { throw new InvalidOperationException("Original nonfinite output cause missing."); }
        }

    }
}
