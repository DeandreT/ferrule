/*
 * The binary64 number parser below adapts the default parser behavior from
 * serde_json 1.0.150 by Erick Tryzelaar and David Tolnay (MIT OR Apache-2.0).
 * The adapted portion is distributed under serde_json's MIT terms:
 *
 * Permission is hereby granted, free of charge, to any
 * person obtaining a copy of this software and associated
 * documentation files (the "Software"), to deal in the
 * Software without restriction, including without
 * limitation the rights to use, copy, modify, merge,
 * publish, distribute, sublicense, and/or sell copies of
 * the Software, and to permit persons to whom the
 * Software is furnished to do so, subject to the following
 * conditions:
 *
 * The above copyright notice and this permission notice
 * shall be included in all copies or substantial portions
 * of the Software.
 *
 * THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF
 * ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED
 * TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A
 * PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT
 * SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY
 * CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION
 * OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR
 * IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
 * DEALINGS IN THE SOFTWARE.
 */

using System.Globalization;
using System.Text;
using System.Text.Json;

namespace Ferrule.Runtime;

public static partial class FerruleJson
{
    // serde_json 1.0.150's default number reader retains integers through u64,
    // then accumulates a bounded u64 significand and scales it with binary64
    // powers of ten. Its formatter uses zmij's shortest decimal representation.
    // Share JSON numeric tags across boundary leaves, embedded metadata, and
    // arbitrary-JSON graph text. Lexical string coercion has its own contract.
    private static readonly double[] SerdePowersOfTen = BuildSerdePowersOfTen();

    private sealed class CanonicalNumberOutOfRangeException : Exception;

    private enum SerdeNumberKind
    {
        Unsigned,
        Signed,
        Float,
    }

    private readonly record struct SerdeParsedNumber(
        SerdeNumberKind Kind,
        ulong UnsignedValue,
        long SignedValue,
        double FloatValue)
    {
        public static SerdeParsedNumber Unsigned(ulong value) =>
            new(SerdeNumberKind.Unsigned, value, 0, 0);

        public static SerdeParsedNumber Signed(long value) =>
            new(SerdeNumberKind.Signed, 0, value, 0);

        public static SerdeParsedNumber Float(double value) =>
            new(SerdeNumberKind.Float, 0, 0, value);

        public double AsDouble() => Kind switch
        {
            SerdeNumberKind.Unsigned => (double)UnsignedValue,
            SerdeNumberKind.Signed => (double)SignedValue,
            _ => FloatValue,
        };
    }

    private static bool TryParseSerdeNumber(string token, out SerdeParsedNumber parsed)
    {
        try
        {
            parsed = ParseSerdeNumber(token);
            return true;
        }
        catch (CanonicalNumberOutOfRangeException)
        {
            parsed = default;
            return false;
        }
    }

    private static bool TryGetSerdeInt64(JsonElement element, out long value)
    {
        if (element.ValueKind == JsonValueKind.Number &&
            TryParseSerdeNumber(element.GetRawText(), out var parsed))
        {
            if (parsed.Kind == SerdeNumberKind.Signed)
            {
                value = parsed.SignedValue;
                return true;
            }
            if (parsed.Kind == SerdeNumberKind.Unsigned &&
                parsed.UnsignedValue <= long.MaxValue)
            {
                value = (long)parsed.UnsignedValue;
                return true;
            }
        }
        value = 0;
        return false;
    }

    private static bool TryGetSerdeUInt64(JsonElement element, out ulong value)
    {
        if (element.ValueKind == JsonValueKind.Number &&
            TryParseSerdeNumber(element.GetRawText(), out var parsed))
        {
            if (parsed.Kind == SerdeNumberKind.Unsigned)
            {
                value = parsed.UnsignedValue;
                return true;
            }
            if (parsed.Kind == SerdeNumberKind.Signed &&
                parsed.SignedValue >= 0)
            {
                value = (ulong)parsed.SignedValue;
                return true;
            }
        }
        value = 0;
        return false;
    }

    private static string CanonicalizeAnyJson(JsonElement element)
    {
        try
        {
            return CanonicalizeAnyJsonUnchecked(element);
        }
        catch (CanonicalNumberOutOfRangeException error)
        {
            throw Boundary("JSON number is out of range.", error);
        }
    }

    // A host-provided json_any string is quoted by the Rust writer when it is
    // not parseable as serde_json::Value, including a non-finite JSON number.
    // Build the whole replacement first so a nested error cannot leave a
    // partially written object in the outer JSON writer.
    private static bool TryCanonicalizeAnyJson(JsonElement element, out string canonical)
    {
        try
        {
            canonical = CanonicalizeAnyJsonUnchecked(element);
            return true;
        }
        catch (CanonicalNumberOutOfRangeException)
        {
            canonical = string.Empty;
            return false;
        }
    }

