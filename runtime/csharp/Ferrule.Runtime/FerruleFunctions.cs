using System.Globalization;
using System.Numerics;
using System.Text;

namespace Ferrule.Runtime;

/// <summary>Scalar functions supported by generated mappings.</summary>
public static class FerruleFunctions
{
    public static FerruleValue Call(string function, IReadOnlyList<FerruleValue> arguments)
    {
        ArgumentNullException.ThrowIfNull(function);
        ArgumentNullException.ThrowIfNull(arguments);
        return function switch
        {
            "concat" => Concat(arguments),
            "normalize_space" => UnaryString(
                function,
                arguments,
                NormalizeXmlSpace),
            "left_trim" => UnaryString(
                function,
                arguments,
                static value => value[CountLeadingXmlWhitespace(value)..]),
            "right_trim" => UnaryString(
                function,
                arguments,
                static value => value[..^CountTrailingXmlWhitespace(value)]),
            "length" => Length(arguments),
            "substring_before" => SplitString(function, arguments, before: true),
            "substring_after" => SplitString(function, arguments, before: false),
            "string" => StringValue(arguments),
            "substitute_missing" => SubstituteMissing(arguments),
            "is_xml_nil" => IsXmlNil(arguments),
            "get_folder" => UnaryString(function, arguments, GetFolder),
            "remove_folder" => UnaryString(function, arguments, RemoveFolder),
            "resolve_filepath" => ResolveFilePath(arguments),
            "and" => BinaryBoolean(function, arguments, (left, right) => left && right),
            "or" => BinaryBoolean(function, arguments, (left, right) => left || right),
            "not" => UnaryBoolean(function, arguments, value => !value),
            "exists" => Exists(arguments),
            "is_empty" => IsEmpty(arguments),
            "starts_with" => BinaryString(function, arguments, static (left, right) =>
                left.StartsWith(right, StringComparison.Ordinal)),
            "contains" => BinaryString(function, arguments, static (left, right) =>
                left.Contains(right, StringComparison.Ordinal)),
            "add" => Numeric(function, arguments, NumericOperation.Add),
            "subtract" => Numeric(function, arguments, NumericOperation.Subtract),
            "multiply" => Numeric(function, arguments, NumericOperation.Multiply),
            "divide" => Divide(arguments),
            "equal" => Comparison(function, arguments, ordering => ordering == 0),
            "not_equal" => Comparison(function, arguments, ordering => ordering != 0),
            "less_than" => Comparison(function, arguments, ordering => ordering < 0),
            "greater_than" => Comparison(function, arguments, ordering => ordering > 0),
            "less_or_equal" => Comparison(function, arguments, ordering => ordering <= 0),
            "greater_or_equal" => Comparison(function, arguments, ordering => ordering >= 0),
            _ => throw new FerruleRuntimeException(
                FerruleRuntimeError.UnknownFunction,
                $"Unknown function '{function}'.",
                function: function),
        };
    }

    public static bool RequireBoolean(FerruleValue value, uint conditionNode)
    {
        if (value.Kind != FerruleValueKind.Bool)
        {
            throw new FerruleRuntimeException(
                FerruleRuntimeError.NotABool,
                $"Graph node {conditionNode} expected a bool, found {TypeName(value)}.",
                node: conditionNode,
                foundKind: value.Kind);
        }

        return value.BooleanValue;
    }

    private static FerruleValue Exists(IReadOnlyList<FerruleValue> arguments)
    {
        RequireArity("exists", arguments, 1);
        return FerruleValue.FromBoolean(arguments[0].Kind != FerruleValueKind.Null);
    }

    private static FerruleValue IsEmpty(IReadOnlyList<FerruleValue> arguments)
    {
        RequireArity("is_empty", arguments, 1);
        return FerruleValue.FromBoolean(RequireString(arguments[0], "is_empty").Length == 0);
    }

    private static FerruleValue Concat(IReadOnlyList<FerruleValue> arguments)
    {
        var result = new StringBuilder();
        foreach (var argument in arguments)
        {
            if (argument.Kind is not (FerruleValueKind.Null or FerruleValueKind.XmlNil))
            {
                result.Append(ScalarText(argument));
            }
        }

        return FerruleValue.FromString(result.ToString());
    }

    private static FerruleValue UnaryString(
        string function,
        IReadOnlyList<FerruleValue> arguments,
        Func<string, string> operation)
    {
        RequireArity(function, arguments, 1);
        return FerruleValue.FromString(operation(RequireString(arguments[0], function)));
    }

    private static FerruleValue Length(IReadOnlyList<FerruleValue> arguments)
    {
        RequireArity("length", arguments, 1);
        long length = 0;
        foreach (var _ in ScalarText(arguments[0]).EnumerateRunes())
        {
            length++;
        }

        return FerruleValue.FromInt64(length);
    }

