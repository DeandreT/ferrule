namespace Ferrule.Generated;

public sealed record NamedInput(
    string Name,
    global::Ferrule.Runtime.FerruleInstance Instance);

public sealed record NamedOutput(
    string Name,
    global::Ferrule.Runtime.FerruleInstance Instance);

public sealed record ExecutionOutputs(
    global::Ferrule.Runtime.FerruleInstance Primary,
    global::System.Collections.Generic.IReadOnlyList<NamedOutput> Extras);

public sealed record NamedJsonInput(
    string Name,
    string Document);

public sealed record NamedJsonBytesInput(
    string Name,
    byte[] Document);

public sealed record NamedJsonOutput(
    string Name,
    string Document);

public sealed record NamedJsonBytesOutput(
    string Name,
    byte[] Document);

public sealed record JsonExecutionOutputs(
    string Primary,
    global::System.Collections.Generic.IReadOnlyList<NamedJsonOutput> Extras);

public sealed record JsonBytesExecutionOutputs(
    byte[] Primary,
    global::System.Collections.Generic.IReadOnlyList<NamedJsonBytesOutput> Extras);

public abstract class SelectedJsonTargetOutput
{
    private protected SelectedJsonTargetOutput() { }

    public sealed class Primary : SelectedJsonTargetOutput
    {
        internal Primary(string document) { Document = document; }
        public string Document { get; }
    }

    public sealed class Named : SelectedJsonTargetOutput
    {
        internal Named(NamedJsonOutput output) { Output = output; }
        public NamedJsonOutput Output { get; }
    }
}

public abstract class SelectedJsonBytesTargetOutput
{
    private protected SelectedJsonBytesTargetOutput() { }

    public sealed class Primary : SelectedJsonBytesTargetOutput
    {
        internal Primary(byte[] document) { Document = document; }
        public byte[] Document { get; }
    }

    public sealed class Named : SelectedJsonBytesTargetOutput
    {
        internal Named(NamedJsonBytesOutput output) { Output = output; }
        public NamedJsonBytesOutput Output { get; }
    }
}

public abstract class SelectedTargetOutput
{
    private protected SelectedTargetOutput() { }

    public sealed class Primary : SelectedTargetOutput
    {
        internal Primary(global::Ferrule.Runtime.FerruleInstance instance) { Instance = instance; }
        public global::Ferrule.Runtime.FerruleInstance Instance { get; }
    }

    public sealed class Named : SelectedTargetOutput
    {
        internal Named(NamedOutput output) { Output = output; }
        public NamedOutput Output { get; }
    }
}

public static class GeneratedMapping
{
    public static global::Ferrule.Runtime.FerruleInstance Execute(
        global::Ferrule.Runtime.FerruleInstance source)
    {
        return ExecuteWithSources(source, global::System.Array.Empty<NamedInput>());
    }

    public static global::Ferrule.Runtime.FerruleInstance Execute(
        global::Ferrule.Runtime.FerruleInstance source,
        global::Ferrule.Runtime.FerruleExecutionContext executionContext)
    {
        return ExecuteWithSources(source, global::System.Array.Empty<NamedInput>(), executionContext);
    }

    public static global::Ferrule.Runtime.FerruleInstance ExecuteWithSources(
        global::Ferrule.Runtime.FerruleInstance source,
        global::System.Collections.Generic.IReadOnlyList<NamedInput> extraSources)
    {
        return ExecuteOutputsWithSources(source, extraSources).Primary;
    }

    public static global::Ferrule.Runtime.FerruleInstance ExecuteWithSources(
        global::Ferrule.Runtime.FerruleInstance source,
        global::System.Collections.Generic.IReadOnlyList<NamedInput> extraSources,
        global::Ferrule.Runtime.FerruleExecutionContext executionContext)
    {
        return ExecuteOutputsWithSources(source, extraSources, executionContext).Primary;
    }

    public static global::Ferrule.Runtime.FerruleInstance ExecuteWithDynamicSourceLoader(
        global::Ferrule.Runtime.FerruleInstance source,
        global::Ferrule.Runtime.IFerruleDynamicSourceLoader loader)
    {
        return ExecuteOutputsWithDynamicSourceLoader(source, loader).Primary;
    }

