using System.Globalization;
using System.Numerics;
using System.Text;

namespace Ferrule.Runtime;

public static partial class FerruleFunctions
{
    private static FerruleValue IsNumeric(IReadOnlyList<FerruleValue> arguments)
    {
        RequireArity("is_numeric", arguments, 1);
        var numeric = arguments[0].Kind switch
        {
            FerruleValueKind.Int64 or FerruleValueKind.Decimal => true,
            FerruleValueKind.Double => double.IsFinite(arguments[0].DoubleValue),
            FerruleValueKind.String => TryFiniteDouble(arguments[0].StringValue, out _),
            _ => false,
        };
        return FerruleValue.FromBoolean(numeric);
    }

    private static FerruleValue ToNumber(IReadOnlyList<FerruleValue> arguments)
    {
        RequireArity("to_number", arguments, 1);
        var value = arguments[0];
        if (value.Kind == FerruleValueKind.Decimal)
        {
            throw Type("to_number", value);
        }
        if (value.Kind is FerruleValueKind.Null or FerruleValueKind.JsonNull)
        {
            return value;
        }
        if (value.Kind == FerruleValueKind.Int64)
        {
            return value;
        }
        if (value.Kind == FerruleValueKind.Double && double.IsFinite(value.DoubleValue))
        {
            return value;
        }
        if (value.Kind == FerruleValueKind.String)
        {
            var text = TrimRustWhitespace(value.StringValue);
            if (long.TryParse(
                    text,
                    NumberStyles.AllowLeadingSign,
                    CultureInfo.InvariantCulture,
                    out var integer))
            {
                return FerruleValue.FromInt64(integer);
            }
            if (TryFiniteDouble(text, out var number))
            {
                return FerruleValue.FromDouble(number);
            }
        }

        throw InvalidArgument("to_number", "requires a finite numeric value");
    }

    private static FerruleValue EffectiveBoolean(IReadOnlyList<FerruleValue> arguments)
    {
        RequireArity("boolean", arguments, 1);
        var value = arguments[0];
        return FerruleValue.FromBoolean(value.Kind switch
        {
            FerruleValueKind.Null or
                FerruleValueKind.JsonNull or
                FerruleValueKind.XmlNil => false,
            FerruleValueKind.Bool => value.BooleanValue,
            FerruleValueKind.Int64 => value.Int64Value != 0,
            FerruleValueKind.Double => value.DoubleValue != 0.0 &&
                !double.IsNaN(value.DoubleValue),
            FerruleValueKind.String => value.StringValue.Length != 0,
            FerruleValueKind.Decimal => throw Type("boolean", value),
            _ => false,
        });
    }

    private static FerruleValue CoerceFloat(IReadOnlyList<FerruleValue> arguments)
    {
        const string function = "coerce_float";
        RequireArity(function, arguments, 1);
        var value = arguments[0];
        if (value.Kind != FerruleValueKind.String)
        {
            return value;
        }
        if (TryFiniteDouble(value.StringValue, out var number))
        {
            return FerruleValue.FromDouble(number);
        }
        throw InvalidArgument(function, "requires finite floating-point text");
    }

    private static FerruleValue Positive(IReadOnlyList<FerruleValue> arguments)
    {
        RequireArity("positive", arguments, 1);
        var operand = NumericOperand.From(arguments[0], "positive");
        return operand.IsDouble
            ? FerruleValue.FromDouble(operand.Double)
            : FerruleValue.FromInt64(operand.Integer);
    }

    private static FerruleValue Floor(IReadOnlyList<FerruleValue> arguments)
    {
        RequireArity("floor", arguments, 1);
        var operand = NumericOperand.From(arguments[0], "floor");
        if (!operand.IsDouble)
        {
            return FerruleValue.FromInt64(operand.Integer);
        }
        if (!double.IsFinite(operand.Double))
        {
            throw InvalidArgument("floor", "requires a finite numeric value");
        }
        return FerruleValue.FromDouble(Math.Floor(operand.Double));
    }

    private static FerruleValue DelayPassthrough(IReadOnlyList<FerruleValue> arguments)
    {
        RequireArity("delay_passthrough", arguments, 2);
        var duration = NumberArgument(arguments[1], "delay_passthrough");
        if (!double.IsFinite(duration) || duration < 0.0)
        {
            throw InvalidArgument(
                "delay_passthrough",
                "requires a finite nonnegative duration");
        }
        return arguments[0];
    }

