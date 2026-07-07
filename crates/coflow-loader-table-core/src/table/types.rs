use coflow_data_model::{CfdDiagnostic, CfdInputRecord, SourceDocument, SourceLocation};
use std::collections::BTreeMap;
use std::path::PathBuf;

const DEFAULT_KEY_COLUMN: &str = "id";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableSheetConfig {
    pub sheet: String,
    pub type_name: Option<String>,
    pub key: Option<String>,
    pub columns: BTreeMap<String, String>,
}

impl TableSheetConfig {
    #[must_use]
    pub fn new(sheet: impl Into<String>) -> Self {
        Self {
            sheet: sheet.into(),
            type_name: None,
            key: None,
            columns: BTreeMap::new(),
        }
    }

    #[must_use]
    pub fn with_type(mut self, type_name: impl Into<String>) -> Self {
        self.type_name = Some(type_name.into());
        self
    }

    #[must_use]
    pub fn with_key(mut self, key: impl Into<String>) -> Self {
        self.key = Some(key.into());
        self
    }

    #[must_use]
    pub fn with_columns(
        mut self,
        columns: impl IntoIterator<Item = (impl Into<String>, impl Into<String>)>,
    ) -> Self {
        self.columns = columns
            .into_iter()
            .map(|(source, field)| (source.into(), field.into()))
            .collect();
        self
    }

    #[must_use]
    pub fn type_name(&self) -> &str {
        self.type_name.as_deref().map_or(&self.sheet, |name| name)
    }

    #[must_use]
    pub fn key_column(&self) -> &str {
        self.key.as_deref().map_or(DEFAULT_KEY_COLUMN, |key| key)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableSource {
    pub name: PathBuf,
    pub document: SourceDocument,
    pub sheets: Vec<TableSheet>,
    pub configs: Vec<TableSheetConfig>,
}

impl TableSource {
    #[must_use]
    pub fn new(
        name: impl Into<PathBuf>,
        sheets: Vec<TableSheet>,
        configs: Vec<TableSheetConfig>,
    ) -> Self {
        let name = name.into();
        Self {
            document: SourceDocument::Local(name.clone()),
            name,
            sheets,
            configs,
        }
    }

    #[must_use]
    pub fn remote(
        name: impl Into<PathBuf>,
        document: impl Into<String>,
        sheets: Vec<TableSheet>,
        configs: Vec<TableSheetConfig>,
    ) -> Self {
        Self {
            name: name.into(),
            document: SourceDocument::Remote(document.into()),
            sheets,
            configs,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableSheet {
    pub name: String,
    pub rows: Vec<Vec<String>>,
    pub start_row: usize,
    pub start_column: usize,
}

impl TableSheet {
    #[must_use]
    pub fn new(name: impl Into<String>, rows: Vec<Vec<String>>) -> Self {
        Self {
            name: name.into(),
            rows,
            start_row: 1,
            start_column: 1,
        }
    }

    #[must_use]
    pub fn with_start(mut self, row: usize, column: usize) -> Self {
        self.start_row = row;
        self.start_column = column;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableDiagnostics {
    pub diagnostics: Vec<TableDiagnostic>,
}

#[derive(Debug, Clone)]
pub struct TableInputRecords {
    /// Each record carries its own [`RecordOrigin`] (a [`RecordOrigin::Table`]
    /// variant). Diagnostics produced before data-model diagnostics are mapped
    /// can use the records' origins to resolve labels back to source cells.
    pub records: Vec<CfdInputRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableWriteLayout {
    pub id_column: usize,
    pub field_columns: BTreeMap<Vec<String>, usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableDiagnostic {
    pub code: String,
    pub stage: String,
    pub message: String,
    pub source: Option<CfdDiagnostic>,
    pub primary: Option<TableLabel>,
    pub related: Vec<TableLabel>,
}

impl TableDiagnostic {
    #[must_use]
    pub fn table(
        code: impl Into<String>,
        stage: impl Into<String>,
        message: impl Into<String>,
        location: TableLocation,
    ) -> Self {
        Self {
            code: code.into(),
            stage: stage.into(),
            message: message.into(),
            source: None,
            primary: Some(TableLabel {
                location,
                message: None,
            }),
            related: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableLabel {
    pub location: TableLocation,
    pub message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableLocation {
    pub file: PathBuf,
    pub sheet: Option<String>,
    pub row: Option<usize>,
    pub column: Option<usize>,
}

impl From<TableLocation> for SourceLocation {
    fn from(location: TableLocation) -> Self {
        Self::TableCell {
            path: location.file,
            sheet: location.sheet,
            row: location.row.unwrap_or(1),
            column: location.column.unwrap_or(1),
        }
    }
}

impl TableLocation {
    #[must_use]
    pub fn new(file: impl Into<PathBuf>) -> Self {
        Self {
            file: file.into(),
            sheet: None,
            row: None,
            column: None,
        }
    }

    #[must_use]
    pub fn sheet(mut self, sheet: impl Into<String>) -> Self {
        self.sheet = Some(sheet.into());
        self
    }

    #[must_use]
    pub fn cell(mut self, row: usize, column: usize) -> Self {
        self.row = Some(row);
        self.column = Some(column);
        self
    }

    #[must_use]
    pub fn with_row(mut self, row: usize) -> Self {
        self.row = Some(row);
        self
    }

    #[must_use]
    pub fn with_column(mut self, column: Option<usize>) -> Self {
        self.column = column;
        self
    }
}
