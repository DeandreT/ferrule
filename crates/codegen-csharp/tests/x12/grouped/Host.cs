using System.Collections;
using System.Reflection;
using System.Text;
using System.Text.Json;
using System.Text.Json.Nodes;

var utf8 = new UTF8Encoding(false, true);
var outcomes = new JsonArray();
var rawObservations = new JsonArray();
int passed = 0, failed = 0;
int referenceProofOrdinal = 0;
try
{
    foreach (JsonNode? entry in Read("libraries.json").AsArray())
    {
        string name = entry!["name"]!.GetValue<string>(), version = entry["version"]!.GetValue<string>();
        string direction = entry["direction"]!.GetValue<string>();
        string input = File.ReadAllText($"{version}-complete.x12"), output = File.ReadAllText($"{version}-output.x12");
        JsonNode schema = Read($"{version}-schema.json"), values = Read($"{version}-values.json");
        JsonNode expected = Read($"{version}-typed-tree.json");
        Assembly assembly = Assembly.LoadFrom(Path.GetFullPath($"{name}/bin/Release/net10.0/{name}.dll"));
        Type map = assembly.GetType("Ferrule.Generated.GeneratedMapping", true)!;
        Type runtime = assembly.GetType("Ferrule.Runtime.FerruleX12", true)!;
        string sourceDescriptor = File.ReadAllText($"{name}-source-descriptor.json");
        string targetDescriptor = File.ReadAllText($"{name}-target-descriptor.json");
        object Context(string? timestamp) => assembly.GetType("Ferrule.Runtime.FerruleExecutionContext")!
            .GetConstructors().Single(ctor => ctor.GetParameters().Length == 4)
            .Invoke(["invented-grouped-profile", "invented-grouped-profile", timestamp, null]);
        bool source = direction != "Target" && direction != "Completion", target = direction != "Source";
        if (source)
        {
            ValueCase($"{name}-parse-text", () => Raw(Call(map, "ParseX12", input)), expected);
            ValueCase($"{name}-parse-bytes", () => Raw(Call(map, "ParseX12Bytes", utf8.GetBytes(input))), expected);
            if (target)
            {
                ValueCase($"{name}-map-text", () => Capture(Call(map, "ExecuteX12ToX12", input)), JsonValue.Create(output));
                ValueCase($"{name}-map-bytes", () => JsonValue.Create(utf8.GetString((byte[])Call(map, "ExecuteX12ToX12Bytes", utf8.GetBytes(input))!)), JsonValue.Create(output));
                ValueCase($"{name}-map-context", () => Capture(Call(map, "ExecuteX12ToX12", input, Context(null))), JsonValue.Create(output));
            }
            else
            {
                ValueCase($"{name}-json-text", () => JsonNode.Parse((string)Call(map, "ExecuteX12ToJson", input)!), Projection(values));
                ValueCase($"{name}-json-bytes", () => JsonNode.Parse(utf8.GetString((byte[])Call(map, "ExecuteX12ToJsonBytes", utf8.GetBytes(input))!)), Projection(values));
            }
        }
        if (target && direction != "Completion")
        {
            object supplied = Make(assembly, schema, values);
            var before = References(supplied);
            ValueCase($"{name}-serialize-text", () => Capture(Call(map, "SerializeX12", supplied)), JsonValue.Create(output));
            ValueCase($"{name}-serialize-bytes", () => JsonValue.Create(utf8.GetString((byte[])Call(map, "SerializeX12Bytes", supplied)!)), JsonValue.Create(output));
            ValueCase($"{name}-serialize-context", () => Capture(Call(map, "SerializeX12", supplied, Context(null))), JsonValue.Create(output));
            ValueCase($"{name}-caller-tree", () => Raw(supplied), expected);
            ValueCase($"{name}-caller-references", () => ReferenceProof(before, supplied), JsonValue.Create(true));
            if (!source)
            {
                string json = Projection(values).ToJsonString();
                ValueCase($"{name}-from-json-text", () => Capture(Call(map, "ExecuteJsonToX12", json)), JsonValue.Create(output));
                ValueCase($"{name}-from-json-bytes", () => JsonValue.Create(utf8.GetString((byte[])Call(map, "ExecuteJsonToX12Bytes", utf8.GetBytes(json))!)), JsonValue.Create(output));
            }
        }
        if (version == "005010" && source)
            foreach (JsonNode? errorCase in Read("error-cases.proposed.json").AsArray())
            {
                string label = errorCase!["label"]!.GetValue<string>();
                string wire = File.ReadAllText(errorCase["wire"]!.GetValue<string>());
                ExactError($"{name}-{label}-text", () => Call(map, "ParseX12", wire), errorCase["expected"]!);
                ExactError($"{name}-{label}-bytes", () => Call(map, "ParseX12Bytes", utf8.GetBytes(wire)), errorCase["expected"]!);
            }
        if (direction != "Completion") continue;
        foreach (JsonNode? selected in Read("completion-cases.proposed.json").AsArray())
        {
            string label = selected!["label"]!.GetValue<string>();
            object supplied = Make(assembly, schema, Read(selected["input_values"]!.GetValue<string>()));
            SharedReferenceInput.Aliased? aliases = null;
            if (selected["constructor"] is not null)
            {
                aliases = SharedReferenceInput.Create((Ferrule.Runtime.FerruleGroup)supplied);
                supplied = aliases.Source;
            }
            var before = References(supplied);
            JsonNode caller = Read(selected["caller_before_after"]!.GetValue<string>());
            ValueCase($"{label}-initial-caller", () => Raw(supplied), caller);
            string? timestamp = selected["context"]?.GetValue<string>();
            foreach (bool bytes in new[] { false, true })
            {
                string callLabel = $"{label}-{(bytes ? "bytes" : "text")}";
                Func<object?> action = () => Call(map, bytes ? "SerializeX12Bytes" : "SerializeX12", supplied, Context(timestamp));
                if (selected["expected_error"] is JsonNode expectedError)
                    ExactError(callLabel, action, expectedError);
                else
                {
                    string expectedWire = File.ReadAllText(selected["expected_wire"]!.GetValue<string>());
                    string? actualWire = null;
                    ValueCase(callLabel, () => {
                        object? actual = action();
                        actualWire = bytes ? utf8.GetString((byte[])actual!) : (string)actual!;
                        return JsonValue.Create(actualWire);
                    }, JsonValue.Create(expectedWire));
                    JsonNode expectedTree = selected["expected_typed_tree"] is JsonNode typedFile
                        ? Read(typedFile.GetValue<string>()) : Expected(schema, Read(selected["expected_values"]!.GetValue<string>()));
                    ValueCase($"{callLabel}-complete-typed-readback", () => Raw(Call(runtime, "ParseEmbedded", targetDescriptor, actualWire)), expectedTree);
                }
                ValueCase($"{callLabel}-caller-tree", () => Raw(supplied), caller);
                ValueCase($"{callLabel}-caller-references", () => ReferenceProof(before, supplied), JsonValue.Create(true));
                if (aliases is not null)
                    ValueCase($"{callLabel}-all-eight-aliases", () => JsonSerializer.SerializeToNode(SharedReferenceInput.ReferenceProof(aliases)),
                        new JsonObject { ["first.GS"] = true, ["second.GS"] = true, ["first.GE"] = true, ["second.GE"] = true,
                            ["B.ST"] = true, ["C.ST"] = true, ["B.SE"] = true, ["C.SE"] = true });
            }
        }
        foreach (JsonNode? selected in Read("completion-schema-cases.proposed.json").AsArray())
        {
            JsonNode descriptor = JsonNode.Parse(targetDescriptor)!;
            descriptor["schema"] = File.ReadAllText(selected!["schema"]!.GetValue<string>());
            object supplied = Make(assembly, schema, Read(selected["input_values"]!.GetValue<string>()));
            ExactError(selected["label"]!.GetValue<string>(), () => Call(runtime, "SerializeEmbedded", descriptor.ToJsonString(), supplied, Context(null)), selected["expected_csharp"]!);
        }
    }
}
catch (Exception failure)
{
    outcomes.Add(new JsonObject { ["label"] = "host-setup-failure", ["error"] = Error(failure), ["pass"] = false });
    failed++;
}
finally { Flush(); }
Console.WriteLine(JsonSerializer.Serialize(new { passed, failed }));
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
void Flush()
{
    File.WriteAllText("complete-outcomes.original.json", outcomes.ToJsonString(new JsonSerializerOptions { WriteIndented = true }));
    File.WriteAllText("raw-observations.original.json", rawObservations.ToJsonString(new JsonSerializerOptions { WriteIndented = true }));
}
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
        object? origin = Property(value, "XmlTypeOrigin");
        rawObservations.Add(new JsonObject { ["class"] = value.GetType().FullName, ["origin"] = origin?.ToString(), ["origin_kind"] = origin is null ? null : Property(origin, "Kind")?.ToString() });
        return new JsonObject { ["kind"] = "Group", ["xml_origin"] = origin is null || Property(origin, "Kind")?.ToString() == "Unknown" ? null : JsonValue.Create(origin.ToString()), ["fields"] = fields };
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
        rawObservations.Add(new JsonObject { ["class"] = scalar.GetType().FullName, ["kind"] = kind, ["full_original"] = scalar.ToString(),
            ["double_bits"] = kind == "Double" ? BitConverter.DoubleToInt64Bits((double)Property(scalar, "DoubleValue")!).ToString("x16", System.Globalization.CultureInfo.InvariantCulture) : null });
        JsonNode? payload = kind switch {
            "Null" => null, "String" => JsonValue.Create((string)Property(scalar, "StringValue")!),
            "Int64" => JsonValue.Create((long)Property(scalar, "Int64Value")!),
            "Double" when double.IsFinite((double)Property(scalar, "DoubleValue")!) => JsonValue.Create((double)Property(scalar, "DoubleValue")!),
            "Bool" => JsonValue.Create((bool)Property(scalar, "BooleanValue")!),
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
        return new JsonObject { ["kind"] = "Group", ["xml_origin"] = null, ["fields"] = fields };
    }
    string scalarKind = value is null ? "Null" : kind["ty"]!.GetValue<string>() switch { "string" => "String", "int" => "Int", "float" => "Float", _ => throw new InvalidOperationException("invented scalar oracle") };
    return new JsonObject { ["kind"] = "Scalar", ["scalar_kind"] = scalarKind, ["value"] = value?.DeepClone() };
}
JsonNode Projection(JsonNode value)
{
    if (value is JsonObject obj) { var result = new JsonObject(); foreach (var field in obj) if (field.Value is not null) result[field.Key] = Projection(field.Value); return result; }
    if (value is JsonArray array) return new JsonArray(array.Select(item => item is null ? null : Projection(item)).ToArray());
    return value.DeepClone();
}
Dictionary<string, object> References(object root)
{
    var result = new Dictionary<string, object>(StringComparer.Ordinal);
    void Visit(object value, string path)
    {
        result.Add(path, value);
        if (value.GetType().Name == "FerruleGroup")
            foreach (object field in (IEnumerable)Property(value, "Fields")!) { string name = (string)Property(field, "Name")!; result.Add(path + "/field:" + name, field); Visit(Property(field, "Value")!, path + "/" + name); }
        else if (value.GetType().Name == "FerruleRepeated")
        { int i = 0; foreach (object item in (IEnumerable)Property(value, "Items")!) Visit(item, path + "[" + i++ + "]"); }
    }
    Visit(root, "root");
    return result;
}
JsonNode ReferenceProof(Dictionary<string, object> before, object value)
{
    var after = References(value);
    bool equal = before.Count == after.Count && before.All(field => after.TryGetValue(field.Key, out object? actual) && ReferenceEquals(field.Value, actual));
    var fields = before.Select(field => new {
        path = field.Key, before_class = field.Value.GetType().FullName,
        before_identity = System.Runtime.CompilerServices.RuntimeHelpers.GetHashCode(field.Value),
        after_class = after.GetValueOrDefault(field.Key)?.GetType().FullName,
        after_identity = after.TryGetValue(field.Key, out object? actual) ? System.Runtime.CompilerServices.RuntimeHelpers.GetHashCode(actual) : (int?)null,
        same_reference = after.TryGetValue(field.Key, out object? current) && ReferenceEquals(field.Value, current),
    }).ToArray();
    File.WriteAllText($"reference-proof-{referenceProofOrdinal++:D4}.original.json", JsonSerializer.Serialize(new { before = before.Keys, after = after.Keys, fields, equal }));
    return JsonValue.Create(equal)!;
}
