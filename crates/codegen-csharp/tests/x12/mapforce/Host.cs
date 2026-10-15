using System.Collections;
using System.Reflection;
using System.Text;
using System.Text.Json;
using System.Text.Json.Nodes;

var utf8 = new UTF8Encoding(false, true);
var outcomes = new JsonArray();
int passed = 0, failed = 0, caseCount = 0;
JsonNode contract = Read("contract.json");
string timestamp = contract["completion_context"]!.GetValue<string>();
try
{
    JsonArray cases = Read("cases.json").AsArray();
    foreach (JsonNode? entry in Read("libraries.json").AsArray())
    {
        string name = entry!["name"]!.GetValue<string>(), version = entry["version"]!.GetValue<string>();
        string form = entry["form"]!.GetValue<string>();
        JsonNode schema = Read($"{version}-schema.json");
        Assembly assembly = Assembly.LoadFrom(Path.GetFullPath($"{name}/bin/Release/net10.0/{name}.dll"));
        Type map = assembly.GetType("Ferrule.Generated.GeneratedMapping", true)!;
        Type runtime = assembly.GetType("Ferrule.Runtime.FerruleX12", true)!;
        string targetDescriptor = File.ReadAllText($"{name}-target-descriptor.json");
        object Context() => assembly.GetType("Ferrule.Runtime.FerruleExecutionContext")!
            .GetConstructors().Single(ctor => ctor.GetParameters().Length == 4)
            .Invoke(["invented-mapforce-profile", "invented-mapforce-profile", timestamp, null]);
        bool completion = form == "Completion";
        object?[] Arguments(object document) => completion ? [document, Context()] : [document];
        string input = File.ReadAllText($"{version}-input.x12"), output = File.ReadAllText($"{version}-output.x12");
        string zero = File.ReadAllText($"{version}-zero-controls.x12");
        string zeroCompleted = File.ReadAllText($"{version}-zero-controls-completed.x12");
        JsonNode inputValues = Read($"{version}-input-values.json"), zeroValues = Read($"{version}-zero-controls-values.json");

        // Reading is independent of output completion; both libraries share one source profile.
        foreach ((string id, string wire, JsonNode values) in new[] { ("input", input, inputValues), ("zero-controls", zero, zeroValues) })
        {
            JsonNode typed = Expected(schema, values);
            ValueCase($"{name}-{id}-parse-text", () => Raw(Call(map, "ParseX12", wire)), typed);
            ValueCase($"{name}-{id}-parse-bytes", () => Raw(Call(map, "ParseX12Bytes", utf8.GetBytes(wire))), typed);
        }

        // Native writes the typed input without changing a nine-digit Float ISA13.
        object supplied = Make(assembly, schema, inputValues);
        JsonNode before = Raw(supplied)!;
        ValueCase($"{name}-serialize-text", () => Capture(Call(map, "SerializeX12", Arguments(supplied))), JsonValue.Create(output));
        ValueCase($"{name}-serialize-bytes", () => JsonValue.Create(utf8.GetString((byte[])Call(map, "SerializeX12Bytes", Arguments(supplied))!)), JsonValue.Create(output));
        ValueCase($"{name}-caller-tree", () => Raw(supplied), before);
        ValueCase($"{name}-map-text", () => Capture(Call(map, "ExecuteX12ToX12", Arguments(input))), JsonValue.Create(output));
        ValueCase($"{name}-map-bytes", () => JsonValue.Create(utf8.GetString((byte[])Call(map, "ExecuteX12ToX12Bytes",
            Arguments(utf8.GetBytes(input)))!)), JsonValue.Create(output));

        // Float controls lose leading zeros: only completion restores nine-digit ISA13 and IEA02.
        object zeroSupplied = Make(assembly, schema, zeroValues);
        if (completion)
        {
            ValueCase($"{name}-zero-controls-serialize", () => Capture(Call(map, "SerializeX12", zeroSupplied, Context())), JsonValue.Create(zeroCompleted));
            ValueCase($"{name}-zero-controls-map", () => Capture(Call(map, "ExecuteX12ToX12", zero, Context())), JsonValue.Create(zeroCompleted));
            ValueCase($"{name}-zero-controls-readback", () => Raw(Call(runtime, "ParseEmbedded", targetDescriptor, zeroCompleted)), Expected(schema, zeroValues));
            ExactError($"{name}-missing-context", () => Call(map, "SerializeX12", zeroSupplied), new JsonObject {
                ["type"] = "FerruleX12Exception", ["error"] = "Envelope", ["message"] = "X12 completion requires a supplied current dateTime.",
                ["segment_index"] = null, ["field_path"] = null });
        }
        else
        {
            ExactError($"{name}-zero-controls-strict", () => Call(map, "SerializeX12", zeroSupplied), contract["zero_controls_strict_error"]!);
            ExactError($"{name}-zero-controls-map-strict", () => Call(map, "ExecuteX12ToX12", zero), contract["zero_controls_strict_error"]!);
        }

        foreach (JsonNode? selected in cases)
        {
            if (selected!["version"]!.GetValue<string>() != version || selected["form"]!.GetValue<string>() != form) continue;
            caseCount++;
            string label = $"{name}-{selected["label"]!.GetValue<string>()}";
            string path = selected["input"]!.GetValue<string>();
            if (selected["kind"]!.GetValue<string>() == "parse")
            {
                string wire = File.ReadAllText(path);
                ExactError($"{label}-text", () => Call(map, "ParseX12", wire), selected["expected_error"]!);
                ExactError($"{label}-bytes", () => Call(map, "ParseX12Bytes", utf8.GetBytes(wire)), selected["expected_error"]!);
                continue;
            }
            object document = Make(assembly, schema, Read(path));
            JsonNode caller = Raw(document)!;
            foreach (bool bytes in new[] { false, true })
            {
                string callLabel = $"{label}-{(bytes ? "bytes" : "text")}";
                Func<object?> action = () => Call(map, bytes ? "SerializeX12Bytes" : "SerializeX12", Arguments(document));
                if (selected["expected_wire"] is JsonNode expectedWire)
                    ValueCase(callLabel, () => {
                        object? actual = action();
                        return JsonValue.Create(bytes ? utf8.GetString((byte[])actual!) : (string)actual!);
                    }, JsonValue.Create(File.ReadAllText(expectedWire.GetValue<string>())));
                else ExactError(callLabel, action, selected["expected_error"]!);
                ValueCase($"{callLabel}-caller-tree", () => Raw(document), caller);
            }
        }
    }
}
catch (Exception failure)
{
    outcomes.Add(new JsonObject { ["label"] = "host-setup-failure", ["error"] = Error(failure), ["pass"] = false });
    failed++;
}
finally { Flush(); }
Console.WriteLine(JsonSerializer.Serialize(new { passed, failed, cases = caseCount }));
return failed == 0 ? 0 : 1;

