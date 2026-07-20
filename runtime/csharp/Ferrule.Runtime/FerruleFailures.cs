namespace Ferrule.Runtime;

/// <summary>Structured mapping-failure construction shared by generated mappings.</summary>
public static class FerruleFailures
{
    public static FerruleRuntimeException MappingFailure(
        int rule,
        FerruleValue? message)
    {
        if (rule < 1)
        {
            throw new ArgumentOutOfRangeException(nameof(rule));
        }

        var text = message.HasValue
            ? FerruleFunctions.ScalarText(message.Value)
            : null;
        var display = text is null
            ? $"mapping failure rule {rule}: mapping exception was raised"
            : $"mapping failure rule {rule}: {text}";
        return new FerruleRuntimeException(
            FerruleRuntimeError.MappingFailure,
            display,
            failureRule: rule,
            mappingFailureMessage: text);
    }
}
