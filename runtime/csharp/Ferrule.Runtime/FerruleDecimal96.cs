using System.Globalization;
using System.Text;

namespace Ferrule.Runtime;

public enum FerruleDecimalError
{
    InvalidLexical,
    InputTooLong,
    InvalidScale,
    Overflow,
    PrecisionLoss,
    InvalidNumberLexical,
}

public sealed class FerruleDecimalException : Exception
{
    public FerruleDecimalException(FerruleDecimalError error) : base(error switch
    {
        FerruleDecimalError.InvalidLexical => "expected a plain ASCII decimal without whitespace or exponent",
        FerruleDecimalError.InputTooLong => "decimal lexical exceeds the byte limit",
        FerruleDecimalError.InvalidScale => "decimal scale must not exceed 28",
        FerruleDecimalError.Overflow => "decimal coefficient exceeds the 96-bit range",
        FerruleDecimalError.PrecisionLoss => "decimal cannot be represented without rounding",
        FerruleDecimalError.InvalidNumberLexical => "expected an ASCII decimal number with an optional exponent",
        _ => throw new ArgumentOutOfRangeException(nameof(error)),
    }) => Error = error;

    public FerruleDecimalError Error { get; }
}

/// <summary>The bounded decimal used by the explicit decimal scalar domain.</summary>
public readonly struct FerruleDecimal96 : IEquatable<FerruleDecimal96>, IComparable<FerruleDecimal96>
{
    public const int MaximumLexicalBytes = 4096;
    public const byte MaximumScale = 28;
    public static readonly UInt128 MaximumCoefficient = ((UInt128)1 << 96) - 1;
    public static FerruleDecimal96 Zero => default;
    private static readonly UTF8Encoding StrictUtf8 = new(false, true);
    private readonly decimal value;

    private FerruleDecimal96(decimal value)
    {
        var bits = decimal.GetBits(value);
        UInt128 coefficient = (UInt128)(uint)bits[0] | ((UInt128)(uint)bits[1] << 32) | ((UInt128)(uint)bits[2] << 64);
        byte scale = (byte)((bits[3] >> 16) & 255);
        while (scale > 0 && coefficient % 10 == 0)
        {
            coefficient /= 10;
            scale--;
        }
        this.value = new decimal(unchecked((int)(uint)coefficient), unchecked((int)(uint)(coefficient >> 32)),
            unchecked((int)(uint)(coefficient >> 64)), coefficient != 0 && bits[3] < 0, coefficient == 0 ? (byte)0 : scale);
    }

    public UInt128 Coefficient
    {
        get
        {
            var bits = decimal.GetBits(value);
            return (UInt128)(uint)bits[0] | ((UInt128)(uint)bits[1] << 32) | ((UInt128)(uint)bits[2] << 64);
        }
    }
    public byte Scale => (byte)((decimal.GetBits(value)[3] >> 16) & 255);
    public bool IsNegative => decimal.GetBits(value)[3] < 0;

    public static FerruleDecimal96 FromParts(bool negative, UInt128 coefficient, byte scale)
    {
        if (scale > MaximumScale) throw new FerruleDecimalException(FerruleDecimalError.InvalidScale);
        if (coefficient > MaximumCoefficient) throw new FerruleDecimalException(FerruleDecimalError.Overflow);
        return new(new decimal(unchecked((int)(uint)coefficient), unchecked((int)(uint)(coefficient >> 32)),
            unchecked((int)(uint)(coefficient >> 64)), negative, scale));
    }

    public static FerruleDecimal96 ParseRounded(string text)
    {
        ValidateLexical(text);
        return ParseRoundedCore(text);
    }

    private static FerruleDecimal96 ParseRoundedCore(string text)
    {
        try
        {
            return new(decimal.Parse(text, NumberStyles.AllowLeadingSign | NumberStyles.AllowDecimalPoint,
                CultureInfo.InvariantCulture));
        }
        catch (OverflowException) { throw new FerruleDecimalException(FerruleDecimalError.Overflow); }
        catch (FormatException) { throw new FerruleDecimalException(FerruleDecimalError.InvalidLexical); }
    }

    /// <summary>Explicit binary64 conversion using .NET's fifteen-digit decimal policy.</summary>
    public static FerruleDecimal96 FromDouble(double value)
    {
        try { return new((decimal)value); }
        catch (OverflowException) { throw new FerruleDecimalException(FerruleDecimalError.Overflow); }
    }

    public static FerruleDecimal96 ParseExact(string text)
    {
        var result = ParseRounded(text);
        bool negative = text[0] == '-';
        string digits = text[0] is '+' or '-' ? text[1..] : text;
        int dot = digits.IndexOf('.');
        string whole = (dot < 0 ? digits : digits[..dot]).TrimStart('0');
        string fraction = (dot < 0 ? "" : digits[(dot + 1)..]).TrimEnd('0');
        if (whole.Length == 0) whole = "0";
        string canonical = (negative && (whole != "0" || fraction.Length != 0) ? "-" : "") + whole
            + (fraction.Length == 0 ? "" : "." + fraction);
        if (result.ToString() != canonical) throw new FerruleDecimalException(FerruleDecimalError.PrecisionLoss);
        return result;
    }

    /// <summary>Mapping decimal text; shifts exponent digits exactly before rounding.</summary>
    public static FerruleDecimal96 ParseDecimalText(string text)
    {
        ValidateTextSize(text, FerruleDecimalError.InvalidNumberLexical);
        text = text.Trim();
        if (!HasExponent(text)) return ParseRounded(text);
        int marker = text.IndexOfAny(['e', 'E']);
        string mantissa = text[..marker];
        string exponentText = text[(marker + 1)..];
        int exponent = 0;
        foreach (char digit in exponentText.TrimStart('+', '-'))
            exponent = Math.Min(8192, exponent * 10 + digit - '0');
        if (exponentText.StartsWith('-')) exponent = -exponent;
        bool negative = mantissa.StartsWith('-');
        mantissa = mantissa.TrimStart('+', '-');
        int dot = mantissa.IndexOf('.');
        string whole = dot < 0 ? mantissa : mantissa[..dot];
        string digits = dot < 0 ? mantissa : whole + mantissa[(dot + 1)..];
        string significant = digits.TrimStart('0');
        if (significant.Length == 0) return Zero;
        int point = whole.Length + exponent - (digits.Length - significant.Length);
        if (point > 29) throw new FerruleDecimalException(FerruleDecimalError.Overflow);
        if (point < -28) return Zero;
        string plain = point <= 0 ? "0." + new string('0', -point) + significant
            : point >= significant.Length ? significant + new string('0', point - significant.Length)
            : significant[..point] + "." + significant[point..];
        return ParseRoundedCore((negative ? "-" : "") + plain);
    }

    /// <summary>Trims numeric text; exponent forms explicitly pass through binary64.</summary>
    public static FerruleDecimal96 ParseNumber(string text)
    {
        ValidateTextSize(text, FerruleDecimalError.InvalidNumberLexical);
        text = text.Trim();
        if (text is "NaN" or "INF" or "-INF") throw new FerruleDecimalException(FerruleDecimalError.Overflow);
        if (!HasExponent(text)) return ParseRounded(text);
        try
        {
            return FromDouble(double.Parse(text,
                NumberStyles.AllowLeadingSign | NumberStyles.AllowDecimalPoint | NumberStyles.AllowExponent,
                CultureInfo.InvariantCulture));
        }
        catch (FormatException) { throw new FerruleDecimalException(FerruleDecimalError.InvalidNumberLexical); }
        catch (OverflowException) { throw new FerruleDecimalException(FerruleDecimalError.Overflow); }
    }

    private static bool HasExponent(string text)
    {
        int index = text.Length > 0 && text[0] is '+' or '-' ? 1 : 0;
        int start = index;
        while (index < text.Length && text[index] is >= '0' and <= '9') index++;
        int digits = index - start;
        if (index < text.Length && text[index] == '.')
        {
            index++;
            start = index;
            while (index < text.Length && text[index] is >= '0' and <= '9') index++;
            digits += index - start;
        }
        if (digits == 0) throw new FerruleDecimalException(FerruleDecimalError.InvalidNumberLexical);
        bool exponent = index < text.Length && text[index] is 'e' or 'E';
        if (exponent)
        {
            index++;
            if (index < text.Length && text[index] is '+' or '-') index++;
            start = index;
            while (index < text.Length && text[index] is >= '0' and <= '9') index++;
            if (index == start) throw new FerruleDecimalException(FerruleDecimalError.InvalidNumberLexical);
        }
        if (index != text.Length) throw new FerruleDecimalException(FerruleDecimalError.InvalidNumberLexical);
        return exponent;
    }

    private static void ValidateTextSize(string text, FerruleDecimalError invalid = FerruleDecimalError.InvalidLexical)
    {
        if (text is null) throw new FerruleDecimalException(invalid);
        int bytes;
        try { bytes = StrictUtf8.GetByteCount(text); }
        catch (EncoderFallbackException) { throw new FerruleDecimalException(invalid); }
        if (bytes > MaximumLexicalBytes) throw new FerruleDecimalException(FerruleDecimalError.InputTooLong);
    }

    private static void ValidateLexical(string text)
    {
        ValidateTextSize(text);
        int start = text.Length > 0 && text[0] is '+' or '-' ? 1 : 0;
        bool dot = false, digit = false;
        for (int index = start; index < text.Length; index++)
        {
            char next = text[index];
            if (next is >= '0' and <= '9') digit = true;
            else if (next == '.' && !dot) dot = true;
            else throw new FerruleDecimalException(FerruleDecimalError.InvalidLexical);
        }
        if (!digit) throw new FerruleDecimalException(FerruleDecimalError.InvalidLexical);
    }

    public FerruleDecimal96 Negated() => new(decimal.Negate(value));
    public FerruleDecimal96 AddRounded(FerruleDecimal96 other)
    {
        try { return new(checked(value + other.value)); }
        catch (OverflowException) { throw new FerruleDecimalException(FerruleDecimalError.Overflow); }
    }
    public FerruleDecimal96 SubtractRounded(FerruleDecimal96 other)
    {
        try { return new(checked(value - other.value)); }
        catch (OverflowException) { throw new FerruleDecimalException(FerruleDecimalError.Overflow); }
    }

    public int CompareTo(FerruleDecimal96 other) => decimal.Compare(value, other.value);
    public bool Equals(FerruleDecimal96 other) => value == other.value;
    public override bool Equals(object? obj) => obj is FerruleDecimal96 other && Equals(other);
    public override int GetHashCode() => value.GetHashCode();
    public override string ToString() => value.ToString(CultureInfo.InvariantCulture);
    public static bool operator ==(FerruleDecimal96 left, FerruleDecimal96 right) => left.Equals(right);
    public static bool operator !=(FerruleDecimal96 left, FerruleDecimal96 right) => !left.Equals(right);
    public static bool operator <(FerruleDecimal96 left, FerruleDecimal96 right) => left.CompareTo(right) < 0;
    public static bool operator >(FerruleDecimal96 left, FerruleDecimal96 right) => left.CompareTo(right) > 0;
    public static bool operator <=(FerruleDecimal96 left, FerruleDecimal96 right) => left.CompareTo(right) <= 0;
    public static bool operator >=(FerruleDecimal96 left, FerruleDecimal96 right) => left.CompareTo(right) >= 0;
}
