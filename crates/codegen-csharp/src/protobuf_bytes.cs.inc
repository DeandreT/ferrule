    private enum PbKind { Text, Bool, Int64 }
    private sealed record PbField(string Name, int Tag, PbKind Kind, bool Required);
    private const int PbByteLimit = 4 * 1024 * 1024, PbBundleLimit = 16 * 1024 * 1024;
    private static readonly global::System.Text.UTF8Encoding PbUtf8 = new(false, true);
    private static global::System.IO.InvalidDataException PbError(string message) => new(message);

    private static ulong PbRead(byte[] bytes, ref int pos)
    {
        ulong value = 0;
        for (int shift = 0; shift <= 63 && pos < bytes.Length; shift += 7)
        {
            var b = bytes[pos++];
            if (shift == 63 && b > 1) break;
            value |= (ulong)(b & 127) << shift;
            if (b < 128) return value;
        }
        throw PbError("Invalid protobuf varint");
    }

    private static global::Ferrule.Runtime.FerruleInstance PbParse(byte[] bytes, PbField[] schema)
    {
        if (bytes.Length > PbByteLimit) throw PbError("Protobuf input exceeds limit");
        var values = new global::Ferrule.Runtime.FerruleScalar?[schema.Length];
        for (int pos = 0, fields = 0; pos < bytes.Length;)
        {
            if (++fields > 65536) throw PbError("Too many protobuf fields");
            ulong key = PbRead(bytes, ref pos);
            if (key < 8 || key > uint.MaxValue) throw PbError("Invalid protobuf key");
            int tag = (int)(key >> 3), wire = (int)(key & 7);
            int index = global::System.Array.FindIndex(schema, f => f.Tag == tag);
            if (index >= 0 && wire != (schema[index].Kind == PbKind.Text ? 2 : 0)) throw PbError("Wrong protobuf wire type");
            if (wire == 0)
            {
                ulong n = PbRead(bytes, ref pos);
                if (index >= 0) values[index] = new(schema[index].Kind == PbKind.Bool ? global::Ferrule.Runtime.FerruleValue.FromBoolean(n != 0) : global::Ferrule.Runtime.FerruleValue.FromInt64(unchecked((long)n)));
            }
            else
            {
                ulong len = wire switch { 1 => 8UL, 2 => PbRead(bytes, ref pos), 5 => 4UL, _ => throw PbError("Unsupported protobuf wire type") };
                if (len > (ulong)(bytes.Length - pos)) throw PbError("Truncated protobuf field");
                if (index >= 0) values[index] = new(global::Ferrule.Runtime.FerruleValue.FromString(PbUtf8.GetString(bytes, pos, (int)len)));
                pos += (int)len;
            }
        }
        var result = new global::Ferrule.Runtime.FerruleField[schema.Length];
        for (int i = 0; i < schema.Length; i++)
            result[i] = new(schema[i].Name, values[i] ?? (schema[i].Required ? throw PbError("Missing required field") : new(schema[i].Kind switch {
                PbKind.Text => global::Ferrule.Runtime.FerruleValue.FromString(""), PbKind.Bool => global::Ferrule.Runtime.FerruleValue.FromBoolean(false), _ => global::Ferrule.Runtime.FerruleValue.FromInt64(0) })));
        return new global::Ferrule.Runtime.FerruleGroup(result);
    }

    private static byte[] PbWrite(global::Ferrule.Runtime.FerruleInstance instance, PbField[] schema, ref int remaining)
    {
        if (instance is not global::Ferrule.Runtime.FerruleGroup group) throw PbError("Expected protobuf group");
        int limit = global::System.Math.Min(PbByteLimit, remaining);
        using var buffer = new global::System.IO.MemoryStream();
        void Reserve(int count) { if (count > limit - buffer.Length) throw PbError("Protobuf output exceeds limit"); }
        void Varint(ulong n) { while (n >= 128) { Reserve(1); buffer.WriteByte((byte)(128 | (n & 127))); n >>= 7; } Reserve(1); buffer.WriteByte((byte)n); }
        foreach (var field in schema)
        {
            if (!group.TryGetField(field.Name, out var item) || item is global::Ferrule.Runtime.FerruleScalar { Value.Kind: global::Ferrule.Runtime.FerruleValueKind.Null })
            { if (field.Required) throw PbError("Missing required field"); continue; }
            if (item is not global::Ferrule.Runtime.FerruleScalar scalar) throw PbError("Expected protobuf scalar");
            if (field.Kind == PbKind.Text)
            {
                var text = scalar.Value.StringValue;
                if (text.Length == 0 && !field.Required) continue;
                var size = PbUtf8.GetByteCount(text);
                Varint(((ulong)field.Tag << 3) | 2); Varint((ulong)size); Reserve(size); buffer.Write(PbUtf8.GetBytes(text));
            }
            else
            {
                ulong n = field.Kind == PbKind.Bool ? (scalar.Value.BooleanValue ? 1UL : 0UL) : unchecked((ulong)scalar.Value.Int64Value);
                if (n == 0 && !field.Required) continue;
                Varint((ulong)field.Tag << 3); Varint(n);
            }
        }
        remaining -= (int)buffer.Length;
        return buffer.ToArray();
    }

    public sealed record ProtobufDocument(string? Name, byte[] Bytes);
    private static (global::Ferrule.Runtime.FerruleInstance Primary, NamedInput[] Named) PbAdmit(byte[] primary, ProtobufDocument[] named)
    {
        if (named.Length != PbNames.Length || primary.Length > PbByteLimit) throw PbError("Protobuf input admission failed");
        var inputs = new byte[PbNames.Length][];
        int remaining = PbBundleLimit - primary.Length;
        foreach (var document in named)
        {
            int index = global::System.Array.IndexOf(PbNames, document.Name);
            if (index < 0 || inputs[index] != null || document.Bytes.Length > PbByteLimit || document.Bytes.Length > remaining) throw PbError("Invalid named protobuf input");
            remaining -= document.Bytes.Length; inputs[index] = document.Bytes;
        }
        var parsed = new NamedInput[inputs.Length];
        for (int i = 0; i < inputs.Length; i++) parsed[i] = new(PbNames[i], PbParse(inputs[i], PbInputs[i+1]));
        return (PbParse(primary, PbInputs[0]), parsed);
    }

    public static ProtobufDocument[] ExecuteProtobufOutputs(byte[] primary, ProtobufDocument[] named, global::Ferrule.Runtime.FerruleExecutionContext? host = null)
    {
        var parsed = PbAdmit(primary, named);
        var mapped = ExecuteOutputs(CreateContext(parsed.Primary, parsed.Named, host));
        var outputs = new ProtobufDocument[PbOutputs.Length]; int remaining = PbBundleLimit;
        outputs[0] = new(null, PbWrite(mapped.Primary, PbOutputs[0], ref remaining));
        for (int i = 0; i < mapped.Extras.Count; i++) outputs[i+1] = new(mapped.Extras[i].Name, PbWrite(mapped.Extras[i].Instance, PbOutputs[i+1], ref remaining));
        return outputs;
    }

    public static ProtobufDocument ExecuteSelectedProtobufOutput(byte[] primary, ProtobufDocument[] named, global::Ferrule.Runtime.FerruleTargetSelection selection, global::Ferrule.Runtime.FerruleExecutionContext? host = null)
    {
        int index = ResolveTarget(selection), remaining = PbBundleLimit;
        var parsed = PbAdmit(primary, named);
        var mapped = ExecuteSelectedTargetWithHost(parsed.Primary, selection, parsed.Named, host, dynamicSourceLoader: null);
        return mapped switch {
            SelectedTargetOutput.Primary p => new(null, PbWrite(p.Instance, PbOutputs[0], ref remaining)),
            SelectedTargetOutput.Named n => new(n.Output.Name, PbWrite(n.Output.Instance, PbOutputs[index], ref remaining)),
            _ => throw PbError("Invalid protobuf target") };
    }
