using System.Collections.ObjectModel;
using System.Globalization;
using System.Text;

namespace Ferrule.Runtime;

/// <summary>One scalar sort expression and its direction.</summary>
public readonly record struct FerruleSortKey<T>(
    Func<T, FerruleValue> Selector,
    bool Descending = false);

/// <summary>Closed sequence-window operations supported by generated mappings.</summary>
public enum FerruleSequenceWindowKind
{
    SkipFirst,
    First,
    From,
    FromTo,
    Last,
}

/// <summary>An evaluated sequence window whose bounds are valid item counts.</summary>
public readonly struct FerruleSequenceWindow
{
    private FerruleSequenceWindow(
        FerruleSequenceWindowKind kind,
        ulong first,
        ulong last = 0)
    {
        Kind = kind;
        FirstValue = first;
        LastValue = last;
    }

    public FerruleSequenceWindowKind Kind { get; }

    internal ulong FirstValue { get; }

    internal ulong LastValue { get; }

    public static FerruleSequenceWindow SkipFirst(ulong count) =>
        new(FerruleSequenceWindowKind.SkipFirst, count);

    public static FerruleSequenceWindow First(ulong count) =>
        new(FerruleSequenceWindowKind.First, count);

    public static FerruleSequenceWindow From(ulong position) =>
        new(FerruleSequenceWindowKind.From, position);

    public static FerruleSequenceWindow FromTo(ulong first, ulong last) =>
        new(FerruleSequenceWindowKind.FromTo, first, last);

    public static FerruleSequenceWindow Last(ulong count) =>
        new(FerruleSequenceWindowKind.Last, count);
}

/// <summary>Engine-compatible sorting, item-count, and window primitives.</summary>
public static class FerruleSequences
{
    /// <summary>
    /// Evaluates every key once in item/key order and returns a stable multi-key sort.
    /// Incomparable values behave as equal for that key.
    /// </summary>
    public static IReadOnlyList<T> StableSort<T>(
        IReadOnlyList<T> items,
        IReadOnlyList<FerruleSortKey<T>> keys)
    {
        ArgumentNullException.ThrowIfNull(items);
        ArgumentNullException.ThrowIfNull(keys);

        var decorated = new List<DecoratedItem<T>>(items.Count);
        for (var itemIndex = 0; itemIndex < items.Count; itemIndex++)
        {
            var item = items[itemIndex];
            var values = new FerruleValue[keys.Count];
            for (var keyIndex = 0; keyIndex < keys.Count; keyIndex++)
            {
                var selector = keys[keyIndex].Selector;
                ArgumentNullException.ThrowIfNull(selector);
                values[keyIndex] = selector(item);
            }
            decorated.Add(new DecoratedItem<T>(item, values, itemIndex));
        }

        decorated.Sort((left, right) => CompareDecorated(left, right, keys));
        return new ReadOnlyCollection<T>(decorated.Select(item => item.Item).ToArray());
    }

    /// <summary>
    /// Compares two scalar sort values. A null result means the values are
    /// incomparable and therefore tie for sorting purposes.
    /// </summary>
    public static int? CompareValues(FerruleValue left, FerruleValue right)
    {
        if (left.Kind == FerruleValueKind.Null)
        {
            return right.Kind == FerruleValueKind.Null ? 0 : -1;
        }
        if (right.Kind == FerruleValueKind.Null)
        {
            return 1;
        }

        return (left.Kind, right.Kind) switch
        {
            (FerruleValueKind.Int64, FerruleValueKind.Int64) =>
                left.Int64Value.CompareTo(right.Int64Value),
            (FerruleValueKind.Double, FerruleValueKind.Double) =>
                CompareFiniteDoubles(left.DoubleValue, right.DoubleValue),
            (FerruleValueKind.Int64, FerruleValueKind.Double) when
                double.IsFinite(right.DoubleValue) =>
                CompareIntegerAndDouble(left.Int64Value, right.DoubleValue),
            (FerruleValueKind.Double, FerruleValueKind.Int64) when
                double.IsFinite(left.DoubleValue) =>
                -CompareIntegerAndDouble(right.Int64Value, left.DoubleValue),
            (FerruleValueKind.String, FerruleValueKind.String) =>
                CompareUnicodeScalars(left.StringValue, right.StringValue),
            (FerruleValueKind.Bool, FerruleValueKind.Bool) =>
                left.BooleanValue.CompareTo(right.BooleanValue),
            _ => null,
        };
    }

    /// <summary>Coerces one scalar to the engine's nonnegative item-count domain.</summary>
    public static ulong ItemCount(uint node, FerruleValue value)
    {
        long? count = value.Kind switch
        {
            FerruleValueKind.Int64 => value.Int64Value,
            FerruleValueKind.Double when double.IsFinite(value.DoubleValue) =>
                TruncatedInt64(value.DoubleValue),
            FerruleValueKind.String => ParseItemCount(value.StringValue),
            _ => null,
        };
        if (!count.HasValue)
        {
            throw new FerruleRuntimeException(
                FerruleRuntimeError.NotAnItemCount,
                $"Node {node}: expected an item count, found {value.Kind}.",
                node: node,
                foundKind: value.Kind);
        }
        return count.Value <= 0 ? 0 : (ulong)count.Value;
    }

