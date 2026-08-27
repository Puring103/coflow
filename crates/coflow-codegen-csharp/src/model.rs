use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct CsharpProject {
    pub namespace: String,
    pub uses_localization: bool,
    pub enums: Vec<CsharpEnum>,
    pub types: Vec<CsharpType>,
    pub singletons: Vec<CsharpSingleton>,
    pub constants: Vec<CsharpConstant>,
}

#[derive(Debug, Serialize)]
pub struct CsharpConstant {
    pub source_name: String,
    pub runtime_type: String,
    pub value_expression: String,
    pub deferred: bool,
}

#[derive(Debug, Serialize)]
pub struct CsharpSingleton {
    pub source_name: String,
}

#[derive(Debug, Serialize)]
pub struct CsharpEnum {
    pub name: String,
    pub namespace: String,
    pub qualified_name: String,
    pub relative_path: String,
    pub metadata_name: String,
    pub source_name: String,
    pub annotations: Vec<CsharpAnnotation>,
    pub is_flags: bool,
    pub summary: Option<String>,
    pub obsolete: bool,
    pub variants: Vec<CsharpEnumVariant>,
}

#[derive(Debug, Serialize)]
pub struct CsharpEnumVariant {
    pub name: String,
    pub source_name: String,
    pub value: i64,
    pub annotations: Vec<CsharpAnnotation>,
    pub summary: Option<String>,
    pub obsolete: bool,
}

#[derive(Debug, Serialize)]
pub struct CsharpType {
    pub name: String,
    pub namespace: String,
    pub qualified_name: String,
    pub relative_path: String,
    pub metadata_name: String,
    pub source_name: String,
    pub annotations: Vec<CsharpAnnotation>,
    pub declaration: String,
    pub constructor_visibility: String,
    pub summary: Option<String>,
    pub obsolete: bool,
    pub properties: Vec<CsharpProperty>,
    pub functions: Vec<CsharpFunction>,
    pub host_fields: Vec<CsharpHostField>,
    pub uses_host_slot: bool,
    pub declares_host_slot: bool,
    pub constructor_parameters: Vec<CsharpParameter>,
    pub base_constructor_args: Vec<String>,
    pub base_constructor_call: Option<String>,
    pub assignments: Vec<CsharpConstructorAssignment>,
    pub equality: Option<CsharpEquality>,
    pub loader_fields: Vec<CsharpLoaderField>,
    pub loader_id_type: Option<String>,
    pub loader_id_reader: Option<String>,
    pub loader_enabled: bool,
    pub is_host: bool,
    pub is_abstract: bool,
    pub is_sealed: bool,
    pub loader_assignable_to: Vec<String>,
    pub loader_variants: Vec<CsharpLoaderVariant>,
}

#[derive(Debug, Serialize)]
pub struct CsharpHostField {
    pub target: String,
    pub parameter: CsharpParameter,
}

#[derive(Debug, Serialize)]
pub struct CsharpFunction {
    pub source_name: String,
    pub method_name: String,
    pub bind_method_name: String,
    pub bind_parameter_name: String,
    pub slot_name: String,
    pub declared_here: bool,
    pub result_type: String,
    pub delegate_type: String,
    pub parameters: Vec<CsharpParameter>,
    pub returns_void: bool,
    pub summary: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct CsharpLoaderField {
    pub source_name: String,
    pub property_name: String,
    pub value_type: String,
    pub is_function: bool,
    pub reader_expression: String,
    pub default_expression: Option<String>,
    pub object_type: Option<String>,
    pub reference_type: Option<String>,
    pub annotations: Vec<CsharpAnnotation>,
}

#[derive(Debug, Serialize)]
pub struct CsharpAnnotation {
    pub name: String,
    pub arguments: Vec<CsharpAnnotationArgument>,
}

#[derive(Debug, Serialize)]
pub struct CsharpAnnotationArgument {
    pub kind: &'static str,
    pub value_expression: String,
}

#[derive(Debug, Serialize)]
pub struct CsharpLoaderVariant {
    pub source_name: String,
    pub type_name: String,
}

#[derive(Debug, Serialize)]
pub struct CsharpProperty {
    pub visibility: String,
    pub name: String,
    pub type_name: String,
    pub backing_field: Option<String>,
    pub guard_host: bool,
    pub summary: Option<String>,
    pub obsolete: bool,
}

#[derive(Debug, Serialize)]
pub struct CsharpConstructorAssignment {
    pub property: String,
    pub target: String,
    pub parameter: String,
}

#[derive(Debug, Serialize)]
pub struct CsharpParameter {
    pub ty: String,
    pub name: String,
}

#[derive(Debug, Serialize)]
pub struct CsharpEquality {
    pub key_property: String,
    pub is_struct: bool,
    /// When true, equality compares all fields (used for inline-only types
    /// without an Id). When false, compares only `key_property`.
    pub by_fields: bool,
    /// Property names participating in by-fields equality.
    pub fields: Vec<String>,
}
