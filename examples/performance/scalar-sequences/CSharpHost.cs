using System.Diagnostics;
using System.Globalization;
using System.Reflection;
using System.Security.Cryptography;
using System.Text;
using System.Text.Json;
using System.Text.Json.Nodes;
using Ferrule.Runtime;

internal static class Host
{
    private const int Ceiling = 64 * 1024 * 1024;
    private static readonly UTF8Encoding Utf8 = new(false, true);
    private static readonly JsonSerializerOptions Pretty = new() { WriteIndented = true };

    private static void Require(bool condition, string message)
    {
        if (!condition) throw new InvalidOperationException(message);
    }

    private static void Fresh(string path)
    {
        Require(Path.IsPathFullyQualified(path) && !Directory.Exists(path) && !File.Exists(path),
            "Absolute fresh directory required.");
        Directory.CreateDirectory(path);
    }

    private static void Write(string path, ReadOnlySpan<byte> bytes)
    {
        using var file = new FileStream(path, FileMode.CreateNew, FileAccess.Write, FileShare.None);
        file.Write(bytes);
        file.Flush(flushToDisk: true);
    }

    private static void Retain(string path, JsonNode value) =>
        Write(path, Utf8.GetBytes(value.ToJsonString(Pretty) + "\n"));

    private static string ReadText(string path)
    {
        Require(Path.IsPathFullyQualified(path), "Absolute input required.");
        using var file = File.OpenRead(path);
        Require(file.Length < Ceiling, "Study document must be below 64 MiB.");
        using var bytes = new MemoryStream();
        var buffer = new byte[65536];
        for (int count; (count = file.Read(buffer)) != 0;)
        {
            Require(bytes.Length + count < Ceiling, "Study document grew beyond bound.");
            bytes.Write(buffer, 0, count);
        }
        return Utf8.GetString(bytes.ToArray());
    }

    private static JsonObject ErrorOriginal(Exception error)
    {
        var properties = new JsonObject();
        // Preserve ordinary runtime properties; feature fields are captured explicitly below.
        foreach (var property in error.GetType().GetProperties(BindingFlags.Public | BindingFlags.Instance))
        {
            if (property.DeclaringType != typeof(FerruleRuntimeException) ||
                property.GetIndexParameters().Length != 0) continue;
            var value = property.GetValue(error);
            properties[property.Name] = value switch
            {
                null => null,
                Enum e => JsonValue.Create(e.ToString()),
                UInt128 n => JsonValue.Create(n.ToString(CultureInfo.InvariantCulture)),
                _ => JsonSerializer.SerializeToNode(value, value.GetType()),
            };
        }
        JsonObject? feature = error switch
        {
            FerruleFilterMapException envelope => new JsonObject
            {
                ["kind"] = "FerruleFilterMapException",
                ["boundary"] = new JsonObject
                {
                    ["Item"] = envelope.Boundary.Item,
                    ["Phase"] = envelope.Boundary.Phase.ToString(),
                    ["CaptureIndex"] = envelope.Boundary.CaptureIndex,
                    ["SourcePosition"] = envelope.Boundary.SourcePosition,
                    ["Function"] = envelope.Boundary.Function,
                    ["Node"] = envelope.Boundary.Node,
                    ["Kind"] = envelope.Boundary.Kind.ToString(),
                },
                // The complete cause is retained once by the unchanged inner traversal.
                ["Cause_aliases_inner"] = ReferenceEquals(envelope.Cause, error.InnerException),
            },
            FerruleFilterMapFailure failure => new JsonObject
            {
                ["kind"] = "FerruleFilterMapFailure",
                ["Kind"] = failure.Kind.ToString(),
                ["ExpectedType"] = failure.ExpectedType?.ToString(),
                ["Found"] = failure.Found is { } found ? Instance(new FerruleScalar(found)) : null,
                ["FloatBits"] = failure.FloatBits?.ToString("x16", CultureInfo.InvariantCulture),
                ["BudgetKind"] = failure.BudgetKind?.ToString(),
                ["Used"] = failure.Used?.ToString(CultureInfo.InvariantCulture),
                ["Requested"] = failure.Requested?.ToString(CultureInfo.InvariantCulture),
                ["Maximum"] = failure.Maximum?.ToString(CultureInfo.InvariantCulture),
            },
            _ => null,
        };
        return new JsonObject
        {
            ["outcome"] = "Err", ["type"] = error.GetType().FullName,
            ["message"] = error.Message, ["complete_to_string"] = error.ToString(),
            ["runtime_properties"] = properties, ["known_feature"] = feature,
            ["inner"] = error.InnerException is { } inner ? ErrorOriginal(inner) : null,
        };
    }

