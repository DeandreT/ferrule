using System.Text;
using System.Text.Json;

namespace Ferrule.Runtime;

public sealed record FerruleSourceBytes(string Identity, byte[] Bytes);

/// <summary>Explicit bounded direct file/HTTP(S) transport. It neither parses
/// input nor supplies proxy, credential or cookie policy on the host's behalf.</summary>
public sealed class FerruleDirectSourceReader : IDisposable
{
    private readonly System.Net.Http.HttpClient client;
    private readonly int maximum;
    private readonly TimeSpan timeout;
    private readonly bool allowFtp;
    public FerruleDirectSourceReader(int maximumBytes, TimeSpan timeout)
        : this(maximumBytes, timeout, allowFtp: false)
    {
    }
    public FerruleDirectSourceReader(int maximumBytes, TimeSpan timeout, bool allowFtp)
    {
        if (maximumBytes is < 0 or > FerruleX12.MaximumDocumentBytes) throw new ArgumentOutOfRangeException(nameof(maximumBytes));
        if (timeout <= TimeSpan.Zero || timeout > TimeSpan.FromMinutes(5)) throw new ArgumentOutOfRangeException(nameof(timeout));
        maximum = maximumBytes;
        this.timeout = timeout;
        this.allowFtp = allowFtp;
        client = new(new System.Net.Http.HttpClientHandler { UseProxy = false, UseCookies = false, AllowAutoRedirect = true, MaxAutomaticRedirections = 50, AutomaticDecompression = System.Net.DecompressionMethods.None }) { Timeout = timeout };
    }
    public FerruleSourceBytes Read(string selected)
    {
        ArgumentNullException.ThrowIfNull(selected);
        string identity = selected.Trim();
        byte[] bytes;
        if (identity.IndexOf(':') > 1)
        {
            if (!Uri.TryCreate(identity, UriKind.Absolute, out var uri)) throw new IOException("Unsupported direct-source URI.");
            if (uri.UserInfo.Length != 0) throw new IOException("Source URI credentials require a separate host policy.");
            if (uri.IsFile) bytes = ReadFile(uri.LocalPath);
            else if (uri.Scheme == "ftp" && allowFtp) return ReadFtpWith(uri, identity, (name, data) => new FerruleSourceBytes(name, data));
            else if (uri.Scheme is "http" or "https")
            {
                try
                {
                    using var cancellation = new System.Threading.CancellationTokenSource(timeout);
                    using var response = client.GetAsync(uri, System.Net.Http.HttpCompletionOption.ResponseHeadersRead, cancellation.Token).GetAwaiter().GetResult();
                    if (!response.IsSuccessStatusCode) throw new IOException("HTTP source returned status " + (int)response.StatusCode);
                    using var stream = response.Content.ReadAsStream(); bytes = Bounded(stream, cancellation.Token);
                }
                catch (System.Net.Http.HttpRequestException) { throw new IOException("HTTP source could not be loaded."); }
                catch (OperationCanceledException) { throw new IOException("HTTP source timed out."); }
            }
            else throw new IOException("Unsupported direct-source URI.");
        }
        else bytes = ReadFile(identity);
        return new(identity, bytes);
    }
    /// <summary>Invoke a parser after a complete read, while the native input is
    /// still open. Local files use the explicit owned-source helper, retaining
    /// a prior read/parse cause if disposal fails. Read remains the separate
    /// byte-only API which closes before returning its bytes. HTTP lifetime and
    /// multi-resource cleanup require separate qualification.</summary>
    public T ReadWith<T>(string selected, Func<string, byte[], T> parse)
    {
        ArgumentNullException.ThrowIfNull(selected);
        ArgumentNullException.ThrowIfNull(parse);
        string identity = selected.Trim();
        if (identity.IndexOf(':') <= 1) return FileWith(identity);
        if (!Uri.TryCreate(identity, UriKind.Absolute, out var uri)) throw new IOException("Unsupported direct-source URI.");
        if (uri.UserInfo.Length != 0) throw new IOException("Source URI credentials require a separate host policy.");
        if (uri.IsFile) return FileWith(uri.LocalPath);
        if (uri.Scheme == "ftp" && allowFtp) return ReadFtpWith(uri, identity, parse);
        if (uri.Scheme is not ("http" or "https")) throw new IOException("Unsupported direct-source URI.");
        using var cancellation = new System.Threading.CancellationTokenSource(timeout);
        System.Net.Http.HttpResponseMessage response;
        try { response = client.GetAsync(uri, System.Net.Http.HttpCompletionOption.ResponseHeadersRead, cancellation.Token).GetAwaiter().GetResult(); }
        catch (System.Net.Http.HttpRequestException) { throw new IOException("HTTP source could not be loaded."); }
        catch (OperationCanceledException) { throw new IOException("HTTP source timed out."); }
        using (response)
        {
            if (!response.IsSuccessStatusCode) throw new IOException("HTTP source returned status " + (int)response.StatusCode);
            using var stream = response.Content.ReadAsStream();
            byte[] bytes;
            try { bytes = Bounded(stream, cancellation.Token); }
            catch (System.Net.Http.HttpRequestException) { throw new IOException("HTTP source could not be loaded."); }
            catch (OperationCanceledException) { throw new IOException("HTTP source timed out."); }
            return parse(identity, bytes);
        }
        T FileWith(string path)
        {
            var stream = new FileStream(path, FileMode.Open, FileAccess.Read, FileShare.Read);
            return FerruleOwnedInput.ReadParseClose(
                new OwnedStream(stream, () => Bounded(stream)), bytes => parse(identity, bytes));
        }
    }
    private sealed class OwnedStream(Stream stream, Func<byte[]> read) : IFerruleOwnedInput
    {
        public byte[] ReadAll() => read();
        public void Close() => stream.Dispose();
    }
    private T ReadFtpWith<T>(Uri uri, string identity, Func<string, byte[], T> parse)
    {
        if (Uri.UnescapeDataString(uri.AbsolutePath).IndexOfAny(['\r', '\n', '\0']) >= 0)
            throw new IOException("Unsupported direct-source URI.");
        // Preserve the default file/HTTP profile. FTP is an explicit legacy
        // compatibility option and never uses caller or OS credentials.
#pragma warning disable SYSLIB0014
        var request = (System.Net.FtpWebRequest)System.Net.WebRequest.Create(uri);
#pragma warning restore SYSLIB0014
        request.Method = System.Net.WebRequestMethods.Ftp.DownloadFile;
        request.Credentials = new System.Net.NetworkCredential("anonymous", "anonymous@");
        request.Proxy = null;
        request.UseBinary = true;
        request.UsePassive = true;
        request.KeepAlive = false;
        request.Timeout = checked((int)timeout.TotalMilliseconds);
        request.ReadWriteTimeout = checked((int)timeout.TotalMilliseconds);
        using var cancellation = new System.Threading.CancellationTokenSource(timeout);
        using var response = request.GetResponse();
        using var stream = response.GetResponseStream();
        return parse(identity, Bounded(stream, cancellation.Token));
    }
    private byte[] ReadFile(string path) { using var stream = new FileStream(path, FileMode.Open, FileAccess.Read, FileShare.Read); return Bounded(stream); }
    private byte[] Bounded(Stream stream, System.Threading.CancellationToken cancellation = default)
    {
        using var buffer = new MemoryStream(); var chunk = new byte[8192];
        while (true)
        {
            int count = stream.ReadAsync(chunk.AsMemory(0, Math.Min(chunk.Length, maximum - (int)buffer.Length + 1)), cancellation).AsTask().GetAwaiter().GetResult();
            if (count == 0) return buffer.ToArray();
            if (count > maximum - buffer.Length) throw new IOException("Source byte limit exceeded.");
            buffer.Write(chunk, 0, count);
        }
    }
    public void Dispose() => client.Dispose();
}

