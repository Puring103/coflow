use coflow_cft::CftContainer;
use coflow_data_model::{CfdDataModel, CfdDiagnostic, CfdDiagnostics, CfdInputRecord, CfdLabel};
use coflow_loader_cfd::parse_cfd_input_records;
use coflow_loader_excel::{
    collect_input_records, ExcelDiagnostic, ExcelDiagnostics, ExcelInputRecords, ExcelLabel,
    ExcelLocation, ExcelOrigins, ExcelSheet, ExcelSource,
};
use coflow_project::{DiagnosticJson, Project, RelatedJson, SourceConfig};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq)]
pub struct ProjectLoadOutput {
    pub model: CfdDataModel,
}

pub fn load_project_data(
    project: &Project,
    schema: &CftContainer,
) -> Result<ProjectLoadOutput, Vec<DiagnosticJson>> {
    let mut records = Vec::new();
    let mut origins = ProjectOrigins::default();
    let mut diagnostics = Vec::new();

    for source in &project.config.sources {
        let source_files = match discover_source_files(project, source) {
            Ok(files) => files,
            Err(message) => {
                diagnostics.push(DiagnosticJson::project(message));
                continue;
            }
        };

        for file in source_files {
            match source_kind(&file) {
                Some(SourceKind::Excel) => {
                    let excel_source = ExcelSource::new(file.clone(), excel_sheets(source));
                    match collect_input_records(schema, &[excel_source]) {
                        Ok(loaded) => push_excel_records(&mut records, &mut origins, loaded),
                        Err(err) => diagnostics.extend(diagnostics_from_excel_checks(&err)),
                    }
                }
                Some(SourceKind::Cfd) => {
                    if source.file.as_ref().is_some_and(|configured| {
                        project.resolve_path(configured).is_file() && !source.sheets.is_empty()
                    }) {
                        diagnostics.push(DiagnosticJson::project(format!(
                            "CFD source `{}` cannot define `sheets`",
                            project_relative_display(project, &file)
                        )));
                        continue;
                    }
                    match load_cfd_records(schema, &file) {
                        Ok(loaded) => push_cfd_records(&mut records, &mut origins, loaded),
                        Err(err) => diagnostics.extend(err),
                    }
                }
                None => {}
            }
        }
    }

    if !diagnostics.is_empty() {
        return Err(diagnostics);
    }

    let mut builder = CfdDataModel::builder(schema);
    for record in records {
        builder.add_input_record(record);
    }
    let model = builder
        .build()
        .map_err(|err| origins.map_diagnostics(err))?;
    if let Err(checks) = coflow_checker::run_checks(schema, &model) {
        return Err(origins.map_diagnostics(checks));
    }
    Ok(ProjectLoadOutput { model })
}

fn push_excel_records(
    records: &mut Vec<CfdInputRecord>,
    origins: &mut ProjectOrigins,
    loaded: ExcelInputRecords,
) {
    let start = records.len();
    records.extend(loaded.records);
    origins.push_excel(start, loaded.origins);
}

fn push_cfd_records(
    records: &mut Vec<CfdInputRecord>,
    origins: &mut ProjectOrigins,
    loaded: CfdInputRecords,
) {
    let start = records.len();
    let count = loaded.records.len();
    records.extend(loaded.records);
    origins.push_cfd(start, count, loaded.file);
}

#[derive(Debug)]
struct CfdInputRecords {
    file: PathBuf,
    records: Vec<CfdInputRecord>,
}

fn load_cfd_records(
    schema: &CftContainer,
    file: &Path,
) -> Result<CfdInputRecords, Vec<DiagnosticJson>> {
    let source = fs::read_to_string(file).map_err(|err| {
        vec![DiagnosticJson::project(format!(
            "failed to read CFD source `{}`: {err}",
            file.display()
        ))]
    })?;
    let records = parse_cfd_input_records(schema, &source)
        .map_err(|err| cfd_text_diagnostics(file, &source, err))?;
    Ok(CfdInputRecords {
        file: file.to_path_buf(),
        records,
    })
}

fn cfd_text_diagnostics(
    file: &Path,
    source: &str,
    err: coflow_loader_cfd::CfdTextLoadError,
) -> Vec<DiagnosticJson> {
    match err {
        coflow_loader_cfd::CfdTextLoadError::Text(diagnostics) => diagnostics
            .diagnostics
            .iter()
            .map(|diagnostic| cfd_text_diagnostic(file, source, diagnostic))
            .collect(),
        coflow_loader_cfd::CfdTextLoadError::DataModel(diagnostics) => {
            plain_cfd_diagnostics(file, diagnostics)
        }
    }
}