    private static JsonObject Origin(FerruleXmlTypeOrigin origin) => new()
    {
        ["tag"] = origin.Kind.ToString(), ["identity"] = origin.Identity, ["literal"] = origin.Literal,
    };

    private static JsonNode Instance(FerruleInstance value)
    {
        switch (value)
        {
            case FerruleScalar scalar:
                var result = new JsonObject { ["kind"] = "Scalar", ["tag"] = scalar.Value.Kind.ToString() };
                switch (scalar.Value.Kind)
                {
                    case FerruleValueKind.Null:
                    case FerruleValueKind.JsonNull:
                    case FerruleValueKind.XmlNil: break;
                    case FerruleValueKind.Bool: result["value"] = scalar.Value.BooleanValue; break;
                    case FerruleValueKind.Int64: result["value"] = scalar.Value.Int64Value; break;
                    case FerruleValueKind.Double:
                        result["bits"] = unchecked((ulong)BitConverter.DoubleToInt64Bits(scalar.Value.DoubleValue))
                            .ToString("x16", CultureInfo.InvariantCulture); break;
                    case FerruleValueKind.String: result["value"] = scalar.Value.StringValue; break;
                    default: throw new InvalidOperationException("Unknown scalar tag.");
                }
                return result;
            case FerruleGroup group:
                var fields = new JsonArray();
                foreach (var field in group.Fields)
                    fields.Add(new JsonObject { ["name"] = field.Name, ["value"] = Instance(field.Value) });
                return new JsonObject { ["kind"] = "Group", ["origin"] = Origin(group.XmlTypeOrigin), ["fields"] = fields };
            case FerruleRepeated repeated:
                var items = new JsonArray();
                foreach (var item in repeated.Items) items.Add(Instance(item));
                return new JsonObject { ["kind"] = "Repeated", ["items"] = items };
            case FerruleMappedSequence mapped:
                var mappedItems = new JsonArray();
                foreach (var item in mapped.Items) mappedItems.Add(Instance(item));
                return new JsonObject { ["kind"] = "MappedSequence", ["items"] = mappedItems };
            case FerruleDocumentSet documents:
                var members = new JsonArray();
                foreach (var document in documents.Documents)
                    members.Add(new JsonObject { ["path"] = document.Path,
                        ["resolved_source_path"] = document.ResolvedSourcePath, ["value"] = Instance(document.Value) });
                return new JsonObject { ["kind"] = "DocumentSet", ["documents"] = members };
            default: throw new InvalidOperationException("Unknown instance kind.");
        }
    }

    private static FerruleGroup Group(params FerruleField[] fields) => new(fields);

    private static FerruleInstance ExpectedSource(int count, int width) => Group(
        new("Count", new FerruleScalar(FerruleValue.FromInt64(count))),
        new("Capture", new FerruleScalar(FerruleValue.FromString(new string('x', width)))));

    private static FerruleInstance ExpectedOutput(string mode, int count, int width)
    {
        var rows = new List<FerruleInstance>();
        for (var index = 1; index <= count; index++)
            rows.Add(Group(new FerruleField("Value", new FerruleScalar(mode == "numeric"
                ? FerruleValue.FromInt64(index) : FerruleValue.FromString(new string('x', width))))));
        return Group(new FerruleField("Rows", new FerruleRepeated(rows)));
    }

