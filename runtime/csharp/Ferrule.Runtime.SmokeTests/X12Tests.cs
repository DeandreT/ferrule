using System.Text;
using System.Text.Json.Nodes;
using Ferrule.Runtime;

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private const string X12Prefix =
        "ISA*00*          *00*          *ZZ*SENDER         *ZZ*RECEIVER       *261009*1200*U*00401*000000001*0*T*:~\n" +
        "GS*OW*SENDER*RECEIVER*20261009*1200*1*X*004010~\n" +
        "ST*940*0001~\n";
    private const string X12Trailer = "SE*3*0001~\nGE*1*1~\nIEA*1*000000001~\n";
    private const string X12UnicodeWire = X12Prefix + "LIN*0006*VN*Véndor/🙂0007~\n" + X12Trailer;

    private static JsonObject X12ScalarSchema(string name, string type = "string", string? fixedValue = null)
    {
        var node = new JsonObject
        {
            ["name"] = name,
            ["kind"] = new JsonObject { ["kind"] = "scalar", ["ty"] = type },
        };
        if (fixedValue is not null) node["fixed"] = fixedValue;
        return node;
    }

    private static JsonObject X12GroupSchema(string name, params JsonNode[] children) => new()
    {
        ["name"] = name,
        ["kind"] = new JsonObject
        {
            ["kind"] = "group",
            ["children"] = new JsonArray(children.Select(child => child.DeepClone()).ToArray()),
        },
    };

    private static JsonObject X12EnvelopeSchema(string name, int width) => X12GroupSchema(name,
        Enumerable.Range(1, width).Select(index => (JsonNode)X12ScalarSchema($"{name}{index:00}",
            fixedValue: (name, index) switch { ("ISA", 12) => "00401", ("GS", 8) => "004010", _ => null })).ToArray());

    private static JsonObject X12RootSchema(params JsonNode[] body) => X12GroupSchema("Root",
        [X12EnvelopeSchema("ISA", 16), X12EnvelopeSchema("GS", 8), X12EnvelopeSchema("ST", 2),
         .. body, X12EnvelopeSchema("SE", 2), X12EnvelopeSchema("GE", 2), X12EnvelopeSchema("IEA", 2)]);

    private static JsonObject X12LinSchema()
    {
        var identifier = X12ScalarSchema("LIN03");
        identifier["string_length_range"] = new JsonObject { ["minimum"] = 12, ["maximum"] = 12 };
        return X12GroupSchema("LIN", X12ScalarSchema("LIN01"), X12ScalarSchema("LIN02", fixedValue: "VN"), identifier);
    }

    private static string X12Descriptor(JsonObject schema, JsonArray? constraints = null, JsonObject? separators = null)
        => new JsonObject
        {
            ["version"] = "004010", ["schema"] = schema.ToJsonString(),
            ["separators"] = separators, ["constraints"] = constraints ?? new JsonArray(),
        }.ToJsonString();

    private static JsonObject X12SchemaChild(JsonObject group, string name)
        => (JsonObject)((JsonArray)group["kind"]!["children"]!).Single(node => node!["name"]!.GetValue<string>() == name)!;

    private static FerruleGroup X12InstanceGroup(FerruleInstance root, string name)
    {
        if (root is not FerruleGroup group || !group.TryGetField(name, out FerruleInstance? child)
            || child is not FerruleGroup result) throw new InvalidOperationException("Missing literal X12 test group.");
        return result;
    }

    private static FerruleGroup X12Replace(FerruleGroup group, string name, FerruleInstance? value)
        => new(group.Fields.Where(field => field.Name != name)
            .Concat(value is null ? [] : new[] { Field(name, value) }));

    private static FerruleX12Exception X12Error(FerruleX12Error expected, Action action)
    {
        try { action(); }
        catch (FerruleX12Exception error) { Equal(expected, error.Error); return error; }
        throw new InvalidOperationException($"Expected X12 boundary error {expected}.");
    }

    private static void X12LiteralUnicodeAndSyntax()
    {
        string descriptor = X12Descriptor(X12RootSchema(X12LinSchema()));
        FerruleInstance instance = FerruleX12.ParseEmbedded(descriptor, X12UnicodeWire);
        var line = X12InstanceGroup(instance, "LIN");
        if (!line.TryGetField("LIN01", out var identity) || identity is not FerruleScalar scalar)
            throw new InvalidOperationException("Missing literal source identity.");
        Equal("0006", scalar.Value.StringValue);
        Equal(X12UnicodeWire, FerruleX12.SerializeEmbedded(descriptor, instance));
        Equal(Convert.ToHexString(Encoding.UTF8.GetBytes(X12UnicodeWire)),
            Convert.ToHexString(FerruleX12.SerializeEmbeddedBytes(descriptor,
                FerruleX12.ParseEmbeddedBytes(descriptor, Encoding.UTF8.GetBytes(X12UnicodeWire)))));

        string lfWire = X12UnicodeWire.Replace("~\n", "\n", StringComparison.Ordinal);
        string lfDescriptor = X12Descriptor(X12RootSchema(X12LinSchema()), separators:
            new JsonObject { ["element"] = "*", ["component"] = ":", ["segment"] = "\n" });
        Equal(lfWire, FerruleX12.SerializeEmbedded(lfDescriptor, FerruleX12.ParseEmbedded(lfDescriptor, lfWire)));
        X12Error(FerruleX12Error.Syntax, () => FerruleX12.ParseEmbedded(lfDescriptor, X12UnicodeWire));
        X12Error(FerruleX12Error.Syntax, () => FerruleX12.ParseEmbedded(descriptor, "\uFEFF" + X12UnicodeWire));
        X12Error(FerruleX12Error.Encoding, () => FerruleX12.ParseEmbeddedBytes(descriptor, [0xff]));
        X12Error(FerruleX12Error.Encoding, () => FerruleX12.ParseEmbedded(descriptor, X12UnicodeWire + "\ud800"));
        X12Error(FerruleX12Error.Envelope, () => FerruleX12.ParseEmbedded(descriptor,
            X12UnicodeWire.Replace("SE*3*0001", "SE*4*0001", StringComparison.Ordinal)));
        X12Error(FerruleX12Error.Envelope, () => FerruleX12.ParseEmbedded(descriptor,
            X12UnicodeWire.Replace("IEA*1*000000001", "IEA*1*000000002", StringComparison.Ordinal)));
        X12Error(FerruleX12Error.UnsupportedProfile, () => FerruleX12.ParseEmbedded(descriptor,
            X12UnicodeWire.Replace("*004010~", "*005010~", StringComparison.Ordinal)));
        X12Error(FerruleX12Error.Schema, () => FerruleX12.ParseEmbedded(descriptor,
            X12UnicodeWire.Replace("Véndor/🙂0007", "Véndor:🙂0007", StringComparison.Ordinal)));
    }

    private static void X12FixedLiteralsAndControlOwnership()
    {
        JsonObject schema = X12RootSchema(X12LinSchema());
        var controls = new (string Segment, string Field, string Value)[]
        {
            ("ISA", "ISA13", "000000001"), ("GS", "GS06", "1"), ("ST", "ST02", "0001"),
            ("SE", "SE02", "0001"), ("GE", "GE02", "1"), ("IEA", "IEA02", "000000001"),
        };
        foreach (var control in controls)
            X12SchemaChild(X12SchemaChild(schema, control.Segment), control.Field)["fixed"] = control.Value;
        string descriptor = X12Descriptor(schema);
        var parsed = (FerruleGroup)FerruleX12.ParseEmbedded(descriptor, X12UnicodeWire);
        var line = X12Replace(X12InstanceGroup(parsed, "LIN"), "LIN02", Scalar(FerruleValue.Null));
        Equal(X12UnicodeWire, FerruleX12.SerializeEmbedded(descriptor, X12Replace(parsed, "LIN", line)));
        foreach (var control in controls)
        {
            var missing = X12Replace(X12InstanceGroup(parsed, control.Segment), control.Field, null);
            X12Error(FerruleX12Error.Envelope, () => FerruleX12.SerializeEmbedded(descriptor,
                X12Replace(parsed, control.Segment, missing)));
        }

        JsonObject numeric = X12RootSchema(X12GroupSchema("QTY", X12ScalarSchema("QTY01", fixedValue: "EA"),
            X12ScalarSchema("QTY02", "int", "002"), X12ScalarSchema("QTY03", "float", "2.00")));
        string numericDescriptor = X12Descriptor(numeric);
        const string numericWire = X12Prefix + "QTY*EA*002*2.00~\n" + X12Trailer;
        var numericInstance = (FerruleGroup)FerruleX12.ParseEmbedded(numericDescriptor, numericWire);
        Equal(numericWire, FerruleX12.SerializeEmbedded(numericDescriptor, numericInstance));
        var invalidQuantity = X12Replace(X12InstanceGroup(numericInstance, "QTY"), "QTY02",
            Scalar(FerruleValue.FromInt64(3)));
        X12Error(FerruleX12Error.Value, () => FerruleX12.SerializeEmbedded(numericDescriptor,
            X12Replace(numericInstance, "QTY", invalidQuantity)));
        var nonfinite = X12Replace(X12InstanceGroup(numericInstance, "QTY"), "QTY03",
            Scalar(FerruleValue.FromDouble(double.PositiveInfinity)));
        X12Error(FerruleX12Error.Value, () => FerruleX12.SerializeEmbedded(numericDescriptor,
            X12Replace(numericInstance, "QTY", nonfinite)));

        const string emptyWire = X12Prefix + "SE*2*0001~\nGE*1*1~\nIEA*1*000000001~\n";
        var envelope = (FerruleGroup)FerruleX12.ParseEmbedded(X12Descriptor(X12RootSchema()), emptyWire);
        var missingNumeric = X12Replace(envelope, "QTY", Group(Field("QTY01", Scalar(Text("EA")))));
        missingNumeric = X12Replace(missingNumeric, "SE",
            X12Replace(X12InstanceGroup(envelope, "SE"), "SE01", Scalar(Text("3"))));
        foreach (var invalid in new (string Type, string Fixed)[]
        {
            ("int", "abc"), ("int", "9223372036854775808"),
            ("float", "NaN"), ("float", "Infinity"), ("float", "1e9999"),
        })
        {
            string invalidDescriptor = X12Descriptor(X12RootSchema(X12GroupSchema("QTY",
                X12ScalarSchema("QTY01", fixedValue: "EA"), X12ScalarSchema("QTY02", invalid.Type, invalid.Fixed))));
            X12Error(FerruleX12Error.Schema, () => FerruleX12.ParseEmbedded(invalidDescriptor,
                X12Prefix + "QTY*EA*" + invalid.Fixed + "~\n" + X12Trailer));
            X12Error(FerruleX12Error.Schema, () => FerruleX12.SerializeEmbedded(invalidDescriptor, missingNumeric));
        }
    }

    private static void X12EnvelopeIdentityOwnership()
    {
        string descriptor = X12Descriptor(X12RootSchema(X12LinSchema()));
        foreach (string blankWire in new[]
        {
            X12UnicodeWire.Replace("*SENDER         *", "*               *", StringComparison.Ordinal),
            X12UnicodeWire.Replace("*RECEIVER       *", "*               *", StringComparison.Ordinal),
            X12UnicodeWire.Replace("GS*OW*SENDER*RECEIVER*", "GS*OW**RECEIVER*", StringComparison.Ordinal),
            X12UnicodeWire.Replace("GS*OW*SENDER*RECEIVER*", "GS*OW*SENDER**", StringComparison.Ordinal),
        })
            X12Error(FerruleX12Error.Envelope, () => FerruleX12.ParseEmbedded(descriptor, blankWire));

        var parsed = (FerruleGroup)FerruleX12.ParseEmbedded(descriptor, X12UnicodeWire);
        foreach (var identity in new (string Segment, string Field)[]
        {
            ("ISA", "ISA06"), ("ISA", "ISA08"), ("GS", "GS02"), ("GS", "GS03"),
        })
        {
            foreach (FerruleInstance? value in new FerruleInstance?[] { null, Scalar(Text(" ")) })
            {
                var group = X12Replace(X12InstanceGroup(parsed, identity.Segment), identity.Field, value);
                X12Error(FerruleX12Error.Envelope, () => FerruleX12.SerializeEmbedded(descriptor,
                    X12Replace(parsed, identity.Segment, group)));
            }
        }
    }

    private static void X12SchemaAndValueRefusals()
    {
        JsonObject schema = X12RootSchema(X12LinSchema());
        X12SchemaChild(schema, "LIN")["nullable"] = true;
        X12Error(FerruleX12Error.Schema, () => FerruleX12.ParseEmbedded(X12Descriptor(schema), X12UnicodeWire));
        schema = X12RootSchema(X12LinSchema());
        var fields = (JsonArray)X12SchemaChild(schema, "LIN")["kind"]!["children"]!;
        fields.Add(fields[0]!.DeepClone());
        X12Error(FerruleX12Error.Schema, () => FerruleX12.ParseEmbedded(X12Descriptor(schema), X12UnicodeWire));

        string ordinary = X12Descriptor(X12RootSchema(X12LinSchema()));
        var extra = X12Error(FerruleX12Error.Schema, () => FerruleX12.ParseEmbedded(ordinary,
            X12UnicodeWire.Replace("Véndor/🙂0007~", "Véndor/🙂0007*PRIVATE-TOKEN~", StringComparison.Ordinal)));
        Equal(false, extra.Message.Contains("PRIVATE-TOKEN", StringComparison.Ordinal));

        JsonObject unqualified = X12LinSchema();
        X12SchemaChild(unqualified, "LIN02").Remove("fixed");
        var constraint = new JsonArray(new JsonObject
        {
            ["path"] = new JsonArray("LIN", "LIN02"), ["min_chars"] = 2, ["max_chars"] = 2,
            ["allowed_values"] = new JsonArray("VN", "BP"),
        });
        string constrained = X12Descriptor(X12RootSchema(unqualified), constraint);
        X12Error(FerruleX12Error.Value, () => FerruleX12.ParseEmbedded(constrained,
            X12UnicodeWire.Replace("*VN*", "*XX*", StringComparison.Ordinal)));
        X12Error(FerruleX12Error.Value, () => FerruleX12.ParseEmbedded(constrained,
            X12UnicodeWire.Replace("*VN*", "**", StringComparison.Ordinal)));

        JsonObject composite = X12GroupSchema("C001",
            Enumerable.Range(1, 1025).Select(index => (JsonNode)X12ScalarSchema($"Part{index}")).ToArray());
        string excessive = X12Descriptor(X12RootSchema(X12GroupSchema("LIN", composite)));
        X12Error(FerruleX12Error.ResourceLimit, () => FerruleX12.ParseEmbedded(excessive, X12UnicodeWire));
        string boundedComposite = X12Descriptor(X12RootSchema(X12GroupSchema("LIN",
            X12GroupSchema("C001", X12ScalarSchema("Part1"), X12ScalarSchema("Part2")))));
        X12Error(FerruleX12Error.ResourceLimit, () => FerruleX12.ParseEmbedded(boundedComposite,
            X12Prefix + "LIN*" + new string(':', 1024) + "~\n" + X12Trailer));
        X12Error(FerruleX12Error.ResourceLimit, () => FerruleX12.ParseEmbedded(
            new string('x', FerruleX12.MaximumSchemaBytes + 1), X12UnicodeWire));
    }

    private static void X12SharedLoopInstanceBudget()
    {
        JsonObject Loop(string name)
        {
            JsonObject segment = X12GroupSchema("N1", X12ScalarSchema("N101"));
            segment["repeating"] = true;
            JsonObject loop = X12GroupSchema(name, segment);
            loop["repeating"] = true;
            return loop;
        }
        string emptyWire = X12Prefix + "SE*2*0001~\nGE*1*1~\nIEA*1*000000001~\n";
        string descriptor = X12Descriptor(X12RootSchema(Loop("LoopA"), Loop("LoopB")));
        var empty = (FerruleGroup)FerruleX12.ParseEmbedded(descriptor, emptyWire);
        FerruleGroup item = Group(Field("N1", new FerruleRepeated([])));
        FerruleGroup WithLoops(int secondCount) => X12Replace(X12Replace(empty, "LoopA",
            new FerruleRepeated(Enumerable.Repeat<FerruleInstance>(item, 50_000))), "LoopB",
            new FerruleRepeated(Enumerable.Repeat<FerruleInstance>(item, secondCount)));
        Equal(emptyWire, FerruleX12.SerializeEmbedded(descriptor, WithLoops(50_000)));
        X12Error(FerruleX12Error.ResourceLimit, () => FerruleX12.SerializeEmbedded(descriptor, WithLoops(50_001)));
    }

    private static void X12OutputBytePreflight()
    {
        // A small shared input crosses the wire-byte cap when expanded by the
        // output shape. The boundary must refuse before a giant joined field
        // or aggregate StringBuilder is allocated.
        string shared = new('x', 65_536);
        string emptyWire = X12Prefix + "SE*2*0001~\nGE*1*1~\nIEA*1*000000001~\n";
        var envelope = (FerruleGroup)FerruleX12.ParseEmbedded(X12Descriptor(X12RootSchema()), emptyWire);

        JsonObject composite = X12GroupSchema("C001", Enumerable.Range(1, 1024)
            .Select(index => (JsonNode)X12ScalarSchema($"Part{index}")).ToArray());
        string compositeDescriptor = X12Descriptor(X12RootSchema(X12GroupSchema("LIN", composite)));
        var components = new FerruleGroup(Enumerable.Range(1, 1024)
            .Select(index => Field($"Part{index}", Scalar(Text(shared)))));
        var compositeInstance = X12Replace(envelope, "LIN", Group(Field("C001", components)));
        compositeInstance = X12Replace(compositeInstance, "SE",
            X12Replace(X12InstanceGroup(envelope, "SE"), "SE01", Scalar(Text("3"))));
        X12AllocationBoundedRefusal(compositeDescriptor, compositeInstance);

        JsonObject repeatedSegment = X12GroupSchema("N1", X12ScalarSchema("N101"));
        repeatedSegment["repeating"] = true;
        string cumulativeDescriptor = X12Descriptor(X12RootSchema(repeatedSegment));
        FerruleGroup segment = Group(Field("N101", Scalar(Text(shared))));
        var cumulativeInstance = X12Replace(envelope, "N1",
            new FerruleRepeated(Enumerable.Repeat<FerruleInstance>(segment, 1025)));
        cumulativeInstance = X12Replace(cumulativeInstance, "SE",
            X12Replace(X12InstanceGroup(envelope, "SE"), "SE01", Scalar(Text("1027"))));
        X12AllocationBoundedRefusal(cumulativeDescriptor, cumulativeInstance);

        // The composite fits the per-field limit but exceeds the remaining
        // document budget after an earlier segment. It must not be joined.
        JsonObject remainingComposite = X12GroupSchema("C001", Enumerable.Range(1, 1023)
            .Select(index => (JsonNode)X12ScalarSchema($"Part{index}")).ToArray());
        string remainingDescriptor = X12Descriptor(X12RootSchema(
            X12GroupSchema("N1", X12ScalarSchema("N101")), X12GroupSchema("LIN", remainingComposite)));
        var remainingComponents = new FerruleGroup(Enumerable.Range(1, 1023)
            .Select(index => Field($"Part{index}", Scalar(Text(shared)))));
        var remainingInstance = X12Replace(X12Replace(envelope, "N1", segment), "LIN",
            Group(Field("C001", remainingComponents)));
        remainingInstance = X12Replace(remainingInstance, "SE",
            X12Replace(X12InstanceGroup(envelope, "SE"), "SE01", Scalar(Text("4"))));
        X12AllocationBoundedRefusal(remainingDescriptor, remainingInstance);
    }

    private static void X12AllocationBoundedRefusal(string descriptor, FerruleInstance instance)
    {
        long before = GC.GetAllocatedBytesForCurrentThread();
        X12Error(FerruleX12Error.ResourceLimit, () => FerruleX12.SerializeEmbedded(descriptor, instance));
        long allocated = GC.GetAllocatedBytesForCurrentThread() - before;
        if (allocated >= 16 * 1024 * 1024)
            throw new InvalidOperationException("The finite X12 refusal fixture materialized a large output buffer.");
    }
}
