use coflow_data_model::cell_value::CellRenderError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableWriteDiagnostics {
    pub diagnostics: Vec<TableWriteDiagnostic>,
}

impl TableWriteDiagnostics {
    pub fn iter(&self) -> std::slice::Iter<'_, TableWriteDiagnostic> {
        self.diagnostics.iter()
    }
}

impl<'a> IntoIterator for &'a TableWriteDiagnostics {
    type Item = &'a TableWriteDiagnostic;
    type IntoIter = std::slice::Iter<'a, TableWriteDiagnostic>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableWriteDiagnostic {
    pub code: String,
    pub stage: String,
    pub message: String,
}

pub(super) fn one_error(code: &'static str, message: impl Into<String>) -> TableWriteDiagnostics {
    TableWriteDiagnostics {
        diagnostics: vec![TableWriteDiagnostic {
            code: code.to_string(),
            stage: "TABLE".to_string(),
            message: message.into(),
        }],
    }
}

pub(super) fn table_render_error(err: CellRenderError) -> TableWriteDiagnostics {
    let message = match err {
        CellRenderError::AnonymousEnum => {
            "writing anonymous enum values into table cells is not supported"
        }
        CellRenderError::NestedObject => {
            "writing nested object values into table cells is not supported"
        }
    };
    one_error("TABLE-WRITE", message)
}
