namespace Ferrule.Runtime;

/// <summary>Host-supplied values available to generated mapping expressions.</summary>
public enum FerruleRuntimeValue
{
    MappingFilePath,
    MainMappingFilePath,
    CurrentDateTime,
}

/// <summary>Immutable host values shared by every scope context in one run.</summary>
public sealed class FerruleExecutionContext
{
    public FerruleExecutionContext(string mappingFilePath)
        : this(mappingFilePath, mappingFilePath, null)
    {
    }

    public FerruleExecutionContext(
        string mappingFilePath,
        string mainMappingFilePath,
        string? currentDateTime = null)
    {
        MappingFilePath = mappingFilePath ??
            throw new ArgumentNullException(nameof(mappingFilePath));
        MainMappingFilePath = mainMappingFilePath ??
            throw new ArgumentNullException(nameof(mainMappingFilePath));
        CurrentDateTime = currentDateTime;
    }

    public string MappingFilePath { get; }

    public string MainMappingFilePath { get; }

    public string? CurrentDateTime { get; }

    internal string? GetValue(FerruleRuntimeValue value) => value switch
    {
        FerruleRuntimeValue.MappingFilePath => MappingFilePath,
        FerruleRuntimeValue.MainMappingFilePath => MainMappingFilePath,
        FerruleRuntimeValue.CurrentDateTime => CurrentDateTime,
        _ => throw new ArgumentOutOfRangeException(nameof(value), value, null),
    };
}