    private static FerruleValue SplitString(
        string function,
        IReadOnlyList<FerruleValue> arguments,
        bool before)
    {
        RequireArity(function, arguments, 2);
        var value = RequireString(arguments[0], function);
        var separator = RequireString(arguments[1], function);
        var index = value.IndexOf(separator, StringComparison.Ordinal);
        if (index < 0)
        {
            return FerruleValue.FromString(string.Empty);
        }

        var result = before
            ? value[..index]
            : value[(index + separator.Length)..];
        return FerruleValue.FromString(result);
    }

    private static FerruleValue StringValue(IReadOnlyList<FerruleValue> arguments)
    {
        RequireArity("string", arguments, 1);
        return FerruleValue.FromString(ScalarText(arguments[0]));
    }

    private static FerruleValue SubstituteMissing(IReadOnlyList<FerruleValue> arguments)
    {
        RequireArity("substitute_missing", arguments, 2);
        return arguments[0].Kind is FerruleValueKind.Null or FerruleValueKind.XmlNil
            ? arguments[1]
            : arguments[0];
    }

    private static FerruleValue IsXmlNil(IReadOnlyList<FerruleValue> arguments)
    {
        RequireArity("is_xml_nil", arguments, 1);
        return FerruleValue.FromBoolean(arguments[0].Kind == FerruleValueKind.XmlNil);
    }

    private static string NormalizeXmlSpace(string value)
    {
        var result = new StringBuilder(value.Length);
        var pendingSeparator = false;
        foreach (var character in value)
        {
            if (IsXmlWhitespace(character))
            {
                pendingSeparator = result.Length > 0;
                continue;
            }

            if (pendingSeparator)
            {
                result.Append(' ');
                pendingSeparator = false;
            }
            result.Append(character);
        }

        return result.ToString();
    }

    private static int CountLeadingXmlWhitespace(string value)
    {
        var count = 0;
        while (count < value.Length && IsXmlWhitespace(value[count]))
        {
            count++;
        }
        return count;
    }

    private static int CountTrailingXmlWhitespace(string value)
    {
        var count = 0;
        while (count < value.Length && IsXmlWhitespace(value[^(count + 1)]))
        {
            count++;
        }
        return count;
    }

    private static bool IsXmlWhitespace(char character) =>
        character is ' ' or '\t' or '\r' or '\n';

    private static string GetFolder(string path)
    {
        var separator = LastSeparator(path);
        return separator >= 0 ? path[..(separator + 1)] : string.Empty;
    }

    private static string RemoveFolder(string path)
    {
        var separator = LastSeparator(path);
        return separator >= 0 ? path[(separator + 1)..] : path;
    }

    private static FerruleValue ResolveFilePath(IReadOnlyList<FerruleValue> arguments)
    {
        RequireArity("resolve_filepath", arguments, 2);
        var basePath = RequireString(arguments[0], "resolve_filepath");
        var path = RequireString(arguments[1], "resolve_filepath");
        if (IsAbsolute(path))
        {
            return FerruleValue.FromString(path);
        }

        var delimiter = CommonSeparator(basePath, path);
        var folder = basePath;
        if (folder.Length > 0 && !EndsWithSeparator(folder))
        {
            folder += delimiter;
        }

        var relative = StripCurrentDirectories(path);
        while (TryParentRemainder(relative, out var remainder))
        {
            folder = ParentFolder(folder, delimiter);
            relative = remainder;
        }

        return FerruleValue.FromString(folder + relative);
    }

    private static int LastSeparator(string path) => path.LastIndexOfAny(['/', '\\']);

    private static char? PathSeparator(string path)
    {
        var forward = path.Contains('/');
        var backward = path.Contains('\\');
        return (forward, backward) switch
        {
            (true, false) => '/',
            (false, true) => '\\',
            _ => null,
        };
    }

    private static char CommonSeparator(string basePath, string path)
    {
        var left = PathSeparator(basePath);
        var right = PathSeparator(path);
        if (left.HasValue && right.HasValue && left == right)
        {
            return left.Value;
        }
        if (left.HasValue && !right.HasValue)
        {
            return left.Value;
        }
        if (!left.HasValue && right.HasValue)
        {
            return right.Value;
        }
        return '\\';
    }

    private static bool IsAbsolute(string path) =>
        StartsWithSeparator(path) || path.Contains(':');

    private static string StripCurrentDirectories(string path)
    {
        while (true)
        {
            if (path == ".")
            {
                return string.Empty;
            }
            if (path.StartsWith("./", StringComparison.Ordinal) ||
                path.StartsWith(".\\", StringComparison.Ordinal))
            {
                path = path[2..];
                continue;
            }
            return path;
        }
    }

