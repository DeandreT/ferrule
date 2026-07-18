//! XSD-lite and bounded DTD-lite schema import plus XML instance read/write.

pub mod dtd;
mod file_set;
mod instance;
pub mod xsd;

pub use file_set::{LocalFileSetError, LocalFileSetLimits, LocalXmlFileSet, read_local_file_set};
pub use instance::{XmlFormatError, from_str, read, to_string, write};
