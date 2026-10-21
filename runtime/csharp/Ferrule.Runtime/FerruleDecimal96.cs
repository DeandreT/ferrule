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
        _ => throw new ArgumentOutOfRangeException(nameof(error)),
    }) => Error = error;

    public FerruleDecimalError Error { get; }
}

/// <summary>A standalone bounded decimal, not an admitted FerruleValue kind.</summary>
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

    private static void ValidateLexical(string text)
    {
        if (text is null) throw new FerruleDecimalException(FerruleDecimalError.InvalidLexical);
        int bytes;
        try { bytes = StrictUtf8.GetByteCount(text); }
        catch (EncoderFallbackException) { throw new FerruleDecimalException(FerruleDecimalError.InvalidLexical); }
        if (bytes > MaximumLexicalBytes) throw new FerruleDecimalException(FerruleDecimalError.InputTooLong);
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
