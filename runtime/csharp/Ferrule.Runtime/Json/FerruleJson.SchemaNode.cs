using System.Text.Json;

namespace Ferrule.Runtime;

public static partial class FerruleJson
{
    private sealed class JsonSchemaNode
    {
        public JsonSchemaNode(
            string name,
            bool repeating,
            bool nullable,
            bool containerNullable,
            bool? jsonObjectItemNullable,
            bool jsonArrayWrapper,
            bool jsonAny,
            IReadOnlyList<string> jsonFormats,
            JsonScalarDomain scalarDomain,
            string? fixedLexical,
            FerruleValue? fixedValue,
            JsonAllowedValues? jsonAllowedValues,
            JsonNumericRange? numericRange,
            JsonMultipleOfConstraints? jsonMultipleOf,
            JsonStringLengthRange? stringLengthRange,
            JsonPatternConstraints? jsonPatterns,
            JsonItemCountRange? itemCountRange,
            IReadOnlyList<JsonContainsConstraint> jsonContains,
            JsonPropertyCountRange? propertyCountRange,
            IReadOnlyList<JsonPropertyDependency> propertyDependencies,
            IReadOnlyList<JsonDependentSchema> dependentSchemas,
            JsonPropertyNameConstraints? propertyNames,
            JsonPatternPropertyNames? patternPropertyNames,
            bool jsonUniqueItems,
            IReadOnlyList<JsonSchemaNode> children,
            JsonSchemaNode? dynamic,
            IReadOnlyList<string> required,
            IReadOnlyList<JsonAlternative> alternatives,
            bool inclusiveAlternatives,
            bool xmlTypeAlternatives,
            string? recursiveReference,
            JsonTupleLayout? jsonTuple)
        {
            JsonTuple = jsonTuple;
            Name = name;
            Repeating = repeating;
            Nullable = nullable;
            ContainerNullable = containerNullable;
            JsonObjectItemNullable = jsonObjectItemNullable;
            JsonArrayWrapper = jsonArrayWrapper;
            JsonAny = jsonAny;
            JsonFormats = jsonFormats;
            ScalarDomain = scalarDomain;
            FixedLexical = fixedLexical;
            Fixed = fixedValue;
            JsonAllowedValues = jsonAllowedValues;
            NumericRange = numericRange;
            JsonMultipleOf = jsonMultipleOf;
            StringLengthRange = stringLengthRange;
            JsonPatterns = jsonPatterns;
            ItemCountRange = itemCountRange;
            JsonContains = jsonContains;
            PropertyCountRange = propertyCountRange;
            PropertyDependencies = propertyDependencies;
            DependentSchemas = dependentSchemas;
            PropertyNames = propertyNames;
            PatternPropertyNames = patternPropertyNames;
            JsonUniqueItems = jsonUniqueItems;
            Children = children;
            var open = jsonTuple is { Open: true } && children.Count != 0;
            TupleMembers = open ? children.Take(children.Count - 1).ToArray() : children;
            TupleRest = open ? children[^1] : null;
            DuplicateChildren = children.Select(child => child.Name).Distinct(StringComparer.Ordinal).Count() != children.Count;
            Dynamic = dynamic;
            Required = required;
            Alternatives = alternatives;
            InclusiveAlternatives = inclusiveAlternatives;
            XmlTypeAlternatives = xmlTypeAlternatives;
            RecursiveReference = recursiveReference;
        }

        public string Name { get; private set; }

        public bool Repeating { get; private set; }

        public bool Nullable { get; }

        public bool ContainerNullable { get; private set; }
        public bool? JsonObjectItemNullable { get; private set; }

        /// <summary>Where this node admits explicit JSON null.</summary>
        public JsonNullLayers NullLayers => JsonNullLayers.Derive(
            IsScalar,
            Repeating,
            Nullable,
            ContainerNullable,
            JsonObjectItemNullable,
            JsonArrayWrapper,
            JsonAny);
        public bool JsonArrayWrapper { get; }

        public bool JsonAny { get; }

        public IReadOnlyList<string> JsonFormats { get; }