    public static global::Ferrule.Runtime.FerruleInstance ExecuteWithSourcesAndDynamicSourceLoader(
        global::Ferrule.Runtime.FerruleInstance source,
        global::System.Collections.Generic.IReadOnlyList<NamedInput> extraSources,
        global::Ferrule.Runtime.IFerruleDynamicSourceLoader loader)
    {
        return ExecuteOutputsWithSourcesAndDynamicSourceLoader(source, extraSources, loader).Primary;
    }

    public static global::Ferrule.Runtime.FerruleInstance ExecuteWithSourcesContextAndDynamicSourceLoader(
        global::Ferrule.Runtime.FerruleInstance source,
        global::System.Collections.Generic.IReadOnlyList<NamedInput> extraSources,
        global::Ferrule.Runtime.FerruleExecutionContext executionContext,
        global::Ferrule.Runtime.IFerruleDynamicSourceLoader loader)
    {
        return ExecuteOutputsWithSourcesContextAndDynamicSourceLoader(
            source,
            extraSources,
            executionContext,
            loader).Primary;
    }

    public static ExecutionOutputs ExecuteOutputs(
        global::Ferrule.Runtime.FerruleInstance source)
    {
        return ExecuteOutputsWithSources(source, global::System.Array.Empty<NamedInput>());
    }

    public static ExecutionOutputs ExecuteOutputs(
        global::Ferrule.Runtime.FerruleInstance source,
        global::Ferrule.Runtime.FerruleExecutionContext executionContext)
    {
        return ExecuteOutputsWithSources(source, global::System.Array.Empty<NamedInput>(), executionContext);
    }

    public static ExecutionOutputs ExecuteOutputsWithSources(
        global::Ferrule.Runtime.FerruleInstance source,
        global::System.Collections.Generic.IReadOnlyList<NamedInput> extraSources)
    {
        return ExecuteOutputs(CreateContext(source, extraSources, null));
    }

    public static ExecutionOutputs ExecuteOutputsWithSources(
        global::Ferrule.Runtime.FerruleInstance source,
        global::System.Collections.Generic.IReadOnlyList<NamedInput> extraSources,
        global::Ferrule.Runtime.FerruleExecutionContext executionContext)
    {
        global::System.ArgumentNullException.ThrowIfNull(executionContext);
        return ExecuteOutputs(CreateContext(source, extraSources, executionContext));
    }

    public static ExecutionOutputs ExecuteOutputsWithDynamicSourceLoader(
        global::Ferrule.Runtime.FerruleInstance source,
        global::Ferrule.Runtime.IFerruleDynamicSourceLoader loader)
    {
        return ExecuteOutputsWithSourcesAndDynamicSourceLoader(
            source,
            global::System.Array.Empty<NamedInput>(),
            loader);
    }

    public static ExecutionOutputs ExecuteOutputsWithSourcesAndDynamicSourceLoader(
        global::Ferrule.Runtime.FerruleInstance source,
        global::System.Collections.Generic.IReadOnlyList<NamedInput> extraSources,
        global::Ferrule.Runtime.IFerruleDynamicSourceLoader loader)
    {
        global::System.ArgumentNullException.ThrowIfNull(loader);
        return ExecuteOutputs(CreateContext(source, extraSources, null, loader));
    }

    public static ExecutionOutputs ExecuteOutputsWithSourcesContextAndDynamicSourceLoader(
        global::Ferrule.Runtime.FerruleInstance source,
        global::System.Collections.Generic.IReadOnlyList<NamedInput> extraSources,
        global::Ferrule.Runtime.FerruleExecutionContext executionContext,
        global::Ferrule.Runtime.IFerruleDynamicSourceLoader loader)
    {
        global::System.ArgumentNullException.ThrowIfNull(executionContext);
        global::System.ArgumentNullException.ThrowIfNull(loader);
        return ExecuteOutputs(CreateContext(source, extraSources, executionContext, loader));
    }

    public static SelectedTargetOutput ExecuteSelectedTarget(
        global::Ferrule.Runtime.FerruleInstance source,
        global::Ferrule.Runtime.FerruleTargetSelection selection)
    {
        return ExecuteSelectedTargetWithHost(source, selection, global::System.Array.Empty<NamedInput>());
    }