    private static string CanonicalizeAnyJsonUnchecked(JsonElement element)
    {
        var text = new StringBuilder();
        AppendCanonicalAnyJson(text, element);
        return text.ToString();
    }

    private static void AppendCanonicalAnyJson(StringBuilder text, JsonElement element)
    {
        switch (element.ValueKind)
        {
            case JsonValueKind.Null:
                text.Append("null");
                return;
            case JsonValueKind.True:
                text.Append("true");
                return;
            case JsonValueKind.False:
                text.Append("false");
                return;
            case JsonValueKind.String:
                AppendSerdeString(text, element.GetString() ?? string.Empty);
                return;
            case JsonValueKind.Number:
                text.Append(CanonicalSerdeNumber(element.GetRawText()));
                return;
            case JsonValueKind.Array:
                text.Append('[');
                var firstItem = true;
                foreach (var item in element.EnumerateArray())
                {
                    if (!firstItem)
                    {
                        text.Append(',');
                    }
                    firstItem = false;
                    AppendCanonicalAnyJson(text, item);
                }
                text.Append(']');
                return;
            case JsonValueKind.Object:
                text.Append('{');
                // serde_json's preserve_order map keeps the first key position
                // when a later duplicate replaces its value.
                var firstProperty = true;
                foreach (var property in OrderedProperties(element))
                {
                    if (!firstProperty)
                    {
                        text.Append(',');
                    }
                    firstProperty = false;
                    AppendSerdeString(text, property.Name);
                    text.Append(':');
                    AppendCanonicalAnyJson(text, property.Value);
                }
                text.Append('}');
                return;
            default:
                throw Boundary("JSON arbitrary value is invalid.");
        }
    }

    internal static void AppendSerdeString(StringBuilder text, string value)
    {
        // serde_json leaves every valid non-ASCII scalar literal, including
        // supplementary characters and U+2028/U+2029; only JSON's required
        // ASCII escapes and C0 controls are transformed.
        const string hex = "0123456789abcdef";
        text.Append('"');
        foreach (var character in value)
        {
            switch (character)
            {
                case '"':
                    text.Append("\\\"");
                    break;
                case '\\':
                    text.Append("\\\\");
                    break;
                case '\b':
                    text.Append("\\b");
                    break;
                case '\t':
                    text.Append("\\t");
                    break;
                case '\n':
                    text.Append("\\n");
                    break;
                case '\f':
                    text.Append("\\f");
                    break;
                case '\r':
                    text.Append("\\r");
                    break;
                case < ' ':
                    text.Append("\\u00");
                    text.Append(hex[character >> 4]);
                    text.Append(hex[character & 15]);
                    break;
                default:
                    text.Append(character);
                    break;
            }
        }
        text.Append('"');
    }

    private static string CanonicalSerdeNumber(string token)
    {
        var parsed = ParseSerdeNumber(token);
        return parsed.Kind switch
        {
            SerdeNumberKind.Unsigned =>
                parsed.UnsignedValue.ToString(CultureInfo.InvariantCulture),
            SerdeNumberKind.Signed =>
                parsed.SignedValue.ToString(CultureInfo.InvariantCulture),
            _ => FormatSerdeFloat(parsed.FloatValue),
        };
    }

