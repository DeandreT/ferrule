using Ferrule.Runtime;

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private const string PreparedBudgetPattern = "^(b{1000}|a*)$";

    // Interned like a generated descriptor constant.
    private const string PreparedBudgetSchema =
        "{\"name\":\"Value\",\"repeating\":true,\"json_patterns\":{\"any_of\":[[\"^(b{1000}|a*)$\"]]},\"kind\":{\"kind\":\"scalar\",\"ty\":\"string\"}}";

    private const string PreparedOrderSchema =
        "{\"name\":\"Order\",\"kind\":{\"kind\":\"group\",\"children\":[" +
        "{\"name\":\"Id\",\"json_patterns\":{\"any_of\":[[\"^ORD-[0-9]{6}$\"]]},\"kind\":{\"kind\":\"scalar\",\"ty\":\"string\"}}," +
        "{\"name\":\"Line\",\"repeating\":true,\"kind\":{\"kind\":\"group\",\"children\":[" +
        "{\"name\":\"Sku\",\"json_patterns\":{\"any_of\":[[\"^[A-Z]{2}-[0-9]{4}$\"]]},\"kind\":{\"kind\":\"scalar\",\"ty\":\"string\"}}," +
        "{\"name\":\"Quantity\",\"numeric_range\":{\"kind\":\"integer\",\"bounds\":{\"minimum\":1,\"maximum\":1000}},\"kind\":{\"kind\":\"scalar\",\"ty\":\"int\"}}," +
        "{\"name\":\"Price\",\"json_multiple_of\":{\"any_of\":[[{\"coefficient\":25,\"decimal_exponent\":-2}]]},\"kind\":{\"kind\":\"scalar\",\"ty\":\"float\"}}]}}," +
        "{\"name\":\"Tag\",\"repeating\":true,\"json_unique_items\":true,\"kind\":{\"kind\":\"scalar\",\"ty\":\"string\"}}]}}";

    private const string PreparedOrderDocument =
        "{\"Id\":\"ORD-000001\",\"Line\":[{\"Sku\":\"AB-0001\",\"Quantity\":2,\"Price\":1.25}],\"Tag\":[\"a\",\"b\"]}";

    /// <summary>`count` values that each use three fifths of one document's pattern work budget.</summary>
    private static string PreparedBudgetValues(int count)
    {
        var instructions = (ulong)FerruleJsonPattern.Compile(PreparedBudgetPattern).InstructionCount;
        var length = (int)(FerruleJsonPattern.MaximumBoundaryWork / instructions * 3 / 5);
        var value = "\"" + new string('a', length) + "\"";
        return "[" + string.Join(",", Enumerable.Repeat(value, count)) + "]";
    }

    /// <summary>A copy that misses the reference-keyed preparation and is parsed again by the call.</summary>
    private static string UnpreparedCopy(string descriptor) => new(descriptor.AsSpan());

    /// <summary>Every success and failure detail the boundary exposes.</summary>
    private static string PreparedOutcome(Func<string> call)
    {
        try
        {
            return "ok:" + call();
        }
        catch (FerruleRuntimeException error)
        {
            return $"error:{error.Error}:{error.Message}:{error.InnerException?.GetType().FullName}:{error.InnerException?.Message}";
        }
    }

    private static void JsonPreparedSchemasMatchUnpreparedCalls()
    {
        var oversized = new byte[FerruleJson.MaximumDocumentBytes + 1];
        oversized[0] = 0xFF;
        var cases = new (string Schema, string Document)[]
        {
            (PreparedOrderSchema, PreparedOrderDocument),
            (PreparedOrderSchema, "﻿{\"Id\":\"ORD-000001\"}"),
            (PreparedOrderSchema, "[{\"Id\":\"ORD-000001\"},{\"Id\":\"ORD-000002\"}]"),
            (PreparedOrderSchema, "{\"Id\":\"ORD-1\"}"),
            (PreparedOrderSchema, "{\"Id\":\"ORD-000001\",\"Line\":[{\"Sku\":\"ab-0001\"}]}"),
            (PreparedOrderSchema, "{\"Id\":\"ORD-000001\",\"Line\":[{\"Quantity\":0}]}"),
            (PreparedOrderSchema, "{\"Id\":\"ORD-000001\",\"Line\":[{\"Price\":1.3}]}"),
            (PreparedOrderSchema, "{\"Id\":\"ORD-000001\",\"Tag\":[\"a\",\"a\"]}"),
            (PreparedOrderSchema, "{\"Id\":\"ORD-000001\","),
            (PreparedBudgetSchema, PreparedBudgetValues(1)),
            (PreparedBudgetSchema, PreparedBudgetValues(2)),
            ("{\"name\":\"Value\",\"kind\":{\"kind\":\"unknown\"}}", "{}"),
            ("{", "{"),
        };
        foreach (var (schema, document) in cases)
        {
            var expected = PreparedOutcome(() => Render(schema, FerruleJson.Parse(UnpreparedCopy(schema), document)));
            Equal(expected, PreparedOutcome(() => Render(schema, FerruleJson.Parse(schema, document))));
            Equal(expected, PreparedOutcome(() => Render(schema, FerruleJson.Parse(schema, document))));
            var bytes = System.Text.Encoding.UTF8.GetBytes(document);
            Equal(
                PreparedOutcome(() => Render(schema, FerruleJson.ParseBytes(UnpreparedCopy(schema), bytes))),
                PreparedOutcome(() => Render(schema, FerruleJson.ParseBytes(schema, bytes))));
            // The document byte limit precedes every schema failure.
            Equal(
                PreparedOutcome(() => Render(schema, FerruleJson.ParseBytes(UnpreparedCopy(schema), oversized))),
                PreparedOutcome(() => Render(schema, FerruleJson.ParseBytes(schema, oversized))));
        }
        Equal(true, PreparedOutcome(() => Render(PreparedBudgetSchema, FerruleJson.Parse(PreparedBudgetSchema, PreparedBudgetValues(1)))).StartsWith("ok:", StringComparison.Ordinal));
        Equal(true, PreparedOutcome(() => Render(PreparedBudgetSchema, FerruleJson.Parse(PreparedBudgetSchema, PreparedBudgetValues(2)))).Contains("work limit", StringComparison.Ordinal));

        var instances = new FerruleInstance[]
        {
            FerruleJson.Parse(PreparedOrderSchema, PreparedOrderDocument),
            Group(Field("Id", Scalar(Text("ORD-1")))),
            Group(Field("Id", Scalar(Text("ORD-000001"))), Field("Tag", new FerruleRepeated(new FerruleInstance[] { Scalar(Text("a")), Scalar(Text("a")) }))),
            Scalar(FerruleValue.FromInt64(7)),
        };
        foreach (var instance in instances)
        {
            Equal(
                PreparedOutcome(() => FerruleJson.Serialize(UnpreparedCopy(PreparedOrderSchema), instance)),
                PreparedOutcome(() => FerruleJson.Serialize(PreparedOrderSchema, instance)));
        }

        // Exactly M pretty bytes, then M + 1.
        const string stringSchema = "{\"name\":\"Value\",\"kind\":{\"kind\":\"scalar\",\"ty\":\"string\"}}";
        foreach (var (length, accepted) in new[] { (FerruleJson.MaximumDocumentBytes - 3, true), (FerruleJson.MaximumDocumentBytes - 2, false) })
        {
            var instance = Scalar(Text(new string('a', length)));
            var expected = PreparedOutcome(() => FerruleJson.Serialize(UnpreparedCopy(stringSchema), instance));
            Equal(accepted, expected.StartsWith("ok:", StringComparison.Ordinal));
            Equal(expected, PreparedOutcome(() => FerruleJson.Serialize(stringSchema, instance)));
        }
    }

    /// <summary>Renders a parse result through an independent unprepared copy of its schema.</summary>
    private static string Render(string schema, FerruleInstance instance) =>
        FerruleJson.Serialize(UnpreparedCopy(schema), instance);

    /// <summary>
    /// Concurrent calls with different inputs reach different limits. Each call
    /// keeps its own pattern work budget, so exhausting one call's budget never
    /// consumes another's.
    /// </summary>
    private static void JsonPreparedSchemasIsolateConcurrentAccounting()
    {
        var within = PreparedBudgetValues(1);
        var exhausted = PreparedBudgetValues(2);
        var oversized = new byte[FerruleJson.MaximumDocumentBytes + 1];
        var expectedWithin = PreparedOutcome(() => Render(PreparedBudgetSchema, FerruleJson.Parse(UnpreparedCopy(PreparedBudgetSchema), within)));
        var expectedExhausted = PreparedOutcome(() => Render(PreparedBudgetSchema, FerruleJson.Parse(UnpreparedCopy(PreparedBudgetSchema), exhausted)));
        var expectedOversized = PreparedOutcome(() => Render(PreparedBudgetSchema, FerruleJson.ParseBytes(UnpreparedCopy(PreparedBudgetSchema), oversized)));
        var expectedOrder = PreparedOutcome(() => Render(PreparedOrderSchema, FerruleJson.Parse(UnpreparedCopy(PreparedOrderSchema), PreparedOrderDocument)));
        var invalidOutput = Group(Field("Id", Scalar(Text("ORD-1"))));
        var expectedInvalidOutput = PreparedOutcome(() => FerruleJson.Serialize(UnpreparedCopy(PreparedOrderSchema), invalidOutput));
        Equal(true, expectedWithin.StartsWith("ok:", StringComparison.Ordinal));
        Equal(true, expectedExhausted.Contains("work limit", StringComparison.Ordinal));
        Equal(true, expectedOversized.Contains("maximum", StringComparison.Ordinal));
        Equal(true, expectedInvalidOutput.StartsWith("error:", StringComparison.Ordinal));

        var calls = new Func<bool>[]
        {
            () => PreparedOutcome(() => Render(PreparedBudgetSchema, FerruleJson.Parse(PreparedBudgetSchema, within))) == expectedWithin,
            () => PreparedOutcome(() => Render(PreparedBudgetSchema, FerruleJson.Parse(PreparedBudgetSchema, exhausted))) == expectedExhausted,
            () => PreparedOutcome(() => Render(PreparedBudgetSchema, FerruleJson.ParseBytes(PreparedBudgetSchema, oversized))) == expectedOversized &&
                PreparedOutcome(() => Render(PreparedOrderSchema, FerruleJson.Parse(PreparedOrderSchema, PreparedOrderDocument))) == expectedOrder,
            () => PreparedOutcome(() => FerruleJson.Serialize(PreparedOrderSchema, invalidOutput)) == expectedInvalidOutput,
        };
        var threads = calls.Concat(calls)
            .Select(call =>
            {
                var result = new bool[1];
                var thread = new Thread(() => result[0] = Enumerable.Range(0, 12).All(_ => call()));
                thread.Start();
                return (thread, result);
            })
            .ToArray();
        foreach (var (thread, result) in threads)
        {
            thread.Join();
            Equal(true, result[0]);
        }
    }

    /// <summary>The padded order fixture of crates/codegen-runtime/tests/json_endpoint_preparation.rs.</summary>
    private static readonly string PreparedReportSchema = PreparedReportSchemaText();

    private static string PreparedReportSchemaText()
    {
        static string Patterned(string name, string pattern) =>
            $"{{\"name\":\"{name}\",\"json_patterns\":{{\"any_of\":[[\"{pattern}\"]]}},\"kind\":{{\"kind\":\"scalar\",\"ty\":\"string\"}}}}";
        static string Plain(string name, string type) =>
            $"{{\"name\":\"{name}\",\"kind\":{{\"kind\":\"scalar\",\"ty\":\"{type}\"}}}}";
        var children = new List<string>
        {
            Patterned("Id", "^ORD-[0-9]{6}$"),
            Plain("Customer", "string"),
            Patterned("Currency", "^[A-Z]{3}$"),
            Plain("Total", "float"),
            "{\"name\":\"Line\",\"repeating\":true,\"kind\":{\"kind\":\"group\",\"children\":[" +
            Patterned("Sku", "^[A-Z]{2}-[0-9]{4}$") + "," +
            "{\"name\":\"Quantity\",\"numeric_range\":{\"kind\":\"integer\",\"bounds\":{\"minimum\":1,\"maximum\":1000}},\"kind\":{\"kind\":\"scalar\",\"ty\":\"int\"}}," +
            Plain("Price", "float") + "," + Plain("Note", "string") + "]}}",
        };
        for (var index = 0; index < 48; index++)
        {
            var name = $"Attribute{index:00}";
            children.Add(index % 4 == 0 ? Patterned(name, $"^[a-z]{{1,{index + 8}}}$") : Plain(name, "string"));
        }
        return "{\"name\":\"Order\",\"kind\":{\"kind\":\"group\",\"children\":[" + string.Join(",", children) + "]}}";
    }

    private static string PreparedReportDocument(int lines)
    {
        var text = new System.Text.StringBuilder("{\"Id\":\"ORD-000042\",\"Customer\":\"Example Customer\",\"Currency\":\"EUR\",\"Total\":1234.5,\"Line\":[");
        for (var index = 0; index < lines; index++)
        {
            text.Append(index == 0 ? "" : ",")
                .Append(System.Globalization.CultureInfo.InvariantCulture, $"{{\"Sku\":\"AB-{index % 10_000:0000}\",\"Quantity\":{index % 999 + 1},\"Price\":{1.25 + index % 100},\"Note\":\"line {index} of the order\"}}");
        }
        return text.Append("],\"Attribute00\":\"alpha\",\"Attribute01\":\"beta\"}").ToString();
    }

    /// <summary>
    /// Allocation accounting per call. A fresh descriptor copy pays the schema
    /// preparation again; the shared descriptor does not. Set
    /// FERRULE_JSON_PREPARATION_REPORT=1 to print the table.
    /// </summary>
    private static void JsonPreparedSchemaAllocationAccounting()
    {
        var report = Environment.GetEnvironmentVariable("FERRULE_JSON_PREPARATION_REPORT") == "1";
        long Allocated(int calls, Action call)
        {
            call();
            var before = GC.GetAllocatedBytesForCurrentThread();
            for (var index = 0; index < calls; index++)
            {
                call();
            }
            return (GC.GetAllocatedBytesForCurrentThread() - before) / calls;
        }
        void Row(string label, long bytes)
        {
            if (report)
            {
                Console.WriteLine($"| {label} | {bytes} |");
            }
        }

        var small = PreparedReportDocument(3);
        var smallBytes = System.Text.Encoding.UTF8.GetBytes(small);
        var instance = FerruleJson.Parse(PreparedReportSchema, small);
        var parseUnprepared = Allocated(200, () => FerruleJson.Parse(UnpreparedCopy(PreparedReportSchema), small));
        var parsePrepared = Allocated(200, () => FerruleJson.Parse(PreparedReportSchema, small));
        var serializeUnprepared = Allocated(200, () => FerruleJson.Serialize(UnpreparedCopy(PreparedReportSchema), instance));
        var serializePrepared = Allocated(200, () => FerruleJson.Serialize(PreparedReportSchema, instance));
        if (report)
        {
            Console.WriteLine($"descriptor {System.Text.Encoding.UTF8.GetByteCount(PreparedReportSchema)} bytes; small document {smallBytes.Length} bytes");
            Console.WriteLine("| Operation | Allocated bytes |");
            Console.WriteLine("| --- | ---: |");
        }
        Row("`Parse`, small, fresh descriptor copy", parseUnprepared);
        Row("`Parse`, small, shared descriptor", parsePrepared);
        Row("`ParseBytes`, small, shared descriptor", Allocated(200, () => FerruleJson.ParseBytes(PreparedReportSchema, smallBytes)));
        Row("`Serialize`, small, fresh descriptor copy", serializeUnprepared);
        Row("`Serialize`, small, shared descriptor", serializePrepared);
        Equal(true, parsePrepared * 2 < parseUnprepared);
        Equal(true, serializePrepared * 2 < serializeUnprepared);
        if (!report)
        {
            return;
        }

        var large = PreparedReportDocument(20_000);
        var largeBytes = System.Text.Encoding.UTF8.GetBytes(large);
        var largeInstance = FerruleJson.ParseBytes(PreparedReportSchema, largeBytes);
        var output = FerruleJson.Serialize(PreparedReportSchema, largeInstance);
        Console.WriteLine($"large document {largeBytes.Length} bytes; large output {System.Text.Encoding.UTF8.GetByteCount(output)} bytes");
        Row("`ParseBytes`, large, shared descriptor", Allocated(3, () => FerruleJson.ParseBytes(PreparedReportSchema, largeBytes)));
        Row("UTF-8 to UTF-16 decoding of the large input", Allocated(3, () => System.Text.Encoding.UTF8.GetString(largeBytes)));
        Row("`System.Text.Json.JsonDocument` of the large input text", Allocated(3, () => System.Text.Json.JsonDocument.Parse(large).Dispose()));
        Row("`System.Text.Json.JsonDocument` of the large input bytes", Allocated(3, () => System.Text.Json.JsonDocument.Parse(largeBytes.AsMemory()).Dispose()));
        Row("`ParseEmbeddedBytes`, large, shared descriptor", Allocated(3, () => FerruleJson.ParseEmbeddedBytes(PreparedReportSchema, largeBytes)));
        Row("`Serialize`, large, shared descriptor", Allocated(3, () => FerruleJson.Serialize(PreparedReportSchema, largeInstance)));
        Row("`SerializeBytes`, large, shared descriptor", Allocated(3, () => FerruleJson.SerializeBytes(PreparedReportSchema, largeInstance)));
        Row("UTF-16 to UTF-8 encoding of the large output", Allocated(3, () => System.Text.Encoding.UTF8.GetBytes(output)));
        Row("`SerializeEmbeddedCompact`, large, shared descriptor", Allocated(3, () => FerruleJson.SerializeEmbeddedCompact(PreparedReportSchema, largeInstance)));
        Row("`SerializeEmbeddedCompactBytes`, large, shared descriptor", Allocated(3, () => FerruleJson.SerializeEmbeddedCompactBytes(PreparedReportSchema, largeInstance)));
        var canonical = output[..^1];
        Row("final newline copy of the large output", Allocated(3, () => _ = canonical + "\n"));
    }
}