    public static SelectedTargetOutput ExecuteSelectedTargetWithHost(
        global::Ferrule.Runtime.FerruleInstance source,
        global::Ferrule.Runtime.FerruleTargetSelection selection,
        global::System.Collections.Generic.IReadOnlyList<NamedInput> extraSources,
        global::Ferrule.Runtime.FerruleExecutionContext? executionContext = null,
        global::Ferrule.Runtime.IFerruleDynamicSourceLoader? dynamicSourceLoader = null)
    {
        global::System.ArgumentNullException.ThrowIfNull(source);
        global::System.ArgumentNullException.ThrowIfNull(selection);
        var selected = ResolveTarget(selection);
        var context = CreateContext(source, extraSources, executionContext, dynamicSourceLoader);
        switch (selected)
        {
            case 0: return new SelectedTargetOutput.Primary(Scope_0(context));
            default: throw new global::System.InvalidOperationException("Resolved target is invalid.");
        }
    }

    private static int ResolveTarget(global::Ferrule.Runtime.FerruleTargetSelection selection)
    {
        if (selection.IsPrimary) return 0;
        var name = selection.Name!;
        throw new global::Ferrule.Runtime.FerruleRuntimeException(
            global::Ferrule.Runtime.FerruleRuntimeError.UnknownTarget,
            $"project has no named target `{name}`",
            detail: name);
    }

    private static global::Ferrule.Runtime.ScopeContext CreateContext(
        global::Ferrule.Runtime.FerruleInstance source,
        global::System.Collections.Generic.IReadOnlyList<NamedInput> extraSources,
        global::Ferrule.Runtime.FerruleExecutionContext? executionContext,
        global::Ferrule.Runtime.IFerruleDynamicSourceLoader? dynamicSourceLoader = null)
    {
        global::System.ArgumentNullException.ThrowIfNull(source);
        global::System.ArgumentNullException.ThrowIfNull(extraSources);
        foreach (var extraSource in extraSources)
        {
            global::System.ArgumentNullException.ThrowIfNull(extraSource);
            global::System.ArgumentNullException.ThrowIfNull(extraSource.Name);
            global::System.ArgumentNullException.ThrowIfNull(extraSource.Instance);
            throw new global::Ferrule.Runtime.FerruleRuntimeException(
                global::Ferrule.Runtime.FerruleRuntimeError.UnexpectedNamedSource,
                $"named source '{extraSource.Name}' is not declared by this mapping",
                detail: extraSource.Name);
        }
        var context = global::Ferrule.Runtime.ScopeContext.FromSource(source, executionContext);
        return dynamicSourceLoader is null
            ? context
            : context.WithDynamicSourceLoader(dynamicSourceLoader);
    }

    private static ExecutionOutputs ExecuteOutputs(
        global::Ferrule.Runtime.ScopeContext context)
    {
        var primary = Scope_0(context);
        return new ExecutionOutputs(
            primary,
            new NamedOutput[]
            {
            });
    }

    private const string SourceJsonSchema = "{\"name\":\"Input\",\"repeating\":false,\"kind\":{\"kind\":\"group\",\"children\":[{\"name\":\"Rows\",\"repeating\":true,\"kind\":{\"kind\":\"group\",\"children\":[{\"name\":\"Start\",\"repeating\":false,\"kind\":{\"kind\":\"scalar\",\"ty\":\"bool\"}},{\"name\":\"Label\",\"repeating\":false,\"kind\":{\"kind\":\"scalar\",\"ty\":\"string\"}}]}}]}}";
    private const string TargetJsonSchema = "{\"name\":\"Output\",\"repeating\":false,\"kind\":{\"kind\":\"group\",\"children\":[{\"name\":\"Groups\",\"repeating\":true,\"kind\":{\"kind\":\"group\",\"children\":[{\"name\":\"First\",\"repeating\":false,\"kind\":{\"kind\":\"scalar\",\"ty\":\"string\"}},{\"name\":\"Position\",\"repeating\":false,\"kind\":{\"kind\":\"scalar\",\"ty\":\"int\"}},{\"name\":\"Members\",\"repeating\":true,\"kind\":{\"kind\":\"group\",\"children\":[{\"name\":\"Label\",\"repeating\":false,\"kind\":{\"kind\":\"scalar\",\"ty\":\"string\"}}]}}]}}]}}";
    private static readonly string[] ExtraTargetJsonSchemas = global::System.Array.Empty<string>();

