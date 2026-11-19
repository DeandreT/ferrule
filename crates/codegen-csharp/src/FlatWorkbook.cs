using System;
using System.IO;
using System.IO.Compression;
using System.Linq;
using System.Globalization;
using System.Collections.Generic;
using System.Xml;
using System.Text;

using System.Xml.Linq;

namespace Ferrule.Runtime;

internal static class FlatWorkbook
{
    internal static byte[] Write(FerruleInstance instance, string sheet, string[] names, FerruleValueKind[] kinds)
    {
        return WriteRows(new[] { instance },sheet,names,kinds);
    }
    internal static byte[] WriteRows(IReadOnlyList<FerruleInstance> rows, string sheet, string[] names, FerruleValueKind[] kinds)
    {
        if(rows.Count>256) throw new InvalidDataException("Too many rows");
        XNamespace s = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
        XNamespace r = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
        XNamespace p = "http://schemas.openxmlformats.org/package/2006/relationships";
        XNamespace t = "http://schemas.openxmlformats.org/package/2006/content-types";
        var header = new XElement(s + "row", new XAttribute("r", 1));
        var table = new XElement(s + "sheetData",header);
        XElement Text(string address, string value) => new(s + "c", new XAttribute("r",address), new XAttribute("t","inlineStr"), new XElement(s+"is",new XElement(s+"t",new XAttribute(XNamespace.Xml + "space","preserve"),value)));
        for(int i=0;i<names.Length;i++) header.Add(Text(((char)('A'+i)).ToString()+"1",names[i]));
        int budget = 256*1024;
        for(int index=0;index<rows.Count;index++){
        if(rows[index] is not FerruleGroup group || group.Fields.Count!=names.Length) throw new InvalidDataException("Incomplete worksheet row");
        string number=(index+2).ToString(CultureInfo.InvariantCulture);
        var data = new XElement(s + "row",new XAttribute("r",number)); table.Add(data);
        for (int i=0; i<names.Length; ++i)
        {
            var col = ((char)('A'+i)).ToString();
            budget -= 16;
            if (!group.TryGetField(names[i],out var cell) || cell is not FerruleScalar scalar || scalar.Value.Kind != kinds[i]) throw new InvalidDataException("Missing or invalid worksheet cell");
            var v = scalar.Value;
            if (v.Kind == FerruleValueKind.String)
            {
                var text = v.StringValue;
                if (text.Length == 0 || text.Length > 1024 || text.Contains('_') || System.Array.Exists(text.ToCharArray(), char.IsControl)) throw new InvalidDataException("Unsupported worksheet text");
                budget -= Encoding.UTF8.GetByteCount(text); data.Add(Text(col+number,text));
            }
            else
            {
                if (v.Kind == FerruleValueKind.Int64 && (v.Int64Value < -999999999999999L || v.Int64Value > 999999999999999L)) throw new InvalidDataException("Worksheet integer precision");
                data.Add(new XElement(s+"c",new XAttribute("r",col+number), new XAttribute("t",v.Kind==FerruleValueKind.Bool?"b":"n"), new XElement(s+"v", v.Kind==FerruleValueKind.Bool?(v.BooleanValue?"1":"0"):v.Int64Value.ToString(System.Globalization.CultureInfo.InvariantCulture))));
            }
        }
            if(budget<0) throw new InvalidDataException("Worksheet limit");
        }
        // A non-expandable stream caps the finished ZIP, including its directory.
        using var bytes = new MemoryStream(new byte[256 * 1024], true);
        bytes.SetLength(0);
        using (var zip = new ZipArchive(bytes,ZipArchiveMode.Create,true))
        {
            void Part(string name,XElement xml)
            {
                var entry=zip.CreateEntry(name,CompressionLevel.NoCompression);
                entry.ExternalAttributes=unchecked((int)0x81a40000);
                entry.LastWriteTime=new DateTimeOffset(1980,1,1,0,0,0,TimeSpan.Zero);
                using var dest=entry.Open();
                xml.Save(dest,SaveOptions.DisableFormatting);
            }
            XElement Type(string part,string mime) => new(t+"Override",new XAttribute("PartName",part),new XAttribute("ContentType",mime));
            XElement Links(string type,string target) => new(p+"Relationships",new XElement(p+"Relationship",new XAttribute("Id","rId1"),new XAttribute("Type",r.NamespaceName+"/"+type),new XAttribute("Target",target)));
            Part("[Content_Types].xml",new XElement(t+"Types",new XElement(t+"Default",new XAttribute("Extension","rels"),new XAttribute("ContentType","application/vnd.openxmlformats-package.relationships+xml")),Type("/xl/workbook.xml","application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"),Type("/xl/worksheets/sheet1.xml","application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml")));
            Part("_rels/.rels",Links("officeDocument","xl/workbook.xml"));
            Part("xl/workbook.xml",new XElement(s+"workbook",new XAttribute(XNamespace.Xmlns+"r",r),new XElement(s+"sheets",new XElement(s+"sheet",new XAttribute("name",sheet),new XAttribute("sheetId",1),new XAttribute(r+"id","rId1")))));
            Part("xl/_rels/workbook.xml.rels",Links("worksheet","worksheets/sheet1.xml"));
            Part("xl/worksheets/sheet1.xml",new XElement(s+"worksheet",table));
        }
        var package=bytes.ToArray();
        // ZipArchive records the host OS in the central directory. This profile
        // always identifies stored regular files with the same creator byte.
        int central=System.Buffers.Binary.BinaryPrimitives.ReadInt32LittleEndian(package.AsSpan(package.Length-6));
        for(int part=0;part<5;part++) {
            package[central+5]=3;
            central+=46+System.Buffers.Binary.BinaryPrimitives.ReadUInt16LittleEndian(package.AsSpan(central+28));
        }
        return package;
    }
}