    private static bool TryFiniteDouble(string value, out double number) =>
        double.TryParse(
            TrimRustWhitespace(value),
            NumberStyles.Float,
            CultureInfo.InvariantCulture,
            out number) &&
        double.IsFinite(number);

    private static FerruleValue Numeric(
        string function,
        IReadOnlyList<FerruleValue> arguments,
        NumericOperation operation)
    {
        if (arguments.Count < 2)
        {
            throw Arity(function, 2, arguments.Count);
        }

        var operands = arguments.Select(value => NumericOperand.From(value, function)).ToArray();
        if (operands.Any(operand => operand.IsDouble))
        {
            var result = operands[0].AsDouble();
            for (var index = 1; index < operands.Length; index++)
            {
                var operand = operands[index].AsDouble();
                result = operation switch
                {
                    NumericOperation.Add => result + operand,
                    NumericOperation.Subtract => result - operand,
                    NumericOperation.Multiply => result * operand,
                    _ => result,
                };
            }

            if (operation == NumericOperation.Multiply && TryExactDecimalProduct(arguments, out var exact))
            {
                result = exact;
            }

            return FerruleValue.FromDouble(result);
        }

        var integer = operands[0].Integer;
        try
        {
            for (var index = 1; index < operands.Length; index++)
            {
                integer = operation switch
                {
                    NumericOperation.Add => checked(integer + operands[index].Integer),
                    NumericOperation.Subtract => checked(integer - operands[index].Integer),
                    NumericOperation.Multiply => checked(integer * operands[index].Integer),
                    _ => integer,
                };
            }
        }
        catch (OverflowException exception)
        {
            throw new FerruleRuntimeException(
                FerruleRuntimeError.IntegerOverflow,
                $"`{function}` integer arithmetic overflowed.",
                exception,
                function: function);
        }

        return FerruleValue.FromInt64(integer);
    }

    private static FerruleValue SqliteMultiply(IReadOnlyList<FerruleValue> arguments)
    {
        const string function = "sqlite_multiply";
        RequireArity(function, arguments, 2);
        if (arguments.Any(value => value.Kind == FerruleValueKind.Decimal))
        {
            throw Type(function, arguments.First(value => value.Kind == FerruleValueKind.Decimal));
        }
        if (arguments.Any(value =>
                value.Kind is FerruleValueKind.Null or FerruleValueKind.JsonNull))
        {
            return FerruleValue.Null;
        }

        var left = NumericOperand.From(arguments[0], function);
        var right = NumericOperand.From(arguments[1], function);
        if (left.IsDouble || right.IsDouble)
        {
            return FerruleValue.FromDouble(left.AsDouble() * right.AsDouble());
        }
        try
        {
            return FerruleValue.FromInt64(checked(left.Integer * right.Integer));
        }
        catch (OverflowException)
        {
            return FerruleValue.FromDouble((double)left.Integer * right.Integer);
        }
    }

    private static FerruleValue Divide(IReadOnlyList<FerruleValue> arguments)
    {
        RequireArity("divide", arguments, 2);
        var left = NumericOperand.From(arguments[0], "divide").AsDouble();
        var right = NumericOperand.From(arguments[1], "divide").AsDouble();
        if (right == 0.0)
        {
            throw new FerruleRuntimeException(
                FerruleRuntimeError.DivideByZero,
                "Division by zero.");
        }

        return FerruleValue.FromDouble(left / right);
    }

    private static FerruleValue Comparison(
        string function,
        IReadOnlyList<FerruleValue> arguments,
        Func<int, bool> matches)
    {
        RequireArity(function, arguments, 2);
        var left = arguments[0];
        var right = arguments[1];
        if (left.Kind == FerruleValueKind.Decimal || right.Kind == FerruleValueKind.Decimal)
        {
            throw Type(function, left.Kind == FerruleValueKind.Decimal ? left : right);
        }
        if (left.Kind is FerruleValueKind.Null or FerruleValueKind.JsonNull or FerruleValueKind.XmlNil ||
            right.Kind is FerruleValueKind.Null or FerruleValueKind.JsonNull or FerruleValueKind.XmlNil)
        {
            return FerruleValue.FromBoolean(false);
        }

        var ordering = ValueOrdering(left, right);
        if (ordering is null)
        {
            throw Type(function, right);
        }

        return FerruleValue.FromBoolean(matches(ordering.Value));
    }

