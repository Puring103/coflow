use coflow_api::SourceLocationSpec;
use coflow_cfd::parse_cfd;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use crate::{
    byte_range, cfd, field_location, is_cfd_path, uri::path_to_file_uri, LspBuild, OpenDocument,
};
use coflow_project::{normalize_path, Project};

/// Find the LSP location (uri + range) of a CFT type definition by name.
pub(crate) fn cft_type_definition_location(build: &LspBuild, type_name: &str) -> Option<Value> {
    use coflow_cft::parser::parse_module;
    use coflow_cft::ModuleId;

    for (module_id, document) in &build.documents {
        let Some(ast) = document
            .ast
            .clone()
            .or_else(|| parse_module(&ModuleId::new(module_id.clone()), &document.source).ok())
        else {
            continue;
        };

        for item in &ast.items {
            use coflow_cft::ast::Item;
            let (name, name_span) = match item {
                Item::Type(t) => (t.name.as_str(), t.name_span),
                Item::Enum(e) => (e.name.as_str(), e.name_span),
                Item::Const(_) => continue,
            };
            if name == type_name {
                let range = byte_range(&document.source, name_span.start, name_span.end);
                return Some(json!({
                    "uri": document.uri,
                    "range": range,
                }));
            }
        }
    }
    None
}

/// Find the LSP location of a CFT field definition by owning type and field name.
pub(crate) fn cft_schema_field_definition_location(
    build: &LspBuild,
    type_name: &str,
    field_name: &str,
) -> Option<Value> {
    field_location(build, type_name, field_name)
}

/// Find the LSP location (uri + range) of a CFD record definition by key.
///
/// Searches configured CFD source files and open CFD documents for a top-level
/// record whose key matches. Open documents override disk content for the same
/// path so dirty editor buffers can still be targeted.
pub(crate) fn cfd_record_definition_location(
    project: &Project,
    open_documents: &BTreeMap<PathBuf, OpenDocument>,
    key: &str,
) -> Option<Value> {
    for source in cfd_project_sources(project, open_documents) {
        if let Some(location) =
            cfd_record_definition_location_in_source(&source.uri, &source.text, key)
        {
            return Some(location);
        }
    }
    None
}

fn cfd_record_definition_location_in_source(uri: &str, text: &str, key: &str) -> Option<Value> {
    let (ast, _) = parse_cfd(text);
    for record in &ast.records {
        if record.key == key {
            let range = cfd::byte_range(text, record.key_span.start, record.key_span.end);
            return Some(json!({
                "uri": uri,
                "range": range,
            }));
        }
    }
    None
}

struct CfdProjectSource {
    path: PathBuf,
    uri: String,
    text: String,
}

fn cfd_project_sources(
    project: &Project,
    open_documents: &BTreeMap<PathBuf, OpenDocument>,
) -> Vec<CfdProjectSource> {
    let mut sources = Vec::new();
    for source in &project.config.sources {
        let SourceLocationSpec::Path(path) = source.location() else {
            continue;
        };
        let resolved = project.resolve_path(path);
        if resolved.is_dir() {
            sources.extend(cfd_sources_in_dir(&resolved));
        } else if is_cfd_path(&resolved) {
            if let Some(source) = cfd_source_from_path(&resolved) {
                sources.push(source);
            }
        }
    }
    let mut project_paths = sources
        .iter()
        .map(|source| source.path.clone())
        .collect::<BTreeSet<_>>();
    for source in &mut sources {
        if let Some(document) = open_documents.get(&source.path) {
            source.uri.clone_from(&document.uri);
            source.text.clone_from(&document.text);
        }
    }
    for (path, document) in open_documents {
        if is_cfd_path(path) && project_paths.insert(path.clone()) {
            sources.push(CfdProjectSource {
                path: path.clone(),
                uri: document.uri.clone(),
                text: document.text.clone(),
            });
        }
    }
    sources.sort_by(|left, right| left.path.cmp(&right.path));
    sources
}

fn cfd_sources_in_dir(dir: &Path) -> Vec<CfdProjectSource> {
    let mut sources = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return sources;
    };
    let mut entries = entries.filter_map(Result::ok).collect::<Vec<_>>();
    entries.sort_by_key(std::fs::DirEntry::path);
    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            sources.extend(cfd_sources_in_dir(&path));
        } else if is_cfd_path(&path) {
            if let Some(source) = cfd_source_from_path(&path) {
                sources.push(source);
            }
        }
    }
    sources
}

fn cfd_source_from_path(path: &Path) -> Option<CfdProjectSource> {
    let text = std::fs::read_to_string(path).ok()?;
    let normalized = normalize_path(path);
    Some(CfdProjectSource {
        uri: path_to_file_uri(&normalized),
        path: normalized,
        text,
    })
}
