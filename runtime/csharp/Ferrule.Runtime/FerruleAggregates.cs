using System.Globalization;

namespace Ferrule.Runtime;

/// <summary>Closed aggregate operations supported by generated mappings.</summary>
public enum FerruleAggregateOperation
{
    Count,
    Sum,
    Avg,
    Min,
    Max,
    Join,
    ItemAt,
}

/// <summary>Applies one scalar reduction over an already-evaluated item sequence.</summary>
public static class FerruleAggregates
{
    public static FerruleValue Apply(
        FerruleAggregateOperation operation,
        IReadOnlyList<FerruleValue> values,
        FerruleValue? argument = null)
    {
        ArgumentNullException.ThrowIfNull(values);
        return operation switch
        {
            FerruleAggregateOperation.Count => FerruleValue.FromInt64(values.Count),
            FerruleAggregateOperation.Sum => Sum(values),
            FerruleAggregateOperation.Avg => Average(values),
            FerruleAggregateOperation.Min => Extreme(operation, values),
            FerruleAggregateOperation.Max => Extreme(operation, values),
            FerruleAggregateOperation.Join => Join(values, argument),
            FerruleAggregateOperation.ItemAt => ItemAt(values, argument),
            _ => throw new ArgumentOutOfRangeException(nameof(operation), operation, null),
        };
    }

    private static FerruleValue Sum(IReadOnlyList<FerruleValue> values)
    {
        var numbers = NumericValues(FerruleAggregateOperation.Sum, values);
        if (numbers.All(number => number.IsInteger))
        {
            long sum = 0;
            try
            {
                foreach (var number in numbers)
                {
                    sum = checked(sum + number.Integer);
                }
            }
            catch (OverflowException exception)
            {
                throw IntegerOverflow(FerruleAggregateOperation.Sum, exception);
            }
            return FerruleValue.FromInt64(sum);
        }

        return FerruleValue.FromDouble(CompensatedSum(numbers));
    }

    private static FerruleValue Average(IReadOnlyList<FerruleValue> values)
    {
        var numbers = NumericValues(FerruleAggregateOperation.Avg, values);
        if (numbers.Count == 0)
        {
            return FerruleValue.Null;
        }
        return FerruleValue.FromDouble(CompensatedAverage(numbers));
    }

    private static FerruleValue Extreme(
        FerruleAggregateOperation operation,
        IReadOnlyList<FerruleValue> values)
    {
        var numbers = NumericValues(operation, values);
        if (numbers.Count == 0)
        {
            return FerruleValue.Null;
        }

        var best = numbers[0];
        var wanted = operation == FerruleAggregateOperation.Min ? -1 : 1;
        for (var index = 1; index < numbers.Count; index++)
        {
            if (Compare(numbers[index], best) == wanted)
            {
                best = numbers[index];
            }
        }
        return best.IntoValue();
    }

    private static FerruleValue Join(
        IReadOnlyList<FerruleValue> values,
        FerruleValue? argument)
    {
        var separator = argument.HasValue
            ? FerruleFunctions.ScalarText(argument.Value)
            : string.Empty;
        return FerruleValue.FromString(string.Join(
            separator,
            values
                .Where(value => value.Kind is not (FerruleValueKind.Null or FerruleValueKind.JsonNull))
                .Select(FerruleFunctions.ScalarText)));
    }

    private static FerruleValue ItemAt(
        IReadOnlyList<FerruleValue> values,
        FerruleValue? argument)
    {
        var index = argument.HasValue ? ItemIndex(argument.Value) : null;
        if (index is null || index < 1 || index > values.Count)
        {
            return FerruleValue.Null;
        }
        return values[(int)index.Value - 1];
    }

    private static long? ItemIndex(FerruleValue value)
    {
        return value.Kind switch
        {
            FerruleValueKind.Int64 => value.Int64Value,
            FerruleValueKind.Double => RoundedInt64(value.DoubleValue),
            FerruleValueKind.String when long.TryParse(
                value.StringValue.Trim(),
                NumberStyles.Integer,
                CultureInfo.InvariantCulture,
                out var index) => index,
            _ => null,
        };
    }

    private static long RoundedInt64(double value)
    {
        if (double.IsNaN(value))
        {
            return 0;
        }
        var rounded = Math.Round(value, MidpointRounding.AwayFromZero);
        if (rounded >= (double)long.MaxValue)
        {
            return long.MaxValue;
        }
        if (rounded <= (double)long.MinValue)
        {
            return long.MinValue;
        }
        return (long)rounded;
    }

    private static List<NumericValue> NumericValues(
        FerruleAggregateOperation operation,
        IReadOnlyList<FerruleValue> values)
    {
        var numbers = new List<NumericValue>(values.Count);
        foreach (var value in values)
        {
            if (TryNumericValue(operation, value, out var number))
            {
                numbers.Add(number);
            }
        }
        return numbers;
    }