    private static void Phase(string label, Stopwatch started)
    {
        Console.WriteLine(new JsonObject { ["phase"] = label, ["pid"] = Environment.ProcessId,
            ["elapsed_ticks"] = started.ElapsedTicks.ToString(CultureInfo.InvariantCulture),
            ["stopwatch_frequency"] = Stopwatch.Frequency,
            ["proc_status_original"] = File.ReadAllText("/proc/self/status"),
            ["proc_stat_original"] = File.ReadAllText("/proc/self/stat") }.ToJsonString());
        Console.Out.Flush();
    }

    private static JsonObject Stat(string path)
    {
        var file = new FileInfo(path);
        return new JsonObject { ["path"] = path, ["bytes"] = file.Length,
            ["last_write_ticks"] = file.LastWriteTimeUtc.Ticks,
            ["creation_ticks"] = file.CreationTimeUtc.Ticks, ["attributes"] = file.Attributes.ToString() };
    }

    private static JsonObject RetainFile(string path, string destination)
    {
        var before = Stat(path);
        using var source = File.OpenRead(path);
        using var output = new FileStream(destination, FileMode.CreateNew, FileAccess.Write, FileShare.None);
        using var hash = IncrementalHash.CreateHash(HashAlgorithmName.SHA256);
        var buffer = new byte[65536];
        long length = 0;
        for (int count; (count = source.Read(buffer)) != 0;)
        {
            output.Write(buffer, 0, count); hash.AppendData(buffer, 0, count); length += count;
        }
        output.Flush(flushToDisk: true);
        var after = Stat(path);
        return new JsonObject { ["path"] = path, ["retained"] = destination,
            ["stable"] = JsonNode.DeepEquals(before, after) && before["bytes"]!.GetValue<long>() == length,
            ["bytes"] = length, ["sha256"] = Convert.ToHexString(hash.GetHashAndReset()).ToLowerInvariant(),
            ["before"] = before, ["after"] = after };
    }

    private static bool CompareBytes(string expected, string actual, string directory)
    {
        var leftPath = Path.Combine(directory, "expected-bytes.original.json");
        var rightPath = Path.Combine(directory, "timed-bytes.original.json");
        var leftRow = RetainFile(expected, leftPath);
        var rightRow = RetainFile(actual, rightPath);
        Retain(Path.Combine(directory, "byte-identities.original.json"),
            new JsonObject { ["expected"] = leftRow.DeepClone(), ["actual"] = rightRow.DeepClone() });
        using var left = File.OpenRead(leftPath);
        using var right = File.OpenRead(rightPath);
        var a = new byte[65536]; var b = new byte[65536];
        static int Fill(Stream file, byte[] buffer)
        {
            var count = 0;
            while (count < buffer.Length)
            { var read = file.Read(buffer, count, buffer.Length - count); if (read == 0) break; count += read; }
            return count;
        }
        var equal = true;
        while (true)
        {
            var na = Fill(left, a); var nb = Fill(right, b);
            equal &= na == nb && a.AsSpan(0, na).SequenceEqual(b.AsSpan(0, nb));
            if (na == 0 && nb == 0) break;
        }
        equal &= leftRow["stable"]!.GetValue<bool>() && rightRow["stable"]!.GetValue<bool>();
        Retain(Path.Combine(directory, "byte-comparison.original.json"),
            new JsonObject { ["complete_eof_equal"] = equal });
        return equal;
    }