    public static string ExecuteJson(string source)
    {
        return ExecuteJsonOutputs(source).Primary;
    }

    public static string ExecuteJson(
        string source,
        global::Ferrule.Runtime.FerruleExecutionContext executionContext)
    {
        return ExecuteJsonOutputs(source, executionContext).Primary;
    }

    public static string ExecuteJsonWithSources(
        string source,
        global::System.Collections.Generic.IReadOnlyList<NamedJsonInput> extraSources)
    {
        return ExecuteJsonOutputsWithSources(source, extraSources).Primary;
    }

    public static string ExecuteJsonWithSources(
        string source,
        global::System.Collections.Generic.IReadOnlyList<NamedJsonInput> extraSources,
        global::Ferrule.Runtime.FerruleExecutionContext executionContext)
    {
        return ExecuteJsonOutputsWithSources(source, extraSources, executionContext).Primary;
    }

    public static JsonExecutionOutputs ExecuteJsonOutputs(string source)
    {
        return ExecuteJsonOutputsWithSources(source, global::System.Array.Empty<NamedJsonInput>());
    }

    public static JsonExecutionOutputs ExecuteJsonOutputs(
        string source,
        global::Ferrule.Runtime.FerruleExecutionContext executionContext)
    {
        return ExecuteJsonOutputsWithSources(
            source,
            global::System.Array.Empty<NamedJsonInput>(),
            executionContext);
    }

    public static JsonExecutionOutputs ExecuteJsonOutputsWithSources(
        string source,
        global::System.Collections.Generic.IReadOnlyList<NamedJsonInput> extraSources)
    {
        global::System.ArgumentNullException.ThrowIfNull(source);
        global::System.ArgumentNullException.ThrowIfNull(extraSources);
        ValidateNamedJsonInputNames(extraSources);
        var parsedSource = global::Ferrule.Runtime.FerruleJson.ParseEmbedded(SourceJsonSchema, source);
        var parsedInputs = ParseNamedJsonInputs(extraSources);
        return SerializeJsonOutputs(ExecuteOutputsWithSources(parsedSource, parsedInputs));
    }

    public static JsonExecutionOutputs ExecuteJsonOutputsWithSources(
        string source,
        global::System.Collections.Generic.IReadOnlyList<NamedJsonInput> extraSources,
        global::Ferrule.Runtime.FerruleExecutionContext executionContext)
    {
        global::System.ArgumentNullException.ThrowIfNull(source);
        global::System.ArgumentNullException.ThrowIfNull(extraSources);
        global::System.ArgumentNullException.ThrowIfNull(executionContext);
        ValidateNamedJsonInputNames(extraSources);
        var parsedSource = global::Ferrule.Runtime.FerruleJson.ParseEmbedded(SourceJsonSchema, source);
        var parsedInputs = ParseNamedJsonInputs(extraSources);
        return SerializeJsonOutputs(ExecuteOutputsWithSources(
            parsedSource,
            parsedInputs,
            executionContext));
    }

    public static byte[] ExecuteJsonBytes(byte[] source)
    {
        return ExecuteJsonBytesOutputs(source).Primary;
    }

    public static byte[] ExecuteJsonBytes(
        byte[] source,
        global::Ferrule.Runtime.FerruleExecutionContext executionContext)
    {
        return ExecuteJsonBytesOutputs(source, executionContext).Primary;
    }

    public static byte[] ExecuteJsonBytesWithSources(
        byte[] source,
        global::System.Collections.Generic.IReadOnlyList<NamedJsonBytesInput> extraSources)
    {
        return ExecuteJsonBytesOutputsWithSources(source, extraSources).Primary;
    }

