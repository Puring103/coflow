use coflow_api::{
    CfdInputRecord, CftContainer, Diagnostic, DiagnosticSet, Label, LoadContext, LoadedRecords,
    LoaderSelectionError, OriginMap, ProjectSourceRef, ProviderRegistry, ResolvedSource,
    SourceLocation, SourceResolveContext,
};
use coflow_project::{Project, SourceConfig, SourceLocationSpec};
use serde_json::Value;
use std::path::Path;
use std::sync::Arc;

type ResolvedLoaderSource = (Arc<dyn coflow_api::DataLoader>, ResolvedSource);

#[derive(Debug, Clone, PartialEq)]
pub struct ProjectLoadOutput {
    pub model: coflow_api::CfdDataModel,
}

pub fn load_project_data(
    project: &Project,
    schema: &CftContainer,
    registry: &ProviderRegistry,
) -> Result<ProjectLoadOutput, DiagnosticSet> {
    let mut records = Vec::new();
    let mut origins = OriginMap::default();
    let mut diagnostics = DiagnosticSet::empty();

    for source in &project.config.sources {
        let configured = configured_source(project, source);
        let resolved_sources = match resolve_sources(project, schema, registry, source, &configured)
        {
            Ok(resolved_sources) => resolved_sources,
            Err(err) => {
                diagnostics.extend(err);
                continue;
            }
        };

        let mut source_diagnostics = DiagnosticSet::empty();
        for (loader, spec) in &resolved_sources {
            source_diagnostics.extend(loader.preflight(
                LoadContext {
                    project_root: &project.root_dir,
                    schema,
                },
                spec,
            ));
        }
        if !source_diagnostics.is_empty() {
            diagnostics.extend(source_diagnostics);
            continue;
        }
        for (loader, spec) in resolved_sources {
            match loader.load(
                LoadContext {
                    project_root: &project.root_dir,
                    schema,
                },
                &spec,
            ) {
                Ok(batch) => push_loaded_records(&mut records, &mut origins, batch),
                Err(err) => diagnostics.extend(err),
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
        .map_err(|err| origins.map_diagnostics(err))?;
    if let Err(checks) = coflow_checker::run_checks(schema, &model) {
        return Err(origins.map_diagnostics(checks));
    }
    Ok(ProjectLoadOutput { model })
}

fn resolve_sources(
    project: &Project,
    schema: &CftContainer,
    registry: &ProviderRegistry,
    source: &SourceConfig,
    configured: &ResolvedSource,
) -> Result<Vec<ResolvedLoaderSource>, DiagnosticSet> {
    let ctx = SourceResolveContext {
        project_root: &project.root_dir,
        schema,
    };
    if source.source_type.is_none()
        && matches!(configured.location, SourceLocationSpec::Path(ref path) if path.is_dir())
    {
        let mut resolved = Vec::new();
        for loader in registry.loaders() {
            for source in loader.resolve(ctx, configured)? {
                resolved.push((Arc::clone(&loader), source));
            }
        }
        return Ok(resolved);
    }

    let option_keys = source_option_keys(&configured.options);
    let source_ref = source_ref(configured, source.source_type.as_deref(), &option_keys);
    let loader = match registry.select_loader(&source_ref) {
        Ok(loader) => loader,
        Err(err) => {
            let mut diagnostics = DiagnosticSet::empty();
            diagnostics.push(loader_selection_diagnostic(
                &project.config_path,
                configured,
                err,
            ));
            return Err(diagnostics);
        }
    };
    Ok(loader
        .resolve(ctx, configured)?
        .into_iter()
        .map(|source| (Arc::clone(&loader), source))
        .collect())
}

const fn source_ref<'a>(
    source: &'a ResolvedSource,
    source_type: Option<&'a str>,
    option_keys: &'a [&'a str],
) -> ProjectSourceRef<'a> {
    ProjectSourceRef {
        source_type,
        location: &source.location,
        option_keys,
    }
}