    private static bool TryNumericValue(
        FerruleAggregateOperation operation,
        FerruleValue value,
        out NumericValue number)
    {
        if (value.Kind == FerruleValueKind.Int64)
        {
            number = NumericValue.FromInteger(value.Int64Value);
            return true;
        }
        if (value.Kind == FerruleValueKind.Double)
        {
            var floating = value.DoubleValue;
            if (!double.IsFinite(floating))
            {
                throw NonFinite(operation);
            }
            number = NumericValue.FromDouble(floating);
            return true;
        }
        if (value.Kind == FerruleValueKind.String)
        {
            var text = value.StringValue.Trim();
            if (long.TryParse(text, NumberStyles.Integer, CultureInfo.InvariantCulture, out var integer))
            {
                number = NumericValue.FromInteger(integer);
                return true;
            }
            if (double.TryParse(text, NumberStyles.Float, CultureInfo.InvariantCulture, out var floating))
            {
                if (!double.IsFinite(floating))
                {
                    throw NonFinite(operation);
                }
                number = NumericValue.FromDouble(floating);
                return true;
            }
            if (text.Equals("inf", StringComparison.OrdinalIgnoreCase) ||
                text.Equals("+inf", StringComparison.OrdinalIgnoreCase) ||
                text.Equals("-inf", StringComparison.OrdinalIgnoreCase))
            {
                throw NonFinite(operation);
            }
        }

        number = default;
        return false;
    }

    private static double CompensatedSum(IReadOnlyList<NumericValue> values)
    {
        var scale = values.Select(value => Math.Abs(value.AsDouble())).DefaultIfEmpty().Max();
        if (scale == 0.0)
        {
            return 0.0;
        }

        var sum = 0.0;
        var correction = 0.0;
        foreach (var number in values)
        {
            var value = number.AsDouble() / scale;
            var next = Finite(FerruleAggregateOperation.Sum, sum + value);
            correction += Math.Abs(sum) >= Math.Abs(value)
                ? (sum - next) + value
                : (value - next) + sum;
            correction = Finite(FerruleAggregateOperation.Sum, correction);
            sum = next;
        }

        var normalized = Finite(FerruleAggregateOperation.Sum, sum + correction);
        return Finite(FerruleAggregateOperation.Sum, normalized * scale);
    }

    private static double CompensatedAverage(IReadOnlyList<NumericValue> values)
    {
        var sum = 0.0;
        var correction = 0.0;
        var unscaled = true;
        foreach (var number in values)
        {
            var value = number.AsDouble();
            var next = sum + value;
            if (!double.IsFinite(next))
            {
                unscaled = false;
                break;
            }
            correction += Math.Abs(sum) >= Math.Abs(value)
                ? (sum - next) + value
                : (value - next) + sum;
            if (!double.IsFinite(correction))
            {
                unscaled = false;
                break;
            }
            sum = next;
        }

        if (unscaled)
        {
            var mean = (sum + correction) / values.Count;
            if (double.IsFinite(mean))
            {
                return mean;
            }
        }

        var scale = values.Select(value => Math.Abs(value.AsDouble())).Max();
        if (scale == 0.0)
        {
            return 0.0;
        }

        sum = 0.0;
        correction = 0.0;
        foreach (var number in values)
        {
            var value = number.AsDouble() / scale;
            var next = Finite(FerruleAggregateOperation.Avg, sum + value);
            correction += Math.Abs(sum) >= Math.Abs(value)
                ? (sum - next) + value
                : (value - next) + sum;
            correction = Finite(FerruleAggregateOperation.Avg, correction);
            sum = next;
        }

        var normalized = Finite(FerruleAggregateOperation.Avg, sum + correction);
        var scaledMean = Finite(FerruleAggregateOperation.Avg, normalized / values.Count);
        return Finite(FerruleAggregateOperation.Avg, scaledMean * scale);
    }

    private static int Compare(NumericValue left, NumericValue right)
    {
        if (left.IsInteger && right.IsInteger)
        {
            return left.Integer.CompareTo(right.Integer);
        }
        if (!left.IsInteger && !right.IsInteger)
        {
            return left.Double.CompareTo(right.Double);
        }
        return left.IsInteger
            ? CompareIntegerAndDouble(left.Integer, right.Double)
            : -CompareIntegerAndDouble(right.Integer, left.Double);
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

    private static double Finite(FerruleAggregateOperation operation, double value)
    {
        if (!double.IsFinite(value))
        {
            throw NonFinite(operation);
        }
        return value;
    }

    private static FerruleRuntimeException IntegerOverflow(
        FerruleAggregateOperation operation,
        OverflowException exception) =>
        new(
            FerruleRuntimeError.AggregateIntegerOverflow,
            $"{operation} aggregate overflowed the integer range",
            exception,
            aggregateOperation: operation);

    private static FerruleRuntimeException NonFinite(FerruleAggregateOperation operation) =>
        new(
            FerruleRuntimeError.AggregateNonFinite,
            $"{operation} aggregate encountered or produced a non-finite number",
            aggregateOperation: operation);

    private readonly struct NumericValue
    {
        private NumericValue(long integer)
        {
            Integer = integer;
            Double = default;
            IsInteger = true;
        }

        private NumericValue(double floating)
        {
            Integer = default;
            Double = floating;
            IsInteger = false;
        }

        internal long Integer { get; }

        internal double Double { get; }

        internal bool IsInteger { get; }

        internal double AsDouble() => IsInteger ? Integer : Double;

        internal FerruleValue IntoValue() => IsInteger
            ? FerruleValue.FromInt64(Integer)
            : FerruleValue.FromDouble(Double);

        internal static NumericValue FromInteger(long value) => new(value);

        internal static NumericValue FromDouble(double value) => new(value);
    }
}
