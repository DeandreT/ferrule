using System.Globalization;
using System.Text;

namespace Ferrule.Runtime;

public sealed partial class FerruleEdi
{
    private sealed class EnvelopeInterchange(string reference)
    {
        internal string Reference { get; } = reference;
        internal int Messages { get; set; }
        internal int Groups { get; set; }
        internal bool? Grouped { get; set; }
    }
    private sealed class EnvelopeGroup(string reference)
    {
        internal string Reference { get; } = reference;
        internal int Messages { get; set; }
    }
    private sealed record EnvelopeMessage(string Reference, int Start);

    /// <summary>Validate complete EDIFACT envelopes independently of the projected schema.</summary>
    public void ValidateEnvelope(byte[] bytes)
    {
        ArgumentNullException.ThrowIfNull(bytes);
        if (_kind != "Edifact") throw Fail(FerruleEdiError.Descriptor, "Strict envelope validation is available for EDIFACT.");
        if (bytes.Length > MaximumBytes) throw Fail(FerruleEdiError.Limit, "EDI input exceeds 64 MiB.");
        try { ValidateEnvelopeSegments(Tokenize(bytes).Item1); }
        catch (DecoderFallbackException) { throw Fail(FerruleEdiError.Format, "EDI input is not UTF-8."); }
    }

    private static FerruleEdiException EnvelopeFailure(int index, string reason) =>
        new(FerruleEdiError.Envelope, $"EDIFACT envelope segment {index}: {reason}", segmentIndex: index);

    private static string Control(Segment segment, int index, int position)
    {
        if (position >= segment.Elements.Count) throw EnvelopeFailure(index, "missing control value");
        var repeats = segment.Elements[position];
        if (repeats.Count != 1) throw EnvelopeFailure(index, "control value must be scalar");
        var components = repeats[0];
        if (components.Count != 1 || components[0].Length == 0) throw EnvelopeFailure(index, "missing or composite control value");
        return components[0];
    }

    private static void CheckTrailer(Segment segment, int index, int count, string reference)
    {
        string raw = Control(segment, index, 0);
        if (!raw.All(char.IsAsciiDigit) || !ulong.TryParse(raw, NumberStyles.None, CultureInfo.InvariantCulture, out ulong value) || value != (ulong)count)
            throw EnvelopeFailure(index, "control count mismatch");
        if (Control(segment, index, 1) != reference) throw EnvelopeFailure(index, "control reference mismatch");
    }

    private static void ValidateEnvelopeSegments(List<Segment> segments)
    {
        bool standalone = segments.Count > 0 && segments[0].Id == "UNH";
        EnvelopeInterchange? interchange = null;
        EnvelopeGroup? group = null;
        EnvelopeMessage? message = null;
        for (int index = 0; index < segments.Count; index++)
        {
            Segment segment = segments[index];
            switch (segment.Id)
            {
                case "UNB":
                    if (standalone || interchange is not null || group is not null || message is not null) throw EnvelopeFailure(index, "interchange begins before prior envelope is closed");
                    interchange = new(Control(segment, index, 4));
                    break;
                case "UNG":
                    if (group is not null || message is not null) throw EnvelopeFailure(index, "group begins before prior envelope is closed");
                    if (interchange is null) throw EnvelopeFailure(index, "group has no interchange");
                    if (interchange.Grouped == false) throw EnvelopeFailure(index, "grouped and ungrouped messages cannot mix");
                    interchange.Grouped = true;
                    group = new(Control(segment, index, 4));
                    break;
                case "UNH":
                    if (message is not null) throw EnvelopeFailure(index, "message begins before UNT");
                    if (!standalone)
                    {
                        if (interchange is null) throw EnvelopeFailure(index, "message has no interchange");
                        if (group is null && interchange.Grouped == true) throw EnvelopeFailure(index, "message is outside its functional group");
                        if (group is null) interchange.Grouped = false;
                    }
                    message = new(Control(segment, index, 0), index);
                    break;
                case "UNT":
                    if (message is null) throw EnvelopeFailure(index, "UNT has no UNH");
                    CheckTrailer(segment, index, index - message.Start + 1, message.Reference);
                    message = null;
                    if (group is not null) group.Messages++;
                    if (interchange is not null) interchange.Messages++;
                    break;
                case "UNE":
                    if (message is not null) throw EnvelopeFailure(index, "UNE precedes UNT");
                    if (group is null) throw EnvelopeFailure(index, "UNE has no UNG");
                    CheckTrailer(segment, index, group.Messages, group.Reference);
                    group = null;
                    if (interchange is null) throw EnvelopeFailure(index, "group has no interchange");
                    interchange.Groups++;
                    break;
                case "UNZ":
                    if (message is not null || group is not null) throw EnvelopeFailure(index, "UNZ precedes its message or group trailer");
                    if (interchange is null) throw EnvelopeFailure(index, "UNZ has no UNB");
                    CheckTrailer(segment, index, interchange.Grouped == true ? interchange.Groups : interchange.Messages, interchange.Reference);
                    interchange = null;
                    break;
                default:
                    if (message is null) throw EnvelopeFailure(index, "segment is outside a message");
                    break;
            }
        }
        if (interchange is not null || group is not null || message is not null) throw EnvelopeFailure(segments.Count, "unterminated envelope");
    }
}