        public JsonScalarDomain ScalarDomain { get; }

        public string? FixedLexical { get; }

        public FerruleValue? Fixed { get; }

        public JsonAllowedValues? JsonAllowedValues { get; }

        public JsonNumericRange? NumericRange { get; }

        public JsonMultipleOfConstraints? JsonMultipleOf { get; }

        public JsonStringLengthRange? StringLengthRange { get; }

        public JsonPatternConstraints? JsonPatterns { get; }

        public JsonItemCountRange? ItemCountRange { get; private set; }

        public IReadOnlyList<JsonContainsConstraint> JsonContains { get; private set; }

        public JsonPropertyCountRange? PropertyCountRange { get; }

        public IReadOnlyList<JsonPropertyDependency> PropertyDependencies { get; }

        public IReadOnlyList<JsonDependentSchema> DependentSchemas { get; }

        public JsonPropertyNameConstraints? PropertyNames { get; }

        public JsonPatternPropertyNames? PatternPropertyNames { get; }

        public bool JsonUniqueItems { get; private set; }

        public string? RecursiveReference { get; }

        public JsonSchemaNode? ResolvedRecursiveSchema { get; private set; }

        public bool IsScalar => ScalarDomain != JsonScalarDomain.None;

        public bool IsScalarUnion => IsScalar && !IsSingleScalar(ScalarDomain);

        public IReadOnlyList<JsonSchemaNode> Children { get; }

        public bool DuplicateChildren { get; }

        public JsonSchemaNode? Dynamic { get; }

        public IReadOnlyList<string> Required { get; }

        public IReadOnlyList<JsonAlternative> Alternatives { get; }

        public bool InclusiveAlternatives { get; }

        public bool XmlTypeAlternatives { get; }

        /// <summary>The array layout when this group is a JSON Schema tuple frame.</summary>
        public JsonTupleLayout? JsonTuple { get; }

        /// <summary>A frame's positional members; member <c>k</c> holds position <c>k</c>.</summary>
        public IReadOnlyList<JsonSchemaNode> TupleMembers { get; }

        /// <summary>The repeating tail item schema of an open frame.</summary>
        public JsonSchemaNode? TupleRest { get; }

        /// <summary>
        /// This open frame's tail schema as one array item: its object null
        /// lives in the item nullability, where a member's lives in the container.
        /// </summary>
        public JsonSchemaNode AsTupleItem()
        {
            var item = (JsonSchemaNode)MemberwiseClone();
            item.Repeating = false;
            if (!IsScalar)
            {
                item.ContainerNullable = JsonObjectItemNullable ?? false;
                item.JsonObjectItemNullable = null;
            }
            return item;
        }

        public JsonSchemaNode? Child(string name) =>
            Children.FirstOrDefault(child =>
                string.Equals(child.Name, name, StringComparison.Ordinal));

        public void BindRecursiveAnchor(JsonSchemaNode anchor)
        {
            var resolved = (JsonSchemaNode)anchor.MemberwiseClone();
            resolved.Name = Name;
            resolved.Repeating = Repeating;
            resolved.ContainerNullable = ContainerNullable;
            resolved.JsonObjectItemNullable = JsonObjectItemNullable;
            resolved.ItemCountRange = ItemCountRange;
            resolved.JsonContains = JsonContains;
            resolved.JsonUniqueItems = JsonUniqueItems;
            ResolvedRecursiveSchema = resolved;
        }
    }

