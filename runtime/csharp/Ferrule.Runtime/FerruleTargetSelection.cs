namespace Ferrule.Runtime;

/// <summary>Selects the primary target or one exact declared target name.</summary>
public sealed class FerruleTargetSelection
{
    private static readonly FerruleTargetSelection PrimarySelection = new(null);

    private FerruleTargetSelection(string? name)
    {
        Name = name;
    }

    public bool IsPrimary => Name is null;

    public string? Name { get; }

    public static FerruleTargetSelection Primary() => PrimarySelection;

    public static FerruleTargetSelection Named(string name)
    {
        ArgumentNullException.ThrowIfNull(name);
        return new FerruleTargetSelection(name);
    }
}
