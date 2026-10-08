using System.Diagnostics.CodeAnalysis;
using System.Text;

namespace Ferrule.Runtime;

/// <summary>
/// Pure bounded normalization of one JSON5 value to compact strict JSON.
/// Duplicate properties and decimal lexicals survive; schema projection is separate.
/// Unquoted identifiers are deliberately ASCII; quoted strings retain Unicode scalars.
/// </summary>
public static partial class FerruleJson5Syntax
{
    public const long MaximumOriginalBytes = 64L * 1024 * 1024;
    public const long MaximumNormalizedBytes = 64L * 1024 * 1024;
    public const int MaximumContainerDepth = 127;
    public const long MaximumSyntaxWork = 536_870_912;

    private static readonly UTF8Encoding StrictUtf8 = new(false, true);

    public static string Normalize(string source) => NormalizeLimited(
        source, MaximumOriginalBytes, MaximumNormalizedBytes, MaximumContainerDepth, MaximumSyntaxWork);

    private static string NormalizeLimited(
        string source, long original, long normalized, int depth, long work)
    {
        ArgumentNullException.ThrowIfNull(source);
        if (original < 0 || normalized < 0 || depth < 0 || work < 0)
        {
            throw new ArgumentOutOfRangeException(nameof(original), "Syntax limits must be nonnegative.");
        }
        // A single allocation-free encoding census precedes grammar work.
        // Invalid text has no complete UTF-8 size, so Encoding is its first
        // failure; valid text retains the complete exact original byte count.
        var bytes = CountOriginalBytes(source);
        if (bytes > original)
        {
            throw FerruleJson5SyntaxException.Limit(
                FerruleJson5SyntaxResource.OriginalDocumentBytes, 0, bytes, original);
        }
        // The census is separate from SyntaxWork: Rust's &str already has
        // valid UTF-8 and does not charge initial source.len() validation.
        return new Parser(source, normalized, depth, work).Run();
    }

    private static long CountOriginalBytes(string source)
    {
        long bytes = 0;
        for (var index = 0; index < source.Length; index++)
        {
            var scalar = (int)source[index];
            if (char.IsHighSurrogate(source[index]))
            {
                if (index + 1 == source.Length || !char.IsLowSurrogate(source[index + 1]))
                {
                    InvalidEncoding(source, index, bytes);
                }
                scalar = char.ConvertToUtf32(source[index], source[++index]);
            }
            else if (char.IsLowSurrogate(source[index]))
            {
                InvalidEncoding(source, index, bytes);
            }
            // UTF-16 strings have at most int.MaxValue units. This long sum
            // is bounded by 3 * int.MaxValue without an overflowing int count.
            bytes += Utf8Width(scalar);
        }
        return bytes;
    }

    [DoesNotReturn]
    private static void InvalidEncoding(string source, int index, long bytes)
    {
        try
        {
            // Retain the BCL's actual failure on the offending original span.
            // Inner.Index is span-scoped; outer indices are absolute.
            _ = StrictUtf8.GetByteCount(source.AsSpan(index, 1));
        }
        catch (EncoderFallbackException error)
        {
            throw FerruleJson5SyntaxException.Encoding(bytes, index, error);
        }
        throw new InvalidOperationException("Throwing UTF-8 encoder accepted an unpaired surrogate.");
    }

    private static int Utf8Width(int scalar) => scalar switch
    {
        <= 0x7f => 1,
        <= 0x7ff => 2,
        <= 0xffff => 3,
        _ => 4,
    };

    private static bool Whitespace(int scalar) => scalar is
        >= 0x0009 and <= 0x000d or 0x0020 or 0x00a0 or 0x1680 or
        >= 0x2000 and <= 0x200a or 0x2028 or 0x2029 or 0x202f or
        0x205f or 0x3000 or 0xfeff;

    private static bool LineTerminator(int scalar) => scalar is '\n' or '\r' or 0x2028 or 0x2029;

    private enum State
    {
        RootValue, RootDone, ObjectKey, ObjectAfterComma, ObjectColon, ObjectValue,
        ObjectNext, ArrayValue, ArrayAfterComma, ArrayNext,
    }

