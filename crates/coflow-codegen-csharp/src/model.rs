use serde::Serialize;

#[derive(Debug, Serialize)]
pub(crate) struct CsharpProject {
    pub(crate) namespace: String,
    pub(crate) database_class: String,
    pub(crate) enums: Vec<CsharpEnum>,
    pub(crate) types: Vec<CsharpType>,
    pub(crate) database: CsharpDatabase,
}

#[derive(Debug, Serialize)]
pub(crate) struct CsharpEnum {
    pub(crate) name: String,
    pub(crate) is_flags: bool,
    pub(crate) summary: Option<String>,
    pub(crate) obsolete: bool,
    pub(crate) variants: Vec<CsharpEnumVariant>,
}

#[derive(Debug, Serialize)]
pub(crate) struct CsharpEnumVariant {
    pub(crate) name: String,
    pub(crate) value: i64,
    pub(crate) summary: Option<String>,
    pub(crate) obsolete: bool,
}

#[derive(Debug, Serialize)]
pub(crate) struct CsharpType {
    pub(crate) name: String,
    pub(crate) declaration: String,
    pub(crate) summary: Option<String>,
    pub(crate) obsolete: bool,
    pub(crate) properties: Vec<CsharpProperty>,
}

#[derive(Debug, Serialize)]
pub(crate) struct CsharpProperty {
    pub(crate) name: String,
    pub(crate) type_name: String,
    pub(crate) setter: String,
    pub(crate) initializer: Option<String>,
    pub(crate) summary: Option<String>,
    pub(crate) obsolete: bool,
}

#[derive(Debug, Serialize)]
pub(crate) struct CsharpDatabase {
    pub(crate) tables: Vec<CsharpTable>,
    pub(crate) indexes: Vec<CsharpIndex>,
    pub(crate) constructor_parameters: Vec<CsharpParameter>,
    pub(crate) load_steps: Vec<String>,
    pub(crate) constructor_args: Vec<String>,
    pub(crate) loaders: Vec<CsharpLoader>,
    pub(crate) polymorphic_loaders: Vec<CsharpPolymorphicLoader>,
    pub(crate) resolve: Option<CsharpResolve>,
}

#[derive(Debug, Serialize)]
pub(crate) struct CsharpTable {
    pub(crate) name: String,
    pub(crate) list_property: String,
    pub(crate) list_var: String,
    pub(crate) item_var: String,
    pub(crate) id_type: String,
    pub(crate) id_property: String,
    pub(crate) id_source_name: String,
    pub(crate) index_field: String,
    pub(crate) index_var: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct CsharpIndex {
    pub(crate) table_name: String,
    pub(crate) list_property: String,
    pub(crate) list_var: String,
    pub(crate) field_property: String,
    pub(crate) key_type: String,
    pub(crate) storage_field: String,
    pub(crate) parameter_name: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct CsharpParameter {
    pub(crate) ty: String,
    pub(crate) name: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct CsharpLoader {
    pub(crate) type_name: String,
    pub(crate) fields: Vec<CsharpLoadField>,
}

#[derive(Debug, Serialize)]
pub(crate) struct CsharpLoadField {
    pub(crate) property: String,
    pub(crate) read_expr: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct CsharpPolymorphicLoader {
    pub(crate) type_name: String,
    pub(crate) cases: Vec<CsharpPolymorphicCase>,
    pub(crate) expected: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct CsharpPolymorphicCase {
    pub(crate) type_name: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct CsharpResolve {
    pub(crate) parameters: Vec<CsharpParameter>,
    pub(crate) table_calls: Vec<CsharpResolveTableCall>,
    pub(crate) methods: Vec<CsharpResolveMethod>,
}

#[derive(Debug, Serialize)]
pub(crate) struct CsharpResolveTableCall {
    pub(crate) table_name: String,
    pub(crate) list_var: String,
    pub(crate) item_var: String,
    pub(crate) id_property: String,
    pub(crate) index_args: String,
    pub(crate) path_expr: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct CsharpResolveMethod {
    pub(crate) type_name: String,
    pub(crate) is_polymorphic: bool,
    pub(crate) parameters: Vec<CsharpParameter>,
    pub(crate) statements: Vec<String>,
    pub(crate) cases: Vec<CsharpResolveCase>,
}

#[derive(Debug, Serialize)]
pub(crate) struct CsharpResolveCase {
    pub(crate) type_name: String,
    pub(crate) var_name: String,
    pub(crate) index_args: String,
}
