using Ferrule.Runtime;

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private static void DirectSourceInputs()
    {
        Throws<ArgumentOutOfRangeException>(() => new FerruleDirectSourceReader(-1, TimeSpan.FromSeconds(1)));
        Throws<ArgumentOutOfRangeException>(() => new FerruleDirectSourceReader(1, TimeSpan.Zero));
        using var reader = new FerruleDirectSourceReader(3, TimeSpan.FromSeconds(1));
        Throws<IOException>(() => reader.Read("ftp://127.0.0.1:1/fixture"));
        Throws<IOException>(() => reader.Read("http://user:secret@127.0.0.1:1/fixture"));
        using var ftp = new FerruleDirectSourceReader(3, TimeSpan.FromSeconds(1), allowFtp: true);
        Throws<IOException>(() => ftp.Read("ftp://user:secret@127.0.0.1:1/fixture"));
        Throws<IOException>(() => ftp.Read("ftp://127.0.0.1:1/bad%0D%0Acommand"));
        string file = Path.GetTempFileName();
        try
        {
            File.WriteAllBytes(file, [1, 2, 3]);
            var loaded = reader.Read(" " + file + " ");
            Equal(file, loaded.Identity); Equal(true, loaded.Bytes.SequenceEqual(new byte[] { 1, 2, 3 }));
            var fromUri = reader.Read(new Uri(file).AbsoluteUri);
            Equal(true, fromUri.Bytes.SequenceEqual(loaded.Bytes));
            File.WriteAllBytes(file, [1, 2, 3, 4]);
            Throws<IOException>(() => reader.Read(file));
        }
        finally { File.Delete(file); }
    }
}
