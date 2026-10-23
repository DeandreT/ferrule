using System.Text;

namespace Ferrule.Runtime;

internal static class FerruleTextDecoding
{
    internal static string BomAwareNonFlushing(byte[] source)
    {
        (Encoding Encoding, int Skip) decoding = source switch
        {
            [0xff, 0xfe, 0, 0, ..] => (new UTF32Encoding(false, false, false), 4),
            [0, 0, 0xfe, 0xff, ..] => (new UTF32Encoding(true, false, false), 4),
            [0xff, 0xfe, ..] => (new UnicodeEncoding(false, false, false), 2),
            [0xfe, 0xff, ..] => (new UnicodeEncoding(true, false, false), 2),
            [0xef, 0xbb, 0xbf, ..] => (new UTF8Encoding(false, false), 3),
            _ => (new UTF8Encoding(false, false), 0),
        };
        var (encoding, skip) = decoding;
        var count = source.Length - skip;
        var characters = new char[encoding.GetMaxCharCount(count)];
        var written = encoding.GetDecoder().GetChars(source, skip, count, characters, 0, flush: false);
        return new string(characters, 0, written);
    }
}
