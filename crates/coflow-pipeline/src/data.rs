use coflow_api::{
    CfdInputRecord, CftContainer, Diagnostic, DiagnosticSet, Label, LoadContext, LoadedRecords,
    LoaderSelectionError, OriginMap, ProviderRegistry, SourceLocation, SourceRef, SourceSpec,
};
use coflow_project::{DiagnosticJson, Project, RelatedJson, SourceConfig};
use serde_json::{Map, Value};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq)]
pub struct ProjectLoadOutput {
    pub model: coflow_api::CfdDataModel,
}

pub fn load_project_data(
    project: &Project,
    schema: &CftContainer,
    registry: &ProviderRegistry,
) -> Result<ProjectLoadOutput, Vec<DiagnosticJson>> {
    let mut records = Vec::new();
    let mut origins = OriginMap::default();
    let mut diagnostics = Vec::new();

    for source in &project.config.sources {
        let source_specs = match source_specs(project, registry, source) {
            Ok(specs) => specs,
            Err(message) => {
                diagnostics.push(DiagnosticJson::project(message));
                continue;
            }
        };

        for spec in source_specs {
            let config_keys = source_config_keys(&spec.options);
            let source_ref = SourceRef {
                source_type: spec.source_type.as_deref(),
                path: spec.file.as_deref().or(spec.dir.as_deref()),
                uri: spec.uri.as_deref(),
                config_keys: &config_keys,
            };
            let loader = match registry.select_loader(&source_ref) {
                Ok(loader) => loader,
                Err(err) => {
                    diagnostics.push(loader_selection_diagnostic(&spec, err));
                    continue;
                }
            };
            match loader.load(
                LoadContext {
                    project_root: &project.root_dir,
                    schema,
                },
                &spec,
            ) {
                Ok(batch) => push_loaded_records(&mut records, &mut origins, batch),
                Err(err) => diagnostics.extend(diagnostics_from_provider(err)),
            }
        }
    }

    if !diagnostics.is_empty() {
        return Err(diagnostics);
    }

    let mut builder = coflow_api::CfdDataModel::builder(schema);
    for record in records {
        builder.add_input_record(record);
    }
    let model = builder
        .build()
        .map_err(|err| diagnostics_from_provider(origins.map_diagnostics(err)))?;
    if let Err(checks) = coflow_checker::run_checks(schema, &model) {
        return Err(diagnostics_from_provider(origins.map_diagnostics(checks)));
    }
    Ok(ProjectLoadOutput { model })
}

fn push_loaded_records(
    records: &mut Vec<CfdInputRecord>,
    origins: &mut OriginMap,
    loaded: LoadedRecords,
) {
    records.extend(loaded.records);
    origins.extend(loaded.origins);
}

fn source_specs(
    project: &Project,
    registry: &ProviderRegistry,
    source: &SourceConfig,
) -> Result<Vec<SourceSpec>, String> {
    if source.lark_sheet.is_some() {
        return Ok(vec![lark_source_spec(source)?]);
    }

    let files = discover_source_files(project, registry, source)?;
    files
        .into_iter()
        .map(|file| {
            if file.extension().and_then(|ext| ext.to_str()) == Some("cfd")
                && source.file.as_ref().is_some_and(|configured| {
                    project.resolve_path(configured).is_file() && !source.sheets.is_empty()
                })
            {
                return Err(format!(
                    "CFD source `{}` cannot define `sheets`",
                    project_relative_display(project, &file)
                ));
            }
            Ok(SourceSpec {
                source_type: source.source_type.clone(),
                file: Some(file),
                dir: None,
                uri: None,
                options: sheet_options(source),
            })
        })
        .collect()
}

fn lark_source_spec(source: &SourceConfig) -> Result<SourceSpec, String> {
    let Some(lark_sheet) = &source.lark_sheet else {
        return Err("source must define `lark_sheet`".to_string());
    };
    let mut lark = Map::new();
    lark.insert(
        "app_id".to_string(),
        Value::String(lark_sheet.app_id.clone()),
    );
    lark.insert(
        "app_secret".to_string(),
        Value::String(lark_sheet.app_secret.clone()),
    );
    if let Some(url) = &lark_sheet.url {
        lark.insert("url".to_string(), Value::String(url.clone()));
    }
    if let Some(token) = &lark_sheet.spreadsheet_token {
        lark.insert(
            "spreadsheet_token".to_string(),
            Value::String(token.clone()),
        );
    }

    let mut options = source_options_map(source);
    options.insert("lark_sheet".to_string(), Value::Object(lark));
    options.insert("sheets".to_string(), source_sheets_value(source));
    Ok(SourceSpec {
        source_type: source
            .source_type
            .clone()
            .or_else(|| Some("lark-sheet".to_string())),
        file: None,
        dir: None,
        uri: lark_sheet.url.clone(),
        options: Value::Object(options),
    })
}

