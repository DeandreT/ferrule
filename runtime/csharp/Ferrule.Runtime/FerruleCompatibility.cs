using System.Reflection;
using System.Text;
using System.Text.Json;

namespace Ferrule.Runtime;

/// <summary>
/// A generated library refused to run because its compatibility manifest or
/// vendored runtime differs from what it was generated with.
/// </summary>
public sealed class FerruleCompatibilityException : Exception
{
    public FerruleCompatibilityException(string field, string expected, string found)
        : base($"ferrule generated library compatibility check failed for {field}: expected {expected}, found {found}")
    {
        Field = field;
        Expected = expected;
        Found = found;
    }

    public string Field { get; }
    public string Expected { get; }
    public string Found { get; }
}

/// <summary>Load-time compatibility check of a generated library.</summary>
public static class FerruleCompatibility
{
    /// <summary>Logical name of the embedded <c>ferrule-manifest.json</c>.</summary>
    public const string ManifestResourceName = "ferrule-manifest.json";
    public const string ManifestFormat = "ferrule-generated-library";
    public const int ManifestFormatVersion = 1;

    /// <summary>
    /// Refuses a library whose vendored runtime identity differs from the one
    /// it was generated for, whose embedded manifest differs from the manifest
    /// compiled into it, or whose manifest has another format or runtime.
    /// </summary>
    public static void Verify(
        Assembly assembly,
        string manifest,
        string requiredRuntime,
        string vendoredRuntime)
    {
        ArgumentNullException.ThrowIfNull(assembly);
        ArgumentNullException.ThrowIfNull(manifest);
        ArgumentNullException.ThrowIfNull(requiredRuntime);
        ArgumentNullException.ThrowIfNull(vendoredRuntime);
        if (!string.Equals(requiredRuntime, vendoredRuntime, StringComparison.Ordinal))
        {
            throw new FerruleCompatibilityException("runtime identity", requiredRuntime, vendoredRuntime);
        }
        using var stream = assembly.GetManifestResourceStream(ManifestResourceName)
            ?? throw new FerruleCompatibilityException("manifest resource", ManifestResourceName, "no embedded resource");
        using var reader = new StreamReader(
            stream,
            new UTF8Encoding(encoderShouldEmitUTF8Identifier: false, throwOnInvalidBytes: true),
            detectEncodingFromByteOrderMarks: false);
        var embedded = reader.ReadToEnd();
        if (!string.Equals(embedded, manifest, StringComparison.Ordinal))
        {
            throw new FerruleCompatibilityException(
                "manifest",
                "the manifest compiled into the library",
                "a different ferrule-manifest.json");
        }
        using var document = JsonDocument.Parse(embedded);
        var root = document.RootElement;
        var format = root.GetProperty("format").GetString() ?? string.Empty;
        if (!string.Equals(format, ManifestFormat, StringComparison.Ordinal))
        {
            throw new FerruleCompatibilityException("manifest format", ManifestFormat, format);
        }
        var version = root.GetProperty("format_version").GetInt32();
        if (version != ManifestFormatVersion)
        {
            throw new FerruleCompatibilityException(
                "manifest format version",
                ManifestFormatVersion.ToString(System.Globalization.CultureInfo.InvariantCulture),
                version.ToString(System.Globalization.CultureInfo.InvariantCulture));
        }
        var runtime = root.GetProperty("runtime").GetProperty("identity").GetString() ?? string.Empty;
        if (!string.Equals(runtime, vendoredRuntime, StringComparison.Ordinal))
        {
            throw new FerruleCompatibilityException("manifest runtime identity", vendoredRuntime, runtime);
        }
    }
}