    private static SerdeParsedNumber ParseSerdeNumber(string token)
    {
        var offset = token[0] == '-' ? 1 : 0;
        var negative = offset != 0;
        ulong significand = 0;
        var decimalExponent = 0;
        var integerOverflow = false;

        while (offset < token.Length && token[offset] is >= '0' and <= '9')
        {
            var digit = (uint)(token[offset++] - '0');
            if (!integerOverflow &&
                significand <= (ulong.MaxValue - digit) / 10)
            {
                significand = significand * 10 + digit;
            }
            else
            {
                integerOverflow = true;
                decimalExponent++;
            }
        }

        var floating = integerOverflow;
        if (offset < token.Length && token[offset] == '.')
        {
            floating = true;
            offset++;
            var fractionOverflow = false;
            while (offset < token.Length && token[offset] is >= '0' and <= '9')
            {
                var digit = (uint)(token[offset++] - '0');
                if (!fractionOverflow &&
                    significand <= (ulong.MaxValue - digit) / 10)
                {
                    significand = significand * 10 + digit;
                    decimalExponent--;
                }
                else
                {
                    fractionOverflow = true;
                }
            }
        }

        if (offset < token.Length && token[offset] is 'e' or 'E')
        {
            floating = true;
            offset++;
            var positiveExponent = true;
            if (offset < token.Length && token[offset] is '+' or '-')
            {
                positiveExponent = token[offset++] == '+';
            }

            var explicitExponent = 0;
            var exponentOverflow = false;
            while (offset < token.Length && token[offset] is >= '0' and <= '9')
            {
                var digit = token[offset++] - '0';
                if (!exponentOverflow &&
                    explicitExponent <= (int.MaxValue - digit) / 10)
                {
                    explicitExponent = explicitExponent * 10 + digit;
                }
                else
                {
                    exponentOverflow = true;
                }
            }

            if (exponentOverflow)
            {
                if (positiveExponent && significand != 0)
                {
                    throw new CanonicalNumberOutOfRangeException();
                }
                return SerdeParsedNumber.Float(negative ? -0.0 : 0.0);
            }

            decimalExponent = positiveExponent
                ? (int)Math.Min((long)decimalExponent + explicitExponent, int.MaxValue)
                : (int)Math.Max((long)decimalExponent - explicitExponent, int.MinValue);
        }

        if (!floating)
        {
            if (!negative)
            {
                return SerdeParsedNumber.Unsigned(significand);
            }
            if (significand == 0)
            {
                return SerdeParsedNumber.Float(-0.0);
            }
            if (significand <= 9_223_372_036_854_775_808UL)
            {
                return SerdeParsedNumber.Signed(
                    significand == 9_223_372_036_854_775_808UL
                        ? long.MinValue
                        : -(long)significand);
            }
        }

        var number = (double)significand;
        while (decimalExponent is < -308 or > 308)
        {
            if (number == 0)
            {
                return SerdeParsedNumber.Float(negative ? -0.0 : 0.0);
            }
            if (decimalExponent > 308)
            {
                throw new CanonicalNumberOutOfRangeException();
            }
            number /= SerdePowersOfTen[308];
            decimalExponent += 308;
        }

        if (decimalExponent >= 0 && decimalExponent <= 308)
        {
            number *= SerdePowersOfTen[decimalExponent];
            if (double.IsInfinity(number))
            {
                throw new CanonicalNumberOutOfRangeException();
            }
        }
        else if (decimalExponent < 0)
        {
            number /= SerdePowersOfTen[-decimalExponent];
        }

        if (negative)
        {
            number = -number;
        }
        return SerdeParsedNumber.Float(number);
    }

    private static double[] BuildSerdePowersOfTen()
    {
        var powers = new double[309];
        for (var exponent = 0; exponent < powers.Length; exponent++)
        {
            powers[exponent] = double.Parse(
                "1e" + exponent.ToString(CultureInfo.InvariantCulture),
                NumberStyles.Float,
                CultureInfo.InvariantCulture);
        }
        return powers;
    }

    internal static string FormatSerdeFloat(double number)
    {
        if (!double.IsFinite(number))
        {
            throw new CanonicalNumberOutOfRangeException();
        }
        if (number == 0)
        {
            return BitConverter.DoubleToInt64Bits(number) < 0 ? "-0.0" : "0.0";
        }

        var negative = number < 0;
        var lexical = Math.Abs(number).ToString("R", CultureInfo.InvariantCulture);
        var exponentMarker = lexical.IndexOfAny(['E', 'e']);
        var decimalPower = exponentMarker < 0
            ? 0
            : int.Parse(lexical.AsSpan(exponentMarker + 1), CultureInfo.InvariantCulture);
        var mantissaEnd = exponentMarker < 0 ? lexical.Length : exponentMarker;
        var digits = new StringBuilder(mantissaEnd);
        var beforePoint = 0;
        var seenPoint = false;
        for (var index = 0; index < mantissaEnd; index++)
        {
            if (lexical[index] == '.')
            {
                seenPoint = true;
            }
            else
            {
                digits.Append(lexical[index]);
                if (!seenPoint)
                {
                    beforePoint++;
                }
            }
        }

        decimalPower += beforePoint - digits.Length;
        while (digits.Length > 1 && digits[0] == '0')
        {
            digits.Remove(0, 1);
        }
        while (digits.Length > 1 && digits[^1] == '0')
        {
            digits.Length--;
            decimalPower++;
        }

        var adjustedExponent = decimalPower + digits.Length - 1;
        string result;
        if (adjustedExponent is >= -5 and <= 15)
        {
            var point = digits.Length + decimalPower;
            result = point <= 0
                ? "0." + new string('0', -point) + digits
                : point >= digits.Length
                    ? digits + new string('0', point - digits.Length) + ".0"
                    : digits.ToString(0, point) + "." + digits.ToString(point, digits.Length - point);
        }
        else
        {
            result = digits.Length == 1
                ? digits.ToString()
                : digits[0] + "." + digits.ToString(1, digits.Length - 1);
            result += adjustedExponent >= 0 ? "e+" : "e-";
            result += Math.Abs(adjustedExponent).ToString(CultureInfo.InvariantCulture);
        }
        return negative ? "-" + result : result;
    }
}
