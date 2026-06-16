use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct CsharpProject {
    pub namespace: String,
    pub database_class: String,
    pub enums: Vec<CsharpEnum>,
    pub types: Vec<CsharpType>,
    pub database: CsharpDatabase,
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
    pub summary: Option<String>,
    pub obsolete: bool,
    pub properties: Vec<CsharpProperty>,
}

#[derive(Debug, Serialize)]
pub struct CsharpProperty {
    pub visibility: String,
    pub name: String,
    pub type_name: String,
    pub setter: String,
    pub initializer: Option<String>,
    pub summary: Option<String>,
    pub obsolete: bool,
}

#[derive(Debug, Serialize)]
pub struct CsharpDatabase {
    pub tables: Vec<CsharpTable>,
    pub ref_indexes: Vec<CsharpRefIndex>,
    pub constructor_parameters: Vec<CsharpParameter>,
    pub load_steps: Vec<String>,
    pub constructor_args: Vec<String>,
    pub loaders: Vec<CsharpLoader>,
    pub polymorphic_loaders: Vec<CsharpPolymorphicLoader>,
    pub resolve: Option<CsharpResolve>,
}

#[derive(Debug, Serialize)]
pub struct CsharpTable {
    pub name: String,
    pub source_name: String,
    pub list_property: String,
    pub list_var: String,
    pub item_var: String,
    pub id_type: String,
    pub id_property: String,
    pub id_source_name: String,
    pub index_field: String,
    pub index_var: String,
}

#[derive(Debug, Serialize)]
pub struct CsharpRefIndex {
    pub target_name: String,
    pub target_source_name: String,
    pub target_id_type: String,
    pub index_field: String,
    pub parameter_name: String,
    pub placeholder_name: Option<String>,
    pub assignable_sources: Vec<CsharpRefIndexSource>,
}

#[derive(Debug, Serialize)]
pub struct CsharpRefIndexSource {
    pub list_var: String,
    pub table_name: String,
    pub source_type_name: String,
    pub index_var: String,
    pub id_property: String,
    pub id_source_name: String,
}

#[derive(Debug, Serialize)]
pub struct CsharpParameter {
    pub ty: String,
    pub name: String,
}

#[derive(Debug, Serialize)]
pub struct CsharpLoader {
    pub type_name: String,
    pub key_type_name: String,
    pub key_local_name: String,
    pub key_read_expr: String,
    pub key_messagepack_read_expr: String,
    pub fields: Vec<CsharpLoadField>,
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
    pub is_required: bool,
    pub assignments: Vec<CsharpLoadAssignment>,
}

#[derive(Debug, Serialize)]
pub struct CsharpLoadAssignment {
    pub property: String,
    pub expr: String,
}

#[derive(Debug, Serialize)]
pub struct CsharpPolymorphicLoader {
    pub type_name: String,
    pub cases: Vec<CsharpPolymorphicCase>,
    pub expected: String,
}

#[derive(Debug, Serialize)]
pub struct CsharpPolymorphicCase {
    pub type_name: String,
    pub source_name: String,
}

#[derive(Debug, Serialize)]
pub struct CsharpResolve {
    pub parameters: Vec<CsharpParameter>,
    pub table_calls: Vec<CsharpResolveTableCall>,
    pub methods: Vec<CsharpResolveMethod>,
}

#[derive(Debug, Serialize)]
pub struct CsharpResolveTableCall {
    pub table_name: String,
    pub source_name: String,
    pub list_var: String,
    pub item_var: String,
    pub id_property: String,
    pub index_args: String,
    pub path_expr: String,
    pub returns_value: bool,
}

#[derive(Debug, Serialize)]
pub struct CsharpResolveMethod {
    pub type_name: String,
    pub returns_value: bool,
    pub is_polymorphic: bool,
    pub parameters: Vec<CsharpParameter>,
    pub statements: Vec<String>,
    pub cases: Vec<CsharpResolveCase>,
}

#[derive(Debug, Serialize)]
pub struct CsharpResolveCase {
    pub type_name: String,
    pub var_name: String,
    pub index_args: String,
}
