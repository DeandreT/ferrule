using System.Collections;
using System.Reflection;
using System.Text;
using System.Text.Json;
using System.Text.Json.Nodes;

var utf8 = new UTF8Encoding(false, true);
JsonNode contract = JsonNode.Parse(File.ReadAllText("contract.json"))!;
var outcomes = new JsonArray();
int passed = 0, failed = 0;
try
{
foreach (JsonNode? entry in JsonNode.Parse(File.ReadAllText("libraries.json"))!.AsArray())
{
    string name = entry!["name"]!.GetValue<string>(), version = entry["version"]!.GetValue<string>();
    string form = entry["form"]!.GetValue<string>();
    JsonNode schema = JsonNode.Parse(File.ReadAllText($"{version}-schema.json"))!;
    Assembly assembly = Assembly.LoadFrom(Path.GetFullPath($"{name}/bin/Release/net10.0/{name}.dll"));
    Type map = assembly.GetType("Ferrule.Generated.GeneratedMapping", throwOnError: true)!;
    Type runtime = assembly.GetType("Ferrule.Runtime.FerruleX12", throwOnError: true)!;
    string sourceDescriptor = File.ReadAllText($"{name}-source-descriptor.json");
    string targetDescriptor = File.ReadAllText($"{name}-target-descriptor.json");
    object Context() => assembly.GetType("Ferrule.Runtime.FerruleExecutionContext")!
        .GetConstructors().Single(ctor => ctor.GetParameters().Length == 4)
        .Invoke(["invented-repeated", "invented-repeated", null, null]);
    string defaultWire = File.ReadAllText($"{version}-default.x12");
    JsonNode defaultValues = JsonNode.Parse(File.ReadAllText($"{version}-default-values.json"))!;

    if (form == "Options")
    {
        JsonNode options = contract["options"]!;
        string Lines(JsonNode lines) => defaultWire
            .Replace("REF*ZZ*A^B^C*1^^3*TAIL~", lines["REF"]!.GetValue<string>(), StringComparison.Ordinal)
            .Replace("N9*X*AB:1.5^^CD~", lines["N9"]!.GetValue<string>(), StringComparison.Ordinal);
        JsonNode Values(JsonNode replacements)
        {
            JsonNode values = defaultValues.DeepClone();
            values["REF"] = replacements["REF"]!.DeepClone();
            values["N9"] = replacements["N9"]!.DeepClone();
            return values;
        }
        string input = Lines(options["input_lines"]!), output = Lines(options["output_lines"]!);
        JsonNode typed = Expected(schema, Values(options["input_values"]!));
        ValueCase($"{name}-implied-typed", () => Raw(Call(map, "ParseX12", input)), typed);
        ValueCase($"{name}-implied-typed-bytes", () => Raw(Call(map, "ParseX12Bytes", utf8.GetBytes(input))), typed);
        ValueCase($"{name}-wire-to-wire", () => JsonValue.Create((string)Call(map, "ExecuteX12ToX12", input)!), JsonValue.Create(output));
        ValueCase($"{name}-wire-to-wire-context", () => JsonValue.Create((string)Call(map, "ExecuteX12ToX12", input, Context())!), JsonValue.Create(output));
        // Output lexical formats apply to each occurrence of a typed caller value.
        object scaled = Call(map, "ParseX12", input)!;
        object[] dates = options["lexical_occurrences"]!.AsArray().Select(date => Scalar(assembly, date!.GetValue<string>())).ToArray();
        object extended = ReplaceField(scaled, "REF", "REF02", Repeated(assembly, dates));
        JsonNode extendedBefore = Raw(extended)!;
        ValueCase($"{name}-lexical-occurrences", () => JsonValue.Create((string)Call(map, "SerializeX12", extended)!), JsonValue.Create(output));
        ValueCase($"{name}-lexical-occurrences-bytes", () => JsonValue.Create(utf8.GetString((byte[])Call(map, "SerializeX12Bytes", extended)!)), JsonValue.Create(output));
        ValueCase($"{name}-lexical-keeps-caller", () => Raw(extended), extendedBefore);
        continue;
    }

    foreach (string id in form == "Custom" ? new[] { "custom" } : new[] { "default", "sparse" })
    {
        string wire = File.ReadAllText($"{version}-{id}.x12");
        JsonNode values = JsonNode.Parse(File.ReadAllText($"{version}-{id}-values.json"))!;
        JsonNode typed = Expected(schema, values);
        ValueCase($"{name}-{id}-typed-text", () => Raw(Call(map, "ParseX12", wire)), typed);
        ValueCase($"{name}-{id}-typed-bytes", () => Raw(Call(map, "ParseX12Bytes", utf8.GetBytes(wire))), typed);
        ValueCase($"{name}-{id}-wire-to-wire", () => JsonValue.Create((string)Call(map, "ExecuteX12ToX12", wire)!), JsonValue.Create(wire));
        ValueCase($"{name}-{id}-wire-to-wire-bytes", () => JsonValue.Create(utf8.GetString((byte[])Call(map, "ExecuteX12ToX12Bytes", utf8.GetBytes(wire))!)), JsonValue.Create(wire));
        ValueCase($"{name}-{id}-wire-to-wire-context", () => JsonValue.Create((string)Call(map, "ExecuteX12ToX12", wire, Context())!), JsonValue.Create(wire));
        object parsed = Call(runtime, "ParseEmbedded", targetDescriptor, wire)!;
        ValueCase($"{name}-{id}-serialize", () => JsonValue.Create((string)Call(map, "SerializeX12", parsed)!), JsonValue.Create(wire));
        ValueCase($"{name}-{id}-serialize-bytes", () => JsonValue.Create(utf8.GetString((byte[])Call(map, "SerializeX12Bytes", parsed)!)), JsonValue.Create(wire));
        ValueCase($"{name}-{id}-caller-preserved", () => Raw(parsed), typed);
    }
    if (form != "Both") continue;

    // A present empty trailing element is one Null occurrence and is trimmed on output.
    string presentEmpty = defaultWire.Replace("N9*X*AB:1.5^^CD~", "N9*X*~", StringComparison.Ordinal);
    JsonNode presentEmptyValues = defaultValues.DeepClone();
    presentEmptyValues["N9"]!["N902"] = new JsonArray(new JsonObject { ["Code"] = null, ["Qty"] = null });
    ValueCase($"{name}-present-empty-typed", () => Raw(Call(map, "ParseX12", presentEmpty)), Expected(schema, presentEmptyValues));
    ValueCase($"{name}-present-empty-trimmed", () => JsonValue.Create((string)Call(map, "ExecuteX12ToX12", presentEmpty)!),
        JsonValue.Create(defaultWire.Replace("N9*X*AB:1.5^^CD~", "N9*X~", StringComparison.Ordinal)));

    ExactError($"{name}-undeclared-repetition", () => Call(map, "ParseX12", defaultWire.Replace("ORDER-00073", "A^B", StringComparison.Ordinal)), "undeclared_repetition");
    ExactError($"{name}-nonrepeating-in-repeating-segment", () => Call(map, "ParseX12", defaultWire.Replace("*TAIL~", "*T^U~", StringComparison.Ordinal)), "nonrepeating_in_repeating_segment");
    ExactError($"{name}-repeated-composite-syntax", () => Call(map, "ParseX12", defaultWire.Replace("A^B^C", "A^B:X^C", StringComparison.Ordinal)), "repeated_scalar_composite_syntax");
    ExactError($"{name}-repeated-invalid-int", () => Call(map, "ParseX12", defaultWire.Replace("1^^3", "1^x^3", StringComparison.Ordinal)), "repeated_invalid_int");
    ExactError($"{name}-repeated-extra-component", () => Call(map, "ParseX12", defaultWire.Replace("AB:1.5^", "AB:1.5:Z^", StringComparison.Ordinal)), "repeated_extra_component");

    JsonNode legacy = JsonNode.Parse(sourceDescriptor)!;
    legacy["version"] = "004010";
    ExactError($"{name}-descriptor-legacy-repeating", () => Call(runtime, "ParseEmbedded", legacy.ToJsonString(), defaultWire), "descriptor_legacy_repeating");
    JsonNode componentSchema = schema.DeepClone();
    Child(Child(componentSchema, "N9"), "N902")["kind"]!["children"]![0]!["repeating"] = true;
    JsonNode componentDescriptor = JsonNode.Parse(sourceDescriptor)!;
    componentDescriptor["schema"] = componentSchema.ToJsonString();
    ExactError($"{name}-descriptor-component-repeating", () => Call(runtime, "ParseEmbedded", componentDescriptor.ToJsonString(), defaultWire), "descriptor_component_repeating");
    JsonNode fixedSchema = schema.DeepClone();
    Child(Child(fixedSchema, "REF"), "REF02")["fixed"] = "A";
    JsonNode fixedDescriptor = JsonNode.Parse(sourceDescriptor)!;
    fixedDescriptor["schema"] = fixedSchema.ToJsonString();
    ExactError($"{name}-descriptor-fixed-repeating", () => Call(runtime, "ParseEmbedded", fixedDescriptor.ToJsonString(), defaultWire), "descriptor_fixed_repeating");

    object targetInput = Call(runtime, "ParseEmbedded", targetDescriptor, defaultWire)!;
    JsonNode before = Raw(targetInput)!;
    ExactError($"{name}-output-not-collection", () => Call(map, "SerializeX12", ReplaceField(targetInput, "REF", "REF02", Scalar(assembly, "X"))), "output_not_collection");
    ExactError($"{name}-output-item-repetition", () => Call(map, "SerializeX12", ReplaceField(targetInput, "REF", "REF02", Repeated(assembly, Scalar(assembly, "A^B")))), "output_item_repetition");
    object zero = ReplaceField(targetInput, "REF", "REF02", Repeated(assembly));
    ValueCase($"{name}-output-zero-occurrences", () => JsonValue.Create((string)Call(map, "SerializeX12", zero)!),
        JsonValue.Create(defaultWire.Replace("REF*ZZ*A^B^C*", "REF*ZZ**", StringComparison.Ordinal)));
    object missing = ReplaceField(targetInput, "REF", "REF02", null);
    ValueCase($"{name}-output-missing-occurrences", () => JsonValue.Create((string)Call(map, "SerializeX12", missing)!),
        JsonValue.Create(defaultWire.Replace("REF*ZZ*A^B^C*", "REF*ZZ**", StringComparison.Ordinal)));
    ValueCase($"{name}-original-keeps-caller", () => Raw(targetInput), before);
}
}
catch (Exception setupFailure)
{
    outcomes.Add(new JsonObject { ["label"] = "host-setup-failure", ["error"] = Error(setupFailure), ["pass"] = false });
    failed++;
}
finally { Flush(); }
Console.WriteLine(JsonSerializer.Serialize(new { passed, failed }));
return failed == 0 ? 0 : 1;

