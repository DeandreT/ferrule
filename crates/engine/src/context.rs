use mapping::RuntimeValue;

pub(super) fn runtime_field(value: RuntimeValue) -> &'static str {
    match value {
        RuntimeValue::MappingFilePath => "\0mapping_file_path",
        RuntimeValue::MainMappingFilePath => "\0main_mapping_file_path",
        RuntimeValue::CurrentDateTime => "\0current_datetime",
    }
}
