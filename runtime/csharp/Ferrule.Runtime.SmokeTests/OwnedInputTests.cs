using Ferrule.Runtime;

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private sealed class OwnedReadFault() : IOException("read", new InvalidOperationException("read-leaf")) { }
    private sealed class OwnedParseFault() : FormatException("parse", new ArgumentException("parse-leaf")) { }
    private sealed class OwnedCloseFault() : IOException("close", new InvalidOperationException("close-leaf")) { }

    private sealed class TestOwnedInput(List<string> events, bool readFails, bool closeFails) : IFerruleOwnedInput
    {
        public OwnedReadFault ReadFailure { get; } = new();
        public OwnedCloseFault CloseFailure { get; } = new();
        public byte[] ReadAll()
        {
            events.Add("read");
            if (readFails) throw ReadFailure;
            return [1, 2, 3];
        }
        public void Close()
        {
            events.Add("close");
            if (closeFails) throw CloseFailure;
        }
    }

    private static void OwnedInputContract()
    {
        for (int bits = 0; bits < 8; bits++)
        {
            bool readFails = (bits & 1) != 0, parseFails = (bits & 2) != 0, closeFails = (bits & 4) != 0;
            var events = new List<string>();
            var source = new TestOwnedInput(events, readFails, closeFails);
            var parseFailure = new OwnedParseFault();
            Exception? observed = null;
            int? value = null;
            try
            {
                value = FerruleOwnedInput.ReadParseClose(source, bytes =>
                {
                    events.Add("parse");
                    Equal(true, bytes.SequenceEqual(new byte[] { 1, 2, 3 }));
                    if (parseFails) throw parseFailure;
                    return 7;
                });
            }
            catch (Exception error) { observed = error; }
            Equal(true, events.SequenceEqual(readFails ? new[] { "read", "close" } : new[] { "read", "parse", "close" }));
            if (closeFails)
            {
                Equal(true, observed is FerruleInputCloseException);
                var close = (FerruleInputCloseException)observed!;
                Equal(true, ReferenceEquals(source.CloseFailure, close.CloseError));
                Equal(true, ReferenceEquals(close.CloseError, close.InnerException));
                Equal(true, close.InnerException is OwnedCloseFault { InnerException: InvalidOperationException });
                Exception? prior = readFails ? source.ReadFailure : parseFails ? parseFailure : null;
                Equal(true, ReferenceEquals(prior, close.PriorFailure));
                Equal(readFails ? FerruleOwnedInputFailureStage.Read : parseFails ? FerruleOwnedInputFailureStage.Parse : (FerruleOwnedInputFailureStage?)null, close.PriorFailureStage);
                if (readFails) Equal(true, close.PriorFailure is OwnedReadFault { InnerException: InvalidOperationException });
                else if (parseFails) Equal(true, close.PriorFailure is OwnedParseFault { InnerException: ArgumentException });
                Equal<int?>(null, value);
            }
            else if (readFails) Equal(true, ReferenceEquals(source.ReadFailure, observed));
            else if (parseFails) Equal(true, ReferenceEquals(parseFailure, observed));
            else { Equal<Exception?>(null, observed); Equal<int?>(7, value); }
        }

        // Invalid arguments do not transfer ownership. The caller still owns it.
        var untouched = new List<string>();
        var callerOwned = new TestOwnedInput(untouched, false, false);
        Throws<ArgumentNullException>(() => FerruleOwnedInput.ReadParseClose<int>(callerOwned, null!));
        Equal(0, untouched.Count);
        callerOwned.Close();
        OwnedInputNativeRelease();
    }

    private static void OwnedInputNativeRelease()
    {
        if (!OperatingSystem.IsWindows()) return;
        string path = Path.GetTempFileName();
        try
        {
            File.WriteAllBytes(path, [1, 2, 3]);
            using var reader = new FerruleDirectSourceReader(3, TimeSpan.FromSeconds(5));
            using var limited = new FerruleDirectSourceReader(2, TimeSpan.FromSeconds(5));
            int length = reader.ReadWith(path, (identity, bytes) =>
            {
                Equal(path, identity);
                Throws<IOException>(() => { using var unavailable = new FileStream(path, FileMode.Open, FileAccess.Read, FileShare.None); });
                return bytes.Length;
            });
            Equal(3, length);
            using (var released = new FileStream(path, FileMode.Open, FileAccess.Read, FileShare.None)) { }
            var parseFailure = new OwnedParseFault();
            Exception? observed = null;
            try { reader.ReadWith<int>(path, (_, _) => throw parseFailure); }
            catch (Exception error) { observed = error; }
            Equal(true, ReferenceEquals(parseFailure, observed));
            using (var released = new FileStream(path, FileMode.Open, FileAccess.Read, FileShare.None)) { }
            bool called = false;
            Throws<IOException>(() => limited.ReadWith(path, (_, _) => { called = true; return 0; }));
            Equal(false, called);
            using (var released = new FileStream(path, FileMode.Open, FileAccess.Read, FileShare.None)) { }
            Equal(true, reader.Read(path).Bytes.SequenceEqual(new byte[] { 1, 2, 3 }));
            using (var released = new FileStream(path, FileMode.Open, FileAccess.Read, FileShare.None)) { }
            Throws<IOException>(() => limited.Read(path));
            using (var released = new FileStream(path, FileMode.Open, FileAccess.Read, FileShare.None)) { }
        }
        finally { File.Delete(path); }
    }
}