    private static int? ValueOrdering(FerruleValue left, FerruleValue right)
    {
        if (left.Kind == FerruleValueKind.Int64 && right.Kind == FerruleValueKind.Int64)
        {
            return left.Int64Value.CompareTo(right.Int64Value);
        }
        if (left.Kind == FerruleValueKind.Double && right.Kind == FerruleValueKind.Double)
        {
            return PartialDoubleComparison(left.DoubleValue, right.DoubleValue);
        }
        if (left.Kind == FerruleValueKind.Int64 && right.Kind == FerruleValueKind.Double)
        {
            return PartialDoubleComparison(left.Int64Value, right.DoubleValue);
        }
        if (left.Kind == FerruleValueKind.Double && right.Kind == FerruleValueKind.Int64)
        {
            return PartialDoubleComparison(left.DoubleValue, right.Int64Value);
        }
        if (left.Kind == FerruleValueKind.String && right.Kind == FerruleValueKind.String)
        {
            return CompareUnicodeScalars(left.StringValue, right.StringValue);
        }
        if (left.Kind == FerruleValueKind.String)
        {
            return CompareUnicodeScalars(left.StringValue, ScalarText(right));
        }
        if (right.Kind == FerruleValueKind.String)
        {
            return CompareUnicodeScalars(ScalarText(left), right.StringValue);
        }
        if (left.Kind == FerruleValueKind.Bool && right.Kind == FerruleValueKind.Bool)
        {
            return left.BooleanValue.CompareTo(right.BooleanValue);
        }

        return null;
    }

    private static int? PartialDoubleComparison(double left, double right) =>
        double.IsNaN(left) || double.IsNaN(right) ? null : left.CompareTo(right);

    private static int CompareUnicodeScalars(string left, string right)
    {
        var leftRunes = left.EnumerateRunes().GetEnumerator();
        var rightRunes = right.EnumerateRunes().GetEnumerator();
        while (true)
        {
            var hasLeft = leftRunes.MoveNext();
            var hasRight = rightRunes.MoveNext();
            if (!hasLeft || !hasRight)
            {
                return hasLeft.CompareTo(hasRight);
            }

            var comparison = leftRunes.Current.Value.CompareTo(rightRunes.Current.Value);
            if (comparison != 0)
            {
                return comparison;
            }
        }
    }

    private static List<Rune> Runes(string value)
    {
        var runes = new List<Rune>(value.Length);
        foreach (var rune in value.EnumerateRunes())
        {
            runes.Add(rune);
        }
        return runes;
    }

    private static int RuneCount(string value)
    {
        var count = 0;
        foreach (var _ in value.EnumerateRunes())
        {
            count++;
        }
        return count;
    }

    private static bool EqualIgnoringAsciiCase(Rune left, Rune right) =>
        AsciiLower(left.Value) == AsciiLower(right.Value);

    private static int AsciiLower(int value) => value is >= 'A' and <= 'Z'
        ? value + ('a' - 'A')
        : value;

    private const string RoundHalfCeilingName = "round_half_ceiling";
    private const string RoundHalfCeilingNotFinite = "requires a finite numeric value";

    // Mirrors the Rust `round_half_ceiling`: decimal digits, halves toward positive infinity.
    // Doubles contribute their shortest round-trip text, as Rust's `{:e}` does.
    private static FerruleValue RoundHalfCeiling(IReadOnlyList<FerruleValue> arguments)
    {
        if (arguments.Count is < 1 or > 2)
        {
            throw Arity(RoundHalfCeilingName, 1, arguments.Count);
        }
        var precision = arguments.Count == 2 ? HalfCeilingPrecision(arguments[1]) : 0;
        var value = arguments[0];
        switch (value.Kind)
        {
            case FerruleValueKind.Int64:
                return FerruleValue.FromInt64(HalfCeilingInteger(value.Int64Value, precision));
            case FerruleValueKind.Double when !double.IsFinite(value.DoubleValue):
                return value;
            case FerruleValueKind.Double:
                return FerruleValue.FromDouble(HalfCeilingDigits
                    .ParseOrThrow(value.DoubleValue.ToString("R", CultureInfo.InvariantCulture))
                    .Round(precision)
                    .ToDouble());
            case FerruleValueKind.String:
                var text = TrimRustWhitespace(value.StringValue);
                if (long.TryParse(
                        text,
                        NumberStyles.AllowLeadingSign,
                        CultureInfo.InvariantCulture,
                        out var integer))
                {
                    return FerruleValue.FromInt64(HalfCeilingInteger(integer, precision));
                }
                if (!TryFiniteDouble(text, out _))
                {
                    throw InvalidArgument(RoundHalfCeilingName, RoundHalfCeilingNotFinite);
                }
                return FerruleValue.FromDouble(
                    HalfCeilingDigits.ParseOrThrow(text).Round(precision).ToDouble());
            default:
                throw Type(RoundHalfCeilingName, value);
        }
    }

