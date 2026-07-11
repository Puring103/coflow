#[derive(Debug)]
pub struct TableConformanceCase {
    pub name: &'static str,
    pub source_rows: Vec<Vec<String>>,
    pub target_header: Vec<String>,
    pub expected_rows: Vec<Vec<String>>,
    pub added: Vec<String>,
    pub removed: Vec<String>,
}

impl TableConformanceCase {
    #[must_use]
    #[allow(dead_code)]
    pub fn expected_storage_rows(&self) -> Vec<Vec<String>> {
        let width = self
            .source_rows
            .first()
            .map_or(0, Vec::len)
            .max(self.target_header.len());
        self.expected_rows
            .iter()
            .map(|row| {
                let mut row = row.clone();
                row.resize(width, String::new());
                row
            })
            .collect()
    }
}

#[must_use]
pub fn table_conformance_cases() -> Vec<TableConformanceCase> {
    vec![
        TableConformanceCase {
            name: "add-remove-reorder",
            source_rows: rows(&[
                &["id", "name", "obsolete", "power"],
                &["sword", "Sword", "legacy", "10"],
                &["shield", "Shield", "old", "5"],
            ]),
            target_header: row(&["power", "id", "name", "rarity"]),
            expected_rows: rows(&[
                &["power", "id", "name", "rarity"],
                &["10", "sword", "Sword", ""],
                &["5", "shield", "Shield", ""],
            ]),
            added: row(&["rarity"]),
            removed: row(&["obsolete"]),
        },
        TableConformanceCase {
            name: "repeated-expand-columns",
            source_rows: rows(&[
                &["id", "env", "", "", "obsolete"],
                &["zone", "1", "2", "3", "legacy"],
            ]),
            target_header: row(&["", "id", "env", "", "rarity"]),
            expected_rows: rows(&[
                &["", "id", "env", "", "rarity"],
                &["2", "zone", "1", "3", ""],
            ]),
            added: row(&["rarity"]),
            removed: row(&["obsolete"]),
        },
        TableConformanceCase {
            name: "remove-trailing-columns",
            source_rows: rows(&[
                &["id", "name", "tail", "tail2"],
                &["sword", "Sword", "stale", "stale2"],
            ]),
            target_header: row(&["name", "id"]),
            expected_rows: rows(&[&["name", "id"], &["Sword", "sword"]]),
            added: Vec::new(),
            removed: row(&["tail", "tail2"]),
        },
    ]
}

fn rows(values: &[&[&str]]) -> Vec<Vec<String>> {
    values.iter().map(|values| row(values)).collect()
}

fn row(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_string()).collect()
}
