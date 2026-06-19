pub mod commands;
pub mod patch;
pub mod types;

#[cfg(test)]
mod tests {
    use crate::types::*;
    use ts_rs::TS;

    #[test]
    fn export_all_bindings() {
        ProjectSnapshot::export_all().unwrap();
        FileRecords::export_all().unwrap();
        RecordRow::export_all().unwrap();
        GraphData::export_all().unwrap();
        FieldSchema::export_all().unwrap();
        SearchHit::export_all().unwrap();
    }
}