    private static void Run(string[] args)
    {
        Require(args.Length is 4 or 8,
            "measure MODE INPUT FRESH_DIR; verify MODE COUNT WIDTH INPUT EXPECTED TIMED FRESH_DIR");
        var mode = args[1]; Require(mode is "numeric" or "capture", "Numeric or capture required.");
        switch (args[0], args.Length)
        {
            case ("measure", 4):
                Fresh(args[3]);
                var started = Stopwatch.StartNew();
                var text = ReadText(args[2]);
                Phase("before-call", started);
                string document;
                try
                {
                    document = mode == "numeric" ? Ferrule.Generated.Numeric.GeneratedMapping.ExecuteJson(text)
                        : Ferrule.Generated.Capture.GeneratedMapping.ExecuteJson(text);
                }
                catch (Exception error)
                { Retain(Path.Combine(args[3], "error.original.json"), ErrorOriginal(error)); throw; }
                Phase("after-call", started);
                // UTF-8 conversion and actual file retention are part of the timed ordinary host.
                var bytes = Utf8.GetBytes(document);
                Require(bytes.Length < Ceiling, "Output exceeds study ceiling.");
                Write(Path.Combine(args[3], "actual.json"), bytes);
                Phase("after-write", started);
                break;
            case ("verify", 8):
                var count = int.Parse(args[2], CultureInfo.InvariantCulture);
                var width = int.Parse(args[3], CultureInfo.InvariantCulture);
                Require((count, width) is (1, 32) or (16, 32) or (128, 32) or (16, 4096) or
                    (16, 262144) or (128, 262144) or (3, 4), "Outside frozen dimensions/controls.");
                Require(args.Skip(4).All(Path.IsPathFullyQualified), "Absolute file arguments required.");
                var dir = args[7]; Fresh(dir);
                var inputPath = Path.Combine(dir, "input.original.json");
                var inputRow = RetainFile(args[4], inputPath);
                Retain(Path.Combine(dir, "input-identity.original.json"), inputRow.DeepClone());
                FerruleInstance? source = null; FerruleInstance? actual = null;
                try
                {
                    source = FerruleJson.ParseEmbedded(File.ReadAllText(Path.Combine(AppContext.BaseDirectory,
                        "source-schema.json")), ReadText(inputPath));
                    Retain(Path.Combine(dir, "source-result.original.json"),
                        new JsonObject { ["outcome"] = "Ok", ["value"] = Instance(source) });
                }
                catch (Exception error)
                { Retain(Path.Combine(dir, "source-result.original.json"), ErrorOriginal(error)); }
                if (source is not null)
                {
                    try
                    {
                        actual = mode == "numeric" ? Ferrule.Generated.Numeric.GeneratedMapping.Execute(source)
                            : Ferrule.Generated.Capture.GeneratedMapping.Execute(source);
                        Retain(Path.Combine(dir, "execute-result.original.json"),
                            new JsonObject { ["outcome"] = "Ok", ["value"] = Instance(actual) });
                    }
                    catch (Exception error)
                    { Retain(Path.Combine(dir, "execute-result.original.json"), ErrorOriginal(error)); }
                }
                else Retain(Path.Combine(dir, "execute-result.original.json"),
                    new JsonObject { ["outcome"] = "NotReached", ["cause"] = "source parse failed" });
                var expectedSource = Instance(ExpectedSource(count, width));
                var expectedOutput = Instance(ExpectedOutput(mode, count, width));
                Retain(Path.Combine(dir, "expected-source.original.json"), expectedSource);
                Retain(Path.Combine(dir, "expected-output.original.json"), expectedOutput);
                var bytesEqual = CompareBytes(args[5], args[6], dir);
                var sourceEqual = source is not null && JsonNode.DeepEquals(Instance(source), expectedSource);
                var outputEqual = actual is not null && JsonNode.DeepEquals(Instance(actual), expectedOutput);
                Retain(Path.Combine(dir, "verification.original.json"), new JsonObject
                { ["source_equal"] = sourceEqual, ["output_equal"] = outputEqual, ["bytes_equal"] = bytesEqual,
                    ["input_stable"] = inputRow["stable"]!.GetValue<bool>() });
                Require(sourceEqual && outputEqual && bytesEqual && inputRow["stable"]!.GetValue<bool>(),
                    "Complete retained typed/byte verification failed.");
                break;
            default: throw new InvalidOperationException("Unsupported command.");
        }
    }

    private static int Main(string[] args)
    {
        try { Run(args); return 0; }
        catch (Exception error) { Console.Error.WriteLine(ErrorOriginal(error).ToJsonString(Pretty)); return 1; }
    }
}
