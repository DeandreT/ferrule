using System.Collections.ObjectModel;
using System.Globalization;
using System.Text;
using System.Text.RegularExpressions;

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
    public const ulong MaximumGeneratedSequenceItems = 1_000_000;
    public const int MaximumRecursiveSequenceDepth = 256;
    private const int MaximumTokenizeRegexPatternBytes = 64 * 1024;

    /// <summary>Splits around a literal delimiter while preserving empty items.</summary>
    public static IReadOnlyList<FerruleValue> Tokenize(
        FerruleValue input,
        FerruleValue delimiter)
    {
        var text = RequireString(input, "tokenize");
        var separator = RequireString(delimiter, "tokenize");
        if (separator.Length == 0)
        {
            throw InvalidArgument("tokenize", "requires a non-empty delimiter");
        }

        var values = new List<FerruleValue>();
        var start = 0;
        while (true)
        {
            var next = text.IndexOf(separator, start, StringComparison.Ordinal);
            if (next < 0)
            {
                values.Add(FerruleValue.FromString(text[start..]));
                break;
            }
            values.Add(FerruleValue.FromString(text[start..next]));
            start = next + separator.Length;
        }
        return new ReadOnlyCollection<FerruleValue>(values);
    }

    /// <summary>Chunks text by Unicode scalar count.</summary>
    public static IReadOnlyList<FerruleValue> TokenizeByLength(
        FerruleValue input,
        FerruleValue length)
    {
        var text = RequireString(input, "tokenize-by-length");
        var chunkLength = length.Kind switch
        {
            FerruleValueKind.Int64 => length.Int64Value,
            FerruleValueKind.Double when double.IsFinite(length.DoubleValue) =>
                TruncatedInt64(length.DoubleValue),
            FerruleValueKind.String => ParseItemCount(length.StringValue),
            _ => null,
        };
        if (!chunkLength.HasValue || chunkLength.Value <= 0)
        {
            throw InvalidArgument(
                "tokenize-by-length",
                "requires a positive integer length");
        }

        var runes = text.EnumerateRunes().ToArray();
        var size = chunkLength.Value > int.MaxValue
            ? int.MaxValue
            : (int)chunkLength.Value;
        var values = new List<FerruleValue>();
        for (var start = 0; start < runes.Length;)
        {
            var count = Math.Min(size, runes.Length - start);
            var builder = new StringBuilder();
            for (var index = 0; index < count; index++)
            {
                builder.Append(runes[start + index].ToString());
            }
            values.Add(FerruleValue.FromString(builder.ToString()));
            start += count;
        }
        return new ReadOnlyCollection<FerruleValue>(values);
    }

    /// <summary>Splits text using bounded XPath-compatible regex flags.</summary>
    public static IReadOnlyList<FerruleValue> TokenizeRegex(
        FerruleValue input,
        FerruleValue pattern,
        FerruleValue? flags)
    {
        var text = RequireString(input, "tokenize-regexp");
        var expression = RequireString(pattern, "tokenize-regexp");
        var flagText = flags.HasValue
            ? RequireString(flags.Value, "tokenize-regexp")
            : string.Empty;
        var patternBytes = Encoding.UTF8.GetByteCount(expression);
        if (patternBytes > MaximumTokenizeRegexPatternBytes)
        {
            throw new FerruleRuntimeException(
                FerruleRuntimeError.TokenizeRegexPatternTooLarge,
                $"tokenize-regexp pattern is {patternBytes} bytes; maximum is {MaximumTokenizeRegexPatternBytes}",
                detail: patternBytes.ToString(CultureInfo.InvariantCulture),
                maximumItems: MaximumTokenizeRegexPatternBytes);
        }

        var options = RegexOptions.CultureInvariant | RegexOptions.NonBacktracking;
        foreach (var flag in flagText)
        {
            options |= flag switch
            {
                'i' => RegexOptions.IgnoreCase,
                'm' => RegexOptions.Multiline,
                's' => RegexOptions.Singleline,
                'x' => RegexOptions.IgnorePatternWhitespace,
                _ => throw new FerruleRuntimeException(
                    FerruleRuntimeError.InvalidTokenizeRegexFlags,
                    $"tokenize-regexp flags `{flagText}` contain an unsupported flag",
                    detail: flagText),
            };
        }

        Regex regex;
        try
        {
            regex = new Regex(expression, options);
        }
        catch (Exception error) when (error is ArgumentException or NotSupportedException)
        {
            throw new FerruleRuntimeException(
                FerruleRuntimeError.InvalidTokenizeRegex,
                $"tokenize-regexp pattern is invalid: {error.Message}",
                error,
                detail: error.Message);
        }
        if (regex.IsMatch(string.Empty))
        {
            throw ZeroWidthRegex();
        }
        if (text.Length == 0)
        {
            return Array.Empty<FerruleValue>();
        }

        var values = new List<FerruleValue>();
        var start = 0;
        foreach (var match in regex.EnumerateMatches(text))
        {
            if (match.Length == 0)
            {
                throw ZeroWidthRegex();
            }
            AddRegexToken(values, text[start..match.Index]);
            start = match.Index + match.Length;
        }
        AddRegexToken(values, text[start..]);
        return new ReadOnlyCollection<FerruleValue>(values);
    }

    /// <summary>Generates a bounded inclusive integer range.</summary>
    public static IReadOnlyList<FerruleValue> GenerateRange(
        FerruleValue? from,
        FerruleValue to)
    {
        var first = from.HasValue ? SequenceInteger(from.Value) : 1L;
        var last = SequenceInteger(to);
        if (first > last)
        {
            return Array.Empty<FerruleValue>();
        }

        var requested = (UInt128)((Int128)last - (Int128)first + 1);
        if (requested > MaximumGeneratedSequenceItems)
        {
            throw new FerruleRuntimeException(
                FerruleRuntimeError.GeneratedSequenceTooLarge,
                $"generate-sequence requested {requested} items; maximum is {MaximumGeneratedSequenceItems}",
                requestedItems: requested,
                maximumItems: MaximumGeneratedSequenceItems);
        }

        var values = new List<FerruleValue>((int)requested);
        var current = first;
        while (true)
        {
            values.Add(FerruleValue.FromInt64(current));
            if (current == last)
            {
                break;
            }
            current++;
        }
        return new ReadOnlyCollection<FerruleValue>(values);
    }

    /// <summary>Converts one recursive sequence prefix or separator.</summary>
    public static string RecursiveCollectArgumentText(FerruleValue value) =>
        value.Kind == FerruleValueKind.Null ? string.Empty : RecursiveScalarText(value);

    /// <summary>Collects scalar leaves from a recursive source tree in preorder.</summary>
    public static IReadOnlyList<FerruleValue> RecursiveCollect(
        ScopeContext context,
        IReadOnlyList<string> collection,
        IReadOnlyList<string> children,
        IReadOnlyList<string> descentValue,
        IReadOnlyList<string> values,
        IReadOnlyList<string> value,
        string prefix,
        string separator)
    {
        ArgumentNullException.ThrowIfNull(context);
        ArgumentNullException.ThrowIfNull(collection);
        ArgumentNullException.ThrowIfNull(children);
        ArgumentNullException.ThrowIfNull(descentValue);
        ArgumentNullException.ThrowIfNull(values);
        ArgumentNullException.ThrowIfNull(value);
        ArgumentNullException.ThrowIfNull(prefix);
        ArgumentNullException.ThrowIfNull(separator);
        ValidatePath(collection);
        ValidatePath(children);
        ValidatePath(descentValue);
        ValidatePath(values);
        ValidatePath(value);

        FerruleInstance? root = null;
        for (var index = context.Frames.Count - 1; index >= 0; index--)
        {
            var frame = context.Frames[index];
            if (collection.Count == 0 || HasField(frame, collection[0]))
            {
                root = frame;
                break;
            }
        }
        if (root is null && context.Frames.Count > 0)
        {
            root = context.Frames[^1];
        }
        if (root is null)
        {
            return Array.Empty<FerruleValue>();
        }

        var roots = new List<FerruleInstance>();
        CollectInstances(root, collection, 0, roots);
        var output = new List<FerruleValue>();
        foreach (var item in roots)
        {
            CollectRecursiveGroup(
                item,
                children,
                descentValue,
                values,
                value,
                prefix,
                separator,
                0,
                output);
        }
        return new ReadOnlyCollection<FerruleValue>(output);
    }

    private static void ValidatePath(IReadOnlyList<string> path)
    {
        for (var index = 0; index < path.Count; index++)
        {
            ArgumentNullException.ThrowIfNull(path[index]);
        }
    }

    private static void CollectRecursiveGroup(
        FerruleInstance group,
        IReadOnlyList<string> children,
        IReadOnlyList<string> descentValue,
        IReadOnlyList<string> values,
        IReadOnlyList<string> value,
        string prefix,
        string separator,
        int depth,
        List<FerruleValue> output)
    {
        if (depth >= MaximumRecursiveSequenceDepth)
        {
            throw new FerruleRuntimeException(
                FerruleRuntimeError.RecursiveSequenceDepth,
                $"recursive sequence exceeds the {MaximumRecursiveSequenceDepth}-group depth limit",
                maximumDepth: MaximumRecursiveSequenceDepth);
        }
        if (!TryScalarAt(group, descentValue, 0, out var segment))
        {
            return;
        }

        var currentPrefix = prefix + separator + RecursiveScalarText(segment);
        var leaves = new List<FerruleInstance>();
        CollectInstances(group, values, 0, leaves);
        foreach (var leaf in leaves)
        {
            if (!TryScalarAt(leaf, value, 0, out var leafValue))
            {
                continue;
            }
            if ((ulong)output.Count >= MaximumGeneratedSequenceItems)
            {
                throw new FerruleRuntimeException(
                    FerruleRuntimeError.RecursiveSequenceTooLarge,
                    $"recursive sequence produced more than {MaximumGeneratedSequenceItems} items",
                    maximumItems: MaximumGeneratedSequenceItems);
            }
            output.Add(FerruleValue.FromString(
                currentPrefix + separator + RecursiveScalarText(leafValue)));
        }

        var childGroups = new List<FerruleInstance>();
        CollectInstances(group, children, 0, childGroups);
        foreach (var child in childGroups)
        {
            CollectRecursiveGroup(
                child,
                children,
                descentValue,
                values,
                value,
                currentPrefix,
                separator,
                depth + 1,
                output);
        }
    }

    private static void CollectInstances(
        FerruleInstance instance,
        IReadOnlyList<string> path,
        int pathIndex,
        List<FerruleInstance> output)
    {
        if (pathIndex == path.Count)
        {
            switch (instance)
            {
                case FerruleRepeated repeated:
                    output.AddRange(repeated.Items);
                    break;
                case FerruleMappedSequence mapped:
                    output.AddRange(mapped.Items);
                    break;
                case FerruleDocumentSet documents:
                    output.AddRange(documents.Documents.Select(document => document.Value));
                    break;
                default:
                    output.Add(instance);
                    break;
            }
            return;
        }

        switch (instance)
        {
            case FerruleGroup group when group.TryGetField(path[pathIndex], out var child):
                CollectInstances(child, path, pathIndex + 1, output);
                break;
            case FerruleRepeated repeated:
                foreach (var item in repeated.Items)
                {
                    CollectInstances(item, path, pathIndex, output);
                }
                break;
            case FerruleMappedSequence mapped:
                foreach (var item in mapped.Items)
                {
                    CollectInstances(item, path, pathIndex, output);
                }
                break;
            case FerruleDocumentSet documents:
                foreach (var document in documents.Documents)
                {
                    CollectInstances(document.Value, path, pathIndex, output);
                }
                break;
        }
    }

    private static bool TryScalarAt(
        FerruleInstance instance,
        IReadOnlyList<string> path,
        int pathIndex,
        out FerruleValue value)
    {
        if (pathIndex == path.Count)
        {
            if (instance is FerruleScalar scalar)
            {
                value = scalar.Value;
                return true;
            }
            value = default;
            return false;
        }

        switch (instance)
        {
            case FerruleGroup group when group.TryGetField(path[pathIndex], out var child):
                return TryScalarAt(child, path, pathIndex + 1, out value);
            case FerruleRepeated { Items.Count: > 0 } repeated:
                return TryScalarAt(repeated.Items[0], path, pathIndex, out value);
            case FerruleMappedSequence { Items.Count: > 0 } mapped:
                return TryScalarAt(mapped.Items[0], path, pathIndex, out value);
            case FerruleDocumentSet { Documents.Count: > 0 } documents:
                return TryScalarAt(documents.Documents[0].Value, path, pathIndex, out value);
            default:
                value = default;
                return false;
        }
    }

    private static bool HasField(FerruleInstance instance, string name) => instance switch
    {
        FerruleGroup group => group.TryGetField(name, out _),
        FerruleDocumentSet { Documents.Count: > 0 } documents =>
            HasField(documents.Documents[0].Value, name),
        _ => false,
    };

    private static string RecursiveScalarText(FerruleValue value)
    {
        if (value.Kind is FerruleValueKind.Bool or FerruleValueKind.Int64 or FerruleValueKind.String ||
            value.Kind == FerruleValueKind.Double && double.IsFinite(value.DoubleValue))
        {
            return FerruleFunctions.ScalarText(value);
        }
        throw new FerruleRuntimeException(
            FerruleRuntimeError.FunctionType,
            $"`recursive-collect` cannot accept a {ValueTypeName(value)} argument.",
            function: "recursive-collect",
            foundKind: value.Kind);
    }

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

    /// <summary>Coerces and validates one positive grouping block size.</summary>
    public static ulong PositiveBlockSize(uint node, FerruleValue value)
    {
        var size = ItemCount(node, value);
        if (size == 0)
        {
            throw new FerruleRuntimeException(
                FerruleRuntimeError.InvalidBlockSize,
                $"Node {node}: grouping block size must be positive.",
                node: node);
        }
        return size;
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

    private static string RequireString(FerruleValue value, string function)
    {
        if (value.Kind == FerruleValueKind.String)
        {
            return value.StringValue;
        }
        throw new FerruleRuntimeException(
            FerruleRuntimeError.FunctionType,
            $"`{function}` cannot accept a {ValueTypeName(value)} argument.",
            function: function,
            foundKind: value.Kind);
    }

    private static void AddRegexToken(List<FerruleValue> values, string token)
    {
        if ((ulong)values.Count >= MaximumGeneratedSequenceItems)
        {
            throw new FerruleRuntimeException(
                FerruleRuntimeError.TokenizeRegexTooLarge,
                $"tokenize-regexp produced more than {MaximumGeneratedSequenceItems} items",
                maximumItems: MaximumGeneratedSequenceItems);
        }
        values.Add(FerruleValue.FromString(token));
    }

    private static FerruleRuntimeException ZeroWidthRegex() =>
        new(
            FerruleRuntimeError.ZeroWidthTokenizeRegex,
            "tokenize-regexp pattern matches a zero-width string");

    private static long SequenceInteger(FerruleValue value)
    {
        long? integer = value.Kind switch
        {
            FerruleValueKind.Int64 => value.Int64Value,
            FerruleValueKind.Double => ExactDoubleInteger(value.DoubleValue),
            FerruleValueKind.String => ParseSequenceInteger(value.StringValue),
            _ => null,
        };
        if (integer.HasValue)
        {
            return integer.Value;
        }
        throw new FerruleRuntimeException(
            FerruleRuntimeError.FunctionType,
            $"`generate-sequence` cannot accept a {ValueTypeName(value)} argument.",
            function: "generate-sequence",
            foundKind: value.Kind);
    }

    private static long? ParseSequenceInteger(string value)
    {
        var trimmed = value.Trim();
        if (long.TryParse(
            trimmed,
            NumberStyles.AllowLeadingSign,
            CultureInfo.InvariantCulture,
            out var integer))
        {
            return integer;
        }
        return double.TryParse(
            trimmed,
            NumberStyles.Float,
            CultureInfo.InvariantCulture,
            out var floating)
            ? ExactDoubleInteger(floating)
            : null;
    }

    private static long? ExactDoubleInteger(double value) =>
        double.IsFinite(value) &&
        Math.Truncate(value) == value &&
        value >= (double)long.MinValue &&
        value < (double)long.MaxValue
            ? (long)value
            : null;

    private static FerruleRuntimeException InvalidArgument(string function, string detail) =>
        new(
            FerruleRuntimeError.FunctionInvalidArgument,
            $"`{function}` {detail}.",
            function: function,
            detail: detail);

    private static string ValueTypeName(FerruleValue value) => value.Kind switch
    {
        FerruleValueKind.Null => "null",
        FerruleValueKind.XmlNil => "xml nil",
        FerruleValueKind.Bool => "bool",
        FerruleValueKind.Int64 => "int",
        FerruleValueKind.Double => "float",
        FerruleValueKind.String => "string",
        _ => "unknown",
    };

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