    /// <summary>
    /// Where one schema node admits explicit JSON null, derived from its saved
    /// role flags exactly as the effective JSON view derives the native
    /// boundary's layers, so object null, array-container null, object-item
    /// null and scalar value null keep their separate meanings.
    /// </summary>
    /// <param name="Whole">One object, or one whole array or nested array, is <c>null</c>.</param>
    /// <param name="Single">
    /// One singular object or object array item is <c>null</c> before its own shape is
    /// read or written. Nested-array frames and arbitrary JSON never consult it.
    /// </param>
    /// <param name="Value">The scalar value, or each scalar item, is <c>null</c>.</param>
    internal readonly record struct JsonNullLayers(bool Whole, bool Single, bool Value)
    {
        /// <summary>The saved role metadata with no effective interpretation, if any.</summary>
        internal static string? Contradiction(bool scalar, bool repeating, bool nullable, bool containerNullable) =>
            nullable && !scalar ? "scalar nullability is declared on a group"
            : containerNullable && scalar && !repeating ? "container nullability is declared on a singular scalar"
            : null;

        internal static JsonNullLayers Derive(
            bool scalar,
            bool repeating,
            bool nullable,
            bool containerNullable,
            bool? objectItemNullable,
            bool arrayWrapper,
            bool jsonAny)
        {
            if (!repeating)
            {
                return scalar
                    ? new JsonNullLayers(false, false, nullable)
                    : new JsonNullLayers(containerNullable, containerNullable, false);
            }
            if (arrayWrapper)
            {
                // Each nested array's nullability belongs to the frame's item child.
                return new JsonNullLayers(containerNullable, false, false);
            }
            // A scalar item's null belongs to its declared value domain; an object
            // item's to its explicit override or else the array container.
            var itemNull = scalar ? nullable : objectItemNullable ?? containerNullable;
            return new JsonNullLayers(containerNullable, !scalar && !jsonAny && itemNull, scalar && itemNull);
        }
    }

    private const string JsonTupleRestField = "rest";
    private const int MaximumJsonTupleMembers = 4096;

    /// <summary>The array layout of one JSON Schema tuple frame.</summary>
    /// <param name="Length">Bounds on the complete array length; absent is unconstrained.</param>
    /// <param name="Open">Whether items after the members follow the <c>rest</c> schema.</param>
    private sealed record JsonTupleLayout(JsonItemCountRange? Length, bool Open);

    private static JsonTupleLayout? ReadJsonTupleLayout(string name, JsonElement element)
    {
        if (!element.TryGetProperty("json_tuple", out var layout) ||
            layout.ValueKind == JsonValueKind.Null)
        {
            return null;
        }
        var invalid = $"Embedded JSON schema node '{name}' has invalid JSON tuple frame metadata.";
        if (layout.ValueKind != JsonValueKind.Object)
        {
            throw Boundary(invalid);
        }
        JsonItemCountRange? length = null;
        bool? open = null;
        var seen = new HashSet<string>(StringComparer.Ordinal);
        foreach (var field in layout.EnumerateObject())
        {
            if (!seen.Add(field.Name))
            {
                throw Boundary(invalid);
            }
            switch (field.Name)
            {
                case "length" when field.Value.ValueKind == JsonValueKind.Null:
                    break;
                case "length" when field.Value.ValueKind == JsonValueKind.Object:
                    foreach (var bound in field.Value.EnumerateObject())
                    {
                        if (bound.Name is not ("minimum" or "maximum"))
                        {
                            throw Boundary(invalid);
                        }
                    }
                    var minimum = OptionalUInt64(field.Value, "minimum", false) ?? 0;
                    var maximum = OptionalUInt64(field.Value, "maximum", true);
                    if (minimum == 0 && maximum is null || maximum is { } upper && minimum > upper)
                    {
                        throw Boundary(invalid);
                    }
                    length = new JsonItemCountRange(minimum, maximum);
                    break;
                case "tail" when field.Value.ValueKind == JsonValueKind.String:
                    open = field.Value.GetString() switch
                    {
                        "open" => true,
                        "closed" => false,
                        _ => throw Boundary(invalid),
                    };
                    break;
                default:
                    throw Boundary(invalid);
            }
        }
        return open is { } tail ? new JsonTupleLayout(length, tail) : throw Boundary(invalid);
    }