    private sealed partial class Parser
    {
        private readonly string _source;
        private readonly long _maximumNormalized;
        private readonly int _maximumDepth;
        private readonly long _maximumWork;
        private readonly StringBuilder _output = new();
        private readonly List<State> _states = [State.RootValue];
        private int _index;
        private long _offset;
        private long _normalized;
        private long _work;

        public Parser(string source, long normalized, int depth, long work)
        {
            _source = source;
            _maximumNormalized = normalized;
            _maximumDepth = depth;
            _maximumWork = work;
        }

        public string Run()
        {
            while (true)
            {
                Charge(1);
                Trivia();
                switch (_states[^1])
                {
                    case State.RootDone:
                        if (Peek() >= 0) { throw Syntax(FerruleJson5SyntaxKind.TrailingRootValue); }
                        // The final owned-string copy is bounded separately by the
                        // normalized byte cap, outside the shared SyntaxWork ledger.
                        // The ledger is not a CPU, allocation, or RSS guarantee.
                        return _output.ToString();
                    case State.RootValue:
                    case State.ObjectValue:
                        Value();
                        break;
                    case State.ArrayValue:
                    case State.ArrayAfterComma:
                        if (Peek() == ']') { Close(']'); }
                        else
                        {
                            if (_states[^1] == State.ArrayAfterComma) { AppendAscii(","); }
                            Value();
                        }
                        break;
                    case State.ObjectKey:
                    case State.ObjectAfterComma:
                        var next = Peek();
                        if (next == '}') { Close('}'); }
                        else if (next is '\'' or '"' || IdentifierChar(next, true) || next == '\\')
                        {
                            if (_states[^1] == State.ObjectAfterComma) { AppendAscii(","); }
                            if (next is '\'' or '"') { String(); } else { Identifier(); }
                            SetState(State.ObjectColon);
                        }
                        else if (next > 0x7f && !Whitespace(next))
                        {
                            throw Syntax(FerruleJson5SyntaxKind.UnsupportedIdentifier);
                        }
                        else { throw Syntax(FerruleJson5SyntaxKind.UnexpectedToken); }
                        break;
                    case State.ObjectColon:
                        if (Peek() != ':') { throw Syntax(FerruleJson5SyntaxKind.UnexpectedToken); }
                        Take();
                        AppendAscii(":");
                        SetState(State.ObjectValue);
                        break;
                    case State.ObjectNext:
                        Next('}', State.ObjectAfterComma);
                        break;
                    case State.ArrayNext:
                        Next(']', State.ArrayAfterComma);
                        break;
                    default: throw Syntax(FerruleJson5SyntaxKind.UnexpectedToken);
                }
            }
        }

        private void Next(int close, State following)
        {
            var next = Peek();
            if (next == ',') { Take(); SetState(following); }
            else if (next == close) { Close(close); }
            else { throw Syntax(FerruleJson5SyntaxKind.UnexpectedToken); }
        }

        private void Value()
        {
            var next = Peek();
            switch (next)
            {
                case '{':
                case '[':
                    var depth = _states.Count;
                    if (depth > _maximumDepth)
                    {
                        throw Limit(FerruleJson5SyntaxResource.ContainerDepth, depth, _maximumDepth);
                    }
                    ValueDone();
                    Take();
                    AppendAscii(next == '{' ? "{" : "[");
                    Charge(1);
                    _states.Add(next == '{' ? State.ObjectKey : State.ArrayValue);
                    break;
                case '\'':
                case '"': String(); ValueDone(); break;
                case '+':
                case '-':
                case '.':
                case >= '0' and <= '9': Number(); ValueDone(); break;
                case 't':
                case 'f':
                case 'n':
                case 'I':
                case 'N':
                    var start = _index;
                    var offset = _offset;
                    var end = TokenEnd();
                    var token = _source.AsSpan(start, end - start);
                    Charge((_offset - offset) * 3);
                    if (token.SequenceEqual("true".AsSpan()) || token.SequenceEqual("false".AsSpan()) || token.SequenceEqual("null".AsSpan()))
                    {
                        AppendAscii(token);
                    }
                    else if (token.SequenceEqual("Infinity".AsSpan()) || token.SequenceEqual("NaN".AsSpan()))
                    {
                        throw Syntax(FerruleJson5SyntaxKind.NonFiniteNumber, offset);
                    }
                    else { throw Syntax(FerruleJson5SyntaxKind.UnexpectedToken, offset); }
                    ValueDone();
                    break;
                default: throw Syntax(FerruleJson5SyntaxKind.UnexpectedToken);
            }
        }