fn cfd_text_diagnostic(
    file: &Path,
    source: &str,
    diagnostic: &coflow_loader_cfd::CfdTextDiagnostic,
) -> DiagnosticJson {
    let start = byte_position(source, diagnostic.span.start);
    let end = byte_position(source, diagnostic.span.end.max(diagnostic.span.start + 1));
    DiagnosticJson {
        code: format!("CFD-TEXT-{:?}", diagnostic.code),
        stage: "CFD".to_string(),
        severity: "error".to_string(),
        message: diagnostic.message.clone(),
        path: file.display().to_string(),
        sheet: None,
        cell: None,
        start_line: start.line,
        start_character: start.character,
        end_line: end.line,
        end_character: end.character,
        related: Vec::new(),
    }
}

fn plain_cfd_diagnostics(file: &Path, diagnostics: CfdDiagnostics) -> Vec<DiagnosticJson> {
    diagnostics
        .diagnostics
        .into_iter()
        .map(|diagnostic| DiagnosticJson {
            code: diagnostic.code.as_str().to_string(),
            stage: diagnostic.stage.to_string(),
            severity: "error".to_string(),
            message: diagnostic.message,
            path: file.display().to_string(),
            sheet: None,
            cell: None,
            start_line: 0,
            start_character: 0,
            end_line: 0,
            end_character: 1,
            related: Vec::new(),
        })
        .collect()
}

fn discover_source_files(project: &Project, source: &SourceConfig) -> Result<Vec<PathBuf>, String> {
    let path = source
        .file
        .as_ref()
        .or(source.dir.as_ref())
        .ok_or_else(|| "source must set exactly one of `file` or `dir`".to_string())?;
    let resolved = project.resolve_path(path);
    if resolved.is_dir() {
        collect_data_files(&resolved)
    } else if source_kind(&resolved).is_none() {
        Err(format!(
            "source file `{}` has unsupported extension",
            project_relative_display(project, &resolved)
        ))
    } else {
        Ok(vec![resolved])
    }
}

fn collect_data_files(dir: &Path) -> Result<Vec<PathBuf>, String> {
    let mut entries = fs::read_dir(dir)
        .map_err(|err| format!("failed to read data source dir `{}`: {err}", dir.display()))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| format!("failed to read data source dir `{}`: {err}", dir.display()))?;
    entries.sort_by_key(fs::DirEntry::path);

    let mut files = Vec::new();
    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            files.extend(collect_data_files(&path)?);
        } else if source_kind(&path).is_some() {
            files.push(path);
        }
    }
    Ok(files)
}

