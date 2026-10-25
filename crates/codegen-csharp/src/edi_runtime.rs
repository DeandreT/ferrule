//! Shared package-free EDI codec inventory for primary and static adapters.
use crate::composition::Library;

pub(crate) fn add_runtime(library: &mut Library) {
    library.add_runtime(&crate::x12_output::X12_SOURCES);
    library.add_runtime(&[
        (
            "Runtime/FerruleEdi.cs",
            include_str!("../../../runtime/csharp/Ferrule.Runtime/FerruleEdi.cs"),
        ),
        (
            "Runtime/FerruleEdi.Tokenizer.cs",
            include_str!("../../../runtime/csharp/Ferrule.Runtime/FerruleEdi.Tokenizer.cs"),
        ),
        (
            "Runtime/FerruleEdi.Writer.cs",
            include_str!("../../../runtime/csharp/Ferrule.Runtime/FerruleEdi.Writer.cs"),
        ),
        (
            "Runtime/FerruleEdi.Completion.cs",
            include_str!("../../../runtime/csharp/Ferrule.Runtime/FerruleEdi.Completion.cs"),
        ),
        (
            "Runtime/FerruleEdi.Envelope.cs",
            include_str!("../../../runtime/csharp/Ferrule.Runtime/FerruleEdi.Envelope.cs"),
        ),
    ]);
}