        private void ValueDone() => SetState(_states[^1] switch
        {
            State.RootValue => State.RootDone,
            State.ObjectValue => State.ObjectNext,
            State.ArrayValue or State.ArrayAfterComma => State.ArrayNext,
            _ => throw Syntax(FerruleJson5SyntaxKind.UnexpectedToken),
        });

        private void SetState(State state) { Charge(1); _states[^1] = state; }

        private void Close(int close)
        {
            Take();
            AppendAscii(close == '}' ? "}" : "]");
            Charge(1);
            _states.RemoveAt(_states.Count - 1);
        }

        private void Trivia()
        {
            while (true)
            {
                var next = Peek();
                if (Whitespace(next)) { Take(); }
                else if (next == '/')
                {
                    var start = _offset;
                    Take();
                    next = Peek();
                    if (next == '/')
                    {
                        Take();
                        while (true)
                        {
                            var c = Peek();
                            if (c < 0 || LineTerminator(c)) { break; }
                            Take();
                        }
                    }
                    else if (next == '*')
                    {
                        Take();
                        var star = false;
                        while (true)
                        {
                            var c = Peek();
                            if (c < 0) { throw Syntax(FerruleJson5SyntaxKind.UnterminatedComment, start); }
                            Take();
                            if (star && c == '/') { break; }
                            star = c == '*';
                        }
                    }
                    else { throw Syntax(FerruleJson5SyntaxKind.UnexpectedToken, start); }
                }
                else { return; }
            }
        }

        private int TokenEnd()
        {
            while (true)
            {
                var next = Peek();
                if (next < 0 || Whitespace(next) || next is ',' or ']' or '}' or '/') { return _index; }
                Take();
            }
        }

        private int Peek()
        {
            var scalar = _index == _source.Length ? -1 :
                char.IsHighSurrogate(_source[_index])
                    ? char.ConvertToUtf32(_source[_index], _source[_index + 1]) : _source[_index];
            Charge(scalar < 0 ? 1 : Utf8Width(scalar));
            return scalar;
        }

        private int Take()
        {
            var scalar = Peek();
            if (scalar < 0) { throw Syntax(FerruleJson5SyntaxKind.UnexpectedToken); }
            var bytes = Utf8Width(scalar);
            Charge(bytes);
            _index += scalar > 0xffff ? 2 : 1;
            _offset += bytes;
            return scalar;
        }

        private void AppendAscii(ReadOnlySpan<char> text)
        {
            Reserve(text.Length);
            _output.Append(text);
        }

        private void Reserve(int bytes)
        {
            var requested = checked(_normalized + bytes);
            if (requested > _maximumNormalized)
            {
                throw Limit(FerruleJson5SyntaxResource.NormalizedDocumentBytes, requested, _maximumNormalized);
            }
            Charge(bytes);
            _normalized = requested;
        }

        private void Charge(long amount)
        {
            var requested = checked(_work + amount);
            if (requested > _maximumWork)
            {
                throw Limit(FerruleJson5SyntaxResource.SyntaxWork, requested, _maximumWork);
            }
            _work = requested;
        }

        private FerruleJson5SyntaxException Syntax(FerruleJson5SyntaxKind kind, long? offset = null) =>
            FerruleJson5SyntaxException.Syntax(kind, offset ?? _offset);

        private FerruleJson5SyntaxException Limit(FerruleJson5SyntaxResource resource, long requested, long maximum) =>
            FerruleJson5SyntaxException.Limit(resource, _offset, requested, maximum);
    }
}
