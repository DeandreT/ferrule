using Ferrule.Runtime;

static class LookupOracle
{
    public static void Equal(FerruleInstance actual, FerruleInstance expected)
    {
        if (actual is FerruleScalar scalar && expected is FerruleScalar expectedScalar)
        {
            if (scalar.Value.Kind != expectedScalar.Value.Kind || !scalar.Value.Equals(expectedScalar.Value))
                throw new InvalidOperationException("Typed scalar output differs");
        }
        else if (actual is FerruleGroup group && expected is FerruleGroup expectedGroup)
        {
            if (group.Fields.Count != expectedGroup.Fields.Count) throw new InvalidOperationException("Whole group differs");
            for (int index = 0; index < group.Fields.Count; index++)
            {
                if (group.Fields[index].Name != expectedGroup.Fields[index].Name) throw new InvalidOperationException("Field order differs");
                Equal(group.Fields[index].Value, expectedGroup.Fields[index].Value);
            }
        }
        else if (actual is FerruleRepeated repeated && expected is FerruleRepeated expectedRepeated)
        {
            if (repeated.Items.Count != expectedRepeated.Items.Count) throw new InvalidOperationException("Whole sequence differs");
            for (int index = 0; index < repeated.Items.Count; index++) Equal(repeated.Items[index], expectedRepeated.Items[index]);
        }
        else throw new InvalidOperationException("Typed instance shape differs");
    }
}