    private static int HalfCeilingPrecision(FerruleValue value)
    {
        switch (value.Kind)
        {
            case FerruleValueKind.Int64:
                return (int)Math.Clamp(value.Int64Value, int.MinValue, int.MaxValue);
            case FerruleValueKind.Double:
                return SaturatingInt32(RustRound(value.DoubleValue));
            case FerruleValueKind.String:
                var text = TrimRustWhitespace(value.StringValue);
                if (long.TryParse(
                        text,
                        NumberStyles.AllowLeadingSign,
                        CultureInfo.InvariantCulture,
                        out var integer))
                {
                    return (int)Math.Clamp(integer, int.MinValue, int.MaxValue);
                }
                if (TryFiniteDouble(text, out var number))
                {
                    return SaturatingInt32(RustRound(number));
                }
                throw InvalidArgument(RoundHalfCeilingName, RoundHalfCeilingNotFinite);
            default:
                throw Type(RoundHalfCeilingName, value);
        }
    }

    private static long HalfCeilingInteger(long integer, int precision)
    {
        if (precision >= 0)
        {
            return integer;
        }
        var rounded = HalfCeilingDigits
            .ParseOrThrow(integer.ToString(CultureInfo.InvariantCulture))
            .Round(precision);
        if (rounded.Digits.Length == 0)
        {
            return 0;
        }
        // A nonzero result has at least `-precision` trailing zeros.
        var magnitude = rounded.Exponent > 19
            ? BigInteger.Pow(10, 20)
            : BigInteger.Parse(rounded.Digits, CultureInfo.InvariantCulture) *
                BigInteger.Pow(10, (int)rounded.Exponent);
        var signed = rounded.Negative ? -magnitude : magnitude;
        if (signed < long.MinValue || signed > long.MaxValue)
        {
            throw new FerruleRuntimeException(
                FerruleRuntimeError.IntegerOverflow,
                $"`{RoundHalfCeilingName}` integer arithmetic overflowed.",
                function: RoundHalfCeilingName);
        }
        return (long)signed;
    }

    /// <summary>
    /// <c>Digits * 10^Exponent</c> with no leading or trailing zeros; zero has
    /// no digits and no sign.
    /// </summary>
    private sealed class HalfCeilingDigits
    {
        private const long ExponentLimit = 1_000_000_000_000;
        private const long FloatMagnitudeLimit = 400;

        private HalfCeilingDigits(bool negative, string digits, long exponent)
        {
            var leading = 0;
            while (leading < digits.Length && digits[leading] == '0')
            {
                leading++;
            }
            var end = digits.Length;
            while (end > leading && digits[end - 1] == '0')
            {
                end--;
            }
            Digits = digits[leading..end];
            Exponent = Digits.Length == 0 ? 0 : exponent + (digits.Length - end);
            Negative = Digits.Length != 0 && negative;
        }

        public bool Negative { get; }
        public string Digits { get; }
        public long Exponent { get; }

        public static HalfCeilingDigits ParseOrThrow(string text) =>
            Parse(text) ?? throw InvalidArgument(RoundHalfCeilingName, RoundHalfCeilingNotFinite);

