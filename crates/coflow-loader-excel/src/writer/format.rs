use std::path::Path;

use coflow_api::{Diagnostic, DiagnosticSet, ResolvedSource, WriterCapabilities};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExcelWorkbookFormat {
    Xlsx,
    Xlsm,
    Xls,
    Unsupported,
}

impl ExcelWorkbookFormat {
    fn from_path(path: &Path) -> Self {
        match path.extension().and_then(|extension| extension.to_str()) {
            Some(extension) if extension.eq_ignore_ascii_case("xlsx") => Self::Xlsx,
            Some(extension) if extension.eq_ignore_ascii_case("xlsm") => Self::Xlsm,
            Some(extension) if extension.eq_ignore_ascii_case("xls") => Self::Xls,
            _ => Self::Unsupported,
        }
    }
}

pub(super) fn excel_writer_capabilities(source: &ResolvedSource) -> WriterCapabilities {
    if ExcelWorkbookFormat::from_path(source.location.path()) == ExcelWorkbookFormat::Xlsx {
        WriterCapabilities::local_full()
    } else {
        WriterCapabilities::read_only()
    }
}

pub(super) fn ensure_writable_excel_path(
    path: &Path,
    operation: &str,
) -> Result<(), DiagnosticSet> {
    let format = ExcelWorkbookFormat::from_path(path);
    if format == ExcelWorkbookFormat::Xlsx {
        return Ok(());
    }

    let reason = match format {
        ExcelWorkbookFormat::Xlsm => {
            "`.xlsm` is read-only because the Excel writer cannot preserve VBA projects"
        }
        ExcelWorkbookFormat::Xls => {
            "legacy `.xls` is read-only because the Excel writer emits OOXML workbooks"
        }
        ExcelWorkbookFormat::Unsupported => "only `.xlsx` workbooks have a mutation implementation",
        ExcelWorkbookFormat::Xlsx => return Ok(()),
    };
    Err(DiagnosticSet::one(Diagnostic::error(
        "EXCEL-FORMAT-READ-ONLY",
        "EXCEL",
        format!("cannot {operation} `{}`: {reason}", path.display()),
    )))
}
