using System.Text;
using Ferrule.Runtime;

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private static void CsvByteDecodingPolicy()
    {
        static void Value(string expected, FerruleInstance value)
        {
            var rows = (FerruleRepeated)value;
            Equal(1, rows.Items.Count);
            var row = (FerruleGroup)rows.Items[0];
            Equal(1, row.Fields.Count);
            Equal("Value", row.Fields[0].Name);
            var scalar = (FerruleScalar)row.Fields[0].Value;
            Equal(FerruleValueKind.String, scalar.Value.Kind);
            Equal(expected, scalar.Value.StringValue);
        }
        static FerruleCsvInputException Refusal(FerruleCsvInputError expected, Action action)
        {
            try { action(); }
            catch (FerruleCsvInputException error) { Equal(expected, error.Error); return error; }
            throw new Exception("Expected CSV input refusal.");
        }
        var fields = new[] { new FerruleCsvReadField("Value", FerruleScalarType.String) };
        const string document = "Value\r\nSnow ☃🙂\r\n";
        foreach (var encoding in new Encoding[] {
            new UTF8Encoding(false), new UTF8Encoding(true), new UnicodeEncoding(false, true),
            new UnicodeEncoding(true, true), new UTF32Encoding(false, true), new UTF32Encoding(true, true),
        })
        {
            var bytes = encoding.GetPreamble().Concat(encoding.GetBytes(document)).ToArray();
            var actual = FerruleCsvInput.ParseBytesWithDecodingPolicy(bytes, fields,
                FerruleCsvByteDecodingPolicy.BomAwareNonFlushingReplacement);
            Value("Snow ☃🙂", actual);
            Equal(0, ((FerruleRepeated)FerruleCsvInput.ParseBytesWithDecodingPolicy(encoding.GetPreamble(), fields,
                FerruleCsvByteDecodingPolicy.BomAwareNonFlushingReplacement)).Items.Count);
        }
        var prefix = Encoding.UTF8.GetBytes("Value\nV");
        var little = new byte[] { 0xff, 0xfe }.Concat(Encoding.Unicode.GetBytes("Value\nV"));
        var big = new byte[] { 0xfe, 0xff }.Concat(Encoding.BigEndianUnicode.GetBytes("Value\nV"));
        var little32 = new byte[] { 0xff, 0xfe, 0, 0 }.Concat(new UTF32Encoding(false, false).GetBytes("Value\nV"));
        var big32 = new byte[] { 0, 0, 0xfe, 0xff }.Concat(new UTF32Encoding(true, false).GetBytes("Value\nV"));
        foreach (var (bytes, value) in new[] {
            (prefix.Concat(new byte[] { 0xe2, 0x82 }).ToArray(), "V"),
            (prefix.Concat(new byte[] { 0xe2, 0x82, 0x0a }).ToArray(), "V\ufffd"),
            (prefix.Concat(new byte[] { 0xc0, 0xaf, 0x0a }).ToArray(), "V\ufffd\ufffd"),
            (little.Concat(new byte[] { 0, 0xd8 }).ToArray(), "V"),
            (little.Concat(new byte[] { 0, 0xd8, 0x0a, 0 }).ToArray(), "V\ufffd"),
            (little.Concat(new byte[] { 0x41 }).ToArray(), "V"),
            (big.Concat(new byte[] { 0xdc, 0, 0, 0x0a }).ToArray(), "V\ufffd"),
            (little32.Concat(new byte[] { 0, 0, 0x11, 0, 0x0a, 0, 0, 0 }).ToArray(), "V\ufffd"),
            (big32.Concat(new byte[] { 0, 0, 0xd8, 0, 0, 0, 0, 0x0a }).ToArray(), "V\ufffd"),
            (big32.Concat(new byte[] { 0, 0x41 }).ToArray(), "V"),
        })
        {
            Value(value, FerruleCsvInput.ParseBytesWithDecodingPolicy(bytes, fields, FerruleCsvByteDecodingPolicy.BomAwareNonFlushingReplacement));
            Refusal(FerruleCsvInputError.Encoding, () => FerruleCsvInput.ParseBytes(bytes, fields));
        }
        Refusal(FerruleCsvInputError.InputTooLarge, () => FerruleCsvInput.ParseBytesWithDecodingPolicy(
            new byte[FerruleCsvInput.MaximumInputBytes + 1], fields, FerruleCsvByteDecodingPolicy.BomAwareNonFlushingReplacement));
        var expanded = Enumerable.Repeat((byte)0xff, FerruleCsvInput.MaximumInputBytes / 3 + 1).ToArray();
        var error = Refusal(FerruleCsvInputError.DecodedInputTooLarge, () => FerruleCsvInput.ParseBytesWithDecodingPolicy(
            expanded, fields, FerruleCsvByteDecodingPolicy.BomAwareNonFlushingReplacement));
        Equal((long?)expanded.Length * 3, error.Observed);
        Value("text", FerruleCsvInput.ParseBytesWithDecodingPolicy(Encoding.UTF8.GetBytes("Value\ntext\n"), fields,
            FerruleCsvByteDecodingPolicy.StrictUtf8));
        Throws<ArgumentOutOfRangeException>(() => FerruleCsvInput.ParseBytesWithDecodingPolicy([], fields,
            (FerruleCsvByteDecodingPolicy)999));
    }
}