fn excel_sheets(source: &SourceConfig) -> Vec<ExcelSheet> {
    source
        .sheets
        .iter()
        .map(|sheet| {
            let mut out = ExcelSheet::new(sheet.sheet.clone());
            if let Some(type_name) = &sheet.type_name {
                out = out.with_type(type_name.clone());
            }
            if !sheet.columns.is_empty() {
                out = out.with_columns(sheet.columns.clone());
            }
            out
        })
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SourceKind {
    Excel,
    Cfd,
}

fn source_kind(path: &Path) -> Option<SourceKind> {
    match path.extension().and_then(|ext| ext.to_str()) {
        Some("xlsx" | "xlsm" | "xls") => Some(SourceKind::Excel),
        Some("cfd") => Some(SourceKind::Cfd),
        _ => None,
    }
}

#[derive(Debug, Default)]
struct ProjectOrigins {
    segments: Vec<ProjectOriginSegment>,
}

impl ProjectOrigins {
    fn push_excel(&mut self, start: usize, origins: ExcelOrigins) {
        let count = origins.record_count();
        self.segments.push(ProjectOriginSegment::Excel {
            start,
            end: start + count,
            origins,
        });
    }

    fn push_cfd(&mut self, start: usize, count: usize, file: PathBuf) {
        self.segments.push(ProjectOriginSegment::Cfd {
            start,
            end: start + count,
            file,
        });
    }

    fn map_diagnostics(&self, diagnostics: CfdDiagnostics) -> Vec<DiagnosticJson> {
        diagnostics
            .diagnostics
            .into_iter()
            .map(|diagnostic| self.map_diagnostic(diagnostic))
            .collect()
    }

    fn map_diagnostic(&self, diagnostic: CfdDiagnostic) -> DiagnosticJson {
        let primary = diagnostic
            .primary
            .as_ref()
            .and_then(|label| self.map_label(label));
        DiagnosticJson {
            code: diagnostic.code.as_str().to_string(),
            stage: diagnostic.stage.to_string(),
            severity: "error".to_string(),
            message: diagnostic.message,
            path: primary
                .as_ref()
                .map_or_else(String::new, |location| location.path.clone()),
            sheet: primary.as_ref().and_then(|location| location.sheet.clone()),
            cell: primary.as_ref().and_then(|location| location.cell.clone()),
            start_line: primary.as_ref().map_or(0, |location| location.line),
            start_character: primary.as_ref().map_or(0, |location| location.character),
            end_line: primary.as_ref().map_or(0, |location| location.line),
            end_character: primary
                .as_ref()
                .map_or(1, |location| location.character.saturating_add(1)),
            related: diagnostic
                .related
                .iter()
                .filter_map(|label| self.map_related(label))
                .collect(),
        }
    }

    fn map_related(&self, label: &CfdLabel) -> Option<RelatedJson> {
        let mapped = self.map_label(label)?;
        Some(RelatedJson {
            path: mapped.path,
            sheet: mapped.sheet,
            cell: mapped.cell,
            start_line: mapped.line,
            start_character: mapped.character,
            end_line: mapped.line,
            end_character: mapped.character.saturating_add(1),
            label: mapped.message,
        })
    }

    fn map_label(&self, label: &CfdLabel) -> Option<MappedLabel> {
        let record = label.record?;
        let index = record.index();
        self.segments.iter().find_map(|segment| match segment {
            ProjectOriginSegment::Excel {
                start,
                end,
                origins,
            } if (*start..*end).contains(&index) => {
                let excel = origins.map_label_with_record_offset(label, *start)?;
                Some(mapped_excel_label(excel))
            }
            ProjectOriginSegment::Cfd { start, end, file } if (*start..*end).contains(&index) => {
                Some(MappedLabel {
                    path: file.display().to_string(),
                    sheet: None,
                    cell: None,
                    line: 0,
                    character: 0,
                    message: label.message.clone(),
                })
            }
            _ => None,
        })
    }
}

#[derive(Debug)]
enum ProjectOriginSegment {
    Excel {
        start: usize,
        end: usize,
        origins: ExcelOrigins,
    },
    Cfd {
        start: usize,
        end: usize,
        file: PathBuf,
    },
}

#[derive(Debug)]
struct MappedLabel {
    path: String,
    sheet: Option<String>,
    cell: Option<String>,
    line: usize,
    character: usize,
    message: Option<String>,
}

fn mapped_excel_label(label: ExcelLabel) -> MappedLabel {
    let (line, character) = excel_position(&label.location);
    let cell = excel_cell(&label.location);
    MappedLabel {
        path: label.location.file.display().to_string(),
        sheet: label.location.sheet,
        cell,
        line,
        character,
        message: label.message,
    }
}

fn diagnostics_from_excel_checks(checks: &ExcelDiagnostics) -> Vec<DiagnosticJson> {
    checks
        .diagnostics
        .iter()
        .map(excel_diagnostic_json)
        .collect()
}

fn excel_diagnostic_json(diagnostic: &ExcelDiagnostic) -> DiagnosticJson {
    let fallback = ExcelLocation::new("");
    let location = diagnostic
        .primary
        .as_ref()
        .map_or(&fallback, |label| &label.location);
    let (line, character) = excel_position(location);
    DiagnosticJson {
        code: diagnostic.code.clone(),
        stage: diagnostic.stage.clone(),
        severity: "error".to_string(),
        message: diagnostic.message.clone(),
        path: location.file.display().to_string(),
        sheet: location.sheet.clone(),
        cell: excel_cell(location),
        start_line: line,
        start_character: character,
        end_line: line,
        end_character: character.saturating_add(1),
        related: diagnostic
            .related
            .iter()
            .map(|label| excel_related_json(&label.location, label.message.clone()))
            .collect(),
    }
}

fn excel_related_json(location: &ExcelLocation, label: Option<String>) -> RelatedJson {
    let (line, character) = excel_position(location);
    RelatedJson {
        path: location.file.display().to_string(),
        sheet: location.sheet.clone(),
        cell: excel_cell(location),
        start_line: line,
        start_character: character,
        end_line: line,
        end_character: character.saturating_add(1),
        label,
    }
}

fn excel_position(location: &ExcelLocation) -> (usize, usize) {
    (
        location.row.unwrap_or(1).saturating_sub(1),
        location.column.unwrap_or(1).saturating_sub(1),
    )
}

fn excel_cell(location: &ExcelLocation) -> Option<String> {
    Some(format!(
        "{}{}",
        excel_column_name(location.column?),
        location.row?
    ))
}

fn excel_column_name(column: usize) -> String {
    let mut value = column;
    let mut name = Vec::new();
    while value > 0 {
        value -= 1;
        #[allow(clippy::cast_possible_truncation)]
        let offset = (value % 26) as u8;
        name.push((b'A' + offset) as char);
        value /= 26;
    }
    name.iter().rev().collect()
}

#[derive(Debug, Clone, Copy)]
struct Position {
    line: usize,
    character: usize,
}

fn byte_position(source: &str, byte_offset: usize) -> Position {
    let target = byte_offset.min(source.len());
    let mut line = 0;
    let mut character = 0;
    for (byte_index, ch) in source.char_indices() {
        if byte_index >= target {
            break;
        }
        if ch == '\n' {
            line += 1;
            character = 0;
        } else {
            character += ch.len_utf16();
        }
    }
    Position { line, character }
}

fn project_relative_display(project: &Project, path: &Path) -> String {
    path.strip_prefix(&project.root_dir)
        .unwrap_or(path)
        .display()
        .to_string()
        .replace('\\', "/")
}
