using System.Runtime.InteropServices;
using System.Text;
using Ferrule.Host;

if (args.Length != 3)
    throw new ArgumentException("Usage: Ferrule.Host.SmokeTests <native-library> <fixture-directory> <smoke|mismatch>");

nint library = NativeLibrary.Load(Path.GetFullPath(args[0]));
NativeLibrary.SetDllImportResolver(typeof(FerruleProject).Assembly,
    (name, _, _) => name == "ferrule_host_ffi" ? library : nint.Zero);

if (args[2] == "mismatch")
{
    var mismatch = Throws<FerruleAbiMismatchException>(() => FerruleProject.Load("{"u8));
    Check(mismatch.ExpectedVersion == 1 && mismatch.ActualVersion == 2, "Incorrect ABI version report.");
    Console.WriteLine(".NET mismatched ABI test passed.");
    return;
}

string orchardPath = Path.Combine(args[1], "orchard-inventory.json");
string ledgerPath = Path.Combine(args[1], "versioned-ledger.json");
byte[] orchard = File.ReadAllBytes(orchardPath);
byte[] ledger = File.ReadAllBytes(ledgerPath);

using var project = FerruleProject.Load(orchard, (uint)orchard.Length);
Array.Fill(orchard, (byte)0);
InspectionReport inspection = project.Inspect(new(MaximumItems: 36, MaximumTextBytes: 2048));
Check(inspection.Counts == new StructuralCounts(9, 2, 3, 0, 1, 36), "Incorrect inspection counts.");
Check(inspection.Boundaries[1] is
    { Id: { Kind: BoundaryKind.NamedSource, Index: 0 }, Shape: BoundaryShape.ScalarUnion,
      NativeLocation: NativeLocation.JsonPointer { Pointer: "/extra_sources/0" } },
    "Incorrect named source identity.");
Check(inspection.UnresolvedResources.Count == 10, "Incorrect resource inventory.");
Check(inspection.UnresolvedResources[2] is
    { Kind: ResourceKind.SchemaReference, NativeLocation: NativeLocation.JsonPointer
      { Pointer: "/source_options/json_schema_unresolved_reference" } }, "Incorrect resource locator.");

var preflight = new ValidationLimits(new(35, 2048), 32_768, 128);
ExpectFailure(() => project.Validate(preflight), HostStatus.InspectionLimitExceeded, "inspectionItemLimit");
ExpectFailure(() => project.Inspect(new(35, 2048)), HostStatus.InspectionLimitExceeded, "inspectionItemLimit");

using var invalid = FerruleProject.Load(ledger);
ValidationReport full = invalid.Validate();
Check(!full.IsValid && !full.Truncated && full.TotalIssueCount == 2 && full.EmittedIssueCount == 2,
    "Incorrect full validation result.");
Check(full.Diagnostics[0] is
    { Owner: ValidationOwner.GraphNode { NodeId: 17, FunctionId: 9_007_199_254_741_027 },
      NativeLocation: NativeLocation.GraphNode { NodeId: 17, FunctionId: 9_007_199_254_741_027 } },
    "Graph owner lost its exact identity.");
Check(full.Diagnostics[1] is
    { Owner: ValidationOwner.UserFunction { FunctionId: 9_007_199_254_741_031 },
      NativeLocation: NativeLocation.JsonPointer { Pointer: "/document/user_functions/9007199254741031" } },
    "Function owner lost its exact identity or pointer.");
ValidationReport limited = invalid.Validate(ValidationLimits.Default with { MaximumDiagnostics = 1 });
Check(limited.Truncated && limited.TotalIssueCount == 2 && limited.EmittedIssueCount == 1
    && limited.Diagnostics[0] == full.Diagnostics[0], "Incorrect truncated validation result.");
ExpectFailure(() => invalid.Validate(ValidationLimits.Default with { MaximumIssues = 1 }),
    HostStatus.ValidationLimitExceeded, "validationIssueLimit");

Parallel.For(0, 32, _ =>
{
    using var independent = FerruleProject.Load(ledger);
    Check(independent.Validate().Diagnostics[0] == full.Diagnostics[0], "Independent project changed diagnostics.");
    Check(invalid.Inspect().Boundaries[0].Name.Text == "Ledger", "Concurrent inspection changed the project.");
});

using var racing = FerruleProject.Load(ledger);
Parallel.Invoke(
    () =>
    {
        for (int i = 0; i < 32; i++)
        {
            try
            {
                Check(racing.Validate().Diagnostics[0] == full.Diagnostics[0], "Disposal changed an active report.");
            }
            catch (ObjectDisposedException)
            {
                break;
            }
        }
    },
    racing.Dispose);
Throws<ObjectDisposedException>(() => racing.Inspect());

project.Dispose();
project.Dispose();
invalid.Dispose();
invalid.Dispose();
Throws<ObjectDisposedException>(() => project.Inspect());
Throws<ObjectDisposedException>(() => invalid.Validate());
Check(inspection.Boundaries[0].Name.Text == "Orchard" && full.Diagnostics[0].Owner is ValidationOwner.GraphNode,
    "Managed reports must remain usable after the project and native buffers are released.");

ExpectFailure(() => FerruleProject.Load("{"u8), HostStatus.DecodeFailed, "decodeFailed");
ExpectFailure(() => FerruleProject.Load([], maximumBytes: 0), HostStatus.DecodeFailed, "decodeFailed");
ExpectFailure(() => FerruleProject.Load(ledger, maximumBytes: 1),
    HostStatus.ProjectTooLarge, "projectTooLarge");
byte[] unsupported = Encoding.UTF8.GetBytes(
    Encoding.UTF8.GetString(ledger).Replace("\"version\": 2", "\"version\": 99", StringComparison.Ordinal));
ExpectFailure(() => FerruleProject.Load(unsupported),
    HostStatus.UnsupportedProjectVersion, "unsupportedProjectVersion");

Console.WriteLine(".NET load, reports, disposal, and concurrency tests passed.");

static void ExpectFailure(Action action, HostStatus status, string code)
{
    var failure = Throws<FerruleHostException>(action);
    Check(failure.Status == status && failure.Code == code && !string.IsNullOrEmpty(failure.Message),
        $"Expected {status}/{code}, got {failure.Status}/{failure.Code}.");
}

static T Throws<T>(Action action) where T : Exception
{
    try
    {
        action();
    }
    catch (T exception)
    {
        return exception;
    }

    throw new InvalidOperationException($"Expected {typeof(T).Name}.");
}

static void Check(bool condition, string message)
{
    if (!condition)
        throw new InvalidOperationException(message);
}
