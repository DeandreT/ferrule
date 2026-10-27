using System.Text;
using System.Text.Json;

namespace Ferrule.Runtime;

public enum FerruleJsonParameterIteration { RootStringOccurrences, DistinctRootStringKeys }
public enum FerruleJsonParameterInvocationError { Plan, DescriptorLimit, InvalidLimits, InvocationLimit, KeyBytes }

public sealed class FerruleJsonParameterInvocationException(FerruleJsonParameterInvocationError error)
    : Exception($"JSON parameter invocation refused: {error}")
{
    public FerruleJsonParameterInvocationError Error { get; } = error;
}

/// <summary>Explicit root-String selection, not a physical source or body plan.</summary>
public sealed class FerruleJsonParameterInvocationPlan
{
    private static readonly UTF8Encoding Utf8 = new(false, true);
    private FerruleJsonParameterInvocationPlan(string parameter, string keyMember,
        string keyParameter, FerruleJsonParameterIteration iteration)
    {
        Parameter = parameter;
        KeyMember = keyMember;
        KeyParameter = keyParameter;
        Iteration = iteration;
    }
    public string Parameter { get; }
    public string KeyMember { get; }
    public string KeyParameter { get; }
    public FerruleJsonParameterIteration Iteration { get; }

    public static FerruleJsonParameterInvocationPlan Parse(string descriptor)
    {
        ArgumentNullException.ThrowIfNull(descriptor);
        if (!FerruleUnicode.IsWellFormed(descriptor))
            throw InvalidPlan();
        if (Utf8.GetByteCount(descriptor) > 1024 * 1024)
            throw new FerruleJsonParameterInvocationException(FerruleJsonParameterInvocationError.DescriptorLimit);
        try
        {
            using var json = JsonDocument.Parse(descriptor);
            var root = json.RootElement;
            var expected = new[] { "parameter", "key_member", "iteration", "key_parameter", "projection" };
            var fields = root.EnumerateObject().Select(property => property.Name).ToArray();
            if (fields.Length != expected.Length || fields.Distinct(StringComparer.Ordinal).Count() != fields.Length || fields.Any(field => !expected.Contains(field)))
                throw InvalidPlan();
            var parameter = root.GetProperty("parameter").GetString()!;
            var member = root.GetProperty("key_member").GetString()!;
            var keyParameter = root.GetProperty("key_parameter").GetString()!;
            if (!ParameterName(parameter) || !ParameterName(keyParameter) || parameter == keyParameter)
                throw InvalidPlan();
            var projection = root.GetProperty("projection");
            _ = FerruleJsonSourceProjection.Parse(projection.GetRawText());
            if (projection.GetProperty("kind").GetString() != "object")
                throw InvalidPlan();
            if (!projection.GetProperty("properties").EnumerateArray().Any(property =>
                property.GetProperty("name").GetString() == member &&
                property.GetProperty("shape").GetProperty("kind").GetString() == "string"))
                throw InvalidPlan();
            var iteration = root.GetProperty("iteration").GetString() switch
            {
                "root_string_occurrences" => FerruleJsonParameterIteration.RootStringOccurrences,
                "distinct_root_string_keys" => FerruleJsonParameterIteration.DistinctRootStringKeys,
                _ => throw InvalidPlan(),
            };
            return new(parameter, member, keyParameter, iteration);
        }
        catch (Exception error) when (error is JsonException or InvalidOperationException or KeyNotFoundException or FerruleJsonProjectionException)
        {
            throw InvalidPlan();
        }
    }

    private static bool ParameterName(string value) =>
        !string.IsNullOrEmpty(value) && !value.Contains('\0') && FerruleUnicode.IsWellFormed(value) &&
        Utf8.GetByteCount(value) <= FerruleRuntimeParameters.MaximumNameUtf8Bytes;
    private static FerruleJsonParameterInvocationException InvalidPlan() => new(FerruleJsonParameterInvocationError.Plan);
}

public sealed record FerruleJsonParameterInvocationLimits
{
    public int Invocations { get; init; } = 4096;
    public int KeyBytes { get; init; } = 8 * 1024 * 1024;
}

public readonly record struct FerruleJsonParameterInvocation(
    string Key, FerruleJsonSourceValue ParameterObject, string KeyParameter)
{
    public KeyValuePair<string, FerruleValue> KeyBinding => new(KeyParameter, FerruleValue.FromString(Key));
}

/// <summary>All parameter syntax and selection complete before a host starts I/O.</summary>
public sealed class FerrulePreparedJsonParameterInvocations
{
    private readonly FerruleMapForceJsonString source;
    private readonly string[] keys;
    private readonly string keyParameter;
    private FerrulePreparedJsonParameterInvocations(FerruleMapForceJsonString source,
        string[] keys, string keyParameter)
    {
        this.source = source;
        this.keys = keys;
        this.keyParameter = keyParameter;
    }
    public int Count => keys.Length;
    public static FerrulePreparedJsonParameterInvocations Prepare(
        FerruleJsonParameterInvocationPlan plan, FerruleValue input,
        FerruleJsonSourceLimits? jsonLimits = null, FerruleJsonParameterInvocationLimits? limits = null)
    {
        ArgumentNullException.ThrowIfNull(plan);
        limits ??= new();
        if (limits.Invocations is < 0 or > 4096 || limits.KeyBytes is < 0 or > 8 * 1024 * 1024)
            throw new FerruleJsonParameterInvocationException(FerruleJsonParameterInvocationError.InvalidLimits);
        var source = FerruleMapForceJsonString.Parse(input, jsonLimits);
        var seen = new HashSet<string>(StringComparer.Ordinal);
        var keys = new List<string>();
        int bytes = 0;
        foreach (var key in source.Strings(plan.KeyMember))
        {
            if (plan.Iteration == FerruleJsonParameterIteration.DistinctRootStringKeys && !seen.Add(key))
                continue;
            if (keys.Count == limits.Invocations)
                throw new FerruleJsonParameterInvocationException(FerruleJsonParameterInvocationError.InvocationLimit);
            bytes = checked(bytes + Encoding.UTF8.GetByteCount(key));
            if (bytes > limits.KeyBytes)
                throw new FerruleJsonParameterInvocationException(FerruleJsonParameterInvocationError.KeyBytes);
            keys.Add(key);
        }
        return new(source, keys.ToArray(), plan.KeyParameter);
    }
    public IEnumerable<FerruleJsonParameterInvocation> Invocations()
    {
        foreach (var key in keys)
            yield return new(key, source.Root, keyParameter);
    }
    public void Visit(Action<FerruleJsonParameterInvocation> invoke)
    {
        ArgumentNullException.ThrowIfNull(invoke);
        foreach (var invocation in Invocations())
            invoke(invocation);
    }
}