    /// <summary>Applies evaluated windows from left to right.</summary>
    public static IReadOnlyList<T> ApplyWindows<T>(
        IReadOnlyList<T> items,
        IReadOnlyList<FerruleSequenceWindow> windows)
    {
        ArgumentNullException.ThrowIfNull(items);
        ArgumentNullException.ThrowIfNull(windows);

        IReadOnlyList<T> current = items;
        foreach (var window in windows)
        {
            var (skip, take) = WindowRange(current.Count, window);
            var next = new T[take];
            for (var index = 0; index < take; index++)
            {
                next[index] = current[skip + index];
            }
            current = next;
        }
        return new ReadOnlyCollection<T>(current.ToArray());
    }

    private static int CompareDecorated<T>(
        DecoratedItem<T> left,
        DecoratedItem<T> right,
        IReadOnlyList<FerruleSortKey<T>> keys)
    {
        for (var index = 0; index < keys.Count; index++)
        {
            var comparison = CompareValues(left.Values[index], right.Values[index]) ?? 0;
            if (keys[index].Descending)
            {
                comparison = -comparison;
            }
            if (comparison != 0)
            {
                return comparison;
            }
        }
        return left.Ordinal.CompareTo(right.Ordinal);
    }

    private static int? CompareFiniteDoubles(double left, double right)
    {
        if (double.IsNaN(left) || double.IsNaN(right))
        {
            return null;
        }
        return left.CompareTo(right);
    }

    private static int CompareIntegerAndDouble(long integer, double floating)
    {
        if (floating >= (double)long.MaxValue)
        {
            return -1;
        }
        if (floating < (double)long.MinValue)
        {
            return 1;
        }

        var truncated = (long)Math.Truncate(floating);
        var comparison = integer.CompareTo(truncated);
        if (comparison != 0)
        {
            return comparison;
        }
        var fraction = floating - truncated;
        if (fraction > 0.0)
        {
            return -1;
        }
        if (fraction < 0.0)
        {
            return 1;
        }
        return 0;
    }

    private static int CompareUnicodeScalars(string left, string right)
    {
        var leftEnumerator = left.EnumerateRunes().GetEnumerator();
        var rightEnumerator = right.EnumerateRunes().GetEnumerator();
        while (true)
        {
            var hasLeft = leftEnumerator.MoveNext();
            var hasRight = rightEnumerator.MoveNext();
            if (!hasLeft || !hasRight)
            {
                return hasLeft.CompareTo(hasRight);
            }

            var comparison = leftEnumerator.Current.Value.CompareTo(rightEnumerator.Current.Value);
            if (comparison != 0)
            {
                return comparison;
            }
        }
    }

    private static long TruncatedInt64(double value)
    {
        if (value >= (double)long.MaxValue)
        {
            return long.MaxValue;
        }
        if (value <= (double)long.MinValue)
        {
            return long.MinValue;
        }
        return (long)Math.Truncate(value);
    }

    private static long? ParseItemCount(string value)
    {
        var trimmed = value.Trim();
        return long.TryParse(
            trimmed,
            NumberStyles.AllowLeadingSign,
            CultureInfo.InvariantCulture,
            out var count)
            ? count
            : null;
    }

    private static (int Skip, int Take) WindowRange(
        int length,
        FerruleSequenceWindow window)
    {
        var size = (ulong)length;
        var skip = window.Kind switch
        {
            FerruleSequenceWindowKind.SkipFirst => Math.Min(window.FirstValue, size),
            FerruleSequenceWindowKind.First => 0UL,
            FerruleSequenceWindowKind.From => Math.Min(
                window.FirstValue == 0 ? 0UL : window.FirstValue - 1,
                size),
            FerruleSequenceWindowKind.FromTo => Math.Min(
                window.FirstValue == 0 ? 0UL : window.FirstValue - 1,
                size),
            FerruleSequenceWindowKind.Last => size - Math.Min(window.FirstValue, size),
            _ => throw new ArgumentOutOfRangeException(nameof(window)),
        };
        var available = size - skip;
        var take = window.Kind switch
        {
            FerruleSequenceWindowKind.SkipFirst or FerruleSequenceWindowKind.From => available,
            FerruleSequenceWindowKind.First => Math.Min(window.FirstValue, size),
            FerruleSequenceWindowKind.FromTo => Math.Min(
                window.LastValue > skip ? window.LastValue - skip : 0UL,
                available),
            FerruleSequenceWindowKind.Last => Math.Min(window.FirstValue, size),
            _ => throw new ArgumentOutOfRangeException(nameof(window)),
        };
        return ((int)skip, (int)take);
    }

    private sealed record DecoratedItem<T>(T Item, FerruleValue[] Values, int Ordinal);
}
