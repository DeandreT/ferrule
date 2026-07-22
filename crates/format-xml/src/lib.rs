//! XSD-lite and bounded DTD-lite schema import plus XML instance read/write.
//! DTD support includes internal parameter entities used as content-model
//! particles; XML parsing never loads external entity or subset identifiers.

pub mod dtd;
mod file_set;
mod instance;
pub mod xsd;

pub use file_set::{LocalFileSetError, LocalFileSetLimits, LocalXmlFileSet, read_local_file_set};
pub use instance::{
    XmlFormatError, XmlWriteOptions, from_str, read, to_string, to_string_with_options, write,
};
