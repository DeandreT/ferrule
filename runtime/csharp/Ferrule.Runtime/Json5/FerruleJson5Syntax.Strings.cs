namespace Ferrule.Runtime;

public static partial class FerruleJson5Syntax
{
    private static bool IdentifierChar(int scalar, bool first) =>
        scalar is >= 'a' and <= 'z' or >= 'A' and <= 'Z' or '_' or '$' ||
        (!first && scalar is >= '0' and <= '9');

    private sealed partial class Parser
    {
        private void String()
        {
            var start = _offset;
            var quote = Take();
            AppendAscii("\"");
            while (true)
            {
                var offset = _offset;
                var scalar = Peek();
                if (scalar < 0) { throw Syntax(FerruleJson5SyntaxKind.UnterminatedString, start); }
                Take();
                if (scalar == quote) { AppendAscii("\""); return; }
                if (scalar == '\\')
                {
                    var decoded = Escape(offset);
                    if (decoded is { } value) { EscapedScalar(value); }
                }
                else if (scalar is '\n' or '\r')
                {
                    throw Syntax(FerruleJson5SyntaxKind.LineTerminatorInString, offset);
                }
                else { EscapedScalar(scalar); }
            }
        }

        private void Identifier()
        {
            AppendAscii("\"");
            var first = true;
            while (true)
            {
                var offset = _offset;
                var scalar = Peek();
                if (scalar < 0) { break; }
                if (scalar == '\\')
                {
                    Take();
                    if (Peek() != 'u') { throw Syntax(FerruleJson5SyntaxKind.InvalidEscape, offset); }
                    Take();
                    var decoded = Hex(4, offset);
                    if (decoded is >= 0xd800 and <= 0xdfff)
                    {
                        throw Syntax(FerruleJson5SyntaxKind.InvalidEscape, offset);
                    }
                    if (!IdentifierChar(decoded, first))
                    {
                        throw Syntax(FerruleJson5SyntaxKind.UnsupportedIdentifier, offset);
                    }
                    EscapedScalar(decoded);
                }
                else if (IdentifierChar(scalar, first)) { Take(); EscapedScalar(scalar); }
                else
                {
                    if (scalar > 0x7f && !Whitespace(scalar))
                    {
                        throw Syntax(FerruleJson5SyntaxKind.UnsupportedIdentifier, offset);
                    }
                    break;
                }
                first = false;
            }
            if (first) { throw Syntax(FerruleJson5SyntaxKind.UnsupportedIdentifier); }
            AppendAscii("\"");
        }

        private int? Escape(long offset)
        {
            var scalar = Peek();
            if (scalar < 0) { throw Syntax(FerruleJson5SyntaxKind.InvalidEscape, offset); }
            Take();
            switch (scalar)
            {
                case '\r':
                    if (Peek() == '\n') { Take(); }
                    return null;
                case '\n':
                case 0x2028:
                case 0x2029: return null;
                case 'b': return 0x08;
                case 't': return '\t';
                case 'n': return '\n';
                case 'v': return 0x0b;
                case 'f': return 0x0c;
                case 'r': return '\r';
                case '0':
                    if (Peek() is >= '0' and <= '9')
                    {
                        throw Syntax(FerruleJson5SyntaxKind.InvalidEscape, offset);
                    }
                    return 0;
                case >= '1' and <= '9': throw Syntax(FerruleJson5SyntaxKind.InvalidEscape, offset);
                case 'x': return Hex(2, offset);
                case 'u':
                    var first = Hex(4, offset);
                    if (first is >= 0xd800 and <= 0xdbff)
                    {
                        if (Peek() != '\\') { throw Syntax(FerruleJson5SyntaxKind.InvalidEscape, offset); }
                        Take();
                        if (Peek() != 'u') { throw Syntax(FerruleJson5SyntaxKind.InvalidEscape, offset); }
                        Take();
                        var second = Hex(4, offset);
                        if (second is < 0xdc00 or > 0xdfff)
                        {
                            throw Syntax(FerruleJson5SyntaxKind.InvalidEscape, offset);
                        }
                        return 0x10000 + ((first - 0xd800) << 10) + second - 0xdc00;
                    }
                    if (first is >= 0xdc00 and <= 0xdfff)
                    {
                        throw Syntax(FerruleJson5SyntaxKind.InvalidEscape, offset);
                    }
                    return first;
                default: return scalar;
            }
        }

        private int Hex(int count, long offset)
        {
            var value = 0;
            for (var index = 0; index < count; index++)
            {
                var scalar = Peek();
                var digit = scalar switch
                {
                    >= '0' and <= '9' => scalar - '0',
                    >= 'a' and <= 'f' => scalar - 'a' + 10,
                    >= 'A' and <= 'F' => scalar - 'A' + 10,
                    _ => -1,
                };
                if (digit < 0) { throw Syntax(FerruleJson5SyntaxKind.InvalidEscape, offset); }
                Take();
                value = value * 16 + digit;
            }
            return value;
        }

        private void EscapedScalar(int scalar)
        {
            switch (scalar)
            {
                case '"': AppendAscii("\\\""); return;
                case '\\': AppendAscii("\\\\"); return;
                case 0x08: AppendAscii("\\b"); return;
                case '\t': AppendAscii("\\t"); return;
                case '\n': AppendAscii("\\n"); return;
                case 0x0c: AppendAscii("\\f"); return;
                case '\r': AppendAscii("\\r"); return;
                case < 0x20:
                    // Match the shared ledger's six one-byte append operations.
                    const string hex = "0123456789abcdef";
                    AppendAsciiCharacter('\\');
                    AppendAsciiCharacter('u');
                    AppendAsciiCharacter('0');
                    AppendAsciiCharacter('0');
                    AppendAsciiCharacter(hex[scalar >> 4]);
                    AppendAsciiCharacter(hex[scalar & 15]);
                    return;
                default:
                    Reserve(Utf8Width(scalar));
                    if (scalar <= 0xffff) { _output.Append((char)scalar); }
                    else
                    {
                        var pair = scalar - 0x10000;
                        _output.Append((char)(0xd800 + (pair >> 10)));
                        _output.Append((char)(0xdc00 + (pair & 0x3ff)));
                    }
                    return;
            }
        }

        private void AppendAsciiCharacter(char value) { Reserve(1); _output.Append(value); }
    }
}
