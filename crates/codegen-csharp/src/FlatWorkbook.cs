using System;
using System.IO;
using System.IO.Compression;

using System.Xml.Linq;

namespace Ferrule.Runtime;

internal static class FlatWorkbook
{
    internal static byte[] Write(FerruleInstance instance, string sheet, string[] names, FerruleValueKind[] kinds)
    {
        if (instance is not FerruleGroup group || group.Fields.Count != names.Length) throw new InvalidDataException("Incomplete worksheet row");
        XNamespace s = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
        XNamespace r = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
        XNamespace p = "http://schemas.openxmlformats.org/package/2006/relationships";
        XNamespace t = "http://schemas.openxmlformats.org/package/2006/content-types";
        var header = new XElement(s + "row", new XAttribute("r", 1));
        var data = new XElement(s + "row", new XAttribute("r", 2));
        XElement Text(string address, string value) => new(s + "c", new XAttribute("r",address), new XAttribute("t","inlineStr"), new XElement(s+"is",new XElement(s+"t",new XAttribute(XNamespace.Xml + "space","preserve"),value)));
        for (int i=0; i<names.Length; ++i)
        {
            var col = ((char)('A'+i)).ToString();
            header.Add(Text(col+"1", names[i]));
            if (!group.TryGetField(names[i],out var cell) || cell is not FerruleScalar scalar || scalar.Value.Kind != kinds[i]) throw new InvalidDataException("Missing or invalid worksheet cell");
            var v = scalar.Value;
            if (v.Kind == FerruleValueKind.String)
            {
                var text = v.StringValue;
                if (text.Length == 0 || text.Length > 1024 || text.Contains('_') || System.Array.Exists(text.ToCharArray(), char.IsControl)) throw new InvalidDataException("Unsupported worksheet text");
                data.Add(Text(col+"2",text));
            }
            else
            {
                if (v.Kind == FerruleValueKind.Int64 && (v.Int64Value < -999999999999999L || v.Int64Value > 999999999999999L)) throw new InvalidDataException("Worksheet integer precision");
                data.Add(new XElement(s+"c",new XAttribute("r",col+"2"), new XAttribute("t",v.Kind==FerruleValueKind.Bool?"b":"n"), new XElement(s+"v", v.Kind==FerruleValueKind.Bool?(v.BooleanValue?"1":"0"):v.Int64Value.ToString(System.Globalization.CultureInfo.InvariantCulture))));
            }
        }
        // A non-expandable stream caps the finished ZIP, including its directory.
        using var bytes = new MemoryStream(new byte[256 * 1024], true);
        bytes.SetLength(0);
        using (var zip = new ZipArchive(bytes,ZipArchiveMode.Create,true))
        {
            void Part(string name,XElement xml)
            {
                var entry=zip.CreateEntry(name,CompressionLevel.NoCompression);
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
            Part("xl/worksheets/sheet1.xml",new XElement(s+"worksheet",new XElement(s+"sheetData",header,data)));
        }
        return bytes.ToArray();
    }
}