    public static byte[] ExecuteJsonBytesWithSources(
        byte[] source,
        global::System.Collections.Generic.IReadOnlyList<NamedJsonBytesInput> extraSources,
        global::Ferrule.Runtime.FerruleExecutionContext executionContext)
    {
        return ExecuteJsonBytesOutputsWithSources(source, extraSources, executionContext).Primary;
    }

    public static JsonBytesExecutionOutputs ExecuteJsonBytesOutputs(byte[] source)
    {
        return ExecuteJsonBytesOutputsWithSources(
            source,
            global::System.Array.Empty<NamedJsonBytesInput>());
    }

    public static JsonBytesExecutionOutputs ExecuteJsonBytesOutputs(
        byte[] source,
        global::Ferrule.Runtime.FerruleExecutionContext executionContext)
    {
        return ExecuteJsonBytesOutputsWithSources(
            source,
            global::System.Array.Empty<NamedJsonBytesInput>(),
            executionContext);
    }

    public static JsonBytesExecutionOutputs ExecuteJsonBytesOutputsWithSources(
        byte[] source,
        global::System.Collections.Generic.IReadOnlyList<NamedJsonBytesInput> extraSources)
    {
        global::System.ArgumentNullException.ThrowIfNull(source);
        global::System.ArgumentNullException.ThrowIfNull(extraSources);
        ValidateNamedJsonBytesInputNames(extraSources);
        var parsedSource =
            global::Ferrule.Runtime.FerruleJson.ParseEmbeddedBytes(SourceJsonSchema, source);
        var parsedInputs = ParseNamedJsonBytesInputs(extraSources);
        return SerializeJsonBytesOutputs(
            ExecuteOutputsWithSources(parsedSource, parsedInputs));
    }

    public static JsonBytesExecutionOutputs ExecuteJsonBytesOutputsWithSources(
        byte[] source,
        global::System.Collections.Generic.IReadOnlyList<NamedJsonBytesInput> extraSources,
        global::Ferrule.Runtime.FerruleExecutionContext executionContext)
    {
        global::System.ArgumentNullException.ThrowIfNull(source);
        global::System.ArgumentNullException.ThrowIfNull(extraSources);
        global::System.ArgumentNullException.ThrowIfNull(executionContext);
        ValidateNamedJsonBytesInputNames(extraSources);
        var parsedSource =
            global::Ferrule.Runtime.FerruleJson.ParseEmbeddedBytes(SourceJsonSchema, source);
        var parsedInputs = ParseNamedJsonBytesInputs(extraSources);
        return SerializeJsonBytesOutputs(ExecuteOutputsWithSources(
            parsedSource,
            parsedInputs,
            executionContext));
    }

    private static void ValidateNamedJsonInputNames(
        global::System.Collections.Generic.IReadOnlyList<NamedJsonInput> extraSources)
    {
        foreach (var extraSource in extraSources)
        {
            global::System.ArgumentNullException.ThrowIfNull(extraSource);
            global::System.ArgumentNullException.ThrowIfNull(extraSource.Name);
            throw new global::Ferrule.Runtime.FerruleRuntimeException(
                global::Ferrule.Runtime.FerruleRuntimeError.UnexpectedNamedSource,
                $"named source '{extraSource.Name}' is not declared by this mapping",
                detail: extraSource.Name);
        }
    }

    private static void ValidateNamedJsonBytesInputNames(
        global::System.Collections.Generic.IReadOnlyList<NamedJsonBytesInput> extraSources)
    {
        foreach (var extraSource in extraSources)
        {
            global::System.ArgumentNullException.ThrowIfNull(extraSource);
            global::System.ArgumentNullException.ThrowIfNull(extraSource.Name);
            throw new global::Ferrule.Runtime.FerruleRuntimeException(
                global::Ferrule.Runtime.FerruleRuntimeError.UnexpectedNamedSource,
                $"named source '{extraSource.Name}' is not declared by this mapping",
                detail: extraSource.Name);
        }
    }