public static partial class FerruleX12
{
    public static bool HasSourceFilenameWildcard(string value)
    {
        ArgumentNullException.ThrowIfNull(value);
        return value.IndexOfAny(['*', '?']) >= 0 && ((value.Length > 1 && value[1] == ':') || !value.Contains(':'));
    }

    /// <summary>Read-only local selection. Literal paths and URLs remain
    /// unchanged. The caller separately authorizes and opens the selected paths.</summary>
    public static IReadOnlyList<string> SelectSourceFilenames(string value, int maximum = 100000)
    {
        ArgumentNullException.ThrowIfNull(value);
        if (maximum is < 0 or > 100000) throw new ArgumentOutOfRangeException(nameof(maximum));
        if (!HasSourceFilenameWildcard(value)) return maximum == 0 ? throw new InvalidOperationException("Source filename selection exceeded its bounds.") : new[] { value };
        if (!OperatingSystem.IsWindows()) throw new PlatformNotSupportedException("Native filename selection requires Windows.");
        if (value.Length > 32760) throw new InvalidOperationException("Source filename selection exceeded its bounds.");
        var selected = new List<string>(); long names = 0; long work = 0; int scanned = 0;
        try
        {
            int split = value.Replace('\\', '/').LastIndexOf('/') + 1;
            string root = split == 0 ? "." : value[..split];
            string expression = System.IO.Enumeration.FileSystemName.TranslateWin32Expression(value[split..]);
            foreach (var item in new DirectoryInfo(root).EnumerateFileSystemInfos("*"))
            {
                if (++scanned > 1000000) throw new InvalidOperationException("Source filename selection exceeded its bounds.");
                if (item is not FileInfo file) continue;
                work += (long)expression.Length * (file.Name.Length + 1);
                if (work > 50000000) throw new InvalidOperationException("Source filename selection exceeded its bounds.");
                if (!System.IO.Enumeration.FileSystemName.MatchesWin32Expression(expression, file.Name, ignoreCase: true)) continue;
                names += file.FullName.Length;
                if (selected.Count == maximum || names > 8 * 1024 * 1024) throw new InvalidOperationException("Source filename selection exceeded its bounds.");
                selected.Add(file.FullName);
            }
        }
        catch (Exception error) when (error is IOException or UnauthorizedAccessException or ArgumentException or NotSupportedException) { selected.Clear(); }
        return selected;
    }

