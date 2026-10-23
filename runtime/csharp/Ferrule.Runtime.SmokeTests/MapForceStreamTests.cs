using System.Text;
using Ferrule.Runtime;

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private static void MapForceStream()
    {
        foreach (int prefix in new[] { 0, 1, 1022, 1023, 1024, 1025, 2047, 2048 })
        {
            string text = new string('A', prefix) + "😀" + new string('B', 3072 - prefix);
            using var stream = new MapForceTraceStream();
            FerruleX12.WriteMapForceText(text, stream);
            int[] expected = prefix switch
            {
                1023 => [1023, 1025, 1024, 2],
                2047 => [1024, 1023, 1025, 2],
                _ => [1024, 1024, 1024, 2],
            };
            Equal(true, expected.SequenceEqual(stream.Writes));
            Equal(1, stream.Flushes);
            Equal(false, stream.Closed);
            Equal(true, Encoding.GetEncoding("ISO-8859-1").GetBytes(text).SequenceEqual(stream.ToArray()));
        }
        using var failed = new MapForceTraceStream { FailWrite = true };
        Throws<IOException>(() => FerruleX12.WriteMapForceText("value", failed));
        Equal(0, failed.Flushes);
        Equal(false, failed.Closed);
    }

    private sealed class MapForceTraceStream : MemoryStream
    {
        public readonly List<int> Writes = [];
        public int Flushes;
        public bool Closed, FailWrite;
        public override void Write(byte[] bytes, int offset, int count)
        {
            if (FailWrite) throw new IOException("injected");
            Writes.Add(count);
            base.Write(bytes, offset, count);
        }
        public override void Flush() { Flushes++; base.Flush(); }
        protected override void Dispose(bool disposing) { Closed = true; base.Dispose(disposing); }
    }
}