    private static global::System.Collections.Generic.IReadOnlyList<NamedInput> ParseNamedJsonInputs(
        global::System.Collections.Generic.IReadOnlyList<NamedJsonInput> extraSources)
    {
        foreach (var extraSource in extraSources)
        {
            global::System.ArgumentNullException.ThrowIfNull(extraSource);
            global::System.ArgumentNullException.ThrowIfNull(extraSource.Name);
            throw new global::Ferrule.Runtime.FerruleRuntimeException(
                global::Ferrule.Runtime.FerruleRuntimeError.UnexpectedNamedSource,
                $"named source '{extraSource.Name}' is not declared by this mapping",
                detail: extraSource.Name);
        }
        return global::System.Array.Empty<NamedInput>();
    }

    private static global::System.Collections.Generic.IReadOnlyList<NamedInput> ParseNamedJsonBytesInputs(
        global::System.Collections.Generic.IReadOnlyList<NamedJsonBytesInput> extraSources)
    {
        foreach (var extraSource in extraSources)
        {
            global::System.ArgumentNullException.ThrowIfNull(extraSource);
            global::System.ArgumentNullException.ThrowIfNull(extraSource.Name);
            throw new global::Ferrule.Runtime.FerruleRuntimeException(
                global::Ferrule.Runtime.FerruleRuntimeError.UnexpectedNamedSource,
                $"named source '{extraSource.Name}' is not declared by this mapping",
                detail: extraSource.Name);
        }
        return global::System.Array.Empty<NamedInput>();
    }

    private static JsonExecutionOutputs SerializeJsonOutputs(ExecutionOutputs outputs)
    {
        if (outputs.Extras.Count != ExtraTargetJsonSchemas.Length)
        {
            throw new global::Ferrule.Runtime.FerruleRuntimeException(
                global::Ferrule.Runtime.FerruleRuntimeError.JsonBoundary,
                "generated mapping returned an unexpected number of named targets",
                detail: "named target count");
        }
        var extras = new global::System.Collections.Generic.List<NamedJsonOutput>(outputs.Extras.Count);
        for (var index = 0; index < outputs.Extras.Count; index++)
        {
            var extra = outputs.Extras[index];
            extras.Add(new NamedJsonOutput(
                extra.Name,
                global::Ferrule.Runtime.FerruleJson.SerializeEmbedded(
                    ExtraTargetJsonSchemas[index],
                    extra.Instance)));
        }
        return new JsonExecutionOutputs(
            global::Ferrule.Runtime.FerruleJson.SerializeEmbedded(TargetJsonSchema, outputs.Primary),
            extras);
    }

    private static JsonBytesExecutionOutputs SerializeJsonBytesOutputs(ExecutionOutputs outputs)
    {
        if (outputs.Extras.Count != ExtraTargetJsonSchemas.Length)
        {
            throw new global::Ferrule.Runtime.FerruleRuntimeException(
                global::Ferrule.Runtime.FerruleRuntimeError.JsonBoundary,
                "generated mapping returned an unexpected number of named targets",
                detail: "named target count");
        }
        var extras = new global::System.Collections.Generic.List<NamedJsonBytesOutput>(outputs.Extras.Count);
        for (var index = 0; index < outputs.Extras.Count; index++)
        {
            var extra = outputs.Extras[index];
            extras.Add(new NamedJsonBytesOutput(
                extra.Name,
                global::Ferrule.Runtime.FerruleJson.SerializeEmbeddedBytes(
                    ExtraTargetJsonSchemas[index],
                    extra.Instance)));
        }
        return new JsonBytesExecutionOutputs(
            global::Ferrule.Runtime.FerruleJson.SerializeEmbeddedBytes(
                TargetJsonSchema,
                outputs.Primary),
            extras);
    }

    public static SelectedJsonTargetOutput ExecuteJsonSelectedTarget(
        string source,
        global::Ferrule.Runtime.FerruleTargetSelection selection)
    {
        return ExecuteJsonSelectedTargetWithHost(source, selection, global::System.Array.Empty<NamedJsonInput>());
    }