    /// <summary>
    /// Refuses a node that departs from the exact tuple frame, and a dynamic
    /// property value or predicate schema that is or contains a frame, as the
    /// schema model's own frame check does for every saved node.
    /// </summary>
    private static void ValidateJsonTupleFrame(
        JsonSchemaNode node,
        JsonElement element,
        JsonElement kindElement)
    {
        var reason = node.JsonTuple is { } layout
            ? JsonTupleFrameError(node, layout, element, kindElement)
            : null;
        if (reason is null && node.Dynamic is { JsonTuple: not null })
        {
            reason = "a dynamic property value schema is a tuple frame";
        }
        if (reason is null &&
            node.JsonContains.Select(constraint => constraint.Predicate.Schema)
                .Concat(node.DependentSchemas.Select(constraint => constraint.Predicate.Schema))
                .Any(schema => schema is not null && ContainsJsonTuple(schema)))
        {
            reason = "a predicate schema contains a tuple frame";
        }
        if (reason is not null)
        {
            throw Boundary(
                $"Embedded JSON schema node '{node.Name}' has invalid JSON tuple frame metadata: {reason}.");
        }
    }

    private static string? JsonTupleFrameError(
        JsonSchemaNode frame,
        JsonTupleLayout layout,
        JsonElement element,
        JsonElement kindElement)
    {
        static bool Some(JsonElement parent, string key) =>
            parent.TryGetProperty(key, out var value) && value.ValueKind != JsonValueKind.Null;
        static bool True(JsonElement parent, string key) =>
            parent.TryGetProperty(key, out var value) && value.ValueKind == JsonValueKind.True;
        static bool NonEmpty(JsonElement parent, string key) =>
            parent.TryGetProperty(key, out var value) && value.ValueKind != JsonValueKind.Null &&
            (value.ValueKind != JsonValueKind.Array || value.GetArrayLength() != 0);
        static bool Differs(JsonElement parent, string key, string fallback) =>
            Some(parent, key) &&
            !(parent.GetProperty(key).ValueKind == JsonValueKind.String &&
              parent.GetProperty(key).GetString() == fallback);

        if (frame.IsScalar)
        {
            return "the frame is not a group";
        }
        if (frame.JsonArrayWrapper)
        {
            return "the frame is also a nested-array wrapper";
        }
        if (frame.RecursiveReference is not null)
        {
            return "the frame declares a recursive reference";
        }
        if (NonEmpty(kindElement, "alternatives") || NonEmpty(kindElement, "required") ||
            Some(kindElement, "dynamic") || Differs(element, "alternative_mode", "exclusive") ||
            Some(element, "json_dependent_schemas") || Some(element, "property_count_range") ||
            Some(element, "json_property_dependencies") ||
            Some(element, "json_pattern_property_names") || Some(element, "json_property_names"))
        {
            return "the frame declares object metadata";
        }
        if (Some(element, "json_contains") || True(element, "json_unique_items"))
        {
            return "the frame declares `contains` or `uniqueItems` assertions";
        }
        if (True(element, "nullable") || True(element, "json_any") ||
            new[]
            {
                "fixed", "json_allowed_values", "numeric_range", "json_multiple_of",
                "string_length_range", "json_patterns", "default", "value_generation",
            }.Any(key => Some(element, key)) ||
            NonEmpty(element, "json_formats"))
        {
            return "the frame declares scalar value metadata";
        }
        if (NonEmpty(kindElement, "xml_restricted_alternatives") ||
            Some(element, "xml_namespace") || NonEmpty(element, "xml_name_alternatives") ||
            Some(element, "xml_wildcard_namespace") ||
            Differs(element, "xml_wildcard_process_contents", "skip") ||
            Some(element, "xml_wildcard_excluded_names") ||
            new[] { "attribute", "text", "nillable", "xml_optional", "xml_attribute_required", "xml_type_alternatives" }
                .Any(key => True(element, key)) ||
            Differs(element, "xml_alternative_kind", "xsi_type") ||
            Some(element, "xml_default_type") ||
            NonEmpty(element, "xml_repeating_sequences") || NonEmpty(element, "xml_repeating_choices"))
        {
            return "the frame declares XML metadata";
        }
        if (Some(element, "database_relation"))
        {
            return "the frame declares a database relation";
        }
        // The outer array's nullability would otherwise be read as each tuple's.
        if (frame.Repeating && frame.JsonObjectItemNullable is null)
        {
            return "the repeating frame does not declare whether each tuple may be null";
        }
        if (layout.Open && (frame.Children.Count == 0 || frame.Children[^1].Name != JsonTupleRestField))
        {
            return $"the open frame's last child is not `{JsonTupleRestField}`";
        }
        var members = frame.TupleMembers;
        if (members.Count is 0 or > MaximumJsonTupleMembers)
        {
            return $"the frame has {members.Count} members; it requires 1 to {MaximumJsonTupleMembers}";
        }
        for (var index = 0; index < members.Count; index++)
        {
            var position = index + 1;
            var member = members[index];
            if (member.Name != $"item{position}")
            {
                return $"member {position} is not named `item{position}`";
            }
            if (member.Repeating || member.JsonArrayWrapper || member.JsonTuple is not null)
            {
                return $"member {position} is a JSON array";
            }
        }
        if (frame.TupleRest is { } rest)
        {
            if (!rest.Repeating)
            {
                return $"the `{JsonTupleRestField}` tail does not repeat";
            }
            if (rest.JsonArrayWrapper || rest.JsonTuple is not null)
            {
                return $"the `{JsonTupleRestField}` tail items are JSON arrays";
            }
            if (rest.ItemCountRange is not null || rest.JsonContains.Count != 0 ||
                rest.JsonUniqueItems || rest.ContainerNullable)
            {
                return $"the `{JsonTupleRestField}` tail declares its own array metadata";
            }
        }
        var count = (ulong)members.Count;
        var maximum = layout.Length?.Maximum;
        if (layout.Open ? maximum is { } open && open <= count : maximum != count)
        {
            return "the length bounds are not canonical for the frame's tail";
        }
        // Positions are compared as JSON boundaries see them: without their
        // own names or XML metadata, which the parsed schema never retains.
        var first = members[0];
        if (members.Skip(1).All(member => SchemaNodesEqualIgnoringRootName(first, member)) &&
            (frame.TupleRest is not { } tail || SchemaNodesEqualIgnoringRootName(first, tail.AsTupleItem())))
        {
            return "the frame describes a homogeneous array";
        }
        return null;
    }