    /// <summary>Tolerant native source value projection. It does not perform
    /// numeric conversions, calendar validation, file acquisition or publication.</summary>
    public static FerruleInstance ParseMapForceSourceBytes(string descriptor, byte[] bytes)
    {
        var source = NativeSourceSettings(descriptor);
        Profile profile = MapForceSourceProfile(source.Boundary);
        FerruleInstance lexical = source.Structure is { } structure
            ? ParseNativeStructureSource(profile, structure, bytes, source.Encoding)
            : ParseLexicalSourceCore(source.Boundary, bytes, nativeProjection: true, source.Encoding);
        int textBytes = 0;
        return ProjectMapForceSource(profile.Root, lexical, positional: false, segment: false, new Budget(), ref textBytes, 0);
    }

    internal static void ValidateMapForceSourceDescriptor(string descriptor)
    {
        var source = NativeSourceSettings(descriptor);
        var profile = MapForceSourceProfile(source.Boundary);
        if (source.Structure is { } structure) { int count = 0; _ = NativeParticle.Read(structure, profile.Root, 0, ref count); }
    }

    private static (string Boundary, string Encoding, JsonElement? Structure) NativeSourceSettings(string descriptor)
    {
        ArgumentNullException.ThrowIfNull(descriptor);
        RequireUtf8(descriptor, MaximumSchemaBytes, "X12 source descriptor");
        try
        {
            using var parsed = JsonDocument.Parse(descriptor);
            if (parsed.RootElement.ValueKind != JsonValueKind.Object || !parsed.RootElement.TryGetProperty("boundary", out var boundary)) return (descriptor, "utf8", null);
            var names = parsed.RootElement.EnumerateObject().Select(p => p.Name).ToArray();
            if (names.Distinct(StringComparer.Ordinal).Count() != names.Length || names.Any(n => n is not ("boundary" or "source_encoding" or "structure")) || boundary.ValueKind != JsonValueKind.String || !parsed.RootElement.TryGetProperty("source_encoding", out var encoding)
                || encoding.ValueKind != JsonValueKind.String || encoding.GetString() is not ("utf8" or "latin1"))
                throw Failure(FerruleX12Error.Schema, "Invalid native X12 source descriptor.");
            return (boundary.GetString()!, encoding.GetString()!, parsed.RootElement.TryGetProperty("structure", out var structure) && structure.ValueKind != JsonValueKind.Null ? structure.Clone() : null);
        }
        catch (JsonException) { throw Failure(FerruleX12Error.Schema, "Invalid native X12 source descriptor."); }
    }