JsonNode Read(string path) => JsonNode.Parse(File.ReadAllText(path))!;
object? Property(object value, string name) => value.GetType().GetProperty(name)?.GetValue(value);
object? Call(Type type, string name, params object?[] arguments)
{
    MethodInfo method = type.GetMethods(BindingFlags.Public | BindingFlags.Static)
        .Single(candidate => candidate.Name == name && candidate.GetParameters().Length == arguments.Length);
    try { return method.Invoke(null, arguments); }
    catch (TargetInvocationException failure) when (failure.InnerException is not null) { throw failure.InnerException; }
}
JsonNode Error(Exception error) => new JsonObject {
    ["type"] = error.GetType().FullName, ["message"] = error.Message, ["full_original"] = error.ToString(),
    ["error"] = Property(error, "Error")?.ToString(), ["segment_index"] = JsonSerializer.SerializeToNode(Property(error, "SegmentIndex")),
    ["field_path"] = JsonSerializer.SerializeToNode(Property(error, "FieldPath")),
};
void Flush() => File.WriteAllText("complete-outcomes.original.json", outcomes.ToJsonString(new JsonSerializerOptions { WriteIndented = true }));
void ValueCase(string label, Func<JsonNode?> action, JsonNode? expected)
{
    JsonNode? actual = null, error = null;
    try { actual = action(); } catch (Exception failure) { error = Error(failure); }
    bool ok = error is null && JsonNode.DeepEquals(actual, expected);
    outcomes.Add(new JsonObject { ["label"] = label, ["actual"] = actual?.DeepClone(), ["error"] = error,
        ["expected"] = expected?.DeepClone(), ["pass"] = ok });
    if (ok) passed++; else failed++;
    Flush();
}
void ExactError(string label, Func<object?> action, JsonNode expected)
{
    JsonNode? actual = null, error = null;
    try { actual = Capture(action()); } catch (Exception failure) { error = Error(failure); }
    bool ok = error is not null && error["type"]?.GetValue<string>() == "Ferrule.Runtime." + expected["type"]!.GetValue<string>()
        && error["error"]?.GetValue<string>() == expected["error"]!.GetValue<string>()
        && error["message"]?.GetValue<string>() == expected["message"]!.GetValue<string>()
        && JsonNode.DeepEquals(error["segment_index"], expected["segment_index"])
        && JsonNode.DeepEquals(error["field_path"], expected["field_path"]);
    outcomes.Add(new JsonObject { ["label"] = label, ["actual"] = actual, ["error"] = error,
        ["expected"] = expected.DeepClone(), ["pass"] = ok });
    if (ok) passed++; else failed++;
    Flush();
}
JsonNode? Capture(object? value) => value switch { null => null, string text => JsonValue.Create(text), byte[] bytes => JsonValue.Create(Convert.ToBase64String(bytes)), _ => Raw(value) };
JsonNode? Raw(object? value)
{
    if (value is null) return null;
    string type = value.GetType().Name;
    if (type == "FerruleGroup")
    {
        var fields = new JsonArray();
        foreach (object field in (IEnumerable)Property(value, "Fields")!)
            fields.Add(new JsonObject { ["name"] = (string)Property(field, "Name")!, ["value"] = Raw(Property(field, "Value")) });
        return new JsonObject { ["kind"] = "Group", ["fields"] = fields };
    }
    if (type == "FerruleRepeated")
    {
        var items = new JsonArray();
        foreach (object item in (IEnumerable)Property(value, "Items")!) items.Add(Raw(item));
        return new JsonObject { ["kind"] = "Repeated", ["items"] = items };
    }
    if (type == "FerruleScalar")
    {
        object scalar = Property(value, "Value")!;
        string kind = Property(scalar, "Kind")!.ToString()!;
        JsonNode? payload = kind switch {
            "Null" => null, "String" => JsonValue.Create((string)Property(scalar, "StringValue")!),
            "Int64" => JsonValue.Create((long)Property(scalar, "Int64Value")!),
            "Double" => JsonValue.Create((double)Property(scalar, "DoubleValue")!),
            _ => JsonValue.Create(scalar.ToString()),
        };
        return new JsonObject { ["kind"] = "Scalar", ["scalar_kind"] = kind == "Int64" ? "Int" : kind == "Double" ? "Float" : kind, ["value"] = payload };
    }
    return new JsonObject { ["kind"] = type, ["full_original"] = value.ToString() };
}
object Make(Assembly assembly, JsonNode schema, JsonNode? value, bool item = false)
{
    Type instance = assembly.GetType("Ferrule.Runtime.FerruleInstance")!;
    object TypedArray(Type type, IEnumerable<object> values) { object[] items = values.ToArray(); Array array = Array.CreateInstance(type, items.Length); for (int i = 0; i < items.Length; i++) array.SetValue(items[i], i); return array; }
    if (!item && schema["repeating"]?.GetValue<bool>() == true)
        return Activator.CreateInstance(assembly.GetType("Ferrule.Runtime.FerruleRepeated")!, TypedArray(instance, value!.AsArray().Select(child => Make(assembly, schema, child, true))))!;
    JsonNode kind = schema["kind"]!;
    if (kind["kind"]!.GetValue<string>() == "group")
    {
        Type field = assembly.GetType("Ferrule.Runtime.FerruleField")!;
        var fields = new List<object>();
        foreach (JsonNode? child in kind["children"]!.AsArray())
            if (value!.AsObject().TryGetPropertyValue(child!["name"]!.GetValue<string>(), out JsonNode? supplied))
                fields.Add(Activator.CreateInstance(field, child["name"]!.GetValue<string>(), Make(assembly, child, supplied))!);
        return Activator.CreateInstance(assembly.GetType("Ferrule.Runtime.FerruleGroup")!, TypedArray(field, fields))!;
    }
    Type scalar = assembly.GetType("Ferrule.Runtime.FerruleValue")!;
    object raw = value is null ? scalar.GetProperty("Null", BindingFlags.Public | BindingFlags.Static)!.GetValue(null)! : kind["ty"]!.GetValue<string>() switch {
        "string" => Call(scalar, "FromString", value.GetValue<string>())!,
        "int" => Call(scalar, "FromInt64", value.GetValue<long>())!,
        "float" => Call(scalar, "FromDouble", value.GetValue<double>())!,
        _ => throw new InvalidOperationException("invented scalar oracle"),
    };
    return Activator.CreateInstance(assembly.GetType("Ferrule.Runtime.FerruleScalar")!, raw)!;
}
JsonNode Expected(JsonNode schema, JsonNode? value, bool item = false)
{
    if (!item && schema["repeating"]?.GetValue<bool>() == true)
        return new JsonObject { ["kind"] = "Repeated", ["items"] = new JsonArray(value!.AsArray().Select(child => Expected(schema, child, true)).ToArray()) };
    JsonNode kind = schema["kind"]!;
    if (kind["kind"]!.GetValue<string>() == "group")
    {
        var fields = new JsonArray();
        foreach (JsonNode? child in kind["children"]!.AsArray())
            if (value!.AsObject().TryGetPropertyValue(child!["name"]!.GetValue<string>(), out JsonNode? supplied))
                fields.Add(new JsonObject { ["name"] = child["name"]!.DeepClone(), ["value"] = Expected(child, supplied) });
        return new JsonObject { ["kind"] = "Group", ["fields"] = fields };
    }
    if (value is null) return new JsonObject { ["kind"] = "Scalar", ["scalar_kind"] = "Null", ["value"] = null };
    return kind["ty"]!.GetValue<string>() switch {
        "string" => new JsonObject { ["kind"] = "Scalar", ["scalar_kind"] = "String", ["value"] = JsonValue.Create(value.GetValue<string>()) },
        "int" => new JsonObject { ["kind"] = "Scalar", ["scalar_kind"] = "Int", ["value"] = JsonValue.Create(value.GetValue<long>()) },
        "float" => new JsonObject { ["kind"] = "Scalar", ["scalar_kind"] = "Float", ["value"] = JsonValue.Create(value.GetValue<double>()) },
        _ => throw new InvalidOperationException("invented scalar oracle"),
    };
}
