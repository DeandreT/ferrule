using System.Text;
using System.Text.Json;
using Ferrule.Runtime;

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private const string DuplicateDeclarationSchema = """
        {"name":"Root","property_count_range":{"minimum":2,"maximum":2},
         "kind":{"kind":"group","children":[
          {"name":"Value","kind":{"kind":"scalar","ty":"int"}},
          {"name":"Value","kind":{"kind":"scalar","ty":"float"}},
          {"name":"Tail","kind":{"kind":"scalar","ty":"string"}}
         ],"required":["Value","Tail"]}}
        """;

    private static void JsonDuplicateDeclarationMaterialization()
    {
        const string input = """{"Tail":"self authored é🙂","Value":7}""";
        foreach (var parsed in new[]
                 {
                     FerruleJson.Parse(DuplicateDeclarationSchema, input),
                     FerruleJson.ParseBytes(DuplicateDeclarationSchema, Encoding.UTF8.GetBytes(input)),
                     FerruleJson.ParseEmbedded(DuplicateDeclarationSchema, input),
                     FerruleJson.ParseEmbeddedBytes(DuplicateDeclarationSchema, Encoding.UTF8.GetBytes(input)),
                 })
        {
            var group = (FerruleGroup)parsed;
            Equal(3, group.Fields.Count);
            Equal("Value", group.Fields[0].Name);
            Equal("Value", group.Fields[1].Name);
            Equal("Tail", group.Fields[2].Name);
            Equal(FerruleValueKind.Int64, ((FerruleScalar)group.Fields[0].Value).Value.Kind);
            Equal(FerruleValueKind.Double, ((FerruleScalar)group.Fields[1].Value).Value.Kind);
            Equal(FerruleValue.FromInt64(7), ScopeContext.FromSource(group).ResolveScalar("Value"));
            Equal(true, group.TryGetField("Value", out var first));
            Equal(true, ReferenceEquals(first, group.Fields[0].Value));
            var copy = ScopeContext.FromSource(group).CopyCurrentGroup();
            Equal(3, copy.Fields.Count);
            Equal(false, ReferenceEquals(copy.Fields[0].Value, group.Fields[0].Value));
            Equal(FerruleValueKind.Double, ((FerruleScalar)copy.Fields[1].Value).Value.Kind);
            Equal(
                FerruleJson.SerializeEmbedded(DuplicateDeclarationSchema, group),
                FerruleJson.SerializeEmbedded(DuplicateDeclarationSchema, copy));
        }
        Error(FerruleRuntimeError.DuplicateField, () => Group(
            Field("Value", Scalar(FerruleValue.FromInt64(1))),
            Field("Value", Scalar(FerruleValue.FromInt64(2)))));
    }

    private static void JsonDuplicateDeclarationProjection()
    {
        const string input = """{"Value":7,"Tail":"self authored é🙂"}""";
        var parsed = FerruleJson.ParseEmbedded(DuplicateDeclarationSchema, input);
        var text = FerruleJson.SerializeEmbedded(DuplicateDeclarationSchema, parsed);
        Equal(text, FerruleJson.Serialize(DuplicateDeclarationSchema, parsed));
        Equal(text, Encoding.UTF8.GetString(FerruleJson.SerializeBytes(DuplicateDeclarationSchema, parsed)));
        Equal(text, Encoding.UTF8.GetString(FerruleJson.SerializeEmbeddedBytes(DuplicateDeclarationSchema, parsed)));
        using (var document = JsonDocument.Parse(text))
        {
            var properties = document.RootElement.EnumerateObject().ToArray();
            Equal(2, properties.Length);
            Equal("Value", properties[0].Name);
            Equal("Tail", properties[1].Name);
            Equal(7L, properties[0].Value.GetInt64());
            Equal("self authored é🙂", properties[1].Value.GetString());
        }
        Equal(text, FerruleJson.SerializeEmbedded(
            DuplicateDeclarationSchema,
            FerruleJson.ParseEmbedded(DuplicateDeclarationSchema, text)));

        const string lexical = """
            {"name":"Root","kind":{"kind":"group","children":[
              {"name":"Value","kind":{"kind":"scalar","ty":"string"}},
              {"name":"Tail","kind":{"kind":"scalar","ty":"string"}},
              {"name":"Value","kind":{"kind":"scalar","ty":"int"}}
            ]}}
            """;
        var value = Group(Field("Value", Scalar(Text("007"))), Field("Tail", Scalar(Text("kept"))));
        using var normalized = JsonDocument.Parse(FerruleJson.SerializeEmbedded(lexical, value));
        var ordered = normalized.RootElement.EnumerateObject().ToArray();
        Equal(2, ordered.Length);
        Equal("Value", ordered[0].Name);
        Equal(7L, ordered[0].Value.GetInt64());
        Equal("Tail", ordered[1].Name);
        var wrongCount = lexical.Replace(
            "\"name\":\"Root\",",
            "\"name\":\"Root\",\"property_count_range\":{\"maximum\":1},",
            StringComparison.Ordinal);
        Error(FerruleRuntimeError.JsonBoundary, () => FerruleJson.SerializeEmbedded(wrongCount, value));
        JsonDuplicateDeclarationProjectedPredicates();
    }

    private static void JsonDuplicateDeclarationProjectedPredicates()
    {
        // The first declaration accepts a string, while the last assignment
        // normalizes it to an integer. Object predicates inspect that final value.
        const string schema = """
            {"name":"Root","property_count_range":{"minimum":2,"maximum":2},
             "json_property_names":{"kind":"schema","allowed":["Tail","Value"]},
             "json_property_dependencies":{"Value":["Tail"]},
             "json_dependent_schemas":[{"trigger":"Value","predicate":{"kind":"schema","schema":
              {"name":"predicate","kind":{"kind":"group","children":[
               {"name":"Value","numeric_range":{"kind":"integer","bounds":{"minimum":7,"maximum":7}},"kind":{"kind":"scalar","ty":"int"}},
               {"name":"Tail","kind":{"kind":"scalar","ty":"string"}}
              ],"required":["Value","Tail"]}}}}],
             "kind":{"kind":"group","children":[
              {"name":"Value","kind":{"kind":"scalar_union","types":["string","int"]}},
              {"name":"Tail","kind":{"kind":"scalar","ty":"string"}},
              {"name":"Value","kind":{"kind":"scalar","ty":"int"}}
             ],"required":["Value","Tail"]}}
            """;
        var value = Group(Field("Value", Scalar(Text("007"))), Field("Tail", Scalar(Text("kept"))));
        var text = FerruleJson.SerializeEmbedded(schema, value);
        Equal(text, Encoding.UTF8.GetString(FerruleJson.SerializeEmbeddedBytes(schema, value)));
        using (var projected = JsonDocument.Parse(text))
        {
            Equal(7L, projected.RootElement.GetProperty("Value").GetInt64());
            Equal(2, projected.RootElement.EnumerateObject().Count());
        }
        Equal(text, FerruleJson.SerializeEmbedded(schema, FerruleJson.ParseEmbedded(schema, text)));
        Error(FerruleRuntimeError.JsonBoundary, () => FerruleJson.SerializeEmbedded(
            schema, Group(Field("Value", Scalar(Text("007"))))));
        var wrongDependent = schema.Replace(
            "\"minimum\":7,\"maximum\":7", "\"minimum\":8,\"maximum\":8", StringComparison.Ordinal);
        var dependentError = Error(FerruleRuntimeError.JsonBoundary, () => FerruleJson.SerializeEmbedded(wrongDependent, value));
        Equal(true, dependentError.Message.Contains("dependent schema", StringComparison.Ordinal));

        const string nullable = """
            {"name":"Root","kind":{"kind":"group","children":[
              {"name":"Maybe","nullable":true,"kind":{"kind":"scalar","ty":"string"}},
              {"name":"Maybe","kind":{"kind":"scalar","ty":"string"}}
            ]}}
            """;
        Error(FerruleRuntimeError.JsonBoundary, () => FerruleJson.SerializeEmbedded(
            nullable, Group(Field("Maybe", Scalar(FerruleValue.JsonNull)))));
        var missing = FerruleJson.ParseEmbedded(nullable, "{}");
        Equal(2, ((FerruleGroup)missing).Fields.Count);
        Equal("{}\n", FerruleJson.SerializeEmbedded(nullable, missing));
    }

    private static void JsonDuplicateDeclarationConstraintsAndDynamicRejection()
    {
        const string schema = """
            {"name":"Root","kind":{"kind":"group","children":[
              {"name":"Value","numeric_range":{"kind":"integer","bounds":{"maximum":5}},"kind":{"kind":"scalar","ty":"int"}},
              {"name":"Value","numeric_range":{"kind":"integer","bounds":{"minimum":2}},"kind":{"kind":"scalar","ty":"int"}}
            ]}}
            """;
        foreach (var input in new[] { "{\"Value\":1}", "{\"Value\":6}" })
        {
            Error(FerruleRuntimeError.JsonBoundary, () => FerruleJson.ParseEmbedded(schema, input));
            Error(FerruleRuntimeError.JsonBoundary, () => FerruleJson.ParseEmbeddedBytes(schema, Encoding.UTF8.GetBytes(input)));
        }
        var valid = (FerruleGroup)FerruleJson.ParseEmbedded(schema, "{\"Value\":3}");
        Equal(2, valid.Fields.Count);
        using (var document = JsonDocument.Parse(FerruleJson.SerializeEmbedded(schema, valid)))
        {
            Equal(1, document.RootElement.EnumerateObject().Count());
            Equal(3L, document.RootElement.GetProperty("Value").GetInt64());
        }
        Error(FerruleRuntimeError.JsonBoundary, () => FerruleJson.SerializeEmbedded(schema, Group(Field("Value", Scalar(FerruleValue.FromInt64(1))))));
        Error(FerruleRuntimeError.JsonBoundary, () => FerruleJson.SerializeEmbedded(schema, Group(Field("Value", Scalar(FerruleValue.FromInt64(6))))));
        const string conflicting = """
            {"name":"Root","kind":{"kind":"group","children":[
              {"name":"Value","kind":{"kind":"scalar","ty":"int"}},
              {"name":"Value","kind":{"kind":"group","children":[]}}
            ]}}
            """;
        Error(FerruleRuntimeError.JsonBoundary, () => FerruleJson.ParseEmbedded(conflicting, "{\"Value\":7}"));
        Error(FerruleRuntimeError.JsonBoundary, () => FerruleJson.SerializeEmbedded(conflicting, Group(Field("Value", Scalar(FerruleValue.FromInt64(7))))));

        const string open = """
            {"name":"Root","kind":{"kind":"group","children":[],
              "dynamic":{"name":"*","kind":{"kind":"scalar","ty":"int"}}}}
            """;
        Error(FerruleRuntimeError.JsonBoundary, () => FerruleJson.SerializeEmbedded(open, valid));
        const string missing = """
            {"name":"Root","kind":{"kind":"group","children":[
              {"name":"Absent","kind":{"kind":"scalar","ty":"int"}},
              {"name":"Absent","kind":{"kind":"scalar","ty":"float"}}
            ],"dynamic":{"name":"*","kind":{"kind":"scalar","ty":"string"}}}}
            """;
        var missingGroup = (FerruleGroup)FerruleJson.ParseEmbedded(missing, "{}");
        Equal(2, missingGroup.Fields.Count);
        Equal(FerruleValue.Null, ((FerruleScalar)missingGroup.Fields[0].Value).Value);
        Equal(FerruleJson.SerializeEmbedded(missing, missingGroup), "{}\n");
    }

    private static void JsonDuplicateDeclarationNestedCloneAndBounds()
    {
        const string row = """
            {"name":"Rows","repeating":true,"kind":{"kind":"group","children":[
              {"name":"Value","kind":{"kind":"scalar","ty":"int"}},
              {"name":"Value","kind":{"kind":"scalar","ty":"float"}}
            ]}}
            """;
        var schema = "{\"name\":\"Root\",\"kind\":{\"kind\":\"group\",\"children\":[" + row + "," + row + "]}}";
        var source = (FerruleGroup)FerruleJson.ParseEmbedded(schema, "{\"Rows\":[{\"Value\":1},{\"Value\":2}]}");
        Equal(2, source.Fields.Count);
        var filtered = (FerruleGroup)FerruleRecursiveFilter.Apply(
            ScopeContext.FromSource(source), "Children", "Rows", 9, _ => Bool(true));
        Equal(2, filtered.Fields.Count);
        var rows = (FerruleRepeated)filtered.Fields[0].Value;
        Equal(2, rows.Items.Count);
        Equal(2, ((FerruleGroup)rows.Items[0]).Fields.Count);
        Equal(FerruleValueKind.Double, ((FerruleScalar)((FerruleGroup)rows.Items[0]).Fields[1].Value).Value.Kind);
        Equal(FerruleJson.SerializeEmbedded(schema, source), FerruleJson.SerializeEmbedded(schema, filtered));
        var flat = (FerruleRepeated)FerruleJson.ParseEmbedded(DuplicateDeclarationSchema,
            "[{\"Value\":7,\"Tail\":\"one\"},{\"Value\":8,\"Tail\":\"two\"}]");
        Equal(2, flat.Items.Count);
        Equal(3, ((FerruleGroup)flat.Items[1]).Fields.Count);
        var rendered = FerruleJson.SerializeEmbedded(DuplicateDeclarationSchema, flat);
        using (var document = JsonDocument.Parse(rendered))
        {
            Equal(2, document.RootElement.GetArrayLength());
            Equal(2, document.RootElement[1].EnumerateObject().Count());
        }
        Error(FerruleRuntimeError.JsonBoundary, () => FerruleJson.ParseEmbeddedBytes(schema, [0xff]));
        var tooLarge = new byte[FerruleJson.MaximumDocumentBytes + 1];
        Error(FerruleRuntimeError.JsonBoundary, () => FerruleJson.ParseEmbeddedBytes(schema, tooLarge));
        var tooDeep = schema;
        for (var depth = 0; depth < 128; depth++)
        {
            tooDeep = "{\"name\":\"Nested\",\"kind\":{\"kind\":\"group\",\"children\":[" + tooDeep + "]}}";
        }
        Error(FerruleRuntimeError.JsonBoundary, () => FerruleJson.ParseEmbedded(tooDeep, "{}"));
    }
}
