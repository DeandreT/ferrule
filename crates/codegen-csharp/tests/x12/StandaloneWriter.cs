using System.Runtime.CompilerServices;
using System.Text;
using Ferrule.Generated;
using Ferrule.Runtime;

try
{
    GeneratedMapping.WriteMapForceX12DocumentWithClock(null!, null!, null!, null!);
    throw new InvalidOperationException("Expected the writer to reject a null clock.");
}
catch (ArgumentNullException error) when (error.ParamName == "clock") { }

static FerruleGroup Group(params FerruleField[] fields) => new(fields);
static FerruleField Repeated(string name, FerruleGroup item) =>
    new(name, new FerruleRepeated([item]));

var message = Group(new FerruleField("ST", Group()));
var group = Group(Repeated("GS", Group()), Repeated("Message_945", message));
var body = Group(
    Repeated("Interchange", Group(Repeated("ISA", Group()), Repeated("Group", group)))
);
const string timestamp = "2031-06-07T08:09:10";
var execution = new FerruleExecutionContext("test", "test", timestamp);
var expected = Encoding.Latin1.GetBytes(
    "ISA*00*          *00*          *ZZ*               *ZZ*               *310607*0809* *00401*000000000*0*P*:~\r\n"
        + "GS****20310607*0809~\r\nST*945*0001~\r\nSE*2*0001~\r\nGE*1~\r\nIEA*1*000000000~\r\n"
);

using var plain = new MemoryStream();
using var clocked = new MemoryStream();
GeneratedMapping.WriteMapForceX12Document(body, execution, plain);
GeneratedMapping.WriteMapForceX12DocumentWithClock(body, execution, _ => timestamp, clocked);
if (
    !expected.SequenceEqual(plain.ToArray())
    || !expected.SequenceEqual(clocked.ToArray())
    || !expected.SequenceEqual(GeneratedMapping.SerializeMapForceX12Document(body, execution))
    || !expected.SequenceEqual(
        GeneratedMapping.SerializeMapForceX12DocumentWithClock(body, execution, _ => timestamp)
    )
)
    throw new InvalidOperationException("Standalone writer output differs.");

// Prove the unrelated initializer really would fail on this generated type.
try
{
    RuntimeHelpers.RunClassConstructor(typeof(GeneratedMapping).TypeHandle);
    throw new InvalidOperationException("Expected the deeply nested parameter plan to be refused.");
}
catch (TypeInitializationException error)
    when (error.InnerException
            is FerruleJsonParameterInvocationException
            {
                Error: FerruleJsonParameterInvocationError.Plan
            }
    ) { }