        // `[+-]?(d+(.d*)?|.d+)([eE][+-]?d+)?`
        private static HalfCeilingDigits? Parse(string text)
        {
            var mantissa = text;
            long exponent = 0;
            var exponentIndex = text.IndexOfAny(['e', 'E']);
            if (exponentIndex >= 0)
            {
                mantissa = text[..exponentIndex];
                if (!TryParseExponent(text[(exponentIndex + 1)..], out exponent))
                {
                    return null;
                }
            }
            var negative = mantissa.StartsWith('-');
            if (mantissa.StartsWith('-') || mantissa.StartsWith('+'))
            {
                mantissa = mantissa[1..];
            }
            var point = mantissa.IndexOf('.');
            var whole = point < 0 ? mantissa : mantissa[..point];
            var fraction = point < 0 ? string.Empty : mantissa[(point + 1)..];
            if ((whole.Length == 0 && fraction.Length == 0) ||
                !whole.All(char.IsAsciiDigit) ||
                !fraction.All(char.IsAsciiDigit))
            {
                return null;
            }
            return new HalfCeilingDigits(negative, whole + fraction, exponent - fraction.Length);
        }

        private static bool TryParseExponent(string text, out long exponent)
        {
            exponent = 0;
            var negative = text.StartsWith('-');
            var digits = text.StartsWith('-') || text.StartsWith('+') ? text[1..] : text;
            if (digits.Length == 0 || !digits.All(char.IsAsciiDigit))
            {
                return false;
            }
            foreach (var digit in digits)
            {
                exponent = Math.Min(exponent * 10 + (digit - '0'), ExponentLimit);
            }
            if (negative)
            {
                exponent = -exponent;
            }
            return true;
        }

        public HalfCeilingDigits Round(int precision)
        {
            var last = -(long)precision;
            if (Digits.Length == 0 || Exponent >= last)
            {
                return this;
            }
            var dropped = last - Exponent;
            var kept = (int)Math.Max(Digits.Length - dropped, 0);
            // Below the first dropped place every digit is an implicit zero.
            var first = dropped > Digits.Length ? 0 : Digits[kept] - '0';
            var rest = dropped <= Digits.Length && Digits.AsSpan(kept + 1).ContainsAnyExcept('0');
            var up = first > 5 || (first == 5 && (rest || !Negative));
            var digits = Digits[..kept];
            if (up)
            {
                digits = Increment(digits);
            }
            return new HalfCeilingDigits(Negative, digits, last);
        }

        public double ToDouble()
        {
            if (Digits.Length == 0)
            {
                return 0.0;
            }
            var magnitude = Exponent + Digits.Length;
            var value = magnitude > FloatMagnitudeLimit
                ? double.PositiveInfinity
                : magnitude < -FloatMagnitudeLimit
                    ? 0.0
                    : double.Parse(
                        $"{Digits}e{Exponent.ToString(CultureInfo.InvariantCulture)}",
                        NumberStyles.Float,
                        CultureInfo.InvariantCulture);
            return Negative ? -value : value;
        }

        private static string Increment(string digits)
        {
            var result = digits.ToCharArray();
            for (var index = result.Length - 1; index >= 0; index--)
            {
                if (result[index] != '9')
                {
                    result[index]++;
                    return new string(result);
                }
                result[index] = '0';
            }
            return "1" + new string(result);
        }
    }

    private static double NumberArgument(FerruleValue value, string function) => value.Kind switch
    {
        FerruleValueKind.Int64 => value.Int64Value,
        FerruleValueKind.Double => value.DoubleValue,
        _ => throw Type(function, value),
    };

    private static double RustRound(double value) =>
        Math.Round(value, MidpointRounding.AwayFromZero);

    private static long SaturatingInt64(double value)
    {
        if (double.IsNaN(value))
        {
            return 0;
        }
        if (value >= -(double)long.MinValue)
        {
            return long.MaxValue;
        }
        if (value <= long.MinValue)
        {
            return long.MinValue;
        }
        return (long)Math.Truncate(value);
    }

    private static int SaturatingInt32(double value)
    {
        if (double.IsNaN(value))
        {
            return 0;
        }
        if (value >= int.MaxValue)
        {
            return int.MaxValue;
        }
        if (value <= int.MinValue)
        {
            return int.MinValue;
        }
        return (int)Math.Truncate(value);
    }

    private static long SaturatingAdd(long left, long right)
    {
        if (right > 0 && left > long.MaxValue - right)
        {
            return long.MaxValue;
        }
        if (right < 0 && left < long.MinValue - right)
        {
            return long.MinValue;
        }
        return left + right;
    }