    private static Profile MapForceSourceProfile(string descriptor)
    {
        Profile profile = ParseProfile(descriptor);
        if (profile.MapForce is null || HasFractionalImplied(profile.Root))
            throw Failure(FerruleX12Error.UnsupportedProfile, "MapForce source projection requires a MapForce envelope without fractional implied decimals.");
        return profile;
    }

    /// <summary>Call within a loader before input cleanup; no file is opened here.</summary>
    public static FerruleParsedX12Source ParseMapForceSourceDocument(string descriptor, string identity, byte[] bytes)
        => new(descriptor, new FerruleDocument(identity, ParseMapForceSourceBytes(descriptor, bytes)));

    private static bool HasFractionalImplied(Node node) => node.ImpliedPlaces is > 0 || node.Children.Any(HasFractionalImplied);

    private static FerruleInstance ProjectMapForceSource(Node node, FerruleInstance value, bool positional, bool segment, Budget budget, ref int textBytes, int depth)
    {
        budget.Visit(depth);
        if (value is FerruleRepeated repeated)
        {
            var items = new List<FerruleInstance>();
            foreach (FerruleInstance item in repeated.Items) { budget.Loop(); items.Add(ProjectMapForceSource(node, item, positional, segment, budget, ref textBytes, depth + 1)); }
            return new FerruleRepeated(items);
        }
        if (value is FerruleScalar scalar)
        {
            if (scalar.Value.Kind == FerruleValueKind.Null) return scalar;
            if (scalar.Value.Kind != FerruleValueKind.String) throw Failure(FerruleX12Error.Schema, "Unsupported lexical source value.");
            string text = scalar.Value.StringValue;
            if (text.Length == 0) return new FerruleScalar(FerruleValue.Null);
            text = node.Lexical?.Kind switch { "compact_date6" or "compact_date8" => MapForceSourceDate(text), "compact_time" => MapForceSourceTime(text), "decimal" => MapForceSourceNumber(text), _ => text };
            int bytes = Encoding.UTF8.GetByteCount(text);
            if (bytes > MaximumDocumentBytes - textBytes) throw Limit("X12 source projected text limit exceeded.");
            textBytes += bytes;
            return new FerruleScalar(FerruleValue.FromString(text));
        }
        if (value is not FerruleGroup group) throw Failure(FerruleX12Error.Schema, "Unsupported lexical source value.");
        var fields = new List<FerruleField>();
        foreach (Node child in node.Children)
        {
            FerruleField? field = group.Fields.FirstOrDefault(field => field.Name == child.Name);
            if (field is null) throw Failure(FerruleX12Error.Schema, "Lexical source field is missing from its schema-shaped view.");
            budget.Name(child.Name);
            bool childPositional = positional || segment;
            bool childSegment = !childPositional && child.Type is null && SegmentId(child.Name) is not null;
            fields.Add(new FerruleField(child.Name, ProjectMapForceSource(child, field.Value, childPositional, childSegment, budget, ref textBytes, depth + 1)));
        }
        if (positional && fields.All(field => field.Value is FerruleScalar item && item.Value.Kind == FerruleValueKind.Null)) return new FerruleScalar(FerruleValue.Null);
        return group.CloneFields(fields);
    }

