using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;
using System.Text.Json;
using System.Text.Json.Serialization.Metadata;
using Microsoft.Win32.SafeHandles;

namespace Ferrule.Host;

internal static partial class NativeMethods
{
    internal const string LibraryName = "ferrule_host_ffi";

    [LibraryImport(LibraryName, EntryPoint = "ferrule_host_abi_version")]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    internal static partial uint AbiVersion();

    [LibraryImport(LibraryName, EntryPoint = "ferrule_host_v1_load")]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    internal static unsafe partial HostStatus Load(
        byte* bytes, nuint length, uint maximumBytes, out ProjectHandle project, out BufferHandle error);

    [LibraryImport(LibraryName, EntryPoint = "ferrule_host_v1_inspect")]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    internal static partial HostStatus Inspect(
        ProjectHandle project, uint maximumItems, uint maximumTextBytes, out BufferHandle result);

    [LibraryImport(LibraryName, EntryPoint = "ferrule_host_v1_validate")]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    internal static partial HostStatus Validate(
        ProjectHandle project, uint maximumItems, uint maximumTextBytes,
        uint maximumIssues, uint maximumDiagnostics, out BufferHandle result);

    [LibraryImport(LibraryName, EntryPoint = "ferrule_host_v1_buffer_data")]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    internal static unsafe partial byte* BufferData(nint buffer, out nuint length);

    [LibraryImport(LibraryName, EntryPoint = "ferrule_host_v1_buffer_free")]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    internal static partial void BufferFree(nint buffer);

    [LibraryImport(LibraryName, EntryPoint = "ferrule_host_v1_project_free")]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    internal static partial void ProjectFree(nint project);
}

internal sealed class ProjectHandle : SafeHandleZeroOrMinusOneIsInvalid
{
    public ProjectHandle() : base(ownsHandle: true) { }

    protected override bool ReleaseHandle()
    {
        NativeMethods.ProjectFree(handle);
        return true;
    }
}

internal sealed class BufferHandle : SafeHandleZeroOrMinusOneIsInvalid
{
    public BufferHandle() : base(ownsHandle: true) { }

    internal unsafe T Read<T>(JsonTypeInfo<T> type) where T : class
    {
        bool retained = false;
        try
        {
            DangerousAddRef(ref retained);
            byte* bytes = NativeMethods.BufferData(DangerousGetHandle(), out nuint length);
            if (bytes == null || length == 0 || length > int.MaxValue)
                throw new InvalidDataException("Ferrule returned an invalid result buffer.");

            return JsonSerializer.Deserialize(new ReadOnlySpan<byte>(bytes, (int)length), type)
                ?? throw new InvalidDataException("Ferrule returned an empty JSON result.");
        }
        finally
        {
            if (retained)
                DangerousRelease();
        }
    }

    protected override bool ReleaseHandle()
    {
        NativeMethods.BufferFree(handle);
        return true;
    }
}
