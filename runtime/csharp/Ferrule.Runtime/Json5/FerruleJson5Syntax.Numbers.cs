using System.Globalization;

namespace Ferrule.Runtime;

public static partial class FerruleJson5Syntax
{
    private sealed partial class Parser
    {
        private void Number()
        {
            var start = _index;
            var offset = _offset;
            var end = TokenEnd();
            var token = _source.AsSpan(start, end - start);
            var negative = token[0] == '-';
            var bare = negative || token[0] == '+' ? token[1..] : token;
            var tokenBytes = _offset - offset;
            // One grammar/finite-word pass, separately from repeated passes.
            Charge(tokenBytes + 16);
            if (bare.SequenceEqual("Infinity".AsSpan()) || bare.SequenceEqual("NaN".AsSpan()))
            {
                throw Syntax(FerruleJson5SyntaxKind.NonFiniteNumber, offset);
            }
            if (bare.Length >= 2 && bare[0] == '0' && bare[1] is 'x' or 'X')
            {
                if (bare.Length == 2) { throw Syntax(FerruleJson5SyntaxKind.InvalidNumber, offset); }
                ulong magnitude = 0;
                var overflow = false;
                foreach (var value in bare[2..])
                {
                    var digit = value switch
                    {
                        >= '0' and <= '9' => value - '0',
                        >= 'a' and <= 'f' => value - 'a' + 10,
                        >= 'A' and <= 'F' => value - 'A' + 10,
                        _ => -1,
                    };
                    if (digit < 0) { throw Syntax(FerruleJson5SyntaxKind.InvalidNumber, offset); }
                    Fold(ref magnitude, ref overflow, (ulong)digit, 16);
                }
                Integer(magnitude, overflow, negative, offset);
                return;
            }

            var index = 0;
            var nonzeroMantissa = false;
            while (index < bare.Length && bare[index] is >= '0' and <= '9')
            {
                nonzeroMantissa |= bare[index] != '0';
                index++;
            }
            var integerDigits = index;
            if (integerDigits > 1 && bare[0] == '0')
            {
                throw Syntax(FerruleJson5SyntaxKind.InvalidNumber, offset);
            }
            var hasPoint = index < bare.Length && bare[index] == '.';
            var fractionDigits = 0;
            if (hasPoint)
            {
                index++;
                var fractionStart = index;
                while (index < bare.Length && bare[index] is >= '0' and <= '9')
                {
                    nonzeroMantissa |= bare[index] != '0';
                    index++;
                }
                fractionDigits = index - fractionStart;
            }
            if (integerDigits == 0 && fractionDigits == 0)
            {
                throw Syntax(FerruleJson5SyntaxKind.InvalidNumber, offset);
            }
            var exponent = index;
            var hasExponent = index < bare.Length && bare[index] is 'e' or 'E';
            if (hasExponent)
            {
                index++;
                if (index < bare.Length && bare[index] is '+' or '-') { index++; }
                var exponentStart = index;
                while (index < bare.Length && bare[index] is >= '0' and <= '9') { index++; }
                if (exponentStart == index) { throw Syntax(FerruleJson5SyntaxKind.InvalidNumber, offset); }
            }
            if (index != bare.Length) { throw Syntax(FerruleJson5SyntaxKind.InvalidNumber, offset); }
            if (!hasPoint && !hasExponent)
            {
                // Checked exact integer folding is a separate complete pass.
                Charge(bare.Length);
                ulong magnitude = 0;
                var overflow = false;
                foreach (var value in bare) { Fold(ref magnitude, ref overflow, (ulong)(value - '0'), 10); }
                Integer(magnitude, overflow, negative, offset);
                return;
            }

            // A complete lexical finite check is charged independently. A zero
            // mantissa stays finite even when its exponent exceeds BCL bounds;
            // the original exponent spelling is still preserved in the result.
            Charge(tokenBytes);
            if (nonzeroMantissa)
            {
                if (!double.TryParse(token, NumberStyles.Float, CultureInfo.InvariantCulture, out var parsed))
                {
                    throw Syntax(FerruleJson5SyntaxKind.InvalidNumber, offset);
                }
                if (!double.IsFinite(parsed)) { throw Syntax(FerruleJson5SyntaxKind.NonFiniteNumber, offset); }
            }
            if (negative) { AppendAscii("-"); }
            if (integerDigits == 0) { AppendAscii("0"); }
            AppendAscii(bare[..exponent]);
            if (hasPoint && fractionDigits == 0) { AppendAscii("0"); }
            AppendAscii(bare[exponent..]);
        }

        private static void Fold(ref ulong magnitude, ref bool overflow, ulong digit, ulong radix)
        {
            // Keep inspecting later digits after overflow to retain syntax priority.
            if (overflow) { return; }
            if (magnitude > (ulong.MaxValue - digit) / radix) { overflow = true; }
            else { magnitude = magnitude * radix + digit; }
        }

        private void Integer(ulong magnitude, bool overflow, bool negative, long offset)
        {
            if (overflow || (negative && magnitude > (1UL << 63)))
            {
                throw Syntax(FerruleJson5SyntaxKind.IntegerOutOfRange, offset);
            }
            Charge(20);
            var digits = magnitude.ToString(CultureInfo.InvariantCulture);
            if (negative && magnitude != 0) { AppendAscii("-"); }
            AppendAscii(digits);
        }
    }
}