    private static string MapForceSourceDate(string text)
    {
        text = text.TrimStart();
        if (text.Length is not (6 or 8) || !text.All(char.IsAsciiDigit)) return text;
        if (text.Length == 6) text = (string.CompareOrdinal(text[..2], "50") < 0 ? "20" : "19") + text;
        return text[..4] + "-" + text[4..6] + "-" + text[6..];
    }

    private static string MapForceSourceTime(string text)
    {
        text = text.TrimStart();
        if (text.Length < 4 || text.Length == 5) return text;
        if (text.Length == 4) text += "00";
        if (text.Length > 6) text = text[..6] + "." + text[6..];
        text = text[..4] + ":" + text[4..];
        text = text[..2] + ":" + text[2..];
        return Encoding.UTF8.GetString(Encoding.UTF8.GetBytes(text));
    }

    private static string MapForceSourceNumber(string input)
    {
        string text = input.Trim();
        if (text.Length == 0) return input;
        bool negative = text[0] == '-';
        if (text[0] is '+' or '-') text = text[1..];
        if (text.Length == 0) return "";
        int separator = text.LastIndexOf('.');
        if (separator < 0) separator = text.LastIndexOf(',');
        string integer = separator < 0 ? text : text[..separator];
        string fraction = separator < 0 ? "" : text[(separator + 1)..];
        integer = integer.Length == 0 ? "0" : integer.Replace(".", "", StringComparison.Ordinal).Replace(",", "", StringComparison.Ordinal);
        if (!integer.All(char.IsAsciiDigit) || !fraction.All(char.IsAsciiDigit)) return text;
        return (negative ? "-" : "") + integer + (fraction.Length == 0 ? "" : "." + fraction);
    }

    /// <summary>Explicit structural/lexical source view; strict typed Parse is unchanged.
    /// Scalar validation and conversion remain the mapping's reached operations.</summary>
    public static FerruleInstance ParseLexicalSourceBytes(string descriptor, byte[] bytes)
        => ParseLexicalSourceCore(descriptor, bytes, nativeProjection: false);

    private static FerruleInstance ParseLexicalSourceCore(string descriptor, byte[] bytes, bool nativeProjection, string sourceEncoding = "utf8")
    {
        ArgumentNullException.ThrowIfNull(bytes);
        if (bytes.Length > MaximumDocumentBytes) throw Limit("X12 source byte limit exceeded.");
        bool bom = bytes.AsSpan().StartsWith(new byte[] { 0xef, 0xbb, 0xbf }) || bytes.AsSpan().StartsWith(new byte[] { 0xff, 0xfe })
            || bytes.AsSpan().StartsWith(new byte[] { 0xfe, 0xff }) || bytes.AsSpan().StartsWith(new byte[] { 0, 0, 0xfe, 0xff });
        string text = sourceEncoding == "latin1" && !bom ? Encoding.Latin1.GetString(bytes) : FerruleTextDecoding.BomAwareNonFlushing(bytes);
        if (nativeProjection)
        {
            char terminator = text.Length > 105 ? text[105] : '\0';
            text = new string(text.Where(ch => ch is not ('\t' or '\r' or '\n') || ch == terminator).ToArray());
        }
        RequireUtf8(text, MaximumDocumentBytes, "X12 source");
        Profile original = ParseProfile(descriptor);
        if (text.Length < 106) throw Failure(FerruleX12Error.Syntax, "X12 requires an ISA header.");
        string version = text.Substring(84, 5);
        Node root = LexicalNode(original.Root);
        Profile profile = original with { Root = root, Version = new(version, version + "0"),
            Separators = null, ConfiguredRepetition = null, LenientSegments = false,
            MapForce = original.MapForce is null ? null : ValidateMapForceShape(root, input: true) };
        Syntax syntax = DiscoverSyntax(text, profile);
        List<Segment> segments = Tokenize(text, syntax, new HashSet<string>(RepeatingSegmentIds(root, root: true), StringComparer.Ordinal), allowControls: nativeProjection);
        var cursor = new Cursor(segments, syntax, profile, lexical: true);
        var value = ReadContainer(root, cursor, [], 0, [], root: true);
        if (cursor.Position != segments.Count) throw Failure(FerruleX12Error.Schema, "Unconsumed source segment.", cursor.Position);
        return value;
    }