    /// <summary>Whether any node in this schema, including predicate schemas, is a tuple frame.</summary>
    private static bool ContainsJsonTuple(JsonSchemaNode root)
    {
        var pending = new Stack<JsonSchemaNode>();
        pending.Push(root);
        while (pending.Count != 0)
        {
            var node = pending.Pop();
            if (node.JsonTuple is not null)
            {
                return true;
            }
            foreach (var child in node.Children)
            {
                pending.Push(child);
            }
            if (node.Dynamic is { } dynamic)
            {
                pending.Push(dynamic);
            }
            foreach (var schema in node.JsonContains.Select(constraint => constraint.Predicate.Schema)
                         .Concat(node.DependentSchemas.Select(constraint => constraint.Predicate.Schema)))
            {
                if (schema is not null)
                {
                    pending.Push(schema);
                }
            }
        }
        return false;
    }

    private static bool ContainsJsonUniqueItems(JsonSchemaNode root)
    {
        // Match the native raw-input gate: private predicate schemas validate
        // only when selected and do not enable a document-wide prepass.
        var pending = new Stack<JsonSchemaNode>();
        pending.Push(root);
        while (pending.Count != 0)
        {
            var node = pending.Pop();
            if (node.JsonUniqueItems) return true;
            foreach (var child in node.Children) pending.Push(child);
            if (node.Dynamic is { } dynamic) pending.Push(dynamic);
        }
        return false;
    }