    private static double PowInteger(double value, int exponent)
    {
        var reciprocal = exponent < 0;
        var result = 1.0;
        while (true)
        {
            if ((exponent & 1) != 0)
            {
                result *= value;
            }
            exponent /= 2;
            if (exponent == 0)
            {
                break;
            }
            value *= value;
        }
        return reciprocal ? 1.0 / result : result;
    }

    private static bool IsRustWhitespace(char character) => character is
        >= '\u0009' and <= '\u000D' or
        '\u0020' or
        '\u0085' or
        '\u00A0' or
        '\u1680' or
        >= '\u2000' and <= '\u200A' or
        '\u2028' or
        '\u2029' or
        '\u202F' or
        '\u205F' or
        '\u3000';

    internal static string ScalarText(FerruleValue value) => value.Kind switch
    {
        FerruleValueKind.Null or FerruleValueKind.JsonNull or FerruleValueKind.XmlNil => string.Empty,
        FerruleValueKind.Bool => value.BooleanValue ? "true" : "false",
        FerruleValueKind.Int64 => value.Int64Value.ToString(CultureInfo.InvariantCulture),
        FerruleValueKind.Double => DoubleText(value.DoubleValue),
        FerruleValueKind.String => value.StringValue,
        FerruleValueKind.Decimal => value.DecimalValue.ToString(),
        _ => string.Empty,
    };

    private static string DoubleText(double value)
    {
        if (double.IsNaN(value))
        {
            return "NaN";
        }
        if (double.IsPositiveInfinity(value))
        {
            return "inf";
        }
        if (double.IsNegativeInfinity(value))
        {
            return "-inf";
        }

        var text = value.ToString("R", CultureInfo.InvariantCulture);
        var exponentMarker = text.IndexOfAny(['E', 'e']);
        if (exponentMarker < 0)
        {
            return text;
        }

        var mantissa = text[..exponentMarker];
        var exponent = int.Parse(text[(exponentMarker + 1)..], CultureInfo.InvariantCulture);
        var negative = mantissa.StartsWith("-", StringComparison.Ordinal);
        if (negative)
        {
            mantissa = mantissa[1..];
        }
        var decimalPoint = mantissa.IndexOf('.');
        var digitsBeforePoint = decimalPoint < 0 ? mantissa.Length : decimalPoint;
        var digits = mantissa.Replace(".", string.Empty, StringComparison.Ordinal);
        var targetPoint = digitsBeforePoint + exponent;
        var expanded = targetPoint switch
        {
            <= 0 => "0." + new string('0', -targetPoint) + digits,
            _ when targetPoint >= digits.Length => digits + new string('0', targetPoint - digits.Length),
            _ => digits.Insert(targetPoint, "."),
        };
        return negative ? "-" + expanded : expanded;
    }

    private static bool TryExactDecimalProduct(
        IReadOnlyList<FerruleValue> values,
        out double result)
    {
        result = 0.0;
        if (!ExactDecimal.TryFrom(values[0], out var product))
        {
            return false;
        }
        for (var index = 1; index < values.Count; index++)
        {
            if (!ExactDecimal.TryFrom(values[index], out var operand) ||
                !product.TryMultiply(operand, out product))
            {
                return false;
            }
        }

        return double.TryParse(
                   $"{product.Coefficient}e{product.Exponent}",
                   NumberStyles.Float,
                   CultureInfo.InvariantCulture,
                   out result) &&
               double.IsFinite(result);
    }

    private enum NumericOperation
    {
        Add,
        Subtract,
        Multiply,
    }

    private readonly struct NumericOperand
    {
        private NumericOperand(long integer)
        {
            Integer = integer;
            Double = default;
            IsDouble = false;
        }

        private NumericOperand(double value)
        {
            Integer = default;
            Double = value;
            IsDouble = true;
        }

        internal long Integer { get; }

        internal double Double { get; }

        internal bool IsDouble { get; }

        internal double AsDouble() => IsDouble ? Double : Integer;

        internal static NumericOperand From(FerruleValue value, string function)
        {
            if (value.Kind == FerruleValueKind.Int64)
            {
                return new NumericOperand(value.Int64Value);
            }
            if (value.Kind == FerruleValueKind.Double)
            {
                return new NumericOperand(value.DoubleValue);
            }
            if (value.Kind == FerruleValueKind.String)
            {
                var text = value.StringValue.Trim();
                if (long.TryParse(text, NumberStyles.Integer, CultureInfo.InvariantCulture, out var integer))
                {
                    return new NumericOperand(integer);
                }
                if (double.TryParse(text, NumberStyles.Float, CultureInfo.InvariantCulture, out var number) &&
                    double.IsFinite(number))
                {
                    return new NumericOperand(number);
                }
            }

            throw Type(function, value);
        }
    }