    private static Node LexicalNode(Node node) => new()
    {
        Name = node.Name, Type = node.Type is null ? null : "string", Repeating = node.Repeating,
        Fixed = node.Fixed, Count = node.Count, Children = node.Children.Select(LexicalNode).ToArray(),
    };
}

public sealed class FerruleParsedX12Source
{
    internal FerruleParsedX12Source(string descriptor, FerruleDocument document) { Descriptor = descriptor; Document = document; }
    internal string Descriptor { get; }
    internal FerruleDocument Document { get; }
}

public enum FerruleX12AcquisitionStage { Filename, Selection, Request, Load, Limit, Descriptor }

public sealed class FerruleX12AcquisitionException : Exception
{
    internal FerruleX12AcquisitionException(FerruleX12AcquisitionStage stage, string message, Exception? cause = null, string? path = null) : base(message, cause) { Stage = stage; Path = path; }
    public FerruleX12AcquisitionStage Stage { get; }
    public string? Path { get; }
}

/// <summary>Lazy, descriptor-bound acquisition. Loaders parse and finish input cleanup before returning.</summary>
public static class FerruleX12SourceAcquisition
{
    public const int MaximumDocuments = 100_000;
    public static IEnumerable<FerruleDocument> Acquire(string descriptor, Func<string> filename, Func<string, IEnumerable<string>> select, Func<string, FerruleParsedX12Source> load, int maximum)
    {
        ArgumentNullException.ThrowIfNull(filename); ArgumentNullException.ThrowIfNull(select); ArgumentNullException.ThrowIfNull(load);
        FerruleX12.ValidateMapForceSourceDescriptor(descriptor);
        if (maximum < 0 || maximum > MaximumDocuments) throw new FerruleX12AcquisitionException(FerruleX12AcquisitionStage.Limit, "Invalid X12 source document limit.");
        return Iterate();
        IEnumerable<FerruleDocument> Iterate()
        {
            string pattern;
            try { pattern = filename(); }
            catch (Exception error) { throw new FerruleX12AcquisitionException(FerruleX12AcquisitionStage.Filename, "X12 source filename failed.", error); }
            IEnumerator<string> requests;
            try { requests = (select(pattern) ?? throw new InvalidOperationException("Null X12 selection.")).GetEnumerator(); }
            catch (Exception error) { throw new FerruleX12AcquisitionException(FerruleX12AcquisitionStage.Selection, "X12 source selection failed.", error); }
            using (requests)
            {
                int count = 0;
                while (true)
                {
                    string path; bool next;
                    try { next = requests.MoveNext(); path = next ? requests.Current : ""; }
                    catch (Exception error) { throw new FerruleX12AcquisitionException(FerruleX12AcquisitionStage.Request, "X12 source request failed.", error); }
                    if (!next) yield break;
                    if (count == maximum) throw new FerruleX12AcquisitionException(FerruleX12AcquisitionStage.Limit, "X12 source document limit reached.");
                    FerruleParsedX12Source source;
                    try { source = load(path) ?? throw new InvalidOperationException("Null X12 source document."); }
                    catch (Exception error) { throw new FerruleX12AcquisitionException(FerruleX12AcquisitionStage.Load, "X12 source load failed.", error, path); }
                    if (source.Descriptor != descriptor) throw new FerruleX12AcquisitionException(FerruleX12AcquisitionStage.Descriptor, "X12 source used a different descriptor.", path: path);
                    count++;
                    yield return source.Document;
                }
            }
        }
    }
}
