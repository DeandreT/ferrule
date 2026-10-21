using System.Text;
using Ferrule.Runtime;

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private static FerruleCsvReadField[] AcquisitionFields() => [new("Text",FerruleScalarType.String),new("Count",FerruleScalarType.Int64)];
    private static FerruleCsvSourceData AcquisitionData(string text) => new(Encoding.UTF8.GetBytes(text),"resolved/same.csv");
    private static FerruleCsvAcquisitionException AcquisitionError(FerruleCsvAcquisitionError stage, Action action)
    {
        try { action(); }
        catch (FerruleCsvAcquisitionException error) { Equal(stage,error.Error);return error; }
        throw new Exception("Expected acquisition failure: "+stage);
    }

    private static void CsvAcquisitionOrder()
    {
        var calls=new List<string>();
        var fields=AcquisitionFields();
        using var source=new FerruleCsvAcquisition(
            () => { calls.Add("filename");return " opaque *.csv "; },
            name => { calls.Add("select:"+name);return ["same.csv","empty.csv","same.csv"]; },
            path => { calls.Add("load:"+path);return AcquisitionData(path=="empty.csv" ? "Text,Count\n" : "Text,Count\n\"Snow\nline\",7\n,8\n"); },
            fields,new FerruleCsvReadOptions {PreserveEmptyStrings=true},FerruleCsvByteDecodingPolicy.StrictUtf8,3);
        fields[0]=new("changed-after-construction",FerruleScalarType.Bool);
        Equal(0,calls.Count);
        Throws<InvalidOperationException>(()=>{_ = source.Current;});
        Equal(true,source.MoveNext());
        Equal("filename|select: opaque *.csv |load:same.csv",string.Join("|",calls));
        var rows=new List<FerruleCsvRowOccurrence> {source.Current};
        while(source.MoveNext()) rows.Add(source.Current);
        Equal(false,source.MoveNext());
        Equal("filename|select: opaque *.csv |load:same.csv|load:empty.csv|load:same.csv",string.Join("|",calls));
        Equal(4,rows.Count);
        for(int index=0;index<rows.Count;index++)
        {
            var row=rows[index];
            Equal("same.csv",row.Document.Path);Equal("resolved/same.csv",row.Document.SourcePath);Equal(index%2+1,row.Position);
            var value=(FerruleGroup)row.Value;
            Equal(index%2==0 ? "Snow\nline" : "",((FerruleScalar)value.Fields[0].Value).Value.StringValue);
            Equal(index%2==0 ? 7L : 8L,((FerruleScalar)value.Fields[1].Value).Value.Int64Value);
        }
        Equal(true,rows[0].Document.SameDocument(rows[1].Document));
        Equal(false,rows[0].Document.SameDocument(rows[2].Document));
        Equal(false,rows[0].SameOccurrence(rows[2]));
        Equal(true,rows[0].SameOccurrence(rows[0].Document.Rows().First()));
    }

    private static void CsvAcquisitionFailures()
    {
        for(int failure=0;failure<3;failure++)
        {
            var loaded=new List<int>();
            using var source=new FerruleCsvAcquisition(()=>"files",_=>["0","1","2"],path=>{
                int index=int.Parse(path);loaded.Add(index);
                return AcquisitionData(index==failure ? "Text,Count\nvalid,1\nbad,invalid\n" : "Text,Count\nvalid,1\n");
            },AcquisitionFields(),null,FerruleCsvByteDecodingPolicy.StrictUtf8,3);
            for(int index=0;index<failure;index++) Equal(true,source.MoveNext());
            var error=AcquisitionError(FerruleCsvAcquisitionError.Input,()=>source.MoveNext());
            Equal(failure.ToString(),error.Path);
            Equal(FerruleCsvInputError.Parse,((FerruleCsvInputException)error.InnerException!).Error);
            Equal(false,source.MoveNext());Equal(failure+1,loaded.Count);
        }
        var cause=new InvalidOperationException("host failure");
        using var missing=new FerruleCsvAcquisition(()=>throw cause,_=>throw new Exception("selected"),_=>throw new Exception("loaded"),AcquisitionFields(),null,FerruleCsvByteDecodingPolicy.StrictUtf8,1);
        Equal(true,ReferenceEquals(cause,AcquisitionError(FerruleCsvAcquisitionError.Filename,()=>missing.MoveNext()).InnerException));
        Equal(false,missing.MoveNext());
        using var limit=new FerruleCsvAcquisition(()=>"files",_=>["empty","never"],name=>{
            Equal("empty",name);return AcquisitionData("Text,Count\n");
        },AcquisitionFields(),null,FerruleCsvByteDecodingPolicy.StrictUtf8,1);
        Equal(1,AcquisitionError(FerruleCsvAcquisitionError.Documents,()=>limit.MoveNext()).Maximum);
        Equal(false,limit.MoveNext());
        using var invalid=new FerruleCsvAcquisition(()=>"files",_=>[""],_=>throw new Exception("invalid path loaded"),AcquisitionFields(),null,FerruleCsvByteDecodingPolicy.StrictUtf8,1);
        AcquisitionError(FerruleCsvAcquisitionError.Input,()=>invalid.MoveNext());Equal(false,invalid.MoveNext());
    }
}