object? Call(Type type, string name, params object?[] arguments)
{
    MethodInfo method = type.GetMethods(BindingFlags.Public | BindingFlags.Static).Single(candidate =>
        candidate.Name == name && candidate.GetParameters().Length == arguments.Length);
    try { return method.Invoke(null, arguments); }
    catch (TargetInvocationException error) when (error.InnerException is not null) { throw error.InnerException; }
}
void ValueCase(string label, Func<JsonNode?> action, JsonNode? expected)
{
    JsonNode? actual = null; JsonNode? error = null;
    try { actual = action(); } catch (Exception failure) { error = Error(failure); }
    bool ok = error is null && JsonNode.DeepEquals(actual, expected);
    outcomes.Add(new JsonObject { ["label"] = label, ["actual"] = actual?.DeepClone(), ["error"] = error, ["expected"] = expected?.DeepClone(), ["pass"] = ok });
    if (ok) passed++; else failed++;
    Flush();
}
void ExactError(string label, Func<object?> action, string key)
{
    JsonNode? actual = null; JsonNode? error = null;
    try { actual = Capture(action()); } catch (Exception failure) { error = Error(failure); }
    JsonNode expected = contract["errors"]![key]!;
    bool ok = error is not null && error["type"]?.GetValue<string>() == "Ferrule.Runtime.FerruleX12Exception"
        && error["error"]?.GetValue<string>() == expected["error"]!.GetValue<string>()
        && error["message"]?.GetValue<string>() == expected["message"]!.GetValue<string>()
        && JsonNode.DeepEquals(error["segment_index"], expected["segment_index"])
        && JsonNode.DeepEquals(error["field_path"], expected["field_path"]);
    outcomes.Add(new JsonObject { ["label"] = label, ["actual"] = actual, ["error"] = error, ["expected"] = expected.DeepClone(), ["pass"] = ok });
    if (ok) passed++; else failed++;
    Flush();
}
void Flush() => File.WriteAllText("complete-outcomes.original.json", outcomes.ToJsonString(new JsonSerializerOptions { WriteIndented = true }));
JsonNode Error(Exception error) => new JsonObject {
    ["type"] = error.GetType().FullName, ["message"] = error.Message, ["full_original"] = error.ToString(),
    ["error"] = Property(error, "Error")?.ToString(), ["segment_index"] = JsonSerializer.SerializeToNode(Property(error, "SegmentIndex")),
    ["field_path"] = JsonSerializer.SerializeToNode(Property(error, "FieldPath")),
};
object? Property(object value, string name) => value.GetType().GetProperty(name)?.GetValue(value);
JsonNode? Capture(object? value) => value switch { null => null, string text => JsonValue.Create(text), byte[] bytes => JsonValue.Create(Convert.ToBase64String(bytes)), _ => Raw(value) };
JsonNode? Raw(object? instance)
{
    if (instance is null) return null;
    if (instance.GetType().Name == "FerruleGroup")
    {
        var fields = new JsonArray();
        foreach (object field in (IEnumerable)Property(instance, "Fields")!)
            fields.Add(new JsonObject { ["name"] = (string)Property(field, "Name")!, ["value"] = Raw(Property(field, "Value")) });
        return new JsonObject { ["kind"] = "group", ["fields"] = fields };
    }
    if (instance.GetType().Name == "FerruleRepeated")
    {
        var items = new JsonArray();
        foreach (object item in (IEnumerable)Property(instance, "Items")!) items.Add(Raw(item));
        return new JsonObject { ["kind"] = "repeated", ["items"] = items };
    }
    if (instance.GetType().Name == "FerruleScalar")
    {
        object value = Property(instance, "Value")!;
        string kind = Property(value, "Kind")!.ToString()!;
        object? payload = kind switch { "String" => Property(value, "StringValue"), "Int64" => Property(value, "Int64Value"), "Double" => Property(value, "DoubleValue"), "Bool" => Property(value, "BooleanValue"), _ => null };
        return new JsonObject { ["kind"] = "scalar", ["type"] = kind, ["value"] = JsonSerializer.SerializeToNode(payload) };
    }
    return new JsonObject { ["unexpected_instance_type"] = instance.GetType().FullName, ["original"] = instance.ToString() };
}
JsonNode Expected(JsonNode schema, JsonNode? literal, bool item = false)
{
    if (!item && schema["repeating"]?.GetValue<bool>() == true)
    {
        var items = new JsonArray();
        foreach (JsonNode? value in literal!.AsArray()) items.Add(Expected(schema, value, item: true));
        return new JsonObject { ["kind"] = "repeated", ["items"] = items };
    }
    if (schema["kind"]!["kind"]!.GetValue<string>() == "scalar")
    {
        string kind = literal is null ? "Null" : schema["kind"]!["ty"]!.GetValue<string>() switch { "string" => "String", "int" => "Int64", "float" => "Double", _ => "Unspecified" };
        JsonNode? value = literal?.DeepClone();
        if (kind == "Double") value = JsonValue.Create(literal!.GetValue<double>());
        return new JsonObject { ["kind"] = "scalar", ["type"] = kind, ["value"] = value };
    }
    var fields = new JsonArray();
    foreach (JsonNode? child in schema["kind"]!["children"]!.AsArray())
    {
        string name = child!["name"]!.GetValue<string>();
        fields.Add(new JsonObject { ["name"] = name, ["value"] = Expected(child, literal?[name]) });
    }
    return new JsonObject { ["kind"] = "group", ["fields"] = fields };
}
JsonNode Child(JsonNode node, string name)
    => node["kind"]!["children"]!.AsArray().Single(child => child!["name"]!.GetValue<string>() == name)!;
