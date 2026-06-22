//! Stage-bucketed diagnostics + converters from upstream diagnostic types
//! (project-config, schema-compile, data-model/check) into the editor's
//! flat `DiagnosticItem` wire shape.
use crate::types::DiagnosticItem;
use coflow_data_model::CfdDataModel;
use std::collections::HashMap;
use std::fmt::Write;
use std::path::Path;

use super::path::path_to_slash;

/// Per-stage diagnostics bucket. Build/load diagnostics are stable until
/// the project is rebuilt; check diagnostics are replaced on every successful
/// write so users see the consequence of their edit without waiting for a
/// full reload.
#[derive(Debug, Default, Clone)]
pub struct Diagnostics {
    pub schema: Vec<DiagnosticItem>,
    pub load: Vec<DiagnosticItem>,
    pub check: Vec<DiagnosticItem>,
}

impl Diagnostics {
    pub fn flatten(&self) -> Vec<DiagnosticItem> {
        let mut out = Vec::with_capacity(self.schema.len() + self.load.len() + self.check.len());
        out.extend(self.schema.iter().cloned());
        out.extend(self.load.iter().cloned());
        out.extend(self.check.iter().cloned());
        out
    }
}

pub(super) fn loader_register_diagnostic(
    err: &coflow_api::ProviderRegistrationError,
) -> DiagnosticItem {
    DiagnosticItem {
        severity: "warning".to_string(),
        code: "REGISTRY".to_string(),
        stage: "PROJECT".to_string(),
        message: format!("provider registration: {err}"),
        file_path: None,
        record_key: None,
        field_path: None,
    }
}

pub(super) fn diagnostic_from_api(d: &coflow_api::Diagnostic) -> DiagnosticItem {
    use coflow_api::{Severity, SourceLocation};
    let severity = match d.severity {
        Severity::Error => "error",
        Severity::Warning => "warning",
        Severity::Info => "info",
    }
    .to_string();
    let file_path = d.primary.as_ref().map(|label| match &label.location {
        SourceLocation::FileSpan { path, .. }
        | SourceLocation::TableCell { path, .. }
        | SourceLocation::ProjectConfig { path, .. }
        | SourceLocation::Artifact { path } => path_to_slash(path),
        SourceLocation::RemoteCell { document, .. } => document.clone(),
    });
    DiagnosticItem {
        severity,
        code: d.code.clone(),
        stage: d.stage.clone(),
        message: d.message.clone(),
        file_path,
        record_key: None,
        field_path: None,
    }
}

pub(super) fn diagnostic_from_project(diag: &coflow_project::DiagnosticJson) -> DiagnosticItem {
    let file_path = if diag.path.is_empty() {
        None
    } else {
        Some(path_to_slash(Path::new(&diag.path)))
    };
    let mut message = diag.message.clone();
    let mut loc_parts: Vec<String> = Vec::new();
    if let Some(sheet) = &diag.sheet {
        loc_parts.push(format!("sheet '{sheet}'"));
    }
    if let Some(cell) = &diag.cell {
        loc_parts.push(format!("cell {cell}"));
    }
    if diag.start_line > 0 || diag.start_character > 0 {
        loc_parts.push(format!(
            "line {}:{}",
            diag.start_line + 1,
            diag.start_character + 1
        ));
    }
    if !loc_parts.is_empty() {
        let _ = write!(message, "\n  at {}", loc_parts.join(", "));
    }
    for related in &diag.related {
        let lbl = related.label.as_deref().unwrap_or("");
        let mut loc = String::new();
        if !related.path.is_empty() {
            loc.push_str(&path_to_slash(Path::new(&related.path)));
        }
        if let Some(sheet) = &related.sheet {
            if !loc.is_empty() {
                loc.push(' ');
            }
            let _ = write!(loc, "sheet '{sheet}'");
        }
        if let Some(cell) = &related.cell {
            if !loc.is_empty() {
                loc.push(' ');
            }
            let _ = write!(loc, "cell {cell}");
        }
        match (loc.is_empty(), lbl.is_empty()) {
            (true, true) => {}
            (false, true) => {
                let _ = write!(message, "\n  · {loc}");
            }
            (true, false) => {
                let _ = write!(message, "\n  · {lbl}");
            }
            (false, false) => {
                let _ = write!(message, "\n  · {loc}: {lbl}");
            }
        }
    }
    DiagnosticItem {
        severity: diag.severity.clone(),
        code: diag.code.clone(),
        stage: diag.stage.clone(),
        message,
        file_path,
        record_key: None,
        field_path: None,
    }
}

