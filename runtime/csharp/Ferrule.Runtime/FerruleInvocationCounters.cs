using System.Text;

namespace Ferrule.Runtime;

public enum FerruleInvocationCounterFailure
{
    CountLimit,
    TextLimit,
    SizeOverflow,
    InvalidText,
}

public sealed class FerruleInvocationCounterException : InvalidOperationException
{
    public FerruleInvocationCounterFailure Failure { get; }
    public long? Maximum { get; }

    internal FerruleInvocationCounterException(FerruleInvocationCounterFailure failure, long? maximum = null)
        : base($"Invocation counter refused: {failure}.")
    {
        Failure = failure;
        Maximum = maximum;
    }
}

/// <summary>
/// Mutable counter state owned by one invocation, separate from immutable host
/// parameters. The owner orders calls; this type does not synchronize callers.
/// Construct another instance for another invocation, not another output document.
/// </summary>
public sealed class FerruleInvocationCounters
{
    private static readonly UTF8Encoding StrictUtf8 = new(false, true);
    private readonly record struct Counter(long Value, string? Restart, int KeyBytes, int RestartBytes);
    private readonly Dictionary<string, Counter> _counters = new(StringComparer.Ordinal);
    private readonly int _maximumCount;
    private readonly long _maximumTextBytes;

    public FerruleInvocationCounters(int maximumCount, long maximumTextBytes)
    {
        ArgumentOutOfRangeException.ThrowIfNegative(maximumCount);
        ArgumentOutOfRangeException.ThrowIfNegative(maximumTextBytes);
        _maximumCount = maximumCount;
        _maximumTextBytes = maximumTextBytes;
    }

    public int Count => _counters.Count;
    public long RetainedTextBytes { get; private set; }

    /// <summary>
    /// A new key or changed nullable restart returns start; an unchanged restart
    /// adds the current step with signed 64-bit wrapping. Limit failures leave
    /// all values and logical storage budgets unchanged.
    /// </summary>
    public long Next(string key, long start, long step, string? restart)
    {
        ArgumentNullException.ThrowIfNull(key);
        bool exists = _counters.TryGetValue(key, out var old);
        if (exists && string.Equals(old.Restart, restart, StringComparison.Ordinal))
        {
            long next = unchecked(old.Value + step);
            _counters[key] = old with { Value = next };
            return next;
        }
        if (!exists && _counters.Count >= _maximumCount)
            throw new FerruleInvocationCounterException(FerruleInvocationCounterFailure.CountLimit, _maximumCount);
        int keyBytes = exists ? old.KeyBytes : TextBytes(key);
        int restartBytes = restart is null ? 0 : TextBytes(restart);
        long retained;
        try
        {
            retained = checked(RetainedTextBytes - old.RestartBytes + (exists ? 0 : keyBytes) + restartBytes);
        }
        catch (OverflowException)
        {
            throw new FerruleInvocationCounterException(FerruleInvocationCounterFailure.SizeOverflow);
        }
        if (retained > _maximumTextBytes)
            throw new FerruleInvocationCounterException(FerruleInvocationCounterFailure.TextLimit, _maximumTextBytes);
        _counters[key] = new Counter(start, restart, keyBytes, restartBytes);
        RetainedTextBytes = retained;
        return start;
    }

    public bool Reset(string key)
    {
        ArgumentNullException.ThrowIfNull(key);
        if (!_counters.Remove(key, out var old))
            return false;
        RetainedTextBytes -= (long)old.KeyBytes + old.RestartBytes;
        return true;
    }

    private static int TextBytes(string text)
    {
        try { return StrictUtf8.GetByteCount(text); }
        catch (EncoderFallbackException)
        {
            throw new FerruleInvocationCounterException(FerruleInvocationCounterFailure.InvalidText);
        }
    }
}