fn sheet_options(source: &SourceConfig) -> Value {
    let mut options = source_options_map(source);
    options.insert("sheets".to_string(), source_sheets_value(source));
    Value::Object(options)
}

fn source_options_map(source: &SourceConfig) -> Map<String, Value> {
    source
        .options
        .iter()
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
}

fn source_sheets_value(source: &SourceConfig) -> Value {
    Value::Array(
        source
            .sheets
            .iter()
            .map(|sheet| {
                let mut object = Map::new();
                object.insert("sheet".to_string(), Value::String(sheet.sheet.clone()));
                if let Some(type_name) = &sheet.type_name {
                    object.insert("type".to_string(), Value::String(type_name.clone()));
                }
                if let Some(key) = &sheet.key {
                    object.insert("key".to_string(), Value::String(key.clone()));
                }
                object.insert(
                    "columns".to_string(),
                    Value::Object(
                        sheet
                            .columns
                            .iter()
                            .map(|(source, field)| (source.clone(), Value::String(field.clone())))
                            .collect(),
                    ),
                );
                Value::Object(object)
            })
            .collect(),
    )
}

fn source_config_keys(options: &Value) -> Vec<&str> {
    options
        .as_object()
        .map(|object| object.keys().map(String::as_str).collect())
        .unwrap_or_default()
}

fn discover_source_files(
    project: &Project,
    registry: &ProviderRegistry,
    source: &SourceConfig,
) -> Result<Vec<PathBuf>, String> {
    let path = source
        .file
        .as_ref()
        .or(source.dir.as_ref())
        .ok_or_else(|| "source must set exactly one of `file` or `dir`".to_string())?;
    let resolved = project.resolve_path(path);
    if resolved.is_dir() {
        collect_data_files(registry, &resolved)
    } else if has_registered_extension(registry, &resolved) {
        Ok(vec![resolved])
    } else {
        Err(format!(
            "source file `{}` has unsupported extension",
            project_relative_display(project, &resolved)
        ))
    }
}

fn collect_data_files(registry: &ProviderRegistry, dir: &Path) -> Result<Vec<PathBuf>, String> {
    let mut entries = fs::read_dir(dir)
        .map_err(|err| format!("failed to read data source dir `{}`: {err}", dir.display()))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| format!("failed to read data source dir `{}`: {err}", dir.display()))?;
    entries.sort_by_key(fs::DirEntry::path);

    let mut files = Vec::new();
    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            files.extend(collect_data_files(registry, &path)?);
        } else if has_registered_extension(registry, &path) {
            files.push(path);
        }
    }
    Ok(files)
}

fn has_registered_extension(registry: &ProviderRegistry, path: &Path) -> bool {
    let Some(extension) = path.extension().and_then(|ext| ext.to_str()) else {
        return false;
    };
    registry
        .loader_descriptors()
        .iter()
        .any(|descriptor| descriptor.extensions.contains(&extension))
}

fn loader_selection_diagnostic(spec: &SourceSpec, err: LoaderSelectionError) -> DiagnosticJson {
    let source = spec
        .file
        .as_ref()
        .or(spec.dir.as_ref())
        .map_or_else(String::new, |path| path.display().to_string());
    match err {
        LoaderSelectionError::UnknownLoader { id } => {
            DiagnosticJson::project(format!("source `{source}` uses unknown loader `{id}`"))
        }
        LoaderSelectionError::NoLoader => {
            DiagnosticJson::project(format!("source `{source}` has no matching loader"))
        }
        LoaderSelectionError::AmbiguousLoaders { ids } => DiagnosticJson::project(format!(
            "source `{source}` matches multiple loaders {}; set source `type` explicitly",
            ids.join(", ")
        )),
    }
}

fn diagnostics_from_provider(diagnostics: DiagnosticSet) -> Vec<DiagnosticJson> {
    diagnostics
        .diagnostics
        .into_iter()
        .map(diagnostic_json_from_provider)
        .collect()
}

