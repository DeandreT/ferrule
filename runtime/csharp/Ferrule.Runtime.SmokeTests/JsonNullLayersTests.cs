using Ferrule.Runtime;

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    /// <summary>
    /// The JSON null layers are compared with the saved-flag reads they
    /// replaced over every JSON role-flag combination, and contradictory
    /// roles are refused when the embedded schema is parsed.
    /// </summary>
    private static void JsonNullLayersMatchPreviousFlagReads()
    {
        var (matched, refused) = (0, 0);
        foreach (var scalar in new[] { false, true })
        foreach (var repeating in new[] { false, true })
        foreach (var nullable in new[] { false, true })
        foreach (var container in new[] { false, true })
        foreach (var wrapper in new[] { false, true })
        foreach (var jsonAny in new[] { false, true })
        foreach (var item in new bool?[] { null, false, true })
        {
            var contradiction = FerruleJson.JsonNullLayers.Contradiction(scalar, repeating, nullable, container);
            Equal(nullable && !scalar || container && scalar && !repeating, contradiction is not null);
            // The descriptor parser already refuses a misplaced nested-array frame
            // or object-item override before any layer is read.
            var frameRefused = wrapper && (scalar || !repeating || nullable || jsonAny);
            var itemRefused = item is not null && (!repeating || scalar || wrapper || jsonAny);
            if (contradiction is not null || frameRefused || itemRefused)
            {
                refused++;
                continue;
            }
            matched++;
            var layers = FerruleJson.JsonNullLayers.Derive(
                scalar, repeating, nullable, container, item, wrapper, jsonAny);
            // Previous reads: ContainerNullable, Nullable and SingleContainerNullable,
            // which frames and arbitrary JSON never consulted.
            Equal(container, layers.Whole);
            Equal(nullable, layers.Value);
            if (!wrapper && !jsonAny)
            {
                Equal(!scalar && (item ?? container), layers.Single);
            }
        }
        Equal((26, 166), (matched, refused));

        static string Child(string metadata, string kind) =>
            "{\"name\":\"Root\",\"kind\":{\"kind\":\"group\",\"children\":[{\"name\":\"x\"," + metadata +
            ",\"kind\":" + kind + "}]}}";
        const string Scalar = "{\"kind\":\"scalar\",\"ty\":\"int\"}";
        const string Group = "{\"kind\":\"group\",\"children\":[]}";
        foreach (var (metadata, kind, reason) in new[]
                 {
                     ("\"nullable\":true", Group, "scalar nullability is declared on a group"),
                     ("\"container_nullable\":true", Scalar, "container nullability is declared on a singular scalar"),
                 })
        {
            try
            {
                FerruleJson.Parse(Child(metadata, kind), "{}");
                throw new InvalidOperationException($"Expected a refused schema: {metadata}");
            }
            catch (FerruleRuntimeException error) when (error.Message.Contains(reason, StringComparison.Ordinal))
            {
            }
        }

        // Each layer admits only its own null.
        var nullableObject = Child("\"container_nullable\":true", Group);
        Equal(FerruleValueKind.JsonNull, Field(FerruleJson.Parse(nullableObject, "{\"x\":null}")).Kind);
        Equal("{\n  \"x\": null\n}\n", FerruleJson.Serialize(nullableObject,
            new FerruleGroup(new[] { new FerruleField("x", new FerruleScalar(FerruleValue.JsonNull)) })));
        var scalarItems = Child("\"repeating\":true,\"nullable\":true", Scalar);
        Throws<FerruleRuntimeException>(() => FerruleJson.Parse(scalarItems, "{\"x\":null}"));
        _ = FerruleJson.Parse(scalarItems, "{\"x\":[null]}");
        var containerOnly = Child("\"repeating\":true,\"container_nullable\":true", Scalar);
        _ = FerruleJson.Parse(containerOnly, "{\"x\":null}");
        Throws<FerruleRuntimeException>(() => FerruleJson.Parse(containerOnly, "{\"x\":[null]}"));
        var objectItems = Child("\"repeating\":true,\"json_object_item_nullable\":true", Group);
        Throws<FerruleRuntimeException>(() => FerruleJson.Parse(objectItems, "{\"x\":null}"));
        _ = FerruleJson.Parse(objectItems, "{\"x\":[null]}");
        // XML nil is not JSON null.
        Throws<FerruleRuntimeException>(() => FerruleJson.Parse(Child("\"nillable\":true", Scalar), "{\"x\":null}"));

        static FerruleValue Field(FerruleInstance instance) =>
            ((FerruleScalar)((FerruleGroup)instance).Fields[0].Value).Value;
    }
}