    private static bool TryParentRemainder(string path, out string remainder)
    {
        if (path == "..")
        {
            remainder = string.Empty;
            return true;
        }
        if (path.StartsWith("../", StringComparison.Ordinal) ||
            path.StartsWith("..\\", StringComparison.Ordinal))
        {
            remainder = path[3..];
            return true;
        }

        remainder = string.Empty;
        return false;
    }

    private static string ParentFolder(string folder, char delimiter)
    {
        var trimmed = folder.TrimEnd('/', '\\');
        if (trimmed.Length == 0 && StartsWithSeparator(folder))
        {
            return delimiter.ToString();
        }
        if (IsDrivePrefix(trimmed))
        {
            return trimmed + delimiter;
        }

        var separator = LastSeparator(trimmed);
        if (separator >= 0)
        {
            return trimmed[..(separator + 1)];
        }
        return trimmed.Length == 0 ? $"..{delimiter}" : string.Empty;
    }

    private static bool StartsWithSeparator(string path) =>
        path.Length > 0 && path[0] is '/' or '\\';

    private static bool EndsWithSeparator(string path) =>
        path.Length > 0 && path[^1] is '/' or '\\';

    private static bool IsDrivePrefix(string path) =>
        path.Length == 2 &&
        ((path[0] is >= 'a' and <= 'z') || (path[0] is >= 'A' and <= 'Z')) &&
        path[1] == ':';

    private static FerruleValue UnaryBoolean(
        string function,
        IReadOnlyList<FerruleValue> arguments,
        Func<bool, bool> operation)
    {
        RequireArity(function, arguments, 1);
        return FerruleValue.FromBoolean(operation(RequireBooleanArgument(arguments[0], function)));
    }

    private static FerruleValue BinaryBoolean(
        string function,
        IReadOnlyList<FerruleValue> arguments,
        Func<bool, bool, bool> operation)
    {
        RequireArity(function, arguments, 2);
        var left = RequireBooleanArgument(arguments[0], function);
        var right = RequireBooleanArgument(arguments[1], function);
        return FerruleValue.FromBoolean(operation(left, right));
    }

    private static FerruleValue BinaryString(
        string function,
        IReadOnlyList<FerruleValue> arguments,
        Func<string, string, bool> operation)
    {
        RequireArity(function, arguments, 2);
        var left = RequireString(arguments[0], function);
        var right = RequireString(arguments[1], function);
        return FerruleValue.FromBoolean(operation(left, right));
    }

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
        if (left.Kind is FerruleValueKind.Null or FerruleValueKind.XmlNil ||
            right.Kind is FerruleValueKind.Null or FerruleValueKind.XmlNil)
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

    internal static string ScalarText(FerruleValue value) => value.Kind switch
    {
        FerruleValueKind.Null or FerruleValueKind.XmlNil => string.Empty,
        FerruleValueKind.Bool => value.BooleanValue ? "true" : "false",
        FerruleValueKind.Int64 => value.Int64Value.ToString(CultureInfo.InvariantCulture),
        FerruleValueKind.Double => DoubleText(value.DoubleValue),
        FerruleValueKind.String => value.StringValue,
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

    private static bool RequireBooleanArgument(FerruleValue value, string function)
    {
        if (value.Kind != FerruleValueKind.Bool)
        {
            throw Type(function, value);
        }
        return value.BooleanValue;
    }

    private static string RequireString(FerruleValue value, string function)
    {
        if (value.Kind != FerruleValueKind.String)
        {
            throw Type(function, value);
        }
        return value.StringValue;
    }

    private static void RequireArity(
        string function,
        IReadOnlyList<FerruleValue> arguments,
        int expected)
    {
        if (arguments.Count != expected)
        {
            throw Arity(function, expected, arguments.Count);
        }
    }

    private static FerruleRuntimeException Arity(string function, int expected, int actual) =>
        new(
            FerruleRuntimeError.FunctionArity,
            $"`{function}` expected {expected} argument(s), got {actual}.",
            function: function,
            expectedArity: expected,
            actualArity: actual);

    private static FerruleRuntimeException Type(string function, FerruleValue value) =>
        new(
            FerruleRuntimeError.FunctionType,
            $"`{function}` cannot accept a {TypeName(value)} argument.",
            function: function,
            foundKind: value.Kind);

    private static string TypeName(FerruleValue value) => value.Kind switch
    {
        FerruleValueKind.Null => "null",
        FerruleValueKind.XmlNil => "xml nil",
        FerruleValueKind.Bool => "bool",
        FerruleValueKind.Int64 => "int",
        FerruleValueKind.Double => "float",
        FerruleValueKind.String => "string",
        _ => "unknown",
    };

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