fn diagnostic_json_from_provider(diagnostic: Diagnostic) -> DiagnosticJson {
    let primary = diagnostic.primary.as_ref().map(label_location);
    DiagnosticJson {
        code: diagnostic.code,
        stage: diagnostic.stage,
        severity: severity_name(diagnostic.severity).to_string(),
        message: diagnostic.message,
        path: primary
            .as_ref()
            .map_or_else(String::new, |location| location.path.clone()),
        sheet: primary.as_ref().and_then(|location| location.sheet.clone()),
        cell: primary.as_ref().and_then(|location| location.cell.clone()),
        start_line: primary.as_ref().map_or(0, |location| location.start_line),
        start_character: primary
            .as_ref()
            .map_or(0, |location| location.start_character),
        end_line: primary.as_ref().map_or(0, |location| location.end_line),
        end_character: primary
            .as_ref()
            .map_or(1, |location| location.end_character),
        related: diagnostic
            .related
            .iter()
            .map(related_json_from_label)
            .collect(),
    }
}

fn related_json_from_label(label: &Label) -> RelatedJson {
    let location = label_location(label);
    RelatedJson {
        path: location.path,
        sheet: location.sheet,
        cell: location.cell,
        start_line: location.start_line,
        start_character: location.start_character,
        end_line: location.end_line,
        end_character: location.end_character,
        label: label.message.clone(),
    }
}

#[derive(Debug)]
struct JsonLocation {
    path: String,
    sheet: Option<String>,
    cell: Option<String>,
    start_line: usize,
    start_character: usize,
    end_line: usize,
    end_character: usize,
}

fn label_location(label: &Label) -> JsonLocation {
    match &label.location {
        SourceLocation::FileSpan {
            path,
            start_line,
            start_character,
            end_line,
            end_character,
        } => JsonLocation {
            path: path.display().to_string(),
            sheet: None,
            cell: None,
            start_line: *start_line,
            start_character: *start_character,
            end_line: *end_line,
            end_character: *end_character,
        },
        SourceLocation::TableCell {
            path,
            sheet,
            row,
            column,
        } => JsonLocation {
            path: path.display().to_string(),
            sheet: sheet.clone(),
            cell: Some(excel_cell(*row, *column)),
            start_line: row.saturating_sub(1),
            start_character: column.saturating_sub(1),
            end_line: row.saturating_sub(1),
            end_character: *column,
        },
        SourceLocation::RemoteCell {
            document,
            sheet,
            row,
            column,
        } => JsonLocation {
            path: document.clone(),
            sheet: sheet.clone(),
            cell: (*row > 0 && *column > 0).then(|| excel_cell(*row, *column)),
            start_line: row.saturating_sub(1),
            start_character: column.saturating_sub(1),
            end_line: row.saturating_sub(1),
            end_character: (*column).max(1),
        },
        SourceLocation::ProjectConfig { path, .. } | SourceLocation::Artifact { path } => {
            JsonLocation {
                path: path.display().to_string(),
                sheet: None,
                cell: None,
                start_line: 0,
                start_character: 0,
                end_line: 0,
                end_character: 1,
            }
        }
    }
}

const fn severity_name(severity: coflow_api::Severity) -> &'static str {
    match severity {
        coflow_api::Severity::Error => "error",
        coflow_api::Severity::Warning => "warning",
        coflow_api::Severity::Info => "info",
    }
}