    /// <summary>
    /// Refuses a recursive reference that expands a tuple frame, or that leads
    /// from inside a frame's position back to that frame: either would nest
    /// the frame inside itself. The frame check does not follow references.
    /// </summary>
    private static void RefuseRecursiveTupleFrames(
        IReadOnlyList<JsonSchemaNode> nodes,
        IReadOnlyDictionary<JsonSchemaNode, JsonSchemaNode> anchors)
    {
        foreach (var node in nodes)
        {
            if (anchors.TryGetValue(node, out var target) && target.JsonTuple is not null)
            {
                throw RecursiveTupleFrame(node.Name, target.Name);
            }
        }
        foreach (var frame in nodes.Where(node => node.JsonTuple is not null))
        {
            // Each pending node carries the last reference on its path.
            var visited = new HashSet<JsonSchemaNode>(ReferenceEqualityComparer.Instance);
            var pending = new Stack<(JsonSchemaNode Node, string? Reference)>(
                SchemaChildren(frame).Select(child => (child, (string?)null)));
            while (pending.Count != 0)
            {
                var (node, reference) = pending.Pop();
                if (ReferenceEquals(node, frame))
                {
                    throw RecursiveTupleFrame(reference ?? frame.Name, frame.Name);
                }
                if (!visited.Add(node))
                {
                    continue;
                }
                foreach (var child in SchemaChildren(node))
                {
                    pending.Push((child, reference));
                }
                if (anchors.TryGetValue(node, out var target))
                {
                    pending.Push((target, node.Name));
                }
            }
        }

        static IEnumerable<JsonSchemaNode> SchemaChildren(JsonSchemaNode node) =>
            node.Dynamic is { } dynamic ? node.Children.Append(dynamic) : node.Children;
    }

    private static FerruleRuntimeException RecursiveTupleFrame(string node, string frame) =>
        Boundary(
            $"Embedded JSON recursive reference '{node}' expands tuple frame '{frame}' through one of its own positions; recursive tuple frames are not supported.");

    /// <summary>
    /// Root rows and JSON Lines records against a non-repeating frame root
    /// would make each row a tuple without a repeating schema.
    /// </summary>
    private static FerruleRuntimeException TupleRootRows(JsonSchemaNode schema) =>
        Boundary(
            $"Non-repeating JSON tuple frame root '{schema.Name}' cannot hold root rows or JSON Lines records.");

    private static bool IsSingleTupleFrame(JsonSchemaNode schema) =>
        schema.JsonTuple is not null && !schema.Repeating;

    /// <summary>Checks the complete array length; a closed frame's maximum is its member count.</summary>
    private static void ValidateTupleLength(JsonSchemaNode frame, JsonTupleLayout layout, int length)
    {
        if (layout.Length is { } range && !range.Contains(length))
        {
            throw Boundary(
                $"JSON tuple '{frame.Name}' requires {DescribeItemCount(range)} items, got {length}.");
        }
    }

    private static string DescribeItemCount(JsonItemCountRange range) => range switch
    {
        { Maximum: { } maximum } when range.Minimum == maximum => $"exactly {maximum}",
        { Minimum: 0, Maximum: { } maximum } => $"at most {maximum}",
        { Maximum: null } => $"at least {range.Minimum}",
        _ => $"between {range.Minimum} and {range.Maximum}",
    };

    /// <summary>Attributes an error inside one position to its one-based array position.</summary>
    private static T AtTuplePosition<T>(JsonSchemaNode frame, int position, Func<T> body)
    {
        try
        {
            return body();
        }
        catch (FerruleRuntimeException error) when (error.Error == FerruleRuntimeError.JsonBoundary)
        {
            var message = $"JSON tuple '{frame.Name}' position {position}: {error.Message}";
            throw new FerruleRuntimeException(
                FerruleRuntimeError.JsonBoundary, message, error, detail: message);
        }
    }