fn push_loaded_records(
    records: &mut Vec<CfdInputRecord>,
    origins: &mut OriginMap,
    loaded: LoadedRecords,
) {
    records.extend(loaded.records);
    origins.extend(loaded.origins);
}

fn configured_source(project: &Project, source: &SourceConfig) -> ResolvedSource {
    let location = match source.location() {
        SourceLocationSpec::Path(path) => SourceLocationSpec::Path(project.resolve_path(path)),
        SourceLocationSpec::Uri(uri) => SourceLocationSpec::Uri(uri.clone()),
    };
    let display_name = match source.location() {
        SourceLocationSpec::Path(path) => path.display().to_string(),
        SourceLocationSpec::Uri(uri) => uri.clone(),
    };
    ResolvedSource {
        provider_id: source.source_type.clone().unwrap_or_default(),
        location,
        options: source.options().clone(),
        display_name,
    }
}

fn source_option_keys(options: &Value) -> Vec<&str> {
    options
        .as_object()
        .map(|object| object.keys().map(String::as_str).collect())
        .unwrap_or_default()
}

fn loader_selection_diagnostic(
    config_path: &Path,
    spec: &ResolvedSource,
    err: LoaderSelectionError,
) -> Diagnostic {
    let source = match &spec.location {
        SourceLocationSpec::Path(path) => path.display().to_string(),
        SourceLocationSpec::Uri(uri) => uri.clone(),
    };
    match err {
        LoaderSelectionError::UnknownLoader { id } => project_diagnostic(
            config_path,
            format!("source `{source}` uses unknown loader `{id}`"),
        ),
        LoaderSelectionError::NoLoader => project_diagnostic(
            config_path,
            format!("source `{source}` has no matching loader"),
        ),
        LoaderSelectionError::AmbiguousLoaders { ids } => project_diagnostic(
            config_path,
            format!(
                "source `{source}` matches multiple loaders {}; set source `type` explicitly",
                ids.join(", ")
            ),
        ),
    }
}

fn project_diagnostic(config_path: &Path, message: impl Into<String>) -> Diagnostic {
    Diagnostic {
        code: "PROJECT-001".to_string(),
        stage: "PROJECT".to_string(),
        severity: coflow_api::Severity::Error,
        message: message.into(),
        primary: Some(Label {
            location: SourceLocation::ProjectConfig {
                path: config_path.to_path_buf(),
                key_path: Vec::new(),
            },
            message: None,
        }),
        related: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use coflow_api::ProviderRegistry;
    use coflow_cft::ModuleId;
    use coflow_data_model::CfdValue;
    use coflow_loader_lark::{LarkHttpClient, LarkSheetLoader};
    use coflow_project::{
        OutputsConfig, ProjectConfig, SchemaConfig, SourceConfig, SourceLocationSpec,
    };
    use serde_json::Value;
    use std::collections::VecDeque;
    use std::path::PathBuf;
    use std::sync::Mutex;

    #[test]
    fn loads_remote_sheet_source_through_registry_pipeline() -> Result<(), String> {
        let root = std::env::temp_dir().join("coflow-pipeline-lark-unit");
        let project = Project {
            config_path: root.join("coflow.yaml"),
            root_dir: root,
            config: ProjectConfig {
                schema: SchemaConfig::One(PathBuf::from("schema")),
                sources: vec![SourceConfig {
                    source_type: Some("lark-sheet".to_string()),
                    location: SourceLocationSpec::Uri(
                        "https://fand3tbr90g.feishu.cn/wiki/wiki_token".to_string(),
                    ),
                    options: serde_json::json!({
                        "app_id": "cli_test",
                        "app_secret": "secret_test",
                        "sheets": [
                            {
                                "sheet": "物品表",
                                "type": "Item",
                                "key": "物品ID",
                                "columns": {
                                    "名称": "name",
                                    "稀有度": "rarity"
                                }
                            }
                        ]
                    }),
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