pub(super) fn diagnostic_from_cfd(
    diag: &coflow_data_model::CfdDiagnostic,
    model: &CfdDataModel,
    key_to_file: &HashMap<String, String>,
) -> DiagnosticItem {
    let stage = diag.stage.to_string();
    let severity = match diag.severity {
        coflow_data_model::CfdSeverity::Error => "error",
    }
    .to_string();
    let mut record_key: Option<String> = None;
    let mut file_path: Option<String> = None;
    let mut field_path: Option<String> = None;
    if let Some(label) = &diag.primary {
        if let Some(rec_id) = label.record {
            if let Some(record) = model.record(rec_id) {
                record_key = Some(record.key.clone());
                file_path = key_to_file.get(&record.key).cloned();
            }
        }
        if !label.path.segments.is_empty() {
            field_path = Some(format_cfd_path(&label.path));
        }
    }
    let mut message = diag.message.clone();
    if let Some(primary) = &diag.primary {
        let mut loc = String::new();
        if let Some(rec_id) = primary.record {
            if let Some(record) = model.record(rec_id) {
                loc.push_str(&record.key);
            }
        }
        if !primary.path.segments.is_empty() {
            if !loc.is_empty() {
                loc.push('.');
            }
            loc.push_str(&format_cfd_path(&primary.path));
        }
        if !loc.is_empty() {
            let _ = write!(message, "\n  at {loc}");
        }
        if let Some(extra) = &primary.message {
            let _ = write!(message, "\n  → {extra}");
        }
    }
    for related in &diag.related {
        let mut loc = String::new();
        if let Some(rec_id) = related.record {
            if let Some(record) = model.record(rec_id) {
                loc.push_str(&record.key);
            }
        }
        if !related.path.segments.is_empty() {
            if !loc.is_empty() {
                loc.push('.');
            }
            loc.push_str(&format_cfd_path(&related.path));
        }
        let detail = related.message.as_deref().unwrap_or("");
        match (loc.is_empty(), detail.is_empty()) {
            (true, true) => {}
            (false, true) => {
                let _ = write!(message, "\n  · {loc}");
            }
            (true, false) => {
                let _ = write!(message, "\n  · {detail}");
            }
            (false, false) => {
                let _ = write!(message, "\n  · {loc}: {detail}");
            }
        }
    }
    DiagnosticItem {
        severity,
        code: diag.code.as_str().to_string(),
        stage,
        message,
        file_path,
        record_key,
        field_path,
    }
}

pub(super) fn diagnostic_from_cft_schema(diag: &coflow_cft::CftDiagnostic) -> DiagnosticItem {
    let mut message = diag.message.clone();
    if let Some(primary) = &diag.primary {
        let _ = write!(message, "\n  at {}", primary.module.as_str());
        if let Some(extra) = &primary.message {
            let _ = write!(message, "\n  → {extra}");
        }
    }
    for related in &diag.related {
        let detail = related.message.as_deref().unwrap_or("");
        if detail.is_empty() {
            let _ = write!(message, "\n  · {}", related.module.as_str());
        } else {
            let _ = write!(message, "\n  · {}: {}", related.module.as_str(), detail);
        }
    }
    DiagnosticItem {
        severity: "error".to_string(),
        code: diag.code.as_str().to_string(),
        stage: diag.stage.to_string(),
        message,
        file_path: None,
        record_key: None,
        field_path: None,
    }
}

fn format_cfd_path(path: &coflow_data_model::CfdPath) -> String {
    let mut out = String::new();
    for seg in &path.segments {
        match seg {
            coflow_data_model::CfdPathSegment::Field(name) => {
                if !out.is_empty() {
                    out.push('.');
                }
                out.push_str(name);
            }
            coflow_data_model::CfdPathSegment::Index(i) => {
                let _ = write!(out, "[{i}]");
            }
            coflow_data_model::CfdPathSegment::DictKey(k) => {
                let _ = write!(out, "[{k}]");
            }
        }
    }
    out
}