    /// <summary>
    /// Reads one frame occurrence: the array, its length, the positions in
    /// order and then the tail items. A position the array does not reach is
    /// <c>Null</c>, so the original JSON is recoverable exactly.
    /// </summary>
    private static FerruleInstance ReadTuple(
        JsonSchemaNode frame,
        JsonTupleLayout layout,
        JsonElement element,
        NodeBudget budget,
        int depth)
    {
        RequireKind(element, JsonValueKind.Array, frame.Name, "array");
        var length = element.GetArrayLength();
        ValidateTupleLength(frame, layout, length);
        var members = frame.TupleMembers;
        var rest = frame.TupleRest;
        if (rest is null && length > members.Count)
        {
            throw Boundary(
                $"JSON tuple '{frame.Name}' requires at most {members.Count} items, got {length}.");
        }
        var fields = new List<FerruleField>(members.Count + 1);
        var tail = new List<FerruleInstance>();
        var position = 0;
        foreach (var item in element.EnumerateArray())
        {
            position++;
            var schema = position <= members.Count ? members[position - 1] : rest!;
            var value = AtTuplePosition(frame, position, () =>
            {
                budget.Visit(depth + 1);
                return ReadSingleNode(schema, item, budget, depth + 1);
            });
            if (position <= members.Count)
            {
                fields.Add(new FerruleField(schema.Name, value));
            }
            else
            {
                tail.Add(value);
            }
        }
        foreach (var member in members.Skip(fields.Count))
        {
            fields.Add(new FerruleField(member.Name, new FerruleScalar(FerruleValue.Null)));
        }
        if (rest is not null)
        {
            fields.Add(new FerruleField(rest.Name, new FerruleRepeated(tail)));
        }
        return FerruleGroup.FromSchemaFields(fields);
    }

    /// <summary>
    /// Writes one frame occurrence after the null and absence rules: the
    /// present positions and then the tail items. An absent position before a
    /// present one is a gap, which is never padded with <c>null</c>.
    /// </summary>
    private static void WriteTuple(
        Utf8JsonWriter writer,
        JsonSchemaNode frame,
        JsonTupleLayout layout,
        FerruleInstance instance,
        NodeBudget budget,
        int depth)
    {
        if (instance is not FerruleGroup group)
        {
            throw Shape(frame.Name, "tuple group of positional fields", InstanceKind(instance));
        }
        var members = frame.TupleMembers;
        var rest = frame.TupleRest;
        foreach (var field in group.Fields)
        {
            if (!members.Any(member => member.Name == field.Name) && rest?.Name != field.Name)
            {
                throw Boundary($"JSON tuple '{frame.Name}' does not declare field '{field.Name}'.");
            }
        }
        IReadOnlyList<FerruleInstance> tail = Array.Empty<FerruleInstance>();
        if (rest is not null && group.TryGetField(rest.Name, out var restValue))
        {
            tail = restValue switch
            {
                FerruleScalar { Value.Kind: FerruleValueKind.Null } => Array.Empty<FerruleInstance>(),
                FerruleRepeated repeated => repeated.Items,
                FerruleMappedSequence mapped => mapped.Items,
                _ => throw Shape(rest.Name, "array", InstanceKind(restValue)),
            };
        }
        var present = members
            .Select(member => group.TryGetField(member.Name, out var value) &&
                              value is not (FerruleScalar { Value.Kind: FerruleValueKind.Null } or FerruleMappedSequence { Items.Count: 0 })
                ? value
                : null)
            .ToArray();
        // A non-empty tail requires every position before it.
        var last = tail.Count != 0 ? members.Count : Array.FindLastIndex(present, value => value is not null) + 1;
        var gap = Array.FindIndex(present, 0, last, value => value is null);
        if (gap >= 0)
        {
            throw Boundary(
                $"JSON tuple '{frame.Name}' cannot write position {gap + 1} as absent before a later present item.");
        }
        ValidateTupleLength(frame, layout, last + tail.Count);
        writer.WriteStartArray();
        for (var index = 0; index < last; index++)
        {
            var member = members[index];
            var value = present[index]!;
            AtTuplePosition(frame, index + 1, () =>
            {
                budget.Visit(depth + 1);
                WriteSingleNode(writer, member, value, budget, depth + 1);
                return true;
            });
        }
        for (var index = 0; index < tail.Count; index++)
        {
            var item = tail[index];
            AtTuplePosition(frame, last + index + 1, () =>
            {
                budget.Visit(depth + 1);
                WriteSingleNode(writer, rest!, item, budget, depth + 1);
                return true;
            });
        }
        writer.WriteEndArray();
    }
}