    public static SelectedJsonTargetOutput ExecuteJsonSelectedTargetWithHost(
        string source,
        global::Ferrule.Runtime.FerruleTargetSelection selection,
        global::System.Collections.Generic.IReadOnlyList<NamedJsonInput> extraSources,
        global::Ferrule.Runtime.FerruleExecutionContext? executionContext = null,
        global::Ferrule.Runtime.IFerruleDynamicJsonSourceLoader? loader = null)
    {
        global::System.ArgumentNullException.ThrowIfNull(source);
        global::System.ArgumentNullException.ThrowIfNull(selection);
        var selected = ResolveTarget(selection);
        global::System.ArgumentNullException.ThrowIfNull(extraSources);
        ValidateNamedJsonInputNames(extraSources);
        var parsedSource = global::Ferrule.Runtime.FerruleJson.ParseEmbedded(SourceJsonSchema, source);
        var parsedInputs = ParseNamedJsonInputs(extraSources);
        global::Ferrule.Runtime.IFerruleDynamicSourceLoader? adapter = null;
        var outcome = ExecuteSelectedTargetWithHost(
            parsedSource, selection, parsedInputs, executionContext, adapter);
        return SerializeSelectedJsonTargetOutput(outcome, selected);
    }

    private static SelectedJsonTargetOutput SerializeSelectedJsonTargetOutput(SelectedTargetOutput outcome, int selected)
    {
        switch (selected)
        {
            case 0 when outcome is SelectedTargetOutput.Primary primary:
                return new SelectedJsonTargetOutput.Primary(
                    global::Ferrule.Runtime.FerruleJson.SerializeEmbedded(TargetJsonSchema, primary.Instance));
            default:
                throw new global::Ferrule.Runtime.FerruleRuntimeException(
                    global::Ferrule.Runtime.FerruleRuntimeError.JsonBoundary,
                    "generated mapping returned a target that does not match the resolved selection");
        }
    }

    public static SelectedJsonBytesTargetOutput ExecuteJsonBytesSelectedTarget(
        byte[] source,
        global::Ferrule.Runtime.FerruleTargetSelection selection)
    {
        return ExecuteJsonBytesSelectedTargetWithHost(source, selection, global::System.Array.Empty<NamedJsonBytesInput>());
    }

    public static SelectedJsonBytesTargetOutput ExecuteJsonBytesSelectedTargetWithHost(
        byte[] source,
        global::Ferrule.Runtime.FerruleTargetSelection selection,
        global::System.Collections.Generic.IReadOnlyList<NamedJsonBytesInput> extraSources,
        global::Ferrule.Runtime.FerruleExecutionContext? executionContext = null,
        global::Ferrule.Runtime.IFerruleDynamicJsonSourceLoader? loader = null)
    {
        global::System.ArgumentNullException.ThrowIfNull(source);
        global::System.ArgumentNullException.ThrowIfNull(selection);
        var selected = ResolveTarget(selection);
        global::System.ArgumentNullException.ThrowIfNull(extraSources);
        ValidateNamedJsonBytesInputNames(extraSources);
        var parsedSource = global::Ferrule.Runtime.FerruleJson.ParseEmbeddedBytes(SourceJsonSchema, source);
        var parsedInputs = ParseNamedJsonBytesInputs(extraSources);
        global::Ferrule.Runtime.IFerruleDynamicSourceLoader? adapter = null;
        var outcome = ExecuteSelectedTargetWithHost(
            parsedSource, selection, parsedInputs, executionContext, adapter);
        return SerializeSelectedJsonBytesTargetOutput(outcome, selected);
    }

    private static SelectedJsonBytesTargetOutput SerializeSelectedJsonBytesTargetOutput(SelectedTargetOutput outcome, int selected)
    {
        switch (selected)
        {
            case 0 when outcome is SelectedTargetOutput.Primary primary:
                return new SelectedJsonBytesTargetOutput.Primary(
                    global::Ferrule.Runtime.FerruleJson.SerializeEmbeddedBytes(TargetJsonSchema, primary.Instance));
            default:
                throw new global::Ferrule.Runtime.FerruleRuntimeException(
                    global::Ferrule.Runtime.FerruleRuntimeError.JsonBoundary,
                    "generated mapping returned a target that does not match the resolved selection");
        }
    }

    private static global::Ferrule.Runtime.FerruleValue Node_1(
        global::Ferrule.Runtime.ScopeContext context) =>
        context.ResolveSourceFieldInFrame(new string[] { "Rows" }, new string[] { "Label" });

    private static global::Ferrule.Runtime.FerruleValue Node_2(
        global::Ferrule.Runtime.ScopeContext context) =>
        context.ResolveSourceFieldInFrame(new string[] { "Rows" }, new string[] { "Start" });

