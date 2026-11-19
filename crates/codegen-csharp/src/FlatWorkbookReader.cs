using System;
using System.Buffers.Binary;
using System.Collections.Generic;
using System.Globalization;
using System.IO;
using System.Linq;
using System.Text;
using System.Xml;
using System.Xml.Linq;

namespace Ferrule.Runtime;

internal static class FlatWorkbookReader
{
    // Only uncompressed flat-profile packages. Never extract, inflate, resolve a
    // relationship or traverse a caller-selected filename. Canonical re-encoding
    // checks the metadata, sheet selection, cell addresses, directory and CRCs.
    internal static FerruleInstance[] Read(byte[] source,string sheet,string[] names,FerruleValueKind[] kinds)
    {
        if(source.Length > 256*1024) throw Bad();
        int pos=0, len=0, start=0;
        foreach(string name in new[]{"[Content_Types].xml","_rels/.rels","xl/workbook.xml","xl/_rels/workbook.xml.rels","xl/worksheets/sheet1.xml"})
        {
            if(source.Length-pos<30) throw Bad();
            var h=source.AsSpan(pos,30);
            if(BinaryPrimitives.ReadUInt32LittleEndian(h)!=0x04034b50 || BinaryPrimitives.ReadUInt32LittleEndian(h[6..])!=0 || BinaryPrimitives.ReadUInt16LittleEndian(h[26..])!=name.Length || BinaryPrimitives.ReadUInt16LittleEndian(h[28..])!=0) throw Bad();
            len=BinaryPrimitives.ReadInt32LittleEndian(h[18..]); start=pos+30+name.Length;
            if(start>source.Length || len<0 || len>source.Length-start || len!=BinaryPrimitives.ReadInt32LittleEndian(h[22..]) || !source.AsSpan(pos+30,name.Length).SequenceEqual(Encoding.ASCII.GetBytes(name))) throw Bad();
            pos=start+len;
        }
        using var stream=new MemoryStream(source,start,len,false);
        var settings=new XmlReaderSettings{DtdProcessing=DtdProcessing.Prohibit,XmlResolver=null,MaxCharactersInDocument=256*1024};
        using(var check=XmlReader.Create(stream,settings)) while(check.Read()) if(check.Depth>8) throw Bad();
        stream.Position=0;
        using var reader=XmlReader.Create(stream,settings);
        XNamespace s="http://schemas.openxmlformats.org/spreadsheetml/2006/main";
        var table=XDocument.Load(reader,LoadOptions.PreserveWhitespace).Root?.Element(s+"sheetData") ?? throw Bad();
        var result=new List<FerruleInstance>();
        foreach(var row in table.Elements(s+"row").Skip(1))
        {
            if(result.Count>=256) throw Bad();
            var fields=new List<FerruleField>();
            foreach(var cell in row.Elements(s+"c"))
            {
                int i=fields.Count;
                if(i>=names.Length) throw Bad();
                var v=cell.Element(s+"v")?.Value;
                var value=kinds[i] switch {
                    FerruleValueKind.String=>FerruleValue.FromString(cell.Element(s+"is")?.Element(s+"t")?.Value ?? throw Bad()),
                    FerruleValueKind.Bool=>FerruleValue.FromBoolean(v switch {"1"=>true,"0"=>false,_=>throw Bad()}),
                    FerruleValueKind.Int64 when long.TryParse(v,NumberStyles.AllowLeadingSign,CultureInfo.InvariantCulture,out long n)=>FerruleValue.FromInt64(n),
                    _=>throw Bad()};
                fields.Add(new FerruleField(names[i],new FerruleScalar(value)));
            }
            if(fields.Count!=names.Length) throw Bad();
            result.Add(new FerruleGroup(fields));
        }
        if(!source.AsSpan().SequenceEqual(FlatWorkbook.WriteRows(result,sheet,names,kinds))) throw Bad();
        return result.ToArray();
    }
    private static InvalidDataException Bad()=>new("Unsupported flat workbook package");
}
