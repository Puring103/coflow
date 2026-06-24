/// Translation key components.
///
/// Keys identify a single translatable cell. They are emitted as
/// `{record_id}` for normal table records and as `{field_name}` for singleton
/// types (the table file name already encodes the type and field).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LocalizationKey {
    pub type_name: String,
    pub field_name: String,
    pub row_id: String,
}

impl LocalizationKey {
    #[must_use]
    pub fn format(&self) -> String {
        self.row_id.clone()
    }

    #[must_use]
    pub fn table_file_stem(&self, is_singleton: bool) -> String {
        if is_singleton {
            self.type_name.clone()
        } else {
            format!("{}_{}", self.type_name, self.field_name)
        }
    }
}

#[must_use]
pub fn format_key(_type_name: &str, _field_name: &str, row_id: &str) -> String {
    row_id.to_string()
}
