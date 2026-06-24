use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct CsharpProject {
    pub namespace: String,
    pub database_class: String,
    pub data_format: String,
    pub uses_json: bool,
    pub uses_messagepack: bool,
    pub uses_localization: bool,
    pub int_type: &'static str,
    pub float_type: &'static str,
    pub enums: Vec<CsharpEnum>,
    pub types: Vec<CsharpType>,
    pub database: CsharpDatabase,
    pub singletons: Vec<CsharpSingleton>,
}

/// Per-singleton metadata used by the database template. Singletons do not
/// generate `Tb*` accessors; the database class exposes a property whose name
/// is the type name (lower-cased identifier) and whose value loads the single
/// row from `<TypeName>.<ext>`.
#[derive(Debug, Serialize)]
pub struct CsharpSingleton {
    pub type_name: String,
    pub source_name: String,
    /// Public property name on the database class. Equals `type_name` per
    /// spec — no `PascalCase` rewrite.
    pub accessor_property: String,
    pub records_var: String,
}

#[derive(Debug, Serialize)]
pub struct CsharpEnum {
    pub name: String,
    pub is_flags: bool,
    pub summary: Option<String>,
    pub obsolete: bool,
    pub variants: Vec<CsharpEnumVariant>,
}

#[derive(Debug, Serialize)]
pub struct CsharpEnumVariant {
    pub name: String,
    pub value: i64,
    pub summary: Option<String>,
    pub obsolete: bool,
}

#[derive(Debug, Serialize)]
pub struct CsharpType {
    pub name: String,
    pub declaration: String,
    pub constructor_visibility: String,
    pub summary: Option<String>,
    pub obsolete: bool,
    pub properties: Vec<CsharpProperty>,
    pub constructor_parameters: Vec<CsharpParameter>,
    pub base_constructor_args: Vec<String>,
    pub base_constructor_call: Option<String>,
    pub assignments: Vec<CsharpConstructorAssignment>,
    pub loader: Option<CsharpLoader>,
    pub equality: Option<CsharpEquality>,
}

#[derive(Debug, Serialize)]
pub struct CsharpProperty {
    pub visibility: String,
    pub name: String,
    pub type_name: String,
    pub summary: Option<String>,
    pub obsolete: bool,
}

#[derive(Debug, Serialize)]
pub struct CsharpConstructorAssignment {
    pub property: String,
    pub parameter: String,
}

#[derive(Debug, Serialize)]
pub struct CsharpDatabase {
    pub tables: Vec<CsharpTable>,
    pub constructor_parameters: Vec<CsharpParameter>,
    pub load_steps: Vec<String>,
    pub constructor_args: Vec<String>,
    pub context_fields: Vec<CsharpContextField>,
    pub context_lookups: Vec<CsharpContextLookup>,
    pub context_constructor_parameters: Vec<CsharpParameter>,
    pub context_assignments: Vec<CsharpContextAssignment>,
}

#[derive(Debug, Serialize)]
pub struct CsharpTable {
    pub name: String,
    pub source_name: String,
    pub accessor_property: String,
    pub accessor_parameter: String,
    pub records_var: String,
    pub id_type: String,
    pub id_property: String,
    pub id_source_name: String,
    pub index_var: String,
}

#[derive(Debug, Serialize)]
pub struct CsharpContextField {
    pub source_name: String,
    pub field_name: String,
    pub id_type: String,
    pub type_name: String,
}

#[derive(Debug, Serialize)]
pub struct CsharpContextAssignment {
    pub field_name: String,
    pub parameter_name: String,
}

#[derive(Debug, Serialize)]
pub struct CsharpContextLookup {
    pub method_name: String,
    pub id_type: String,
    pub return_type: String,
    pub fields: Vec<CsharpContextLookupField>,
}

#[derive(Debug, Serialize)]
pub struct CsharpContextLookupField {
    pub field_name: String,
    pub value_name: String,
}

#[derive(Debug, Serialize)]
pub struct CsharpParameter {
    pub ty: String,
    pub name: String,
}

#[derive(Debug, Serialize)]
pub struct CsharpLoader {
    pub type_name: String,
    pub source_name: String,
    pub key_type_name: String,
    pub key_local_name: String,
    pub key_property: String,
    pub key_read_expr: String,
    pub key_messagepack_read_expr: String,
    pub has_id: bool,
    pub fields: Vec<CsharpLoadField>,
    pub polymorphic_cases: Vec<CsharpPolymorphicCase>,
    pub is_polymorphic: bool,
    pub expected: String,
}

#[derive(Debug, Serialize)]
pub struct CsharpLoadField {
    pub property: String,
    pub source_name: String,
    pub local_name: String,
    pub type_name: String,
    pub read_expr: String,
    pub messagepack_read_expr: String,
    pub default_expr: Option<String>,
    pub missing_expr: Option<String>,
    pub is_required: bool,
    pub has_name: String,
}

#[derive(Debug, Serialize)]
pub struct CsharpPolymorphicCase {
    pub type_name: String,
    pub source_name: String,
}

#[derive(Debug, Serialize)]
pub struct CsharpEquality {
    pub key_property: String,
    pub is_struct: bool,
}