    private readonly struct ExactDecimal
    {
        private static readonly BigInteger Minimum = -(BigInteger.One << 127);
        private static readonly BigInteger Maximum = (BigInteger.One << 127) - 1;

        private ExactDecimal(BigInteger coefficient, int exponent)
        {
            Coefficient = coefficient;
            Exponent = exponent;
        }

        internal BigInteger Coefficient { get; }

        internal int Exponent { get; }

        internal static bool TryFrom(FerruleValue value, out ExactDecimal result)
        {
            string? lexical = value.Kind switch
            {
                FerruleValueKind.Int64 => value.Int64Value.ToString(CultureInfo.InvariantCulture),
                FerruleValueKind.Double when value.DoubleValue == 0.0 &&
                                             double.IsNegative(value.DoubleValue) => null,
                FerruleValueKind.Double when double.IsFinite(value.DoubleValue) =>
                    DoubleText(value.DoubleValue),
                FerruleValueKind.String => CanonicalNumericString(value.StringValue),
                _ => null,
            };
            return TryParse(lexical, out result);
        }

        internal bool TryMultiply(ExactDecimal other, out ExactDecimal result)
        {
            var coefficient = Coefficient * other.Coefficient;
            var exponent = (long)Exponent + other.Exponent;
            if (!FitsInt128(coefficient) || exponent is < int.MinValue or > int.MaxValue)
            {
                result = default;
                return false;
            }
            result = Normalize(coefficient, (int)exponent);
            return true;
        }

        private static string? CanonicalNumericString(string value)
        {
            var text = value.Trim();
            if (long.TryParse(text, NumberStyles.Integer, CultureInfo.InvariantCulture, out var integer))
            {
                return integer.ToString(CultureInfo.InvariantCulture);
            }
            return double.TryParse(text, NumberStyles.Float, CultureInfo.InvariantCulture, out var number) &&
                   double.IsFinite(number) &&
                   !(number == 0.0 && double.IsNegative(number))
                ? DoubleText(number)
                : null;
        }

        private static bool TryParse(string? lexical, out ExactDecimal result)
        {
            result = default;
            if (lexical is null)
            {
                return false;
            }
            var exponentMarker = lexical.IndexOfAny(['e', 'E']);
            var mantissa = exponentMarker < 0 ? lexical : lexical[..exponentMarker];
            var scientificExponent = 0;
            if (exponentMarker >= 0 &&
                !int.TryParse(
                    lexical[(exponentMarker + 1)..],
                    NumberStyles.Integer,
                    CultureInfo.InvariantCulture,
                    out scientificExponent))
            {
                return false;
            }
            var negative = mantissa.StartsWith("-", StringComparison.Ordinal);
            if (negative)
            {
                mantissa = mantissa[1..];
            }
            var parts = mantissa.Split('.', 2);
            var whole = parts[0];
            var fraction = parts.Length == 2 ? parts[1] : string.Empty;
            if (whole.Length == 0 ||
                whole.Any(character => character is < '0' or > '9') ||
                fraction.Any(character => character is < '0' or > '9') ||
                !BigInteger.TryParse(
                    whole + fraction,
                    NumberStyles.None,
                    CultureInfo.InvariantCulture,
                    out var coefficient))
            {
                return false;
            }
            if (negative)
            {
                coefficient = -coefficient;
            }
            var exponent = (long)scientificExponent - fraction.Length;
            if (!FitsInt128(coefficient) || exponent is < int.MinValue or > int.MaxValue)
            {
                return false;
            }
            result = Normalize(coefficient, (int)exponent);
            return true;
        }

        private static ExactDecimal Normalize(BigInteger coefficient, int exponent)
        {
            while (!coefficient.IsZero && coefficient % 10 == 0)
            {
                coefficient /= 10;
                exponent++;
            }
            return new ExactDecimal(coefficient, exponent);
        }

        private static bool FitsInt128(BigInteger value) => value >= Minimum && value <= Maximum;
    }
}
