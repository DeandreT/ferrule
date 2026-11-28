using System.Text;
using System.Text.Json;
using System.Text.Json.Nodes;
using Ferrule.Runtime;

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private static void InterchangeWriterPreparation(string boundary, FerruleGroup body)
    {
        string[] path = ["Interchange", "Group", "Message_850", "G69"];
        string Descriptor(int minimum) =>
            JsonSerializer.Serialize(new { boundary, minimums = new[] { new { path, minimum } } });
        var context = new FerruleExecutionContext("test", "test", "2026-01-02T03:04:05");
        static string Clock(FerruleMapForceX12ClockField field) => "2031-06-07T08:09:10";

        string[] descriptors = [Descriptor(0), Descriptor(1)];
        var writers = descriptors
            .Select(d => new FerruleX12.InterchangeDocumentWriter(d))
            .ToArray();
        var bytes = descriptors
            .Select(d => FerruleX12.SerializeMapForceDocumentBytes(d, body, context))
            .ToArray();
        var clockedBytes = descriptors
            .Select(d =>
                FerruleX12.SerializeMapForceDocumentBytesWithClock(d, body, context, Clock)
            )
            .ToArray();
        Equal(
            false,
            Encoding.Latin1.GetString(bytes[0]).Contains("G69~\r\n", StringComparison.Ordinal)
        );
        Equal(
            true,
            Encoding.Latin1.GetString(bytes[1]).Contains("G69~\r\n", StringComparison.Ordinal)
        );
        Equal(false, bytes[0].SequenceEqual(clockedBytes[0]));

        // Same boundary, different minimums. Each output and clock belongs to its caller.
        Parallel.For(0, 32, i =>
        {
            int owner = i % 2;
            var writer = writers[owner];
            using var output = new InterchangeTraceStream();
            if ((i / 2) % 2 == 0)
            {
                writer.WriteWithClock(body, context, Clock, output);
                Equal(true, clockedBytes[owner].SequenceEqual(output.ToArray()));
                Equal(true, bytes[owner].SequenceEqual(writer.Serialize(body, context)));
            }
            else
            {
                writer.Write(body, context, output);
                Equal(true, bytes[owner].SequenceEqual(output.ToArray()));
                Equal(
                    true,
                    clockedBytes[owner]
                        .SequenceEqual(writer.SerializeWithClock(body, context, Clock))
                );
            }
            Equal(1, output.Flushes);
            Equal(false, output.Closed);
        });

        var interchanges = (
            (FerruleRepeated)body.Fields.Single(f => f.Name == "Interchange").Value
        ).Items;
        var twice = new FerruleGroup([
            Field("Interchange", new FerruleRepeated(interchanges.Concat(interchanges))),
        ]);
        var nextContext = new FerruleExecutionContext("test", "test", "2032-08-09T10:11:12");
        var twiceBytes = FerruleX12.SerializeMapForceDocumentBytes(
            descriptors[1],
            twice,
            nextContext
        );
        Equal(true, twiceBytes.SequenceEqual(writers[1].Serialize(twice, nextContext)));
        Equal(true, bytes[1].SequenceEqual(writers[1].Serialize(body, context)));

        var plainBoundary = JsonNode.Parse(boundary)!.AsObject();
        plainBoundary.Remove("autocomplete");
        var plain = JsonNode.Parse(descriptors[1])!;
        plain["boundary"] = plainBoundary.ToJsonString();
        string plainDescriptor = plain.ToJsonString();
        var plainBytes = FerruleX12.SerializeMapForceDocumentBytes(plainDescriptor, body, context);
        var plainWriter = new FerruleX12.InterchangeDocumentWriter(plainDescriptor);
        Equal(
            true,
            plainBytes.SequenceEqual(
                plainWriter.SerializeWithClock(body, context, _ =>
                    throw new InvalidOperationException("Unexpected clock")
                )
            )
        );

        var fields = Enum.GetValues<FerruleMapForceX12ClockField>();
        var seen = new List<FerruleMapForceX12ClockField>();
        using var stream = new InterchangeTraceStream();
        for (int failAt = 1; failAt <= fields.Length; failAt++)
        {
            seen.Clear();
            Throws<IOException>(() =>
                writers[0]
                    .WriteWithClock(
                        body,
                        context,
                        field =>
                        {
                            seen.Add(field);
                            return seen.Count == failAt
                                ? throw new IOException("clock")
                                : Clock(field);
                        },
                        stream
                    )
            );
            Equal(true, seen.SequenceEqual(fields.Take(failAt)));
            Equal(0L, stream.Length);
            Equal(0, stream.Flushes);
            Equal(false, stream.Closed);
        }
        X12Error(FerruleX12Error.Envelope, () =>
            writers[0]
                .WriteWithClock(
                    new FerruleGroup([]),
                    context,
                    _ => throw new InvalidOperationException("Unexpected clock"),
                    stream
                )
        );
        writers[0].WriteWithClock(body, context, Clock, stream);
        Equal(true, clockedBytes[0].SequenceEqual(stream.ToArray()));
        Equal(1, stream.Flushes);

        string invalid = Descriptor(1025);
        var rejected = new FerruleX12.InterchangeDocumentWriter(invalid);
        var original = X12Error(FerruleX12Error.Schema, () =>
            FerruleX12.SerializeMapForceDocumentBytes(invalid, body, context)
        );
        static void NullArgument(string name, Action call)
        {
            try
            {
                call();
            }
            catch (ArgumentNullException error) when (error.ParamName == name)
            {
                return;
            }
            throw new InvalidOperationException($"Expected null {name}.");
        }
        NullArgument("clock", () => rejected.WriteWithClock(null!, null!, null!, null!));
        NullArgument("output", () => rejected.WriteWithClock(null!, null!, Clock, null!));
        NullArgument("output", () => rejected.Write(null!, null!, null!));
        NullArgument("clock", () => rejected.SerializeWithClock(null!, null!, null!));
        NullArgument("document", () => rejected.Serialize(null!, null!));
        NullArgument("execution", () => rejected.Serialize(body, null!));
        var first = X12Error(FerruleX12Error.Schema, () => rejected.Serialize(body, context));
        var retry = X12Error(FerruleX12Error.Schema, () =>
            rejected.WriteWithClock(
                body,
                context,
                _ => throw new InvalidOperationException("Unexpected clock"),
                stream
            )
        );
        Equal(original.Message, first.Message);
        Equal(first.Message, retry.Message);
        Equal(false, ReferenceEquals(first, retry));
        Equal(1, stream.Flushes);
        Equal(true, clockedBytes[0].SequenceEqual(stream.ToArray()));

        var duplicate = JsonNode.Parse(descriptors[0])!;
        var minimums = duplicate["minimums"]!.AsArray();
        minimums.Add(minimums[0]!.DeepClone());
        X12Error(FerruleX12Error.Schema, () =>
            new FerruleX12.InterchangeDocumentWriter(duplicate.ToJsonString()).Serialize(
                body,
                context
            )
        );
        var oversized = new FerruleX12.InterchangeDocumentWriter(
            new string(' ', 2 * FerruleX12.MaximumSchemaBytes + 1)
        );
        X12Error(FerruleX12Error.ResourceLimit, () => oversized.Serialize(body, context));

        var sourceOnly = JsonNode.Parse(boundary)!;
        var schema = JsonNode.Parse(sourceOnly["schema"]!.GetValue<string>())!.AsObject();
        var group = X12SchemaChild(X12SchemaChild(schema, "Interchange"), "Group");
        var acknowledgment = X12GroupSchema(
            "ParserErrors_Group",
            X12GroupSchema("ERR", X12ScalarSchema("code"))
        );
        acknowledgment["repeating"] = true;
        group["kind"]!["children"]!.AsArray().Add(acknowledgment);
        sourceOnly["schema"] = schema.ToJsonString();
        X12Error(FerruleX12Error.Syntax, () =>
            FerruleX12.ParseEmbedded(sourceOnly.ToJsonString(), "")
        );
        var wrongMode = JsonNode.Parse(descriptors[0])!;
        wrongMode["boundary"] = sourceOnly.ToJsonString();
        var outputError = X12Error(FerruleX12Error.Schema, () =>
            new FerruleX12.InterchangeDocumentWriter(wrongMode.ToJsonString()).Serialize(
                body,
                context
            )
        );
        Equal(
            "Acknowledgment branches in repeating interchanges are source-only.",
            outputError.Message
        );
    }
}