object Scalar(Assembly assembly, string text)
{
    object value = Call(assembly.GetType("Ferrule.Runtime.FerruleValue")!, "FromString", text)!;
    return Activator.CreateInstance(assembly.GetType("Ferrule.Runtime.FerruleScalar")!, value)!;
}
object Repeated(Assembly assembly, params object[] items)
{
    Type instanceType = assembly.GetType("Ferrule.Runtime.FerruleInstance")!;
    var list = (IList)Activator.CreateInstance(typeof(List<>).MakeGenericType(instanceType))!;
    foreach (object item in items) list.Add(item);
    Type repeatedType = assembly.GetType("Ferrule.Runtime.FerruleRepeated")!;
    ConstructorInfo constructor = repeatedType.GetConstructors().Single(candidate => candidate.GetParameters().Length == 1);
    return constructor.Invoke([list]);
}
object ReplaceField(object instance, string segment, string field, object? replacement)
{
    Assembly assembly = instance.GetType().Assembly;
    Type fieldType = assembly.GetType("Ferrule.Runtime.FerruleField")!;
    Type groupType = assembly.GetType("Ferrule.Runtime.FerruleGroup")!;
    object Group(IEnumerable<(string Name, object Value)> entries)
    {
        var materialized = entries.ToArray(); Array fields = Array.CreateInstance(fieldType, materialized.Length);
        for (int index = 0; index < fields.Length; index++) fields.SetValue(Activator.CreateInstance(fieldType, materialized[index].Name, materialized[index].Value), index);
        return Activator.CreateInstance(groupType, fields)!;
    }
    var outer = new List<(string, object)>();
    foreach (object original in (IEnumerable)Property(instance, "Fields")!)
    {
        string name = (string)Property(original, "Name")!; object value = Property(original, "Value")!;
        if (name == segment)
        {
            var inner = new List<(string, object)>();
            foreach (object member in (IEnumerable)Property(value, "Fields")!)
            {
                string memberName = (string)Property(member, "Name")!;
                if (memberName == field) { if (replacement is not null) inner.Add((memberName, replacement)); }
                else inner.Add((memberName, Property(member, "Value")!));
            }
            value = Group(inner);
        }
        outer.Add((name, value));
    }
    return Group(outer);
}
