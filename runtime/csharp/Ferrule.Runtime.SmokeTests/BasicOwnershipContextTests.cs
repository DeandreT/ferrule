using Ferrule.Runtime;

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private static FerruleOwnedSourceLimits BasicLimits => new(32, 16, 64, 1_048_576, 128);
    private static FerruleBasicScopeContext BasicRoot(FerruleInstance primary,
        params FerruleDeclaredNamedSourceRoot[] declared) =>
        BasicReached("F03.chained.1", () => BasicReached("F03.subject.1", () => FerruleBasicSourceRoots.Create(primary, declared, BasicLimits)).RootContext());
    private static FerruleGroup BasicValue(string value) => Group(Field("Value", Scalar(Text(value))));
    private static FerruleBasicScopeContext BasicCommon()
    {
        var primary = Group(
            Field("Value", Scalar(Text("P.root"))),
            Field("Ref", Group(Field("Value", Scalar(Text("P.Ref"))),
                Field("Rows", Repeated(BasicValue("P.Ref.1"))))),
            Field("Details", Group(Field("Counter", Scalar(FerruleValue.FromInt64(99))))),
            Field("Rows", Repeated(
                Group(Field("Value", Scalar(Text("P1"))),
                    Field("Rows", Repeated(BasicValue("P.inner11"), BasicValue("P.inner12"))),
                    Field("Details", Group(Field("Counter", Scalar(FerruleValue.FromInt64(7)))))),
                Group(Field("Value", Scalar(Text("P2"))),
                    Field("Rows", Repeated(BasicValue("P.inner21"))),
                    Field("Details", Scalar(FerruleValue.Null))),
                Group(Field("Value", Scalar(Text("P3"))), Field("Rows", Repeated())))));
        return BasicRoot(primary,
            new("Ref", Group(Field("Value", Scalar(Text("N.root"))),
                Field("Rows", Repeated(BasicValue("N1"), BasicValue("N2"), BasicValue("N3"), BasicValue("N4"))))),
            new("primary", BasicValue("named.literal.primary")),
            new(" Ref 雪 ", BasicValue("named.literal.whitespace.unicode")),
            new("ref", BasicValue("named.literal.lowercase")));
    }
    private static FerruleBasicScopeContext BasicGaps()
    {
        return BasicRoot(BasicValue("P.root"),
            new("Missing", null),
            new("Ref", Group(Field("Value", Scalar(Text("N.root"))),
                Field("Rows", Repeated(BasicValue("N1"), BasicValue("N2"))))),
            new("Dup", null), new("Dup", BasicValue("D1")), new("Gap", null),
            new("Other", BasicValue("O")), new("Dup", BasicValue("D2")));
    }
    // Observation only: capture reached native payloads before assertions or the next subject call.
    private static object BasicValueSnapshot(FerruleValue value) => new
    {
        kind = value.Kind.ToString(),
        value = value.Kind switch
        {
            FerruleValueKind.String => (object?)value.StringValue,
            FerruleValueKind.Int64 => value.Int64Value,
            FerruleValueKind.Bool => value.BooleanValue,
            FerruleValueKind.Double => BitConverter.DoubleToInt64Bits(value.DoubleValue),
            _ => null,
        },
        doubleRepresentation = value.Kind == FerruleValueKind.Double ? "IEEE754_bits" : null,
    };
    private static object? BasicInstanceSnapshot(FerruleInstance? value) => value switch
    {
        null => null,
        FerruleScalar scalar => new { kind = "Scalar", value = BasicValueSnapshot(scalar.Value) },
        FerruleGroup group => new { kind = "Group", fields = group.Fields.Select(field =>
            new { field.Name, value = BasicInstanceSnapshot(field.Value) }).ToArray() },
        FerruleRepeated repeated => new { kind = "Repeated", items = repeated.Items.Select(BasicInstanceSnapshot).ToArray() },
        _ => new { kind = value.GetType().FullName },
    };
    private static object BasicContextSnapshot(FerruleBasicScopeContext context) => new
    {
        legacyFrames = context.LegacyView.Frames.Select(BasicInstanceSnapshot).ToArray(),
        owned = Enumerable.Range(0, context.OwnedView.FrameCount).Select(i =>
        {
            var frame = context.OwnedView.FrameAt(i);
            return new { ownerKey = frame.Owner, name = context.Roots.OwnedRoots.Name(frame.Owner),
                path = frame.Path.ToArray(), index = (ulong)frame.Index, item = BasicInstanceSnapshot(frame.Item) };
        }).ToArray(),
        origins = Enumerable.Range(0, context.OriginCount).Select(i =>
        {
            var frame = context.OriginAt(i);
            return new { key = frame.OwnerKey, path = frame.Canonical.ToArray(),
                raw = frame.RawOrdinal.HasValue ? (ulong?)frame.RawOrdinal.Value : null, projection = frame.Projection };
        }).ToArray(),
        legacy = Enumerable.Range(0, context.LegacyView.BasicCollectionCount).Select(i => new
        { path = context.LegacyView.BasicCollectionPath(i).ToArray(), index = context.LegacyView.BasicCollectionIndex(i) }).ToArray(),
        runStateType = context.LegacyView.BasicRunStateIdentity.GetType().FullName,
    };
    private static object BasicCandidateSnapshot(FerruleBasicSourceCandidate candidate) => new
    {
        sourceId = (ulong)candidate.SourceId,
        raw = candidate.RawSourceOrdinal.HasValue ? (ulong?)candidate.RawSourceOrdinal.Value : null,
        cursor = candidate.HasNewCursor, context = BasicContextSnapshot(candidate.Context),
    };
    private static object? BasicReachedSnapshot(object? value) => value switch
    {
        null => null,
        nuint index => (ulong)index,
        FerruleValue scalar => BasicValueSnapshot(scalar),
        FerruleBasicSourceCandidate candidate => BasicCandidateSnapshot(candidate),
        FerruleBasicScopeContext context => BasicContextSnapshot(context),
        IReadOnlyList<FerruleBasicSourceCandidate> rows => new
        { count = rows.Count, rows = rows.Select(BasicCandidateSnapshot).ToArray() },
        FerruleBasicSourceRoots roots => new
        {
            count = (ulong)roots.OwnedRoots.RegistryCount,
            declarations = Enumerable.Range(0, (int)roots.OwnedRoots.RegistryCount).Select(i => new
            { key = i, name = roots.OwnedRoots.Name(i), source = BasicInstanceSnapshot(roots.OwnedRoots.Instance(i)) }).ToArray(),
            supplied = Enumerable.Range(0, roots.SuppliedCount).Select(roots.SuppliedIndex).ToArray(),
            unique = Enumerable.Range(0, roots.UniqueCount).Select(roots.UniqueIndex).ToArray(),
        },
        FerrulePhaseSelection selection => new
        { count = selection.Count, indices = Enumerable.Range(0, selection.Count).Select(i => (ulong)selection.Index(i)).ToArray(),
          current = BasicReachedSnapshot(selection.Current) },
        FerruleInstance instance => BasicInstanceSnapshot(instance),
        Exception error => BasicErrorSnapshot(error),
        Enum tag => new { name = tag.ToString(), numeric = Convert.ToInt64(tag) },
        _ => value,
    };
    private static object BasicErrorSnapshot(Exception error)
    {
        var outer = error as FerruleRuntimeException;
        var owned = error as FerruleOwnedSourceException;
        var bridge = error as FerruleBasicSourceBridgeException;
        var traversal = error as FerruleOwnedSourceTraversalException;
        return new
        {
            type = error.GetType().FullName, error.Message, error.HResult, fullNative = error.ToString(),
            argument = (error as ArgumentException)?.ParamName,
            runtime = outer is null ? null : new
            {
                category = outer.Error.ToString(), outer.Node, outer.Function, outer.ExpectedArity, outer.ActualArity,
                foundKind = outer.FoundKind?.ToString(), aggregate = outer.AggregateOperation?.ToString(), outer.Detail,
                requestedItems = outer.RequestedItems?.ToString(), maximumItems = outer.MaximumItems?.ToString(), outer.MaximumDepth,
                runtimeValue = outer.RuntimeValue?.ToString(), outer.FailureRule, outer.MappingFailureMessage,
                outer.MappingExceptionMessage, outer.Join, outer.UserFunction, outer.FunctionParameter,
                expectedScalar = outer.ExpectedScalarType?.ToString(), outer.RuntimeParameter, outer.SourceField,
                outer.FoundInstance, outer.PrimaryRoot,
                ownedSameInner = outer.OwnedSourceFailure is not null && ReferenceEquals(outer.OwnedSourceFailure, outer.InnerException),
                bridgeSameInner = outer.BasicSourceBridgeFailure is not null && ReferenceEquals(outer.BasicSourceBridgeFailure, outer.InnerException),
                owned = outer.OwnedSourceFailure is null ? null : BasicErrorSnapshot(outer.OwnedSourceFailure),
                bridge = outer.BasicSourceBridgeFailure is null ? null : BasicErrorSnapshot(outer.BasicSourceBridgeFailure),
            },
            owned = owned is null ? null : new
            {
                kind = owned.Error.ToString(), owner = owned.Owner?.Name, primary = owned.Owner?.IsPrimary,
                name = owned.RequestedName, declarations = owned.DeclarationIndices?.ToArray(),
                path = owned.Path?.ToArray(), frame = owned.Frame?.ToArray(), collection = owned.Collection?.ToArray(),
                segment = owned.Segment.HasValue ? (ulong?)owned.Segment.Value : null,
                found = owned.FoundInstance?.ToString(), index = owned.RequestedIndex.HasValue ? (ulong?)owned.RequestedIndex.Value : null,
                limit = owned.LimitKind?.ToString(), used = owned.Used.HasValue ? (ulong?)owned.Used.Value : null,
                requested = owned.Requested.HasValue ? (ulong?)owned.Requested.Value : null,
                maximum = owned.Maximum.HasValue ? (ulong?)owned.Maximum.Value : null,
            },
            bridge = bridge is null ? null : new
            {
                kind = bridge.Error.ToString(), component = bridge.Component?.ToString(),
                used = bridge.Used.HasValue ? (ulong?)bridge.Used.Value : null,
                requested = bridge.Requested.HasValue ? (ulong?)bridge.Requested.Value : null,
                maximum = bridge.Maximum.HasValue ? (ulong?)bridge.Maximum.Value : null,
                input = bridge.InputOrdinal.HasValue ? (ulong?)bridge.InputOrdinal.Value : null,
                candidate = bridge.CandidateOrdinal.HasValue ? (ulong?)bridge.CandidateOrdinal.Value : null,
                declaration = bridge.DeclarationIndex.HasValue ? (ulong?)bridge.DeclarationIndex.Value : null,
                declared = bridge.DeclaredCount.HasValue ? (ulong?)bridge.DeclaredCount.Value : null,
                found = bridge.Found?.ToString(),
            },
            traversal = traversal is null ? null : new
            { kind = traversal.Error.ToString(), path = traversal.Path.ToArray(),
              segment = traversal.Segment.HasValue ? (ulong?)traversal.Segment.Value : null, found = traversal.Found.ToString() },
            inner = error.InnerException is null ? null : BasicErrorSnapshot(error.InnerException),
        };
    }
    private static void BasicErrorRaw(string id, Exception error, object? additionalActualFacts = null) =>
        OwnedRaw(id, new { classification = "actual_reached_error", actual = BasicErrorSnapshot(error), additionalActualFacts });
    private static T BasicReached<T>(string id, Func<T> subject)
    {
        try
        {
            var actual = subject();
            OwnedRaw(id, new { classification = "actual_reached_result", actual = BasicReachedSnapshot(actual) });
            return actual;
        }
        catch (Exception error) { BasicErrorRaw(id + ".error", error); throw; }
    }
    private static void BasicReachedAction(string id, Action subject)
    {
        try { subject(); OwnedRaw(id, new { classification = "actual_reached_completion", completed = true }); }
        catch (Exception error) { BasicErrorRaw(id + ".error", error); throw; }
    }

    private static FerruleOwnedSourceException BasicOwnedError(string id,
        FerruleOwnedSourceErrorKind expected, Action action)
    {
        try { action(); OwnedRaw(id + ".completed", new { classification = "actual_reached", completed = true }); }
        catch (FerruleRuntimeException outer)
        {
            BasicErrorRaw(id, outer);
            Equal(FerruleRuntimeError.OwnedSource, BasicReached("F03.assert.1", () => outer.Error));
            var e = outer.OwnedSourceFailure ?? throw new InvalidOperationException("Missing owned cause.");
            Equal(true, BasicReached("F03.assert.2", () => ReferenceEquals(e, outer.InnerException)));
            Equal(expected, BasicReached("F03.assert.3", () => e.Error)); return e;
        }
        catch (Exception error) { BasicErrorRaw(id, error); throw; }
        throw new InvalidOperationException("Expected lifted owned refusal.");
    }
    private static void BasicBridgeErrorFactories()
    {
        var root = BasicGaps();
        var unknown = BasicOwnedError("E489-01", FerruleOwnedSourceErrorKind.UnknownOwner,
            () => BasicReached("F03.subject.2", () => root.ResolveOwnedField(OwnedNamedOwner("Unknown"), new[] { "Value" })));
        Equal("Unknown", BasicReached("F03.assert.4", () => unknown.RequestedName));
        var duplicate = BasicOwnedError("E489-02", FerruleOwnedSourceErrorKind.AmbiguousOwner,
            () => BasicReached("F03.subject.3", () => root.ResolveOwnedField(OwnedNamedOwner("Dup"), new[] { "Value" })));
        Equal("Dup", BasicReached("F03.assert.5", () => duplicate.RequestedName)); Equal(3, BasicReached("F03.assert.6", () => duplicate.DeclarationIndices!.Count));
        Equal(2, BasicReached("F03.assert.7", () => duplicate.DeclarationIndices![0])); Equal(3, BasicReached("F03.assert.8", () => duplicate.DeclarationIndices![1])); Equal(6, BasicReached("F03.assert.9", () => duplicate.DeclarationIndices![2]));
        Equal("Missing", BasicReached("F03.assert.10", () => BasicOwnedError("E489-03", FerruleOwnedSourceErrorKind.SourceUnavailable,
            () => BasicReached("F03.subject.4", () => root.ResolveOwnedField(OwnedNamedOwner("Missing"), new[] { "Value" }))).RequestedName));
        var common = BasicCommon();
        var none = BasicOwnedError("E489-04", FerruleOwnedSourceErrorKind.InactiveSourceFrame,
            () => BasicReached("F03.subject.5", () => common.OwnedPosition(OwnedPrimaryOwner)));
        Equal<bool?>(true, BasicReached("F03.assert.11", () => none.Owner?.IsPrimary)); Equal(true, BasicReached("F03.assert.12", () => none.Collection is null));
        var empty = BasicOwnedError("E489-05", FerruleOwnedSourceErrorKind.InactiveSourceFrame,
            () => BasicReached("F03.subject.6", () => common.OwnedPosition(OwnedPrimaryOwner, Array.Empty<string>())));
        Equal<bool?>(true, BasicReached("F03.assert.13", () => empty.Owner?.IsPrimary)); OwnedPath(Array.Empty<string>(), empty.Collection);
        var incompatible = BasicOwnedError("E489-06", FerruleOwnedSourceErrorKind.IncompatibleFramePath,
            () => BasicReached("F03.subject.7", () => common.ResolveOwnedField(OwnedPrimaryOwner, new[] { "Value" }, new[] { "Rows" })));
        OwnedPath(new[] { "Value" }, incompatible.Path); OwnedPath(new[] { "Rows" }, incompatible.Frame);
        Equal<bool?>(true, BasicReached("F03.assert.14", () => incompatible.Owner?.IsPrimary));
        var malformed = BasicRoot(Group(Field("Details", Scalar(FerruleValue.FromInt64(7)))));
        var failure = BasicOwnedError("E489-07", FerruleOwnedSourceErrorKind.Traversal,
            () => BasicReached("F03.subject.8", () => malformed.ResolveOwnedField(OwnedPrimaryOwner, new[] { "Details", "Counter" })));
        var cause = (FerruleOwnedSourceTraversalException)failure.InnerException!;
        Equal(FerruleOwnedSourceTraversalKind.CannotTraverse, BasicReached("F03.assert.15", () => cause.Error));
        Equal<nuint?>(1, BasicReached("F03.assert.16", () => cause.Segment)); Equal(FerruleOwnedSourceInstanceKind.Scalar, BasicReached("F03.assert.17", () => cause.Found));
        OwnedPath(new[] { "Details", "Counter" }, cause.Path); Equal<bool?>(true, BasicReached("F03.assert.18", () => failure.Owner?.IsPrimary));
        var wrappedAgain = BasicReached("F03.subject.9", () => FerruleRuntimeException.FromOwnedSource(failure));
        Equal(true, BasicReached("F03.assert.19", () => ReferenceEquals(failure, wrappedAgain.OwnedSourceFailure)));
        Equal(true, BasicReached("F03.assert.20", () => ReferenceEquals(failure, wrappedAgain.InnerException)));
        Equal(true, BasicReached("F03.assert.21", () => ReferenceEquals(cause, wrappedAgain.InnerException!.InnerException)));
        var terminal = BasicOwnedError("E489-08", FerruleOwnedSourceErrorKind.Traversal,
            () => BasicReached("F03.subject.10", () => common.ResolveOwnedField(OwnedPrimaryOwner, new[] { "Details" })));
        var terminalCause = (FerruleOwnedSourceTraversalException)terminal.InnerException!;
        Equal(FerruleOwnedSourceTraversalKind.ExpectedScalar, BasicReached("F03.assert.22", () => terminalCause.Error));
        Equal(FerruleOwnedSourceInstanceKind.Group, BasicReached("F03.assert.23", () => terminalCause.Found)); OwnedPath(new[] { "Details" }, terminalCause.Path);
        var repeatedRoot = BasicRoot(Repeated(BasicValue("v")));
        var uniterated = BasicOwnedError("E489-09", FerruleOwnedSourceErrorKind.UniteratedRepetition,
            () => BasicReached("F03.subject.11", () => repeatedRoot.ResolveOwnedField(OwnedPrimaryOwner, new[] { "Value" })));
        Equal<nuint?>(0, BasicReached("F03.assert.24", () => uniterated.Segment)); OwnedPath(new[] { "Value" }, uniterated.Path);
        var documents = BasicRoot(new FerruleDocumentSet(new[] { new FerruleDocument("doc", Group()) }));
        var unsupported = BasicOwnedError("E489-10", FerruleOwnedSourceErrorKind.UnsupportedSourceKind,
            () => BasicReached("F03.subject.12", () => documents.IterateOwnedSource(OwnedPrimaryOwner, Array.Empty<string>())));
        Equal<FerruleOwnedSourceInstanceKind?>(FerruleOwnedSourceInstanceKind.DocumentSet, BasicReached("F03.assert.25", () => unsupported.FoundInstance));
        OwnedPath(Array.Empty<string>(), unsupported.Path);
        BasicOwnedError("E489-11", FerruleOwnedSourceErrorKind.InvalidNamedOwner,
            () => BasicReached("F03.subject.13", () => common.ResolveOwnedField(OwnedNamedOwner(""), new[] { "Value" })));
        var zero = BasicReached("F03.chained.39", () => BasicReached("F03.subject.14", () => FerruleBasicSourceRoots.Create(Group(), Array.Empty<FerruleDeclaredNamedSourceRoot>(),
            new(1, 0, 0, 0, 0))).RootContext());
        var bytes = BasicOwnedError("E489-13", FerruleOwnedSourceErrorKind.MetadataLimit,
            () => BasicReached("F03.subject.15", () => zero.ResolveOwnedField(OwnedNamedOwner("X"), Array.Empty<string>())));
        Equal<FerruleOwnedSourceLimitKind?>(FerruleOwnedSourceLimitKind.MetadataBytes, BasicReached("F03.assert.26", () => bytes.LimitKind));
        Equal<nuint?>(0, BasicReached("F03.assert.27", () => bytes.Used)); Equal<nuint?>(1, BasicReached("F03.assert.28", () => bytes.Requested)); Equal<nuint?>(0, BasicReached("F03.assert.29", () => bytes.Maximum));
        Equal(true, BasicReached("F03.assert.30", () => bytes.Owner is null)); Equal(true, BasicReached("F03.assert.31", () => bytes.RequestedName is null));
        var overflow = BasicOwnedError("E489-14", FerruleOwnedSourceErrorKind.MetadataArithmeticOverflow,
            () => BasicReached("F03.subject.16", () => FerruleBasicSourceRoots.Create(Group(), Array.Empty<FerruleDeclaredNamedSourceRoot>(),
                new(2, 2, 3, 1_048_576, nuint.MaxValue))));
        Equal<FerruleOwnedSourceLimitKind?>(FerruleOwnedSourceLimitKind.MetadataSlots, BasicReached("F03.assert.32", () => overflow.LimitKind));
        Throws<ArgumentNullException>(() => BasicReached("F03.subject.17", () => FerruleRuntimeException.FromOwnedSource(null!)));
        Throws<ArgumentNullException>(() => BasicReached("F03.subject.18", () => FerruleRuntimeException.FromBasicSourceBridge(null!)));
    }

    private static void BasicPairedNamespaceAndFallback()
    {
        var common = BasicCommon();
        OwnedValue("B489-C01.legacy", Text("P.root"), BasicReached("F03.subject.19", () => common.ResolveLegacyField(new[] { "Value" })));
        OwnedValue("B489-C01.owned", Text("P.root"), BasicReached("F03.subject.20", () => common.ResolveOwnedField(OwnedPrimaryOwner, new[] { "Value" })));
        Equal(1L, BasicReached("F03.assert.33", () => BasicReached("F03.subject.21", () => common.LegacyPosition(Array.Empty<string>()))));
        BasicOwnedError("B489-C01.position", FerruleOwnedSourceErrorKind.InactiveSourceFrame,
            () => BasicReached("F03.subject.22", () => common.OwnedPosition(OwnedPrimaryOwner)));
        var named = BasicReached("F03.subject.23", () => common.IterateOwnedSource(OwnedNamedOwner("Ref"), new[] { "Rows" }));
        Equal(4, BasicReached("F03.assert.34", () => named.Count));
        var n1 = named[0].Context;
        OwnedValue("B489-C02.current", Text("N1"), BasicReached("F03.subject.24", () => n1.ResolveLegacyField(new[] { "Value" })));
        OwnedValue("B489-C02.outward", FerruleValue.FromInt64(99), BasicReached("F03.subject.25", () => n1.ResolveLegacyField(new[] { "Details", "Counter" })));
        OwnedValue("B489-C02.absence", FerruleValue.Null,
            BasicReached("F03.subject.26", () => n1.ResolveOwnedField(OwnedNamedOwner("Ref"), new[] { "Rows", "Details", "Counter" })));
        OwnedValue("B489-C02.primary", Text("P.root"), BasicReached("F03.subject.27", () => n1.ResolveOwnedField(OwnedPrimaryOwner, new[] { "Value" })));
        Equal(1L, BasicReached("F03.assert.35", () => BasicReached("F03.subject.28", () => n1.LegacyPosition(new[] { "Ref", "Rows" }))));
        OwnedIndex("B489-C02.position", 1, BasicReached("F03.subject.29", () => n1.OwnedPosition(OwnedNamedOwner("Ref"), new[] { "Rows" })));
        var primaryCollision = BasicReached("F03.subject.30", () => common.IterateSource(new[] { "Ref", "Rows" }));
        Equal(1, BasicReached("F03.assert.36", () => primaryCollision.Count));
        OwnedValue("B489-C03.legacy", Text("P.Ref.1"), BasicReached("F03.subject.31", () => primaryCollision[0].Context.ResolveLegacyField(new[] { "Value" })));
        OwnedValue("B489-C03.primary", Text("P.Ref.1"), BasicReached("F03.subject.32", () => primaryCollision[0].Context.ResolveOwnedField(
            OwnedPrimaryOwner, new[] { "Ref", "Rows", "Value" })));
        Equal(Text("N4"), BasicReached("F03.assert.37", () => BasicReached("F03.subject.33", () => named[3].Context.ResolveOwnedField(OwnedNamedOwner("Ref"), new[] { "Rows", "Value" }))));
        var gaps = BasicGaps();
        OwnedValue("B489-C15.primary", Text("P.root"), BasicReached("F03.subject.34", () => gaps.ResolveOwnedField(OwnedPrimaryOwner, new[] { "Value" })));
        Equal("Ref,Dup,Other", BasicReached("F03.assert.38", () => string.Join(',', ((FerruleGroup)gaps.LegacyView.Frames[0]).Fields.Select(f => f.Name))));
        OwnedValue("B489-C16.legacy", Text("D1"), BasicReached("F03.subject.35", () => gaps.ResolveLegacyField(new[] { "Dup", "Value" })));
        var ambiguous = BasicOwnedError("B489-C16.owned", FerruleOwnedSourceErrorKind.AmbiguousOwner,
            () => BasicReached("F03.subject.36", () => gaps.ResolveOwnedField(OwnedNamedOwner("Dup"), new[] { "Value" })));
        Equal(3, BasicReached("F03.assert.39", () => ambiguous.DeclarationIndices!.Count)); Equal(2, BasicReached("F03.assert.40", () => ambiguous.DeclarationIndices![0]));
        Equal(3, BasicReached("F03.assert.41", () => ambiguous.DeclarationIndices![1])); Equal(6, BasicReached("F03.assert.42", () => ambiguous.DeclarationIndices![2]));
        BasicOwnedError("B489-C17.unknown", FerruleOwnedSourceErrorKind.UnknownOwner,
            () => BasicReached("F03.subject.37", () => gaps.ResolveOwnedField(OwnedNamedOwner("Unknown"), new[] { "Value" })));
        BasicOwnedError("B489-C17.unavailable", FerruleOwnedSourceErrorKind.SourceUnavailable,
            () => BasicReached("F03.subject.38", () => gaps.ResolveOwnedField(OwnedNamedOwner("Missing"), new[] { "Value" })));
        var literals = BasicRoot(BasicValue("P.root"), new("", null),
            new("primary", BasicValue("literal.primary")), new(" Ref 雪 ", BasicValue("literal.spaces.unicode")),
            new("ref", BasicValue("literal.lowercase")));
        OwnedValue("B489-C18.primary", Text("P.root"), BasicReached("F03.subject.39", () => literals.ResolveOwnedField(OwnedPrimaryOwner, new[] { "Value" })));
        OwnedValue("B489-C18.literal-primary", Text("literal.primary"), BasicReached("F03.subject.40", () => literals.ResolveOwnedField(OwnedNamedOwner("primary"), new[] { "Value" })));
        OwnedValue("B489-C18.literal-spaced", Text("literal.spaces.unicode"), BasicReached("F03.subject.41", () => literals.ResolveOwnedField(OwnedNamedOwner(" Ref 雪 "), new[] { "Value" })));
        OwnedValue("B489-C18.literal-lower", Text("literal.lowercase"), BasicReached("F03.subject.42", () => literals.ResolveOwnedField(OwnedNamedOwner("ref"), new[] { "Value" })));
        BasicOwnedError("B489-C18.empty", FerruleOwnedSourceErrorKind.InvalidNamedOwner,
            () => BasicReached("F03.subject.43", () => literals.ResolveOwnedField(OwnedNamedOwner(""), new[] { "Value" })));
        BasicNamedOwnedLegacyProjectionNativePaths();
    }

    private static void BasicNamedOwnedLegacyProjectionNativePaths()
    {
        var owner = OwnedNamedOwner("Ref");
        var namedRow1 = BasicValue("N1");
        var child1 = BasicValue("NN21"); var child2 = BasicValue("NN22");
        var namedRow2 = Group(Field("Value", Scalar(Text("N2"))), Field("Rows", Repeated(child1, child2)));
        var named = Group(Field("Value", Scalar(Text("N.root"))), Field("Rows", Repeated(namedRow1, namedRow2)));
        var primaryRow1 = BasicValue("P.Ref.1"); var primaryRow2 = BasicValue("P.Ref.2");
        var primary = Group(Field("Value", Scalar(Text("P.root"))),
            Field("Ref", Group(Field("Rows", Repeated(primaryRow1, primaryRow2)))));
        OwnedRaw("N681.input", new { classification = "complete_constructed_input_before_subject",
            primary = BasicInstanceSnapshot(primary), named = BasicInstanceSnapshot(named), name = "Ref",
            path = new[] { "Rows" }, prefixed = new[] { "Ref", "Rows" }, relative = new[] { "Rows" },
            generic = Array.Empty<string>() });
        var root = BasicReached("N681.root", () => BasicRoot(primary, new FerruleDeclaredNamedSourceRoot("Ref", named)));
        var original = BasicReached("N681.walk", () => root.WalkOwnedSourceCohort(owner, new[] { "Rows" }));
        BasicPhaseRaw("N681.original", original);
        Equal(2, original.Count);
        var second = original[1];
        Equal<nuint>(2, second.Context.OwnedPosition(owner, new[] { "Rows" }));
        Equal(1L, second.Context.LegacyPosition(new[] { "Ref", "Rows" }));
        Equal(2L, second.Context.LegacyPosition(new[] { "Rows" }));
        Equal(2L, second.Context.LegacyPosition(Array.Empty<string>()));
        Equal(true, second.Context.LegacyView.BasicCollectionPath(0).SequenceEqual(new[] { "Rows" }));
        Equal(true, ReferenceEquals(namedRow2, second.Context.OwnedView.FrameAt(0).Item));
        Equal(true, ReferenceEquals(namedRow2, second.Context.LegacyView.Frames[^1]));
        Equal<nuint?>(2, second.RawSourceOrdinal);
        OwnedValue("N681.original.value", Text("N2"), second.Context.ResolveOwnedField(owner, new[] { "Rows", "Value" }));
        OwnedValue("N681.original.root", Text("N.root"), second.Context.ResolveOwnedFieldWithRootPin(owner,
            new[] { "Value" }, Array.Empty<string>(), true));

        // A real ordinary named wrapper remains qualified; owned projection does not rewrite it.
        var ordinaryPrimary = BasicValue("P.root");
        OwnedRaw("N681.ordinary.input", new { primary = BasicInstanceSnapshot(ordinaryPrimary), named = BasicInstanceSnapshot(named), name = "Ref" });
        var ordinaryRoot = BasicRoot(ordinaryPrimary, new FerruleDeclaredNamedSourceRoot("Ref", named));
        var ordinary = BasicReached("N681.ordinary.walk", () => ordinaryRoot.WalkSourceCohort(new[] { "Ref", "Rows" }));
        BasicPhaseRaw("N681.ordinary", ordinary);
        Equal(2, ordinary.Count);
        Equal(true, ordinary[1].Context.LegacyView.BasicCollectionPath(0).SequenceEqual(new[] { "Ref", "Rows" }));
        Equal(2L, ordinary[1].Context.LegacyPosition(new[] { "Ref", "Rows" }));
        Equal<nuint>(2, ordinary[1].Context.OwnedPosition(owner, new[] { "Rows" }));
        Equal(true, ReferenceEquals(namedRow2, ordinary[1].Context.LegacyView.Frames[^1]));
        Equal(true, ReferenceEquals(namedRow2, ordinary[1].Context.OwnedView.FrameAt(0).Item));

        // The genuine outer Primary Ref/Rows cursor survives inner named compaction.
        var outer = BasicReached("N681.outer.walk", () => root.WalkOwnedSourceCohort(OwnedPrimaryOwner, new[] { "Ref", "Rows" }));
        var parent = outer[1].Context;
        var inner = BasicReached("N681.inner.walk", () => parent.WalkOwnedSourceCohort(owner, new[] { "Rows" }));
        var phase = BasicReached("N681.inner.begin", () => parent.BeginRetainedFilterPhase(inner));
        var produced = BasicReached("N681.inner.produce", () => phase.Produce(1));
        BasicPhaseRaw("N681.outer", outer); BasicPhaseRaw("N681.inner.original", inner);
        BasicPhaseRaw("N681.inner.compact", new[] { produced });
        Equal<nuint>(2, produced.Context.OwnedPosition(OwnedPrimaryOwner, new[] { "Ref", "Rows" }));
        Equal<nuint>(1, produced.Context.OwnedPosition(owner, new[] { "Rows" }));
        Equal(2L, produced.Context.LegacyPosition(new[] { "Ref", "Rows" }));
        Equal(1L, produced.Context.LegacyPosition(new[] { "Rows" }));
        Equal(1L, produced.Context.LegacyPosition(Array.Empty<string>()));
        Equal(2, produced.Context.LegacyView.BasicCollectionIndex(0));
        Equal(1, produced.Context.LegacyView.BasicCollectionIndex(1));
        Equal(true, produced.Context.LegacyView.BasicCollectionPath(0).SequenceEqual(new[] { "Ref", "Rows" }));
        Equal(true, produced.Context.LegacyView.BasicCollectionPath(1).SequenceEqual(new[] { "Rows" }));
        Equal(true, ReferenceEquals(primaryRow2, produced.Context.OwnedView.FrameAt(0).Item));
        Equal(true, ReferenceEquals(namedRow2, produced.Context.OwnedView.FrameAt(1).Item));
        Equal(true, ReferenceEquals(primaryRow2, produced.Context.LegacyView.Frames[^2]));
        Equal(true, ReferenceEquals(namedRow2, produced.Context.LegacyView.Frames[^1]));
        Equal<nuint?>(2, produced.RawSourceOrdinal); Equal<nuint?>(2, produced.Context.OriginAt(3).RawOrdinal);
        Equal<nuint>(2, inner[1].Context.OwnedPosition(owner, new[] { "Rows" }));
        // Framed fields select the nearest nonempty suffix; this inner Rows item is N2.
        // The Ref/Rows position above remains the genuine outer cursor 2.
        OwnedValue("N681.outer.framed", Text("N2"), produced.Context.ResolveLegacyField(new[] { "Value" }, new[] { "Ref", "Rows" }));
        OwnedValue("N681.inner.value", Text("N2"), produced.Context.ResolveOwnedField(owner, new[] { "Rows", "Value" }));

        var nested = BasicReached("N681.nested.walk", () => second.Context.WalkOwnedSourceCohort(owner, new[] { "Rows", "Rows" }));
        BasicPhaseRaw("N681.nested", nested);
        Equal(2, nested.Count);
        Equal<nuint>(2, nested[1].Context.OwnedPosition(owner, new[] { "Rows" }));
        Equal<nuint>(2, nested[1].Context.OwnedPosition(owner, new[] { "Rows", "Rows" }));
        Equal(1L, nested[1].Context.LegacyPosition(new[] { "Ref", "Rows" }));
        Equal(2L, nested[1].Context.LegacyPosition(new[] { "Rows", "Rows" }));
        Equal(true, nested[1].Context.LegacyView.BasicCollectionPath(1).SequenceEqual(new[] { "Rows", "Rows" }));
        Equal(true, ReferenceEquals(namedRow2, nested[1].Context.OwnedView.FrameAt(0).Item));
        Equal(true, ReferenceEquals(child2, nested[1].Context.OwnedView.FrameAt(1).Item));
        Equal(true, ReferenceEquals(child2, nested[1].Context.LegacyView.Frames[^1]));
        OwnedValue("N681.nested.value", Text("NN22"), nested[1].Context.ResolveOwnedField(owner, new[] { "Rows", "Rows", "Value" }));

        var repeat1 = BasicValue("R1"); var repeat2 = BasicValue("R2");
        var repeating = Repeated(repeat1, repeat2);
        OwnedRaw("N681.repeating.input", new { primary = BasicInstanceSnapshot(ordinaryPrimary), named = BasicInstanceSnapshot(repeating), name = "Ref", path = Array.Empty<string>() });
        var repeatingRoot = BasicRoot(ordinaryPrimary, new FerruleDeclaredNamedSourceRoot("Ref", repeating));
        var rootRows = BasicReached("N681.repeating.walk", () => repeatingRoot.WalkOwnedSourceCohort(owner, Array.Empty<string>()));
        var identity = BasicReached("N681.identity.walk", () => rootRows[1].Context.WalkOwnedSourceCohort(owner, Array.Empty<string>()));
        BasicPhaseRaw("N681.repeating", rootRows); BasicPhaseRaw("N681.identity", identity);
        Equal(2, rootRows.Count); Equal(1, identity.Count);
        Equal(0, rootRows[1].Context.LegacyView.BasicCollectionPath(0).Count);
        Equal(1L, rootRows[1].Context.LegacyPosition(new[] { "Ref" }));
        Equal(2L, identity[0].Context.LegacyPosition(Array.Empty<string>()));
        Equal<nuint>(2, identity[0].Context.OwnedPosition(owner, Array.Empty<string>()));
        Equal(false, identity[0].HasNewCursor); Equal<nuint?>(null, identity[0].RawSourceOrdinal);
        Equal(1, identity[0].Context.OwnedView.FrameCount);
        Equal(true, ReferenceEquals(rootRows[1].Context.LegacyView, identity[0].Context.LegacyView));
        Equal(true, ReferenceEquals(repeat2, identity[0].Context.OwnedView.FrameAt(0).Item));
        Equal(true, ReferenceEquals(repeat2, identity[0].Context.LegacyView.Frames[^1]));
        OwnedValue("N681.identity.value", Text("R2"), identity[0].Context.ResolveOwnedField(owner,
            new[] { "Value" }, Array.Empty<string>()));

        var gapRow3 = BasicValue("N3");
        var gaps = Group(Field("Value", Scalar(Text("N.root"))), Field("Rows", Repeated(
            Scalar(FerruleValue.Null), namedRow1, Scalar(FerruleValue.JsonNull), namedRow2, gapRow3)));
        OwnedRaw("N681.phase.input", new { primary = BasicInstanceSnapshot(primary), named = BasicInstanceSnapshot(gaps), name = "Ref", acceptedOriginalIndices = new[] { 1, 2 } });
        var gapRoot = BasicRoot(primary, new FerruleDeclaredNamedSourceRoot("Ref", gaps));
        var gapOriginal = BasicReached("N681.phase.walk", () => gapRoot.WalkOwnedSourceCohort(owner, new[] { "Rows" }));
        var gapPhase = BasicReached("N681.phase.begin", () => gapRoot.BeginRetainedFilterPhase(gapOriginal));
        var gapFirst = BasicReached("N681.phase.first", () => gapPhase.Produce(1));
        var gapSecond = BasicReached("N681.phase.second", () => gapPhase.Produce(2));
        BasicPhaseRaw("N681.phase.original", gapOriginal); BasicPhaseRaw("N681.phase.compact", new[] { gapFirst, gapSecond });
        Equal(3, gapOriginal.Count);
        var raw = new nuint[] { 2, 4, 5 };
        for (var i = 0; i < raw.Length; i++) Equal(raw[i], gapOriginal[i].Context.OwnedPosition(owner, new[] { "Rows" }));
        Equal<nuint?>(4, gapFirst.RawSourceOrdinal); Equal<nuint?>(5, gapSecond.RawSourceOrdinal);
        Equal<nuint>(1, gapFirst.Context.OwnedPosition(owner, new[] { "Rows" }));
        Equal<nuint>(2, gapSecond.Context.OwnedPosition(owner, new[] { "Rows" }));
        Equal(1L, gapFirst.Context.LegacyPosition(Array.Empty<string>()));
        Equal(2L, gapSecond.Context.LegacyPosition(Array.Empty<string>()));
        Equal(1L, gapSecond.Context.LegacyPosition(new[] { "Ref", "Rows" }));
        Equal(true, ReferenceEquals(namedRow2, gapFirst.Context.LegacyView.Frames[^1]));
        Equal(true, ReferenceEquals(gapRow3, gapSecond.Context.OwnedView.FrameAt(0).Item));

        // Isolate the new projection storage; this is not a whole-operation budget claim.
        // Root registration, query, original-cohort retention and phase charges remain separate.
        OwnedRaw("N681.accounting.input", new { classification = "isolated_ProjectBasicOwned_storage_only",
            observedCandidate = 1, inheritedOwnedFrames = 0, path = new[] { "Rows" },
            exactNewPathSlots = 1, exactAssociationSlots = 1, exactProjectionSlots = 9,
            exactPathUtf8Bytes = 4, noDeclarationNamePathCellOrText = true });
        var exactBudget = new BasicSourceBudget(BasicLimits with { MaxMetadataBytes = 4 }, true);
        var projected = BasicReached("N681.accounting.exact", () => root.LegacyView.ProjectBasicOwned(
            second.Context.OwnedView, 0, namedRow2, new[] { "Rows" }, false, exactBudget));
        OwnedRaw("N681.accounting.exact.counter", new { slots = (ulong)exactBudget.Slots.Used, bytes = (ulong)exactBudget.Bytes.Used,
            path = projected.BasicCollectionPath(0), actualItemSame = ReferenceEquals(namedRow2, projected.Frames[^1]) });
        Equal<nuint>(9, exactBudget.Slots.Used); Equal<nuint>(4, exactBudget.Bytes.Used);
        var shortBudget = new BasicSourceBudget(BasicLimits with { MaxMetadataBytes = 3 }, true);
        var shortPath = BasicBridgeFailure("N681.accounting.short", FerruleBasicSourceBridgeErrorKind.PairedMetadataLimit,
            () => root.LegacyView.ProjectBasicOwned(second.Context.OwnedView, 0, namedRow2, new[] { "Rows" }, false, shortBudget),
            () => new { slots = (ulong)shortBudget.Slots.Used, bytes = (ulong)shortBudget.Bytes.Used });
        Equal<FerruleBasicSourceBridgeComponent?>(FerruleBasicSourceBridgeComponent.MetadataBytes, shortPath.Component);
        Equal<nuint?>(0, shortPath.Used); Equal<nuint?>(4, shortPath.Requested); Equal<nuint?>(3, shortPath.Maximum);
        Equal<nuint>(9, shortBudget.Slots.Used); Equal<nuint>(0, shortBudget.Bytes.Used);
        var exactSlots = new BasicSourceBudget(BasicLimits, true);
        exactSlots.Charge(exactSlots.Slots.Maximum - 9);
        BasicReached("N681.accounting.slots.exact", () => root.LegacyView.ProjectBasicOwned(
            second.Context.OwnedView, 0, namedRow2, new[] { "Rows" }, false, exactSlots));
        Equal(exactSlots.Slots.Maximum, exactSlots.Slots.Used);
        var shortSlots = new BasicSourceBudget(BasicLimits, true);
        shortSlots.Charge(shortSlots.Slots.Maximum - 8);
        var slotsFailure = BasicBridgeFailure("N681.accounting.slots.short", FerruleBasicSourceBridgeErrorKind.PairedMetadataLimit,
            () => root.LegacyView.ProjectBasicOwned(second.Context.OwnedView, 0, namedRow2, new[] { "Rows" }, false, shortSlots),
            () => new { slots = (ulong)shortSlots.Slots.Used, maximum = (ulong)shortSlots.Slots.Maximum });
        Equal<FerruleBasicSourceBridgeComponent?>(FerruleBasicSourceBridgeComponent.MetadataSlots, slotsFailure.Component);
        Equal<nuint?>(shortSlots.Slots.Maximum - 1, slotsFailure.Used);
        Equal<nuint?>(2, slotsFailure.Requested); Equal<nuint?>(shortSlots.Slots.Maximum, slotsFailure.Maximum);
    }

    private static void BasicPairedNestedAndIdentity()
    {
        var noRef = BasicRoot(Group(Field("Value", Scalar(Text("P.root"))),
            Field("Details", Group(Field("Counter", Scalar(FerruleValue.FromInt64(99))))),
            Field("Rows", Repeated(BasicValue("P1"), BasicValue("P2")))),
            new("Absent", null), new("Ref", Group(Field("Value", Scalar(Text("N.root"))),
                Field("Rows", Repeated(Group(Field("Value", Scalar(Text("N1"))),
                    Field("Rows", Repeated(BasicValue("NN1")))), BasicValue("N2"))))));
        var fallback = BasicReached("F03.subject.44", () => noRef.IterateSource(new[] { "Ref", "Rows" })); Equal(2, BasicReached("F03.assert.43", () => fallback.Count));
        Equal(1, BasicReached("F03.assert.44", () => ((FerruleRetainedSourceCohort)fallback).SourceWitness.SelectionCalls));
        OwnedValue("B489-C04.first", Text("N1"), BasicReached("F03.subject.45", () => fallback[0].Context.ResolveLegacyField(new[] { "Value" })));
        OwnedValue("B489-C04.second", Text("N2"), BasicReached("F03.subject.46", () => fallback[1].Context.ResolveLegacyField(new[] { "Value" })));
        OwnedIndex("B489-C04.raw", 2, BasicReached("F03.subject.47", () => fallback[1].Context.OwnedPosition(OwnedNamedOwner("Ref"), new[] { "Rows" })));
        var namedInner = BasicReached("F03.subject.48", () => fallback[0].Context.IterateSource(new[] { "Rows" })); Equal(1, BasicReached("F03.assert.45", () => namedInner.Count));
        OwnedValue("B489-C05.value", Text("NN1"), BasicReached("F03.subject.49", () => namedInner[0].Context.ResolveOwnedField(
            OwnedNamedOwner("Ref"), new[] { "Rows", "Rows", "Value" })));
        Equal(1L, BasicReached("F03.assert.46", () => BasicReached("F03.subject.50", () => namedInner[0].Context.LegacyPosition(new[] { "Rows" }))));
        Equal<nuint>(1, BasicReached("F03.assert.47", () => BasicReached("F03.subject.51", () => namedInner[0].Context.OwnedPosition(OwnedNamedOwner("Ref"), new[] { "Rows" }))));
        Equal<nuint>(1, BasicReached("F03.assert.48", () => BasicReached("F03.subject.52", () => namedInner[0].Context.OwnedPosition(OwnedNamedOwner("Ref"), new[] { "Rows", "Rows" }))));
        var common = BasicCommon(); var allInner = BasicReached("F03.subject.53", () => common.IterateSource(new[] { "Rows", "Rows" })); Equal(3, BasicReached("F03.assert.49", () => allInner.Count));
        var expected = new[] { "P.inner11", "P.inner12", "P.inner21" };
        var outer = new nuint[] { 1, 1, 2 }; var inner = new nuint[] { 1, 2, 1 };
        for (var i = 0; i < allInner.Count; i++)
        {
            OwnedValue("B489-C08.value" + i, Text(expected[i]), BasicReached("F03.subject.54", () => allInner[i].Context.ResolveLegacyField(new[] { "Value" })));
            OwnedIndex("B489-C08.outer" + i, outer[i], BasicReached("F03.subject.55", () => allInner[i].Context.OwnedPosition(OwnedPrimaryOwner, new[] { "Rows" })));
            OwnedIndex("B489-C08.inner" + i, inner[i], BasicReached("F03.subject.56", () => allInner[i].Context.OwnedPosition(OwnedPrimaryOwner, new[] { "Rows", "Rows" })));
        }
        var first = BasicReached("F03.subject.57", () => common.IterateSource(new[] { "Rows" }))[0].Context;
        var relative = BasicReached("F03.subject.58", () => first.IterateSource(new[] { "Rows" })); Equal(2, BasicReached("F03.assert.50", () => relative.Count));
        Equal(2L, BasicReached("F03.assert.51", () => BasicReached("F03.subject.59", () => relative[1].Context.LegacyPosition(new[] { "Rows" }))));
        Equal<nuint>(1, BasicReached("F03.assert.52", () => BasicReached("F03.subject.60", () => relative[1].Context.OwnedPosition(OwnedPrimaryOwner, new[] { "Rows" }))));
        OwnedValue("B489-C09.pinned", Text("P1"), BasicReached("F03.subject.61", () => relative[1].Context.ResolveOwnedField(
            OwnedPrimaryOwner, new[] { "Rows", "Value" }, new[] { "Rows" })));
        var identity = BasicReached("F03.subject.62", () => relative[1].Context.IterateOwnedSource(OwnedPrimaryOwner, new[] { "Rows" })); Equal(1, BasicReached("F03.assert.53", () => identity.Count));
        OwnedValue("B489-C10.identity", Text("P.inner12"), BasicReached("F03.subject.63", () => identity[0].Context.ResolveLegacyField(new[] { "Value" })));
        Equal<nuint>(2, BasicReached("F03.assert.54", () => BasicReached("F03.subject.64", () => identity[0].Context.OwnedPosition(OwnedPrimaryOwner))));
        Equal<nuint>(1, BasicReached("F03.assert.55", () => BasicReached("F03.subject.65", () => identity[0].Context.OwnedPosition(OwnedPrimaryOwner, new[] { "Rows" }))));
    }

    private static void BasicPairedPlainAbsenceAndMalformed()
    {
        var common = BasicCommon();
        foreach (var owned in new[] { false, true })
        {
            var plain = owned ? BasicReached("F03.subject.66", () => common.IterateOwnedSource(OwnedPrimaryOwner, new[] { "Details" })) : BasicReached("F03.subject.67", () => common.IterateSource(new[] { "Details" }));
            Equal(1, BasicReached("F03.assert.56", () => plain.Count));
            OwnedValue(owned ? "B489-C06.legacy" : "B489-C07.legacy", FerruleValue.FromInt64(99),
                BasicReached("F03.subject.68", () => plain[0].Context.ResolveLegacyField(new[] { "Counter" })));
            OwnedValue(owned ? "B489-C06.owned" : "B489-C07.owned", FerruleValue.FromInt64(99),
                BasicReached("F03.subject.69", () => plain[0].Context.ResolveOwnedField(OwnedPrimaryOwner, new[] { "Details", "Counter" })));
            BasicOwnedError(owned ? "B489-C06.position" : "B489-C07.position", FerruleOwnedSourceErrorKind.InactiveSourceFrame,
                () => BasicReached("F03.subject.70", () => plain[0].Context.OwnedPosition(OwnedPrimaryOwner)));
        }
        var rows = BasicReached("F03.subject.71", () => common.IterateSource(new[] { "Rows" })); Equal(3, BasicReached("F03.assert.57", () => rows.Count));
        var terminalNull = BasicReached("F03.subject.72", () => rows[1].Context.IterateSource(new[] { "Details" }));
        BasicPhaseRaw("B489-C11.legacy", terminalNull); Equal(1, BasicReached("F03.assert.58", () => terminalNull.Count));
        Equal(false, BasicReached("F03.assert.59", () => terminalNull[0].HasNewCursor));
        OwnedValue("B489-C11.terminal", FerruleValue.Null,
            BasicReached("F03.subject.73", () => terminalNull[0].Context.ResolveLegacyField(Array.Empty<string>())));
        Equal(0, BasicReached("F03.assert.60", () => BasicReached("F03.subject.74", () => rows[1].Context.IterateOwnedSource(OwnedPrimaryOwner, new[] { "Rows", "Details" })).Count));
        OwnedValue("B489-C13.unframed", FerruleValue.Null,
            BasicReached("F03.subject.75", () => rows[2].Context.ResolveOwnedField(OwnedPrimaryOwner, new[] { "Rows", "Details", "Counter" })));
        OwnedValue("B489-C13.pinned", FerruleValue.Null,
            BasicReached("F03.subject.76", () => rows[2].Context.ResolveOwnedField(OwnedPrimaryOwner, new[] { "Rows", "Details", "Counter" }, new[] { "Rows" })));
        var malformed = BasicRoot(Group(Field("Details", Scalar(FerruleValue.FromInt64(7)))));
        Equal(0, BasicReached("F03.assert.61", () => BasicReached("F03.subject.77", () => malformed.IterateSource(new[] { "Details", "Counter" })).Count));
        var failure = BasicOwnedError("B489-C14.owned", FerruleOwnedSourceErrorKind.Traversal,
            () => BasicReached("F03.subject.78", () => malformed.ResolveOwnedField(OwnedPrimaryOwner, new[] { "Details", "Counter" })));
        var cause = (FerruleOwnedSourceTraversalException)failure.InnerException!;
        Equal(FerruleOwnedSourceTraversalKind.CannotTraverse, BasicReached("F03.assert.62", () => cause.Error)); Equal<nuint?>(1, BasicReached("F03.assert.63", () => cause.Segment));
        OwnedPath(new[] { "Details", "Counter" }, cause.Path); Equal(FerruleOwnedSourceInstanceKind.Scalar, BasicReached("F03.assert.64", () => cause.Found));
        var named = BasicReached("F03.subject.79", () => common.IterateOwnedSource(OwnedNamedOwner("Ref"), new[] { "Rows" }))[0].Context;
        var foreign = BasicOwnedError("B489-C28", FerruleOwnedSourceErrorKind.UniteratedRepetition,
            () => BasicReached("F03.subject.80", () => named.ResolveOwnedField(OwnedPrimaryOwner, new[] { "Rows", "Value" })));
        Equal<nuint?>(1, BasicReached("F03.assert.65", () => foreign.Segment)); Equal<bool?>(true, BasicReached("F03.assert.66", () => foreign.Owner?.IsPrimary));
        OwnedPath(new[] { "Rows", "Value" }, foreign.Path);
    }

    private static void BasicPairedNullIdentityAndDistinctRoots()
    {
        var nullRows = BasicRoot(Group(Field("Rows", Repeated(Scalar(FerruleValue.Null), BasicValue("kept1"),
            Scalar(FerruleValue.JsonNull), BasicValue("kept2")))));
        var legacy = BasicReached("F03.subject.81", () => nullRows.IterateSource(new[] { "Rows" })); Equal(4, BasicReached("F03.assert.67", () => legacy.Count));
        for (var i = 0; i < legacy.Count; i++)
            OwnedIndex("B489-C12.legacy-raw" + i, (nuint)i + 1,
                BasicReached("F03.subject.82", () => legacy[i].Context.OwnedPosition(OwnedPrimaryOwner, new[] { "Rows" })));
        var owned = BasicReached("F03.subject.83", () => nullRows.IterateOwnedSource(OwnedPrimaryOwner, new[] { "Rows" })); Equal(2, BasicReached("F03.assert.68", () => owned.Count));
        OwnedIndex("B489-C12.owned-raw2", 2, BasicReached("F03.subject.84", () => owned[0].Context.OwnedPosition(OwnedPrimaryOwner, new[] { "Rows" })));
        OwnedIndex("B489-C12.owned-raw4", 4, BasicReached("F03.subject.85", () => owned[1].Context.OwnedPosition(OwnedPrimaryOwner, new[] { "Rows" })));
        OwnedValue("B489-C12.kept1", Text("kept1"), BasicReached("F03.subject.86", () => owned[0].Context.ResolveLegacyField(new[] { "Value" })));
        OwnedValue("B489-C12.kept2", Text("kept2"), BasicReached("F03.subject.87", () => owned[1].Context.ResolveLegacyField(new[] { "Value" })));
        foreach (var index in new[] { 0, 2 })
        {
            OwnedValue("B489-C12.null-field" + index, FerruleValue.Null,
                BasicReached("F03.subject.88", () => legacy[index].Context.ResolveOwnedField(OwnedPrimaryOwner, new[] { "Rows", "Value" })));
            Equal(0, BasicReached("F03.assert.69", () => BasicReached("F03.subject.89", () => legacy[index].Context.IterateOwnedSource(OwnedPrimaryOwner, new[] { "Rows" })).Count));
        }
        var same = Group(Field("Value", Scalar(Text("same.root"))), Field("Rows", Repeated(BasicValue("a"), BasicValue("b"))));
        var both = BasicRoot(same, new FerruleDeclaredNamedSourceRoot("Ref", same));
        var a = BasicReached("F03.subject.90", () => both.IterateOwnedSource(OwnedPrimaryOwner, new[] { "Rows" }))[0].Context;
        var b = BasicReached("F03.subject.91", () => a.IterateOwnedSource(OwnedNamedOwner("Ref"), new[] { "Rows" }))[1].Context;
        OwnedValue("B489-C19.primary", Text("a"), BasicReached("F03.subject.92", () => b.ResolveOwnedField(OwnedPrimaryOwner, new[] { "Rows", "Value" })));
        OwnedValue("B489-C19.named", Text("b"), BasicReached("F03.subject.93", () => b.ResolveOwnedField(OwnedNamedOwner("Ref"), new[] { "Rows", "Value" })));
        Equal<nuint>(1, BasicReached("F03.assert.70", () => BasicReached("F03.subject.94", () => b.OwnedPosition(OwnedPrimaryOwner)))); Equal<nuint>(2, BasicReached("F03.assert.71", () => BasicReached("F03.subject.95", () => b.OwnedPosition(OwnedNamedOwner("Ref")))));
    }

    private static void BasicPairedFlatRootAndLazyAccess()
    {
        BasicDeclaredEmptyRootPins();
        BasicOwnedLeafNodeProvenance();
        BasicStreamingFilterPhase();
        var flat = BasicRoot(Repeated(BasicValue("flat1"), BasicValue("flat2"), BasicValue("flat3")));
        var rows = BasicReached("F03.subject.96", () => flat.IterateOwnedSource(OwnedPrimaryOwner, Array.Empty<string>())); Equal(3, BasicReached("F03.assert.72", () => rows.Count));
        OwnedValue("B489-C26", Text("flat2"), BasicReached("F03.subject.97", () => rows[1].Context.ResolveOwnedField(
            OwnedPrimaryOwner, new[] { "Value" }, Array.Empty<string>())));
        Equal(2L, BasicReached("F03.assert.73", () => BasicReached("F03.subject.98", () => rows[1].Context.LegacyPosition(Array.Empty<string>()))));
        Equal<nuint>(2, BasicReached("F03.assert.74", () => BasicReached("F03.subject.99", () => rows[1].Context.OwnedPosition(OwnedPrimaryOwner, Array.Empty<string>()))));
        var common = BasicCommon(); var callbacks = 0;
        FerruleValue DemandPosition()
        {
            callbacks++;
            return FerruleValue.FromInt64((long)BasicReached("F03.subject.100", () => common.OwnedPosition(OwnedPrimaryOwner, new[] { "Rows" })));
        }
        var chooseFirst = true;
        OwnedValue("B489-C25.selected", Text("selected"), chooseFirst ? Text("selected") : DemandPosition());
        Equal(0, BasicReached("F03.assert.75", () => callbacks));
        chooseFirst = false;
        BasicOwnedError("B489-C25", FerruleOwnedSourceErrorKind.InactiveSourceFrame,
            () => { _ = chooseFirst ? Text("selected") : DemandPosition(); });
        Equal(1, BasicReached("F03.assert.76", () => callbacks));
        var incompatible = BasicOwnedError("B489-C27", FerruleOwnedSourceErrorKind.IncompatibleFramePath,
            () => BasicReached("F03.subject.101", () => common.ResolveOwnedField(OwnedPrimaryOwner, new[] { "Value" }, new[] { "Rows" })));
        OwnedPath(new[] { "Value" }, incompatible.Path); OwnedPath(new[] { "Rows" }, incompatible.Frame);
    }

    private static FerruleBasicSourceBridgeException BasicBridgeFailure(string id,
        FerruleBasicSourceBridgeErrorKind expected, Action action, Func<object>? actualFacts = null)
    {
        FerruleBasicSourceBridgeException? found = null;
        FerruleRuntimeException? wrapper = null;
        try { action(); OwnedRaw(id + ".completed", new { classification = "actual_reached", completed = true }); }
        catch (FerruleBasicSourceBridgeException e) { BasicErrorRaw(id, e, actualFacts?.Invoke()); found = e; }
        catch (FerruleRuntimeException outer)
        {
            BasicErrorRaw(id, outer, actualFacts?.Invoke());
            wrapper = outer; found = outer.BasicSourceBridgeFailure;
        }
        catch (Exception error) { BasicErrorRaw(id, error); throw; }
        if (wrapper is not null)
        {
            Equal(FerruleRuntimeError.BasicSourceBridge, BasicReached("F03.assert.77", () => wrapper.Error));
            Equal(true, BasicReached("F03.assert.78", () => ReferenceEquals(found, wrapper.InnerException)));
        }
        if (found is null) throw new InvalidOperationException("Expected bridge refusal.");
        Equal(expected, BasicReached("F03.assert.79", () => found.Error)); return found;
    }
    private static void BasicPhaseRaw(string id, IReadOnlyList<FerruleBasicSourceCandidate> rows)
    {
        OwnedRaw(id, new { count = rows.Count, rows = rows.Select(c => new {
            sourceId = (ulong)c.SourceId, raw = c.RawSourceOrdinal.HasValue ? (ulong?)c.RawSourceOrdinal.Value : null,
            cursor = c.HasNewCursor,
            owned = Enumerable.Range(0, c.Context.OwnedView.FrameCount).Select(i => {
                var f = c.Context.OwnedView.FrameAt(i); return new { ownerKey = f.Owner,
                    name = c.Context.Roots.OwnedRoots.Name(f.Owner), path = f.Path, index = (ulong)f.Index,
                    itemKind = f.Item.GetType().FullName }; }).ToArray(),
            origins = Enumerable.Range(0, c.Context.OriginCount).Select(i => {
                var f = c.Context.OriginAt(i); return new { key = f.OwnerKey, path = f.Canonical,
                    raw = f.RawOrdinal.HasValue ? (ulong?)f.RawOrdinal.Value : null, projection = f.Projection }; }).ToArray(),
            legacy = Enumerable.Range(0, c.Context.LegacyView.BasicCollectionCount).Select(i => new {
                path = c.Context.LegacyView.BasicCollectionPath(i), index = c.Context.LegacyView.BasicCollectionIndex(i) }).ToArray()
        }).ToArray() });
    }
    private static void BasicPhaseValues(string id, IReadOnlyList<FerruleBasicSourceCandidate> rows,
        string[] values, nuint[] sourceIds, nuint[] raw)
    {
        BasicPhaseRaw(id, rows); Equal(values.Length, BasicReached("F03.assert.80", () => rows.Count));
        for (var i = 0; i < rows.Count; i++)
        {
            Equal(Text(values[i]), BasicReached("F03.assert.81", () => BasicReached("F03.subject.102", () => rows[i].Context.ResolveLegacyField(new[] { "Value" }))));
            Equal(sourceIds[i], BasicReached("F03.assert.82", () => rows[i].SourceId)); Equal<nuint?>(raw[i], BasicReached("F03.assert.83", () => rows[i].RawSourceOrdinal));
            Equal<nuint>((nuint)i + 1, BasicReached("F03.assert.84", () => BasicReached("F03.subject.103", () => rows[i].Context.OwnedPosition(OwnedPrimaryOwner, new[] { "Rows" }))));
            Equal((long)i + 1, BasicReached("F03.assert.85", () => BasicReached("F03.subject.104", () => rows[i].Context.LegacyPosition(new[] { "Rows" }))));
        }
    }
    private static FerruleBasicScopeContext BasicABC(nuint bytes = 1024) => BasicReached("F03.chained.190", () => BasicReached("F03.subject.105", () => FerruleBasicSourceRoots.Create(
        Group(Field("Rows", Repeated(BasicValue("a"), BasicValue("b"), BasicValue("c")))),
        Array.Empty<FerruleDeclaredNamedSourceRoot>(), new(4, 4, 8, bytes, 3))).RootContext());
    private static void BasicFullCohortPhasesAndCapture()
    {
        var root = BasicABC(); var original = BasicReached("F03.subject.106", () => root.WalkOwnedSourceCohort(OwnedPrimaryOwner, new[] { "Rows" }));
        var caller = new nuint[] { 2, 0 };
        var captured = BasicReached("F03.subject.107", () => FerrulePhaseSelection.FromOriginal(original, caller));
        caller[0] = 1; caller[1] = 1;
        var first = BasicReached("F03.subject.108", () => root.ReindexRetainedPhase(original, captured));
        BasicPhaseValues("N502-01/N502-24", first, new[] { "c", "a" }, new nuint[] { 2, 0 }, new nuint[] { 3, 1 });
        Equal(2 + 3 + 3, BasicReached("F03.assert.86", () => first.Count + original.Count + original.Count));
        for (var i = 0; i < original.Count; i++) Equal<nuint>((nuint)i + 1,
            BasicReached("F03.assert.87", () => BasicReached("F03.subject.109", () => original[i].Context.OwnedPosition(OwnedPrimaryOwner, new[] { "Rows" }))));
        var second = BasicReached("F03.subject.110", () => root.ReindexRetainedPhase(original, BasicReached("F03.subject.111", () => FerrulePhaseSelection.FromPhase(first, new nuint[] { 0 }))));
        BasicPhaseValues("N502-02", second, new[] { "c" }, new nuint[] { 2 }, new nuint[] { 3 });
        Equal(6, BasicReached("F03.assert.88", () => original.Count + first.Count + second.Count));
        var empty = BasicReached("F03.subject.112", () => root.ReindexRetainedPhase(original, BasicReached("F03.subject.113", () => FerrulePhaseSelection.FromOriginal(original, Array.Empty<nuint>()))));
        BasicPhaseRaw("N502-03", empty); Equal(0, BasicReached("F03.assert.89", () => empty.Count));
        var all = BasicReached("F03.subject.114", () => root.ReindexRetainedPhase(original, BasicReached("F03.subject.115", () => FerrulePhaseSelection.FromOriginal(original, new nuint[] { 2, 0, 1 }))));
        BasicPhaseValues("N502-16/N502-26", all, new[] { "c", "a", "b" }, new nuint[] { 2, 0, 1 }, new nuint[] { 3, 1, 2 });
        Equal(9, BasicReached("F03.assert.90", () => original.Count + original.Count + all.Count));
        var noneRoot = BasicReached("F03.chained.206", () => BasicReached("F03.subject.116", () => FerruleBasicSourceRoots.Create(Group(Field("Rows", Repeated())),
            Array.Empty<FerruleDeclaredNamedSourceRoot>(), new(4, 4, 8, 1024, 0))).RootContext());
        var noneOriginal = BasicReached("F03.subject.117", () => noneRoot.WalkOwnedSourceCohort(OwnedPrimaryOwner, new[] { "Rows" }));
        Equal(0, BasicReached("F03.assert.91", () => noneOriginal.Count)); Equal(true, BasicReached("F03.assert.92", () => noneOriginal.SourceWitness.IsComplete));
        var nonePhase = BasicReached("F03.subject.118", () => noneRoot.ReindexRetainedPhase(noneOriginal,
            BasicReached("F03.subject.119", () => FerrulePhaseSelection.FromOriginal(noneOriginal, Array.Empty<nuint>()))));
        BasicPhaseRaw("N502-13", nonePhase); Equal(0, BasicReached("F03.assert.93", () => nonePhase.Count));
    }
    private sealed class BasicIdRecipe(nuint[] values) : BasicSourceIds, BasicPhaseIndices
    {
        public int Count => values.Length;
        public nuint SourceIdAt(int index) => values[index];
        public nuint IndexAt(int index) => values[index];
    }
    private static void BasicFullCohortRefusals()
    {
        var ids = new BasicIdRecipe(new nuint[] { 0, 1, 2 }); var noIndices = new BasicIdRecipe(Array.Empty<nuint>());
        var omitted = BasicBridgeFailure("N502-04", FerruleBasicSourceBridgeErrorKind.InvalidCohortWitness,
            () => BasicPhaseAdmission.ValidateIds(3, new BasicIdRecipe(new nuint[] { 0, 2 }), ids, noIndices));
        Equal<nuint?>(1, BasicReached("F03.assert.94", () => omitted.InputOrdinal));
        var duplicateOriginal = BasicBridgeFailure("N502-06", FerruleBasicSourceBridgeErrorKind.DuplicatePhaseCandidate,
            () => BasicPhaseAdmission.ValidateIds(3, new BasicIdRecipe(new nuint[] { 0, 0, 2 }), ids, noIndices));
        Equal<nuint?>(1, BasicReached("F03.assert.95", () => duplicateOriginal.InputOrdinal)); Equal<nuint?>(0, BasicReached("F03.assert.96", () => duplicateOriginal.CandidateOrdinal));
        var duplicateCurrent = BasicBridgeFailure("N502-07", FerruleBasicSourceBridgeErrorKind.DuplicatePhaseCandidate,
            () => BasicPhaseAdmission.ValidateIds(3, ids, new BasicIdRecipe(new nuint[] { 0, 0 }), noIndices));
        Equal<nuint?>(1, BasicReached("F03.assert.97", () => duplicateCurrent.InputOrdinal)); Equal<nuint?>(0, BasicReached("F03.assert.98", () => duplicateCurrent.CandidateOrdinal));
        var nonmember = BasicBridgeFailure("N502-12", FerruleBasicSourceBridgeErrorKind.InvalidCohortWitness,
            () => BasicPhaseAdmission.ValidateIds(3, ids, new BasicIdRecipe(new nuint[] { 3 }), noIndices));
        Equal<nuint?>(0, BasicReached("F03.assert.99", () => nonmember.InputOrdinal));
        var root = BasicABC(); var original = BasicReached("F03.subject.120", () => root.WalkOwnedSourceCohort(OwnedPrimaryOwner, new[] { "Rows" }));
        var independent = BasicReached("F03.subject.121", () => root.WalkOwnedSourceCohort(OwnedPrimaryOwner, new[] { "Rows" }));
        var mixed = BasicBridgeFailure("N502-05", FerruleBasicSourceBridgeErrorKind.InvalidCohortWitness,
            () => BasicReached("F03.subject.122", () => root.ReindexRetainedPhase(original, BasicReached("F03.subject.123", () => FerrulePhaseSelection.FromOriginal(independent, Array.Empty<nuint>())))));
        Equal<nuint?>(0, BasicReached("F03.assert.100", () => mixed.InputOrdinal));
        var duplicateIndex = BasicBridgeFailure("N502-08", FerruleBasicSourceBridgeErrorKind.DuplicatePhaseCandidate,
            () => BasicReached("F03.subject.124", () => FerrulePhaseSelection.FromOriginal(original, new nuint[] { 1, 1 })));
        Equal<nuint?>(1, BasicReached("F03.assert.101", () => duplicateIndex.InputOrdinal)); Equal<nuint?>(1, BasicReached("F03.assert.102", () => duplicateIndex.CandidateOrdinal));
        var range = BasicBridgeFailure("N502-09", FerruleBasicSourceBridgeErrorKind.InvalidCohortWitness,
            () => BasicReached("F03.subject.125", () => FerrulePhaseSelection.FromOriginal(original, new nuint[] { 3 })));
        Equal<nuint?>(0, BasicReached("F03.assert.103", () => range.InputOrdinal));
        var otherRoot = BasicABC(); var other = BasicReached("F03.subject.126", () => otherRoot.WalkOwnedSourceCohort(OwnedPrimaryOwner, new[] { "Rows" }));
        BasicBridgeFailure("N502-10", FerruleBasicSourceBridgeErrorKind.RegistryMismatch,
            () => BasicReached("F03.subject.127", () => root.ReindexRetainedPhase(original, BasicReached("F03.subject.128", () => FerrulePhaseSelection.FromOriginal(other, Array.Empty<nuint>())))));
        var anotherParent = BasicReached("F03.subject.129", () => root.CloneView()); var another = BasicReached("F03.subject.130", () => anotherParent.WalkOwnedSourceCohort(OwnedPrimaryOwner, new[] { "Rows" }));
        var parent = BasicBridgeFailure("N502-11", FerruleBasicSourceBridgeErrorKind.InvalidCohortWitness,
            () => BasicReached("F03.subject.131", () => root.ReindexRetainedPhase(original, BasicReached("F03.subject.132", () => FerrulePhaseSelection.FromOriginal(another, Array.Empty<nuint>())))));
        Equal<nuint?>(0, BasicReached("F03.assert.104", () => parent.InputOrdinal));
        var count = BasicBridgeFailure("N502-17", FerruleBasicSourceBridgeErrorKind.PairedMetadataLimit,
            () => BasicReached("F03.subject.133", () => FerrulePhaseSelection.FromOriginal(original, new nuint[] { 0, 1, 2, 3 })));
        Equal<FerruleBasicSourceBridgeComponent?>(FerruleBasicSourceBridgeComponent.WalkContexts, BasicReached("F03.assert.105", () => count.Component));
        Equal<nuint?>(0, BasicReached("F03.assert.106", () => count.Used)); Equal<nuint?>(4, BasicReached("F03.assert.107", () => count.Requested)); Equal<nuint?>(3, BasicReached("F03.assert.108", () => count.Maximum));
        var bytesRoot = BasicABC(71); var bytesOriginal = BasicReached("F03.subject.134", () => bytesRoot.WalkOwnedSourceCohort(OwnedPrimaryOwner, new[] { "Rows" }));
        var byteFailure = BasicBridgeFailure("N502-25", FerruleBasicSourceBridgeErrorKind.PairedMetadataLimit,
            () => BasicReached("F03.subject.135", () => bytesRoot.ReindexRetainedPhase(bytesOriginal, BasicReached("F03.subject.136", () => FerrulePhaseSelection.FromOriginal(bytesOriginal, Array.Empty<nuint>())))));
        Equal<FerruleBasicSourceBridgeComponent?>(FerruleBasicSourceBridgeComponent.MetadataBytes, BasicReached("F03.assert.109", () => byteFailure.Component));
        Equal<nuint?>(68, BasicReached("F03.assert.110", () => byteFailure.Used)); Equal<nuint?>(4, BasicReached("F03.assert.111", () => byteFailure.Requested)); Equal<nuint?>(71, BasicReached("F03.assert.112", () => byteFailure.Maximum));
    }

    private static FerruleBasicSourceCandidate BasicDetachedCandidate(FerruleBasicScopeContext root, int index)
    {
        var original = BasicReached("F03.subject.137", () => root.IterateOwnedSource(OwnedPrimaryOwner, new[] { "Rows" }));
        return original[index]; // the holder/backing is not retained by the candidate's witness
    }
    private static FerruleBasicScopeContext BasicDetachedParent(FerruleBasicScopeContext root, int index, nuint phase)
    {
        var raw = BasicDetachedCandidate(root, index);
        return BasicReached("F03.subject.138", () => raw.WithPhasePosition(phase)).Context; // raw candidate is dropped before child phase work
    }
    private static void BasicFullCohortNestedAndPlain()
    {
        var parents = new FerruleInstance[]
        {
            Group(Field("Value", Scalar(Text("r1"))),
                Field("Kids", Repeated(BasicValue("c1"), BasicValue("c2"), BasicValue("c3")))),
            Group(Field("Value", Scalar(Text("r2"))),
                Field("Kids", Repeated(BasicValue("c1"), BasicValue("c2"), BasicValue("c3")))),
            Group(Field("Value", Scalar(Text("r3"))),
                Field("Kids", Repeated(BasicValue("c1"), BasicValue("c2"), BasicValue("c3")))),
            Group(Field("Value", Scalar(Text("r4"))),
                Field("Kids", Repeated(BasicValue("c1"), BasicValue("c2"), BasicValue("c3")))),
            Group(Field("Value", Scalar(Text("r5"))),
                Field("Kids", Repeated(BasicValue("c1"), BasicValue("c2"), BasicValue("c3")))),
            Group(Field("Value", Scalar(Text("r6"))),
                Field("Kids", Repeated(BasicValue("c1"), BasicValue("c2"), BasicValue("c3")))),
            Group(Field("Value", Scalar(Text("r7"))),
                Field("Kids", Repeated(BasicValue("c1"), BasicValue("c2"), BasicValue("c3")))),
            Group(Field("Value", Scalar(Text("r8"))),
                Field("Kids", Repeated(BasicValue("c1"), BasicValue("c2"), BasicValue("c3")))),
            Group(Field("Value", Scalar(Text("r9"))),
                Field("Kids", Repeated(BasicValue("c1"), BasicValue("c2"), BasicValue("c3")))),
            Group(Field("Value", Scalar(Text("r10"))),
                Field("Kids", Repeated(BasicValue("c1"), BasicValue("c2"), BasicValue("c3"))))
        };
        var primary = Group(Field("Rows", new FerruleRepeated(parents)));
        var nestedLimits = new FerruleOwnedSourceLimits(1, 4, 8, 16384, 10);
        OwnedRaw("N502-14.complete-input", new { classification = "actual_constructed_input",
            primary = BasicInstanceSnapshot(primary), declarations = Array.Empty<string>(),
            limits = new { R = (ulong)nestedLimits.MaxRoots, F = (ulong)nestedLimits.MaxActiveFrames,
                S = (ulong)nestedLimits.MaxPathSegments, bytes = (ulong)nestedLimits.MaxMetadataBytes,
                W = (ulong)nestedLimits.MaxWalkContexts } });
        var root = BasicReached("F03.chained.251", () => BasicReached("F03.subject.139", () => FerruleBasicSourceRoots.Create(primary, Array.Empty<FerruleDeclaredNamedSourceRoot>(), nestedLimits)).RootContext());
        var parent = BasicDetachedParent(root, 9, 7);
        var original = BasicReached("F03.subject.140", () => parent.WalkOwnedSourceCohort(OwnedPrimaryOwner, new[] { "Rows", "Kids" }));
        var output = BasicReached("F03.subject.141", () => parent.ReindexRetainedPhase(original, BasicReached("F03.subject.142", () => FerrulePhaseSelection.FromOriginal(original, new nuint[] { 2 }))));
        BasicPhaseRaw("N502-14", output); Equal(1, BasicReached("F03.assert.113", () => output.Count));
        Equal<nuint?>(3, BasicReached("F03.assert.114", () => output[0].RawSourceOrdinal));
        Equal<nuint>(1, BasicReached("F03.assert.115", () => BasicReached("F03.subject.143", () => output[0].Context.OwnedPosition(OwnedPrimaryOwner, new[] { "Rows", "Kids" }))));
        Equal<nuint>(7, BasicReached("F03.assert.116", () => BasicReached("F03.subject.144", () => output[0].Context.OwnedPosition(OwnedPrimaryOwner, new[] { "Rows" }))));
        // Physical associations retain the parent and child independently;
        // the owned Rows/Kids selector names the exact child occurrence.
        Equal(7, BasicReached("F03.assert.117", () => output[0].Context.LegacyView.BasicCollectionIndex(0)));
        Equal(1, BasicReached("F03.assert.118", () => output[0].Context.LegacyView.BasicCollectionIndex(1)));
        Equal(1L, BasicReached("F03.assert.119", () => BasicReached("F03.subject.145", () => output[0].Context.LegacyPosition(new[] { "Rows", "Kids" }))));
        Equal<nuint?>(10, BasicReached("F03.assert.120", () => output[0].Context.OriginAt(2).RawOrdinal));
        var plainRoot = BasicRoot(Group(Field("Rows", Repeated(
            Group(Field("Value", Scalar(Text("r1"))), Field("Details", Group(Field("Counter", Scalar(FerruleValue.FromInt64(1)))))),
            Group(Field("Value", Scalar(Text("r2"))), Field("Details", Group(Field("Counter", Scalar(FerruleValue.FromInt64(2))))))))));
        var plainParent = BasicDetachedParent(plainRoot, 1, 7);
        foreach (var path in new[] { new[] { "Rows" }, new[] { "Rows", "Details" } })
        {
            var source = BasicReached("F03.subject.146", () => plainParent.WalkOwnedSourceCohort(OwnedPrimaryOwner, path));
            var phase = BasicReached("F03.subject.147", () => plainParent.ReindexRetainedPhase(source, BasicReached("F03.subject.148", () => FerrulePhaseSelection.FromOriginal(source, new nuint[] { 0 }))));
            BasicPhaseRaw("N502-15." + path.Length, phase);
            Equal(false, BasicReached("F03.assert.121", () => phase[0].HasNewCursor));
            Equal<nuint>(7, BasicReached("F03.assert.122", () => BasicReached("F03.subject.149", () => phase[0].Context.OwnedPosition(OwnedPrimaryOwner, new[] { "Rows" }))));
            Equal(7L, BasicReached("F03.assert.123", () => BasicReached("F03.subject.150", () => phase[0].Context.LegacyPosition(new[] { "Rows" }))));
        }
        var oldPlainParent = BasicDetachedCandidate(plainRoot, 1).Context;
        var singleton = BasicReached("F03.chained.274", () => BasicReached("F03.subject.151", () => oldPlainParent.IterateSource(new[] { "Details" }))[0].WithPhasePosition(1));
        Equal(false, BasicReached("F03.assert.124", () => singleton.HasNewCursor));
        Equal(2L, BasicReached("F03.assert.125", () => BasicReached("F03.subject.152", () => singleton.Context.LegacyPosition(new[] { "Rows" }))));
        Equal<nuint>(2, BasicReached("F03.assert.126", () => BasicReached("F03.subject.153", () => singleton.Context.OwnedPosition(OwnedPrimaryOwner, new[] { "Rows" }))));
        OwnedValue("B489-C20", FerruleValue.FromInt64(2), BasicReached("F03.subject.154", () => singleton.Context.ResolveLegacyField(new[] { "Counter" })));
        var common = BasicCommon(); var first = BasicReached("F03.subject.155", () => common.IterateSource(new[] { "Rows" }))[0].Context;
        var inner = BasicReached("F03.subject.156", () => first.IterateSource(new[] { "Rows" }))[1].Context;
        var identity = BasicReached("F03.chained.283", () => BasicReached("F03.subject.157", () => inner.IterateOwnedSource(OwnedPrimaryOwner, new[] { "Rows" }))[0].WithPhasePosition(1));
        Equal(false, BasicReached("F03.assert.127", () => identity.HasNewCursor)); Equal<nuint>(2, BasicReached("F03.assert.128", () => BasicReached("F03.subject.158", () => identity.Context.OwnedPosition(OwnedPrimaryOwner))));
        Equal<nuint>(1, BasicReached("F03.assert.129", () => BasicReached("F03.subject.159", () => identity.Context.OwnedPosition(OwnedPrimaryOwner, new[] { "Rows" }))));
        OwnedValue("B489-C10.phase", Text("P.inner12"), BasicReached("F03.subject.160", () => identity.Context.ResolveLegacyField(new[] { "Value" })));
        var rawRoot = BasicRoot(Group(Field("Rows", Repeated(Group(Field("Value", Scalar(FerruleValue.FromInt64(30)))),
            Group(Field("Value", Scalar(FerruleValue.FromInt64(10)))), Group(Field("Value", Scalar(FerruleValue.FromInt64(20))))))));
        var raw = BasicDetachedCandidate(rawRoot, 2);
        var copied = BasicReached("F03.subject.161", () => raw.WithPhasePosition(1));
        Equal<nuint>(3, BasicReached("F03.assert.130", () => BasicReached("F03.subject.162", () => raw.Context.OwnedPosition(OwnedPrimaryOwner, new[] { "Rows" }))));
        Equal<nuint>(1, BasicReached("F03.assert.131", () => BasicReached("F03.subject.163", () => copied.Context.OwnedPosition(OwnedPrimaryOwner, new[] { "Rows" }))));
        Equal<nuint?>(3, BasicReached("F03.assert.132", () => copied.RawSourceOrdinal));
        OwnedValue("B489-C21", FerruleValue.FromInt64(20), BasicReached("F03.subject.164", () => copied.Context.ResolveLegacyField(new[] { "Value" })));
        var invalid = BasicOwnedError("E489-12", FerruleOwnedSourceErrorKind.InvalidPosition,
            () => BasicReached("F03.subject.165", () => raw.WithPhasePosition(0))); Equal<nuint?>(0, BasicReached("F03.assert.133", () => invalid.RequestedIndex));
        var range = BasicBridgeFailure("K489-06", FerruleBasicSourceBridgeErrorKind.LegacyPositionRange,
            () => BasicReached("F03.subject.166", () => raw.WithPhasePosition((nuint)int.MaxValue + 1)));
        Equal<nuint?>((nuint)int.MaxValue + 1, BasicReached("F03.assert.134", () => range.Requested)); Equal<nuint?>(int.MaxValue, BasicReached("F03.assert.135", () => range.Maximum));
    }
    private static void BasicPairedSortFilterWindow()
    {
        var root = BasicRoot(Group(Field("Rows", Repeated(Group(Field("Value", Scalar(FerruleValue.FromInt64(30)))),
            Group(Field("Value", Scalar(FerruleValue.FromInt64(10)))), Group(Field("Value", Scalar(FerruleValue.FromInt64(20))))))));
        var original = BasicReached("F03.subject.167", () => root.WalkOwnedSourceCohort(OwnedPrimaryOwner, new[] { "Rows" }));
        long Value(FerruleBasicSourceCandidate c) => BasicReached("F03.subject.168", () => c.Context.ResolveLegacyField(new[] { "Value" })).Int64Value;
        var sortedIndices = Enumerable.Range(0, original.Count).OrderBy(i => Value(original[i])).Select(i => (nuint)i).ToArray();
        var sorted = BasicReached("F03.subject.169", () => root.ReindexRetainedPhase(original, BasicReached("F03.subject.170", () => FerrulePhaseSelection.FromOriginal(original, sortedIndices))));
        BasicPhaseRaw("B489-C22.sorted", sorted);
        Equal(10L, BasicReached("F03.assert.136", () => Value(sorted[0]))); Equal(20L, BasicReached("F03.assert.137", () => Value(sorted[1]))); Equal(30L, BasicReached("F03.assert.138", () => Value(sorted[2])));
        var filteredIndices = Enumerable.Range(0, sorted.Count).Where(i => Value(sorted[i]) >= 20).Select(i => (nuint)i).ToArray();
        var filtered = BasicReached("F03.subject.171", () => root.ReindexRetainedPhase(original, BasicReached("F03.subject.172", () => FerrulePhaseSelection.FromPhase(sorted, filteredIndices))));
        BasicPhaseRaw("B489-C22.filtered", filtered); Equal(2, BasicReached("F03.assert.139", () => filtered.Count));
        Equal(20L, BasicReached("F03.assert.140", () => Value(filtered[0]))); Equal(30L, BasicReached("F03.assert.141", () => Value(filtered[1])));
        Equal<nuint>(1, BasicReached("F03.assert.142", () => BasicReached("F03.subject.173", () => filtered[0].Context.OwnedPosition(OwnedPrimaryOwner, new[] { "Rows" }))));
        Equal<nuint>(2, BasicReached("F03.assert.143", () => BasicReached("F03.subject.174", () => filtered[1].Context.OwnedPosition(OwnedPrimaryOwner, new[] { "Rows" }))));
        var firstFilter = BasicReached("F03.subject.175", () => root.ReindexRetainedPhase(original,
            BasicReached("F03.subject.176", () => FerrulePhaseSelection.FromOriginal(original, new nuint[] { 1, 2 }))));
        var afterSort = BasicReached("F03.subject.177", () => root.ReindexRetainedPhase(original, BasicReached("F03.subject.178", () => FerrulePhaseSelection.FromPhase(firstFilter, new nuint[] { 0, 1 }))));
        BasicPhaseRaw("B489-C23", afterSort); Equal(10L, BasicReached("F03.assert.144", () => Value(afterSort[0]))); Equal(20L, BasicReached("F03.assert.145", () => Value(afterSort[1])));
        var window = BasicReached("F03.subject.179", () => root.ReindexRetainedPhase(original, BasicReached("F03.subject.180", () => FerrulePhaseSelection.FromPhase(sorted, new nuint[] { 1 }))));
        BasicPhaseRaw("B489-C24", window); Equal(1, BasicReached("F03.assert.146", () => window.Count)); Equal(20L, BasicReached("F03.assert.147", () => Value(window[0])));
        Equal<nuint?>(3, BasicReached("F03.assert.148", () => window[0].RawSourceOrdinal)); Equal<nuint>(1, BasicReached("F03.assert.149", () => BasicReached("F03.subject.181", () => window[0].Context.OwnedPosition(OwnedPrimaryOwner, new[] { "Rows" }))));
        for (var i = 0; i < original.Count; i++) Equal<nuint>((nuint)i + 1,
            BasicReached("F03.assert.150", () => BasicReached("F03.subject.182", () => original[i].Context.OwnedPosition(OwnedPrimaryOwner, new[] { "Rows" }))));
    }
    private static void BasicPairedAllocationControls()
    {
        var zero = new FerruleOwnedSourceLimits(1, 0, 0, 0, 0);
        var small = new FerruleOwnedSourceLimits(2, 2, 3, 1024, 3);
        Equal<nuint>(138, BasicReached("F03.assert.151", () => BasicReached("F03.subject.183", () => BasicSourceAccounting.Maximum(zero, false))));
        Equal<nuint>(4374, BasicReached("F03.assert.152", () => BasicReached("F03.subject.184", () => BasicSourceAccounting.Maximum(small, false))));
        Equal<nuint>(143, BasicReached("F03.assert.153", () => BasicReached("F03.subject.185", () => BasicSourceAccounting.Maximum(zero, true))));
        Equal<nuint>(5957, BasicReached("F03.assert.154", () => BasicReached("F03.subject.186", () => BasicSourceAccounting.Maximum(small, true))));
        Equal<nuint>(7, BasicReached("F03.assert.155", () => BasicReached("F03.subject.187", () => OwnedSourceAccounting.CoreSlots(zero)))); Equal<nuint>(9, BasicReached("F03.assert.156", () => BasicReached("F03.subject.188", () => OwnedSourceAccounting.CSharpSlots(zero))));
        Equal<nuint>(62, BasicReached("F03.assert.157", () => BasicReached("F03.subject.189", () => OwnedSourceAccounting.CoreSlots(small)))); Equal<nuint>(82, BasicReached("F03.assert.158", () => BasicReached("F03.subject.190", () => OwnedSourceAccounting.CSharpSlots(small))));
        var representable = BasicReached("F03.subject.191", () => BasicSourceAccounting.ArrayLength((nuint)Array.MaxLength));
        OwnedRaw("F01.array-capacity", new { actual = representable, expected = Array.MaxLength });
        Equal(Array.MaxLength, BasicReached("F03.assert.159", () => representable));
        var unrepresentable = BasicBridgeFailure("F01.array-capacity-plus-one",
            FerruleBasicSourceBridgeErrorKind.PairedMetadataArithmeticOverflow,
            () => BasicReached("F03.subject.192", () => BasicSourceAccounting.ArrayLength((nuint)Array.MaxLength + 1)));
        Equal<FerruleBasicSourceBridgeComponent?>(FerruleBasicSourceBridgeComponent.MetadataSlots, BasicReached("F03.assert.160", () => unrepresentable.Component));
        foreach (var maximum in new nuint[] { 48, 143, 5957 })
        {
            var counter = new OwnedCounter(maximum, FerruleOwnedSourceLimitKind.MetadataSlots);
            BasicReachedAction("N502-20.first-charge", () => BasicSourceAccounting.Charge(counter, maximum));
            OwnedRaw("N502-20.first-counter", new { actual = new { counterUsed = (ulong)counter.Used }, expectedInput = (ulong)maximum });
            var error = BasicBridgeFailure(maximum == 48 ? "K489-03" : "N502-20." + maximum,
                FerruleBasicSourceBridgeErrorKind.PairedMetadataLimit, () => BasicSourceAccounting.Charge(counter, 1), () => new { counterUsed = (ulong)counter.Used });
            Equal<FerruleBasicSourceBridgeComponent?>(FerruleBasicSourceBridgeComponent.MetadataSlots, BasicReached("F03.assert.161", () => error.Component));
            Equal<nuint?>(maximum, BasicReached("F03.assert.162", () => error.Used)); Equal<nuint?>(1, BasicReached("F03.assert.163", () => error.Requested)); Equal<nuint?>(maximum, BasicReached("F03.assert.164", () => error.Maximum));
            Equal(maximum, BasicReached("F03.assert.165", () => counter.Used));
        }
        if (IntPtr.Size == 8)
        {
            BasicBridgeFailure("N502-18", FerruleBasicSourceBridgeErrorKind.PairedMetadataArithmeticOverflow,
                () => BasicReached("F03.subject.193", () => BasicSourceAccounting.ThreeSets(unchecked((nuint)6_148_914_691_236_517_206UL))));
            var huge = new FerruleOwnedSourceLimits(1, 0, 0, 0, unchecked((nuint)400_000_000_000_000_000UL));
            Equal(unchecked((nuint)15_200_000_000_000_000_138UL), BasicReached("F03.assert.166", () => BasicReached("F03.subject.194", () => BasicSourceAccounting.Maximum(huge, false))));
            BasicBridgeFailure("N502-19", FerruleBasicSourceBridgeErrorKind.PairedMetadataArithmeticOverflow,
                () => BasicReached("F03.subject.195", () => BasicSourceAccounting.Maximum(huge, true)));
        }
        var roots = BasicReached("F03.subject.196", () => FerruleBasicSourceRoots.Create(BasicValue("P.root"),
            new FerruleDeclaredNamedSourceRoot[] { new("Missing", null), new("Ref", BasicValue("N.root")),
                new("Dup", null), new("Dup", BasicValue("D1")), new("Gap", null),
                new("Other", BasicValue("O")), new("Dup", BasicValue("D2")) }, BasicLimits));
        var suppliedIndices = Enumerable.Range(0, roots.SuppliedCount).Select(roots.SuppliedIndex).ToArray();
        var uniqueIndices = Enumerable.Range(0, roots.UniqueCount).Select(roots.UniqueIndex).ToArray();
        var uniqueNames = uniqueIndices.Select(i => roots.OwnedRoots.Name(i + 1)).ToArray();
        OwnedRaw("N502-22", new { actual = new { suppliedCount = roots.SuppliedCount,
            uniqueCount = roots.UniqueCount, supplied = suppliedIndices, unique = uniqueIndices,
            fields = uniqueNames, registryCount = (ulong)roots.OwnedRoots.RegistryCount },
            expectedSourceFacts = new { factorySlots = 5, callerPeak = 18, BT = 40 } });
        Equal(4, BasicReached("F03.assert.167", () => roots.SuppliedCount)); Equal(3, BasicReached("F03.assert.168", () => roots.UniqueCount));
        Equal(1, BasicReached("F03.assert.169", () => roots.SuppliedIndex(0))); Equal(3, BasicReached("F03.assert.170", () => roots.SuppliedIndex(1)));
        Equal(5, BasicReached("F03.assert.171", () => roots.SuppliedIndex(2))); Equal(6, BasicReached("F03.assert.172", () => roots.SuppliedIndex(3)));
        Equal(1, BasicReached("F03.assert.173", () => roots.UniqueIndex(0))); Equal(3, BasicReached("F03.assert.174", () => roots.UniqueIndex(1))); Equal(5, BasicReached("F03.assert.175", () => roots.UniqueIndex(2)));
        var factoryCounter = new OwnedCounter(2, FerruleOwnedSourceLimitKind.MetadataSlots);
        var projection = BasicReached("F03.subject.197", () => FerruleGroup.FromBasicSourceProjection(Array.Empty<FerruleField>(), 0, factoryCounter));
        OwnedRaw("N502-21", new { actual = new { fields = projection.Fields.Select(field =>
            new { field.Name, value = BasicInstanceSnapshot(field.Value) }).ToArray(),
            fieldCount = projection.Fields.Count, counterUsed = (ulong)factoryCounter.Used },
            expectedSourceFacts = new { factorySlots = 2, callerPeak = 2, BT = 5 } });
        Equal(0, BasicReached("F03.assert.176", () => projection.Fields.Count)); Equal<nuint>(2, BasicReached("F03.assert.177", () => factoryCounter.Used));
        var emptyRoot = BasicReached("F03.chained.375", () => BasicReached("F03.subject.198", () => FerruleBasicSourceRoots.Create(Repeated(), Array.Empty<FerruleDeclaredNamedSourceRoot>(), zero)).RootContext());
        var emptyOriginal = BasicReached("F03.subject.199", () => emptyRoot.WalkOwnedSourceCohort(OwnedPrimaryOwner, Array.Empty<string>()));
        var emptyPhase = BasicReached("F03.subject.200", () => emptyRoot.ReindexRetainedPhase(emptyOriginal,
            BasicReached("F03.subject.201", () => FerrulePhaseSelection.FromOriginal(emptyOriginal, Array.Empty<nuint>()))));
        BasicPhaseRaw("N502-23.empty-phase", emptyPhase);
        Equal(0, BasicReached("F03.assert.178", () => emptyPhase.Count));
        var state = emptyRoot.LegacyView.BasicRunStateIdentity;
        OwnedRaw("N502-23.root-state", new { actual = new { stateType = state.GetType().FullName } });
        var clone = BasicReached("F03.subject.202", () => emptyRoot.CloneView());
        var sharesRunState = ReferenceEquals(state, clone.LegacyView.BasicRunStateIdentity);
        OwnedRaw("N502-23", new { actual = new { ordinaryFormula = (ulong)BasicReached("F03.subject.203", () => BasicSourceAccounting.Maximum(zero, false)),
            fullFormula = (ulong)BasicReached("F03.subject.204", () => BasicSourceAccounting.Maximum(zero, true)), sharesRunState,
            originalStateType = state.GetType().FullName, cloneStateType = clone.LegacyView.BasicRunStateIdentity.GetType().FullName },
            expectedSourceFacts = new { ordinary = 138, full = 143, rootStates = 1, newCloneState = false } });
        Equal(true, BasicReached("F03.assert.179", () => ReferenceEquals(state, clone.LegacyView.BasicRunStateIdentity)));
    }

    // These calls model the exact-owner schema proof emitted by the Basic family.
    // Generic owned reads and unproved Basic reads retain their existing policy.
    private static void BasicDeclaredEmptyRootPins()
    {
        var empty = Array.Empty<string>();
        var primary = OwnedPrimaryOwner;
        var named = OwnedNamedOwner("Ref");
        var root = BasicCommon();
        var rows = BasicReached("G632-C01.primary-rows", () => root.IterateOwnedSource(primary, new[] { "Rows" }));
        var paired = BasicReached("G632-C01.named-rows", () => rows[0].Context.IterateOwnedSource(named, new[] { "Rows" }))[1].Context;
        var originalFrames = paired.OwnedView.FrameCount;
        var originalPrimaryItem = paired.OwnedView.FrameAt(0).Item;
        var originalNamedItem = paired.OwnedView.FrameAt(1).Item;
        var primaryRoot = BasicReached("G632-C01.primary-root", () => paired.ResolveOwnedFieldWithRootPin(primary, new[] { "Value" }, empty, true));
        var namedRoot = BasicReached("G632-C01.named-root", () => paired.ResolveOwnedFieldWithRootPin(named, new[] { "Value" }, empty, true));
        var primaryRow = BasicReached("G632-C01.primary-nearest", () => paired.ResolveOwnedFieldWithRootPin(primary, new[] { "Rows", "Value" }, null, true));
        var namedRow = BasicReached("G632-C01.named-nearest", () => paired.ResolveOwnedFieldWithRootPin(named, new[] { "Rows", "Value" }, null, true));
        OwnedValue("G632-C01.primary-root-check", Text("P.root"), primaryRoot);
        OwnedValue("G632-C01.named-root-check", Text("N.root"), namedRoot);
        OwnedValue("G632-C01.primary-nearest-check", Text("P1"), primaryRow);
        OwnedValue("G632-C01.named-nearest-check", Text("N2"), namedRow);
        Equal(originalFrames, paired.OwnedView.FrameCount);
        Equal(true, ReferenceEquals(originalPrimaryItem, paired.OwnedView.FrameAt(0).Item));
        Equal(true, ReferenceEquals(originalNamedItem, paired.OwnedView.FrameAt(1).Item));
        OwnedIndex("G632-C01.primary-position", 1, BasicReached("G632-C01.primary-position-raw", () => paired.OwnedPosition(primary, new[] { "Rows" })));
        OwnedIndex("G632-C01.named-position", 2, BasicReached("G632-C01.named-position-raw", () => paired.OwnedPosition(named, new[] { "Rows" })));
        var rootPath = BasicOwnedError("G632-C01.absolute-root-path", FerruleOwnedSourceErrorKind.UniteratedRepetition,
            () => paired.ResolveOwnedFieldWithRootPin(primary, new[] { "Rows", "Value" }, empty, true));
        OwnedPath(new[] { "Rows", "Value" }, rootPath.Path); Equal<nuint?>(1, rootPath.Segment);
        BasicOwnedError("G632-C01.root-position-stays-inactive", FerruleOwnedSourceErrorKind.InactiveSourceFrame,
            () => root.OwnedPosition(primary, empty));

        var unproved = BasicOwnedError("G632-C02.unproved-Basic-root", FerruleOwnedSourceErrorKind.InactiveSourceFrame,
            () => root.ResolveOwnedField(primary, new[] { "Value" }, empty));
        OwnedPath(empty, unproved.Collection); Equal<string?>(null, unproved.RequestedName);
        var declaredRepeated = BasicOwnedError("G632-C02.declared-repeating-root", FerruleOwnedSourceErrorKind.InactiveSourceFrame,
            () => root.ResolveOwnedFieldWithRootPin(primary, new[] { "Value" }, empty, false));
        OwnedPath(empty, declaredRepeated.Collection); Equal<string?>(null, declaredRepeated.RequestedName);
        var generic = FerruleOwnedSourceRoots.Create(BasicValue("generic"), Array.Empty<FerruleDeclaredNamedSourceRoot>(), BasicLimits).RootContext();
        try { _ = generic.ResolveField(primary, new[] { "Value" }, empty); throw new InvalidOperationException("Expected unchanged generic empty-pin refusal."); }
        catch (FerruleOwnedSourceException error)
        {
            BasicErrorRaw("G632-C02.generic-root", error);
            Equal(FerruleOwnedSourceErrorKind.InactiveSourceFrame, error.Error); OwnedPath(empty, error.Collection);
            Equal(true, error.Owner?.IsPrimary); Equal<string?>(null, error.RequestedName);
        }
        var nonempty = BasicOwnedError("G632-C02.nonempty-stays-inactive", FerruleOwnedSourceErrorKind.InactiveSourceFrame,
            () => root.ResolveOwnedFieldWithRootPin(primary, new[] { "Rows", "Value" }, new[] { "Rows" }, true));
        OwnedPath(new[] { "Rows" }, nonempty.Collection);
        var incompatible = BasicOwnedError("G632-C02.nonempty-prefix-priority", FerruleOwnedSourceErrorKind.IncompatibleFramePath,
            () => root.ResolveOwnedFieldWithRootPin(primary, new[] { "Value" }, new[] { "Rows" }, true));
        OwnedPath(new[] { "Value" }, incompatible.Path); OwnedPath(new[] { "Rows" }, incompatible.Frame);

        foreach (var declaredNonrepeating in new[] { false, true })
        {
            var actualRepeated = BasicRoot(Repeated(BasicValue("flat1"), BasicValue("flat2")));
            var inactive = BasicOwnedError("G632-C03.actual-repeated-unwalked-" + declaredNonrepeating,
                FerruleOwnedSourceErrorKind.InactiveSourceFrame,
                () => actualRepeated.ResolveOwnedFieldWithRootPin(primary, new[] { "Value" }, empty, declaredNonrepeating));
            OwnedPath(empty, inactive.Collection);
            var actualRows = BasicReached("G632-C03.actual-root-walk-" + declaredNonrepeating,
                () => actualRepeated.IterateOwnedSource(primary, empty));
            OwnedValue("G632-C03.actual-root-value-" + declaredNonrepeating, Text("flat2"),
                BasicReached("G632-C03.actual-root-return-" + declaredNonrepeating,
                    () => actualRows[1].Context.ResolveOwnedFieldWithRootPin(primary, new[] { "Value" }, empty, declaredNonrepeating)));
            OwnedIndex("G632-C03.actual-root-position-" + declaredNonrepeating, 2,
                BasicReached("G632-C03.actual-root-index-" + declaredNonrepeating, () => actualRows[1].Context.OwnedPosition(primary, empty)));
        }
        var foreign = BasicRoot(Repeated(BasicValue("primary-row")), new FerruleDeclaredNamedSourceRoot("Ref", Repeated(BasicValue("named-row"))));
        var foreignRows = BasicReached("G632-C03.foreign-walk", () => foreign.IterateOwnedSource(named, empty));
        var foreignInactive = BasicOwnedError("G632-C03.foreign-root-is-not-primary", FerruleOwnedSourceErrorKind.InactiveSourceFrame,
            () => foreignRows[0].Context.ResolveOwnedFieldWithRootPin(primary, new[] { "Value" }, empty, true));
        OwnedPath(empty, foreignInactive.Collection); Equal(true, foreignInactive.Owner?.IsPrimary);

        var scalarRoot = BasicRoot(Scalar(Text("scalar-root")), new FerruleDeclaredNamedSourceRoot("Ref", Scalar(FerruleValue.Null)));
        var scalar = BasicReached("G632-C04.scalar", () => scalarRoot.ResolveOwnedFieldWithRootPin(primary, empty, empty, true));
        var presentNull = BasicReached("G632-C04.present-null", () => scalarRoot.ResolveOwnedFieldWithRootPin(named, empty, empty, true));
        OwnedValue("G632-C04.scalar-check", Text("scalar-root"), scalar);
        OwnedValue("G632-C04.present-null-check", FerruleValue.Null, presentNull);
        var absent = BasicRoot(Group(), new FerruleDeclaredNamedSourceRoot("Ref", Group()));
        var missing = BasicReached("G632-C04.missing-field", () => absent.ResolveOwnedFieldWithRootPin(named, new[] { "Value" }, empty, true));
        OwnedValue("G632-C04.missing-field-check", FerruleValue.Null, missing);
        var unavailable = BasicOwnedError("G632-C04.unavailable-before-pin", FerruleOwnedSourceErrorKind.SourceUnavailable,
            () => BasicGaps().ResolveOwnedFieldWithRootPin(OwnedNamedOwner("Missing"), new[] { "Value" }, empty, true));
        Equal("Missing", unavailable.RequestedName);
        var malformed = BasicOwnedError("G632-C04.malformed-root", FerruleOwnedSourceErrorKind.Traversal,
            () => scalarRoot.ResolveOwnedFieldWithRootPin(primary, new[] { "Value" }, empty, true));
        Equal<nuint?>(0, malformed.Segment); Equal<FerruleOwnedSourceInstanceKind?>(FerruleOwnedSourceInstanceKind.Scalar, malformed.FoundInstance);
        OwnedPath(new[] { "Value" }, malformed.Path);
        Equal(FerruleOwnedSourceTraversalKind.CannotTraverse, ((FerruleOwnedSourceTraversalException)malformed.InnerException!).Error);
        foreach (var unsupported in new FerruleInstance[] {
            new FerruleDocumentSet(new[] { new FerruleDocument("doc", Group()) }),
            new FerruleMappedSequence(new FerruleInstance[] { BasicValue("mapped") }),
        })
        {
            var actual = BasicOwnedError("G632-C04.unsupported-root-" + unsupported.GetType().Name,
                FerruleOwnedSourceErrorKind.UnsupportedSourceKind,
                () => BasicRoot(unsupported).ResolveOwnedFieldWithRootPin(primary, empty, empty, true));
            OwnedPath(empty, actual.Path);
            Equal<FerruleOwnedSourceInstanceKind?>(unsupported is FerruleDocumentSet ? FerruleOwnedSourceInstanceKind.DocumentSet : FerruleOwnedSourceInstanceKind.MappedSequence, actual.FoundInstance);
        }
    }

    private static void BasicOwnedLeafNodeProvenance()
    {
        FerruleRuntimeException Capture(string label, Action action)
        {
            try { action(); }
            catch (FerruleRuntimeException error)
            {
                BasicErrorRaw(label, error);
                return error;
            }
            throw new InvalidOperationException("Expected a complete typed leaf refusal.");
        }

        void Annotate(string label, FerruleRuntimeException error, uint node)
        {
            var cause = error.InnerException;
            var causeInner = cause?.InnerException;
            var originalStack = error.StackTrace;
            var originalCauseStack = cause?.StackTrace;
            var originalSource = error.Source;
            var originalHResult = error.HResult;
            var originalTarget = error.TargetSite;
            var originalMessage = error.Message;
            var originalCategory = error.Error;
            var marker = new object();
            error.HelpLink = "https://example.invalid/owned-leaf";
            error.Data["sentinel"] = marker;
            var properties = typeof(FerruleRuntimeException)
                .GetProperties(System.Reflection.BindingFlags.Public | System.Reflection.BindingFlags.Instance | System.Reflection.BindingFlags.DeclaredOnly)
                .Where(property => property.Name != "Node")
                .Select(property => (Property: property, Value: property.GetValue(error))).ToArray();
            var returned = BasicReached(label + ".annotation", () => error.WithOwnedSourceNode(node));
            BasicErrorRaw(label + ".complete", returned, new
            {
                sameOuter = ReferenceEquals(error, returned), sameCause = ReferenceEquals(cause, returned.InnerException),
                sameNestedCause = ReferenceEquals(causeInner, returned.InnerException?.InnerException),
                originalStack, originalCauseStack, originalSource, originalHResult,
                helpLink = returned.HelpLink, sameData = ReferenceEquals(marker, returned.Data["sentinel"]),
            });
            Equal(true, ReferenceEquals(error, returned));
            Equal<uint?>(node, returned.Node);
            Equal(true, ReferenceEquals(cause, returned.InnerException));
            Equal(true, ReferenceEquals(causeInner, returned.InnerException?.InnerException));
            Equal(originalStack, returned.StackTrace);
            Equal(originalCauseStack, returned.InnerException?.StackTrace);
            Equal(originalSource, returned.Source);
            Equal(originalHResult, returned.HResult);
            Equal(originalTarget, returned.TargetSite);
            Equal(originalMessage, returned.Message);
            Equal(originalCategory, returned.Error);
            Equal("https://example.invalid/owned-leaf", returned.HelpLink);
            Equal(true, ReferenceEquals(marker, returned.Data["sentinel"]));
            foreach (var property in properties)
                Equal(property.Value, property.Property.GetValue(returned));
            var enclosing = BasicReached(label + ".enclosing", () => returned.WithOwnedSourceNode(node == 0 ? uint.MaxValue : 0));
            Equal(true, ReferenceEquals(returned, enclosing));
            Equal<uint?>(node, enclosing.Node);
            Equal(originalStack, enclosing.StackTrace);
        }

        foreach (var owner in new[] { OwnedPrimaryOwner, OwnedNamedOwner("Ref") })
        {
            var label = owner.IsPrimary ? "N651.primary" : "N651.named";
            var position = Capture(label + ".position", () => BasicCommon().OwnedPosition(owner, Array.Empty<string>()));
            Equal<uint?>(null, position.Node);
            Equal(FerruleRuntimeError.OwnedSource, position.Error);
            var positionCause = position.OwnedSourceFailure!;
            Equal(FerruleOwnedSourceErrorKind.InactiveSourceFrame, positionCause.Error);
            Equal(owner.IsPrimary, positionCause.Owner!.IsPrimary);
            Equal(owner.Name, positionCause.Owner!.Name);
            Equal(owner.Name, positionCause.RequestedName);
            OwnedPath(Array.Empty<string>(), positionCause.Collection);
            Annotate(label + ".position", position, 2);
        }

        var malformed = Capture("N651.field", () => BasicCommon().ResolveOwnedField(OwnedPrimaryOwner, new[] { "Details" }));
        Equal<uint?>(null, malformed.Node);
        Equal(FerruleOwnedSourceErrorKind.Traversal, malformed.OwnedSourceFailure!.Error);
        var traversal = (FerruleOwnedSourceTraversalException)malformed.InnerException!.InnerException!;
        Equal(FerruleOwnedSourceTraversalKind.ExpectedScalar, traversal.Error);
        Equal(FerruleOwnedSourceInstanceKind.Group, traversal.Found);
        OwnedPath(new[] { "Details" }, traversal.Path);
        Annotate("N651.field", malformed, 0);

        var missing = Capture("N651.missing", () => BasicGaps().ResolveOwnedField(OwnedNamedOwner("Missing"), new[] { "Value" }));
        Equal<uint?>(null, missing.Node);
        Equal(FerruleOwnedSourceErrorKind.SourceUnavailable, missing.OwnedSourceFailure!.Error);
        Equal("Missing", missing.OwnedSourceFailure.RequestedName);
        Annotate("N651.missing", missing, uint.MaxValue);

        var bridgeCause = new FerruleBasicSourceBridgeException(
            FerruleBasicSourceBridgeErrorKind.PairedMetadataLimit,
            component: FerruleBasicSourceBridgeComponent.WalkContexts, used: 1, requested: 2, maximum: 2,
            inputOrdinal: 3, candidateOrdinal: 4, declarationIndex: 5, declaredCount: 6,
            found: FerruleOwnedSourceInstanceKind.Group);
        var bridge = FerruleRuntimeException.FromBasicSourceBridge(bridgeCause);
        Equal<uint?>(null, bridge.Node);
        Equal(true, ReferenceEquals(bridgeCause, bridge.BasicSourceBridgeFailure));
        Annotate("N651.bridge", bridge, uint.MaxValue);

        var ordinary = new FerruleRuntimeException(FerruleRuntimeError.MappingException, "ordinary", node: 7, mappingExceptionMessage: "ordinary");
        var returnedOrdinary = BasicReached("N651.ordinary", () => ordinary.WithOwnedSourceNode(2));
        Equal(true, ReferenceEquals(ordinary, returnedOrdinary));
        Equal<uint?>(7, returnedOrdinary.Node);
        Equal("ordinary", returnedOrdinary.MappingExceptionMessage);
        var untyped = new FerruleRuntimeException(FerruleRuntimeError.OwnedSource, "no typed cause");
        Equal(true, ReferenceEquals(untyped, BasicReached("N651.untyped", () => untyped.WithOwnedSourceNode(2))));
        Equal<uint?>(null, untyped.Node);
        var unrelated = new FerruleRuntimeException(FerruleRuntimeError.DivideByZero, "unrelated", new InvalidOperationException("unrelated inner"));
        Equal(true, ReferenceEquals(unrelated, BasicReached("N651.unrelated", () => unrelated.WithOwnedSourceNode(2))));
        Equal<uint?>(null, unrelated.Node);

        var untouched = Capture("N651.untouched-default", () => BasicCommon().OwnedPosition(OwnedPrimaryOwner, Array.Empty<string>()));
        Equal<uint?>(null, untouched.Node);
        Equal(FerruleOwnedSourceErrorKind.InactiveSourceFrame, untouched.OwnedSourceFailure!.Error);
    }

    private static void BasicStreamingFilterPhase()
    {
        var data = Group(Field("Rows", Repeated(BasicValue("a"), BasicValue("b"), BasicValue("c"))));
        var policy = new FerruleOwnedSourceLimits(4, 4, 8, 1024, 3);
        OwnedRaw("N645.input", new { classification = "complete_constructed_input_before_subject",
            primary = BasicInstanceSnapshot(data), declarations = Array.Empty<string>(),
            path = new[] { "Rows" }, acceptedOriginalIndices = new[] { 1, 2 },
            limits = new { roots = (ulong)policy.MaxRoots, frames = (ulong)policy.MaxActiveFrames,
                segments = (ulong)policy.MaxPathSegments, bytes = (ulong)policy.MaxMetadataBytes,
                contexts = (ulong)policy.MaxWalkContexts } });
        var root = BasicReached("N645.root", () => FerruleBasicSourceRoots.Create(data,
            Array.Empty<FerruleDeclaredNamedSourceRoot>(), policy).RootContext());
        var original = BasicReached("N645.walk", () => root.WalkOwnedSourceCohort(OwnedPrimaryOwner, new[] { "Rows" }));
        BasicPhaseRaw("N645.original", original);
        var phase = BasicReached("N645.begin", () => root.BeginRetainedFilterPhase(original));
        var b = BasicReached("N645.produce-b", () => phase.Produce(1));
        var c = BasicReached("N645.produce-c", () => phase.Produce(2));
        BasicPhaseRaw("N645.compact", new[] { b, c });
        Equal<nuint?>(2, b.RawSourceOrdinal); Equal<nuint?>(3, c.RawSourceOrdinal);
        Equal<nuint>(1, b.SourceId); Equal<nuint>(2, c.SourceId);
        Equal<nuint>(2, original[1].Context.OwnedPosition(OwnedPrimaryOwner, new[] { "Rows" }));
        Equal<nuint>(1, b.Context.OwnedPosition(OwnedPrimaryOwner, new[] { "Rows" }));
        Equal<nuint>(2, c.Context.OwnedPosition(OwnedPrimaryOwner, new[] { "Rows" }));
        Equal(1L, b.Context.LegacyPosition(new[] { "Rows" }));
        Equal(2L, c.Context.LegacyPosition(new[] { "Rows" }));
        OwnedValue("N645.value", Text("b"), b.Context.ResolveOwnedField(OwnedPrimaryOwner, new[] { "Rows", "Value" }));

        var duplicate = BasicBridgeFailure("N645.duplicate", FerruleBasicSourceBridgeErrorKind.DuplicatePhaseCandidate, () => phase.Produce(2));
        Equal<nuint?>(2, duplicate.InputOrdinal); Equal<nuint?>(2, duplicate.CandidateOrdinal);
        var decreasing = BasicBridgeFailure("N645.decreasing", FerruleBasicSourceBridgeErrorKind.InvalidCohortWitness, () => phase.Produce(0));
        Equal<nuint?>(0, decreasing.InputOrdinal);
        var range = BasicBridgeFailure("N645.range", FerruleBasicSourceBridgeErrorKind.InvalidCohortWitness, () => phase.Produce(nuint.MaxValue));
        Equal<nuint?>(nuint.MaxValue, range.InputOrdinal);
        var other = BasicABC();
        BasicBridgeFailure("N645.registry", FerruleBasicSourceBridgeErrorKind.RegistryMismatch, () => other.BeginRetainedFilterPhase(original));
        var cloned = root.CloneView();
        BasicBridgeFailure("N645.parent", FerruleBasicSourceBridgeErrorKind.InvalidCohortWitness, () => cloned.BeginRetainedFilterPhase(original));

        // Independent byte boundary: three "Rows" paths per original/copy;
        // 3 originals plus 3 copies require 6 * 3 * 4 = 72 UTF-8 bytes.
        var boundedRoot = BasicABC(71);
        var boundedOriginal = boundedRoot.WalkOwnedSourceCohort(OwnedPrimaryOwner, new[] { "Rows" });
        var bounded = BasicReached("N645.bytes.begin", () => boundedRoot.BeginRetainedFilterPhase(boundedOriginal));
        BasicReached("N645.bytes.first", () => bounded.Produce(0));
        BasicReached("N645.bytes.second", () => bounded.Produce(1));
        var failure = BasicBridgeFailure("N645.bytes.third", FerruleBasicSourceBridgeErrorKind.PairedMetadataLimit, () => bounded.Produce(2));
        Equal<FerruleBasicSourceBridgeComponent?>(FerruleBasicSourceBridgeComponent.MetadataBytes, failure.Component);
        Equal<nuint?>(68, failure.Used); Equal<nuint?>(4, failure.Requested); Equal<nuint?>(71, failure.Maximum);
        BasicReached("N645.bytes.detached-convenience", () => boundedOriginal[2].WithPhasePosition(1));
        var exactRoot = BasicABC(72);
        var exactOriginal = exactRoot.WalkOwnedSourceCohort(OwnedPrimaryOwner, new[] { "Rows" });
        var exact = exactRoot.BeginRetainedFilterPhase(exactOriginal);
        BasicPhaseRaw("N645.bytes.exact", new[] { exact.Produce(0), exact.Produce(1), exact.Produce(2) });

        var noRows = BasicRoot(Group(Field("Rows", Repeated())));
        var empty = noRows.WalkOwnedSourceCohort(OwnedPrimaryOwner, new[] { "Rows" });
        var emptyPhase = BasicReached("N645.empty.begin", () => noRows.BeginRetainedFilterPhase(empty));
        BasicBridgeFailure("N645.empty.range", FerruleBasicSourceBridgeErrorKind.InvalidCohortWitness, () => emptyPhase.Produce(0));
        var nestedRoot = BasicRoot(Group(Field("Rows", Repeated(
            Group(Field("Value", Scalar(Text("r1"))), Field("Kids", Repeated(BasicValue("k1")))),
            Group(Field("Value", Scalar(Text("r2"))), Field("Kids", Repeated(BasicValue("k1"), BasicValue("k2"))))))));
        var parents = nestedRoot.WalkOwnedSourceCohort(OwnedPrimaryOwner, new[] { "Rows" });
        var parentPhase = nestedRoot.BeginRetainedFilterPhase(parents);
        var parent = parentPhase.Produce(1).Context;
        var children = parent.WalkOwnedSourceCohort(OwnedPrimaryOwner, new[] { "Rows", "Kids" });
        var child = parent.BeginRetainedFilterPhase(children).Produce(1);
        BasicPhaseRaw("N645.nested", new[] { child });
        Equal<nuint?>(2, child.RawSourceOrdinal);
        Equal<nuint>(1, child.Context.OwnedPosition(OwnedPrimaryOwner, new[] { "Rows" }));
        Equal<nuint>(1, child.Context.OwnedPosition(OwnedPrimaryOwner, new[] { "Rows", "Kids" }));
        Equal(1L, child.Context.LegacyPosition(new[] { "Rows", "Kids" }));
        var identity = parent.WalkOwnedSourceCohort(OwnedPrimaryOwner, new[] { "Rows" });
        var unchanged = parent.BeginRetainedFilterPhase(identity).Produce(0);
        BasicPhaseRaw("N645.identity", new[] { unchanged });
        Equal(false, unchanged.HasNewCursor);
        Equal<nuint>(1, unchanged.Context.OwnedPosition(OwnedPrimaryOwner, new[] { "Rows" }));
        Equal<nuint?>(null, unchanged.RawSourceOrdinal);

        var plain = BasicReached("N645.plain.walk", () => root.WalkSourceCohort(new[] { "Rows" }));
        BasicPhaseRaw("N645.plain.original", plain);
        var plainPhase = root.BeginRetainedFilterPhase(plain);
        var plainB = BasicReached("N645.plain.produce", () => plainPhase.Produce(1));
        BasicPhaseRaw("N645.plain.compact", new[] { plainB });
        Equal<nuint>(1, plainB.SourceId); Equal<nuint?>(2, plainB.RawSourceOrdinal);
        Equal<nuint>(2, plain[1].Context.OwnedPosition(OwnedPrimaryOwner, new[] { "Rows" }));
        Equal<nuint>(1, plainB.Context.OwnedPosition(OwnedPrimaryOwner, new[] { "Rows" }));
        Equal(1L, plainB.Context.LegacyPosition(new[] { "Rows" }));
        var plainBoundedOriginal = boundedRoot.WalkSourceCohort(new[] { "Rows" });
        var plainBounded = boundedRoot.BeginRetainedFilterPhase(plainBoundedOriginal);
        BasicReached("N645.plain.bytes.first", () => plainBounded.Produce(0));
        BasicReached("N645.plain.bytes.second", () => plainBounded.Produce(1));
        var plainFailure = BasicBridgeFailure("N645.plain.bytes.third", FerruleBasicSourceBridgeErrorKind.PairedMetadataLimit, () => plainBounded.Produce(2));
        Equal<FerruleBasicSourceBridgeComponent?>(FerruleBasicSourceBridgeComponent.MetadataBytes, plainFailure.Component);
        Equal<nuint?>(68, plainFailure.Used); Equal<nuint?>(4, plainFailure.Requested); Equal<nuint?>(71, plainFailure.Maximum);
        var plainExactOriginal = exactRoot.WalkSourceCohort(new[] { "Rows" });
        var plainExact = exactRoot.BeginRetainedFilterPhase(plainExactOriginal);
        BasicPhaseRaw("N645.plain.bytes.exact", new[] { plainExact.Produce(0), plainExact.Produce(1), plainExact.Produce(2) });
        GC.KeepAlive(plain); GC.KeepAlive(plainBoundedOriginal); GC.KeepAlive(plainExactOriginal);
        GC.KeepAlive(original); GC.KeepAlive(boundedOriginal); GC.KeepAlive(exactOriginal);
        GC.KeepAlive(parents); GC.KeepAlive(children); GC.KeepAlive(identity);
    }
}
