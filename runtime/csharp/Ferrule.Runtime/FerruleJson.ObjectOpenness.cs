namespace Ferrule.Runtime;

public static partial class FerruleJson
{
    private static void ValidateDeclaredProperties(
        JsonSchemaNode schema,
        IReadOnlyList<JsonProperty> properties)
    {
        if (schema.Dynamic is not null)
        {
            return;
        }

        foreach (var property in properties)
        {
            if (schema.Child(property.Name) is null)
            {
                throw Boundary(
                    $"JSON object '{schema.Name}' does not allow property '{property.Name}'.");
            }
        }
    }
}