    private static global::Ferrule.Runtime.FerruleValue Node_3(
        global::Ferrule.Runtime.ScopeContext context) =>
        global::Ferrule.Runtime.FerruleValue.FromInt64(context.Position(new string[] { "Rows" }));

    private static global::Ferrule.Runtime.FerruleInstance Scope_0(
        global::Ferrule.Runtime.ScopeContext context)
    {
        var item_0 = ScopeItem_0(context);
        return item_0;
    }

    private static global::Ferrule.Runtime.FerruleInstance ScopeItem_0(
        global::Ferrule.Runtime.ScopeContext context)
    {
        var fields_0 = new global::System.Collections.Generic.List<global::Ferrule.Runtime.FerruleField>
        {
        };
        fields_0.Add(new global::Ferrule.Runtime.FerruleField("Groups", Scope_1(context)));
        var group_0 = new global::Ferrule.Runtime.FerruleGroup(fields_0);
        return group_0;
    }

    private static global::Ferrule.Runtime.FerruleInstance Scope_1(
        global::Ferrule.Runtime.ScopeContext context)
    {
        var candidates_1 = new global::System.Collections.Generic.List<global::Ferrule.Runtime.ScopeContext>(context.IterateSource(new string[] { "Rows" }));
        candidates_1 = new global::System.Collections.Generic.List<global::Ferrule.Runtime.ScopeContext>(context.GroupStartingWith(candidates_1, new string[] { "Rows" }, candidate_1 => global::Ferrule.Runtime.FerruleFunctions.RequireBoolean(Node_2(candidate_1), 2U)));
        var items_1 = new global::System.Collections.Generic.List<global::Ferrule.Runtime.FerruleInstance>();
        foreach (var item_context_1 in candidates_1)
        {
            var output_context_1 = item_context_1.WithCompactedPosition(items_1.Count + 1);
            items_1.Add(ScopeItem_1(output_context_1));
        }
        return new global::Ferrule.Runtime.FerruleRepeated(items_1);
    }

    private static global::Ferrule.Runtime.FerruleInstance ScopeItem_1(
        global::Ferrule.Runtime.ScopeContext context)
    {
        var value_1_0 = Node_1(context);
        var value_1_1 = Node_3(context);
        var fields_1 = new global::System.Collections.Generic.List<global::Ferrule.Runtime.FerruleField>
        {
            new global::Ferrule.Runtime.FerruleField("First", TargetBuilder.Scalar(value_1_0, TargetScalarDomain.String)),
            new global::Ferrule.Runtime.FerruleField("Position", TargetBuilder.Scalar(value_1_1, TargetScalarDomain.Int64)),
        };
        fields_1.Add(new global::Ferrule.Runtime.FerruleField("Members", Scope_2(context)));
        var group_1 = new global::Ferrule.Runtime.FerruleGroup(fields_1);
        return group_1;
    }

    private static global::Ferrule.Runtime.FerruleInstance Scope_2(
        global::Ferrule.Runtime.ScopeContext context)
    {
        var candidates_2 = new global::System.Collections.Generic.List<global::Ferrule.Runtime.ScopeContext>(context.IterateSource(global::System.Array.Empty<string>()));
        var items_2 = new global::System.Collections.Generic.List<global::Ferrule.Runtime.FerruleInstance>();
        foreach (var item_context_2 in candidates_2)
        {
            items_2.Add(ScopeItem_2(item_context_2));
        }
        return new global::Ferrule.Runtime.FerruleRepeated(items_2);
    }

    private static global::Ferrule.Runtime.FerruleInstance ScopeItem_2(
        global::Ferrule.Runtime.ScopeContext context)
    {
        var value_2_0 = Node_1(context);
        var fields_2 = new global::System.Collections.Generic.List<global::Ferrule.Runtime.FerruleField>
        {
            new global::Ferrule.Runtime.FerruleField("Label", TargetBuilder.Scalar(value_2_0, TargetScalarDomain.String)),
        };
        var group_2 = new global::Ferrule.Runtime.FerruleGroup(fields_2);
        return group_2;
    }
}