fn excel_cell(row: usize, column: usize) -> String {
    format!("{}{}", excel_column_name(column), row)
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

fn project_relative_display(project: &Project, path: &Path) -> String {
    path.strip_prefix(&project.root_dir)
        .unwrap_or(path)
        .display()
        .to_string()
        .replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use coflow_api::ProviderRegistry;
    use coflow_cft::ModuleId;
    use coflow_data_model::CfdValue;
    use coflow_loader_lark::{LarkHttpClient, LarkSheetLoader};
    use coflow_project::{
        LarkSheetConfig, OutputsConfig, ProjectConfig, SchemaConfig, SheetConfig, SourceConfig,
    };
    use serde_json::Value;
    use std::collections::{BTreeMap, VecDeque};
    use std::sync::Mutex;

    #[test]
    fn loads_lark_sheet_source_through_registry_pipeline() -> Result<(), String> {
        let root = std::env::temp_dir().join("coflow-pipeline-lark-unit");
        let project = Project {
            config_path: root.join("coflow.yaml"),
            root_dir: root,
            config: ProjectConfig {
                schema: SchemaConfig::One(PathBuf::from("schema")),
                sources: vec![SourceConfig {
                    source_type: None,
                    file: None,
                    dir: None,
                    lark_sheet: Some(LarkSheetConfig {
                        app_id: "cli_test".to_string(),
                        app_secret: "secret_test".to_string(),
                        url: Some("https://fand3tbr90g.feishu.cn/wiki/wiki_token".to_string()),
                        spreadsheet_token: None,
                    }),
                    options: BTreeMap::new(),
                    sheets: vec![SheetConfig {
                        sheet: "物品表".to_string(),
                        type_name: Some("Item".to_string()),
                        key: Some("物品ID".to_string()),
                        columns: BTreeMap::from([
                            ("名称".to_string(), "name".to_string()),
                            ("稀有度".to_string(), "rarity".to_string()),
                        ]),
                    }],
                }],
                outputs: OutputsConfig::default(),
            },
        };
        let schema = compile_schema(
            r"
                enum Rarity { Common = 0, Rare = 10, }
                type Item {
                    name: string;
                    rarity: Rarity = Rarity.Common;
                }
            ",
        )?;
        let client = FakeLarkClient::new([
            Response::post(
                "https://open.feishu.cn/open-apis/auth/v3/tenant_access_token/internal",
                r#"{"code":0,"tenant_access_token":"tenant_token"}"#,
            ),
            Response::get(
                "https://open.feishu.cn/open-apis/wiki/v2/spaces/get_node?token=wiki_token",
                r#"{"code":0,"data":{"node":{"obj_type":"sheet","obj_token":"sht_token"}}}"#,
            ),
            Response::get(
                "https://open.feishu.cn/open-apis/sheets/v3/spreadsheets/sht_token/sheets/query",
                r#"{"code":0,"data":{"sheets":[{"sheet_id":"sheet_a","title":"物品表","grid_properties":{"row_count":2,"column_count":3}}]}}"#,
            ),
            Response::get(
                "https://open.feishu.cn/open-apis/sheets/v2/spreadsheets/sht_token/values/sheet_a%21A1%3AC2?valueRenderOption=ToString",
                r#"{"code":0,"data":{"valueRange":{"values":[["物品ID","名称","稀有度"],["sword_01","铁剑","Rare"]]}}}"#,
            ),
        ]);
        let mut registry = ProviderRegistry::default();
        registry
            .register_loader(LarkSheetLoader::new(client))
            .map_err(|err| err.to_string())?;

        let output = load_project_data(&project, &schema, &registry)
            .map_err(|diagnostics| format!("{diagnostics:?}"))?;

        let item = output
            .model
            .table("Item")
            .and_then(|table| table.primary_index.get("sword_01"))
            .and_then(|record_id| output.model.record(*record_id))
            .ok_or_else(|| "expected sword_01 item".to_string())?;
        if item.field("name") != Some(&CfdValue::String("铁剑".to_string())) {
            return Err(format!("unexpected item name: {:?}", item.field("name")));
        }
        Ok(())
    }

    fn compile_schema(source: &str) -> Result<CftContainer, String> {
        let mut container = CftContainer::new();
        container
            .add_module(ModuleId::from("main"), source)
            .map_err(|err| format!("schema should parse: {err:?}"))?;
        container
            .compile()
            .map_err(|err| format!("schema should compile: {err:?}"))?;
        Ok(container)
    }

    #[derive(Debug, Clone)]
    struct Response {
        method: &'static str,
        url: &'static str,
        body: &'static str,
    }

    impl Response {
        fn get(url: &'static str, body: &'static str) -> Self {
            Self {
                method: "GET",
                url,
                body,
            }
        }

        fn post(url: &'static str, body: &'static str) -> Self {
            Self {
                method: "POST",
                url,
                body,
            }
        }
    }

    #[derive(Debug)]
    struct FakeLarkClient {
        responses: Mutex<VecDeque<Response>>,
    }

    impl FakeLarkClient {
        fn new(responses: impl IntoIterator<Item = Response>) -> Self {
            Self {
                responses: Mutex::new(responses.into_iter().collect()),
            }
        }

        fn next(&self, method: &str, url: &str) -> Result<String, String> {
            let response = self
                .responses
                .lock()
                .map_err(|err| err.to_string())?
                .pop_front()
                .ok_or_else(|| format!("unexpected {method} {url}"))?;
            if response.method != method || response.url != url {
                return Err(format!(
                    "expected {} {}, got {method} {url}",
                    response.method, response.url
                ));
            }
            Ok(response.body.to_string())
        }
    }

    impl LarkHttpClient for FakeLarkClient {
        fn get(&self, url: &str, _tenant_access_token: &str) -> Result<String, String> {
            self.next("GET", url)
        }

        fn post_json(
            &self,
            url: &str,
            _body: &Value,
            _tenant_access_token: Option<&str>,
        ) -> Result<String, String> {
            self.next("POST", url)
        }
    }
}
