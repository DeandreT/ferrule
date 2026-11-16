namespace Ferrule.Host;

public enum HostStatus : uint
{
    Ok = 0,
    InvalidArgument = 1,
    DecodeFailed = 2,
    UnsupportedProjectVersion = 3,
    ProjectTooLarge = 4,
    InspectionLimitExceeded = 5,
    ValidationLimitExceeded = 6,
    InternalError = 7,
}

public sealed class FerruleHostException : Exception
{
    internal FerruleHostException(HostStatus status, NativeError error) : base(error.Message)
    {
        Status = status;
        Code = error.Code;
    }

    public HostStatus Status { get; }
    public string Code { get; }
}

public sealed class FerruleAbiMismatchException : NotSupportedException
{
    internal FerruleAbiMismatchException(uint expected, uint actual)
        : base($"Ferrule host ABI {expected} is required. The loaded library reports ABI {actual}.")
    {
        ExpectedVersion = expected;
        ActualVersion = actual;
    }

    public uint ExpectedVersion { get; }
    public uint ActualVersion { get; }
}

/// <summary>Owns a read-only native project. Inspection and validation may run concurrently.</summary>
public sealed class FerruleProject : IDisposable
{
    public const uint RequiredAbiVersion = 1;
    public const uint DefaultMaximumProjectBytes = 4 * 1024 * 1024;

    private static readonly Lazy<uint> AbiVersion = new(NativeMethods.AbiVersion);
    private readonly ProjectHandle handle;
    private int disposed;

    private FerruleProject(ProjectHandle handle) => this.handle = handle;

    /// <summary>Loads the exact native project bytes. The caller retains ownership of those bytes.</summary>
    public static unsafe FerruleProject Load(
        ReadOnlySpan<byte> projectBytes, uint maximumBytes = DefaultMaximumProjectBytes)
    {
        uint actualVersion = AbiVersion.Value;
        if (actualVersion != RequiredAbiVersion)
            throw new FerruleAbiMismatchException(RequiredAbiVersion, actualVersion);

        fixed (byte* bytes = projectBytes)
        {
            HostStatus status = NativeMethods.Load(
                bytes, (nuint)projectBytes.Length, maximumBytes, out ProjectHandle project, out BufferHandle error);
            using (error)
            {
                try
                {
                    if (status != HostStatus.Ok)
                        throw ReadFailure(status, error);
                    if (project.IsInvalid)
                        throw new InvalidDataException("Ferrule returned no project handle.");
                    return new FerruleProject(project);
                }
                catch
                {
                    project.Dispose();
                    throw;
                }
            }
        }
    }

    public InspectionReport Inspect(InspectionLimits? limits = null)
    {
        ThrowIfDisposed();
        InspectionLimits admitted = limits ?? InspectionLimits.Default;
        HostStatus status = NativeMethods.Inspect(
            handle, admitted.MaximumItems, admitted.MaximumTextBytes, out BufferHandle response);
        using (response)
        {
            if (status != HostStatus.Ok)
                throw ReadFailure(status, response);
            return response.Read(NativeJsonContext.Default.InspectionReport);
        }
    }

    public ValidationReport Validate(ValidationLimits? limits = null)
    {
        ThrowIfDisposed();
        ValidationLimits admitted = limits ?? ValidationLimits.Default;
        HostStatus status = NativeMethods.Validate(
            handle, admitted.Inspection.MaximumItems, admitted.Inspection.MaximumTextBytes,
            admitted.MaximumIssues, admitted.MaximumDiagnostics, out BufferHandle response);
        using (response)
        {
            if (status != HostStatus.Ok)
                throw ReadFailure(status, response);
            return response.Read(NativeJsonContext.Default.ValidationReport);
        }
    }

    public void Dispose()
    {
        if (Interlocked.Exchange(ref disposed, 1) == 0)
            handle.Dispose();
    }

    private void ThrowIfDisposed() =>
        ObjectDisposedException.ThrowIf(Volatile.Read(ref disposed) != 0, this);

    private static FerruleHostException ReadFailure(HostStatus status, BufferHandle response)
    {
        NativeError error = response.IsInvalid
            ? new("nativeFailure", $"Ferrule returned status {status} without an error buffer.")
            : response.Read(NativeJsonContext.Default.NativeError);
        return new(status, error);
    }
}
