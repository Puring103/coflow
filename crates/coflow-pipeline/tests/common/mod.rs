#![allow(dead_code, unused_imports)]

pub use coflow_data_model::CfdErrorCode;
pub use coflow_pipeline::{BuildOptions, CodegenOptions, ExportOptions, PipelineOutcome};
pub use coflow_project::{
    OutputConfig, OutputsConfig, Project, ProjectConfig, SchemaConfig, SheetConfig, SourceConfig,
};
pub use rust_xlsxwriter::Workbook;
pub use std::collections::BTreeMap;
use std::fmt::Write as _;
pub use std::path::{Path, PathBuf};

pub fn test_registry() -> coflow_api::ProviderRegistry {
    let mut registry = coflow_api::ProviderRegistry::default();
    registry
        .register_loader(coflow_loader_excel::ExcelLoader)
        .expect("register excel loader");
    registry
        .register_loader(coflow_loader_lark::LarkSheetLoader::default())
        .expect("register lark loader");
    registry
        .register_loader(coflow_loader_cfd::CfdLoader)
        .expect("register cfd loader");
    registry
        .register_exporter(coflow_exporter_json::JsonExporter)
        .expect("register json exporter");
    registry
        .register_exporter(coflow_exporter_messagepack::MessagePackExporter)
        .expect("register messagepack exporter");
    registry
        .register_codegen(coflow_codegen_csharp::CsharpCodeGenerator)
        .expect("register csharp codegen");
    registry
}

pub fn check_project(
    project: &Project,
) -> Result<PipelineOutcome<coflow_pipeline::CheckReport>, String> {
    coflow_pipeline::check_project(project, &test_registry())
}

pub fn build_project(
    project: &Project,
    options: BuildOptions<'_>,
) -> Result<PipelineOutcome<coflow_pipeline::BuildReport>, String> {
    coflow_pipeline::build_project(project, &test_registry(), options)
}

pub fn export_project_data(
    project: &Project,
    exporter_id: &str,
    options: ExportOptions<'_>,
) -> Result<PipelineOutcome<coflow_pipeline::ExportReport>, String> {
    coflow_pipeline::export_project_data(project, &test_registry(), exporter_id, options)
}

pub fn generate_project_code(
    project: &Project,
    codegen_id: &str,
    options: CodegenOptions<'_>,
) -> Result<PipelineOutcome<coflow_pipeline::CodegenReport>, String> {
    coflow_pipeline::generate_project_code(project, &test_registry(), codegen_id, options)
}

pub fn write_project_with_missing_excel_source(root: &Path, include_code_output: bool) {
    std::fs::create_dir_all(root.join("schema")).expect("create schema dir");
    std::fs::write(
        root.join("schema").join("main.cft"),
        "type Item { value: int; }\n",
    )
    .expect("write schema");
    let code_output = if include_code_output {
        "  code:\n    type: csharp\n    dir: generated/csharp\n    namespace: Game.Config\n"
    } else {
        ""
    };
    std::fs::write(
        root.join("coflow.yaml"),
        format!(
            r"schema: schema/
sources:
  - file: data/missing.xlsx
    sheets:
      - sheet: Items
        type: Item
        columns:
          A: id
outputs:
  data:
    type: json
    dir: generated/data
{code_output}"
        ),
    )
    .expect("write config");
}

pub fn write_single_item_project(
    root: &Path,
    outputs: OutputsConfig,
) -> Result<(), rust_xlsxwriter::XlsxError> {
    std::fs::create_dir_all(root.join("schema")).expect("create schema dir");
    std::fs::create_dir_all(root.join("data")).expect("create data dir");
    std::fs::write(
        root.join("schema").join("main.cft"),
        "type Item { value: int; }\n",
    )
    .expect("write schema");
    let workbook_path = root.join("data").join("configs.xlsx");
    let mut workbook = Workbook::new();
    let sheet = workbook.add_worksheet();
    sheet.set_name("Item")?;
    sheet.write_string(0, 0, "id")?;
    sheet.write_string(0, 1, "value")?;
    sheet.write_string(1, 0, "item_1")?;
    sheet.write_number(1, 1, 1.0)?;
    workbook.save(&workbook_path)?;

    let mut config = String::from(
        r"schema: schema/
sources:
  - file: data/configs.xlsx
    sheets:
      - sheet: Item
        columns:
          id: id
          value: value
outputs:
",
    );
    if let Some(data) = outputs.data {
        write!(
            config,
            "  data:\n    type: {}\n    dir: {}\n",
            data.output_type,
            data.dir.display()
        )
        .expect("append data output config");
    }
    if let Some(code) = outputs.code {
        write!(
            config,
            "  code:\n    type: {}\n    dir: {}\n",
            code.output_type,
            code.dir.display()
        )
        .expect("append code output config");
        if let Some(namespace) = code.namespace {
            writeln!(config, "    namespace: {namespace}").expect("append namespace config");
        }
    }
    std::fs::write(root.join("coflow.yaml"), config).expect("write config");
    Ok(())
}

pub fn write_invalid_check_project(root: &Path) -> Result<(), rust_xlsxwriter::XlsxError> {
    std::fs::create_dir_all(root.join("schema")).expect("create schema dir");
    std::fs::create_dir_all(root.join("data")).expect("create data dir");
    std::fs::write(
        root.join("schema").join("main.cft"),
        r#"
            type Item {
                level: int;
                check { level > 0; }
            }
        "#,
    )
    .expect("write schema");
    let workbook_path = root.join("data").join("configs.xlsx");
    let mut workbook = Workbook::new();
    let sheet = workbook.add_worksheet();
    sheet.set_name("Item")?;
    sheet.write_string(0, 0, "id")?;
    sheet.write_string(0, 1, "level")?;
    sheet.write_string(1, 0, "item_1")?;
    sheet.write_number(1, 1, 0.0)?;
    workbook.save(&workbook_path)?;
    std::fs::write(
        root.join("coflow.yaml"),
        r"schema: schema/
sources:
  - file: data/configs.xlsx
    sheets:
      - sheet: Item
        columns:
          id: id
          level: level
outputs:
  data:
    type: json
    dir: generated/data
",
    )
    .expect("write config");
    Ok(())
}

pub fn schema_only_project_with_outputs(
    name: &str,
    outputs: OutputsConfig,
) -> (Project, TempDirCleanup) {
    let root = temp_project_dir(name);
    std::fs::create_dir_all(root.join("schema")).expect("create schema dir");
    std::fs::write(
        root.join("schema").join("main.cft"),
        r#"
            @keyAsEnum("GeneId")
            type GeneConfig {}
        "#,
    )
    .expect("write schema");
    let project = Project {
        config_path: root.join("coflow.yaml"),
        root_dir: root.clone(),
        config: ProjectConfig {
            schema: SchemaConfig::One(PathBuf::from("schema")),
            sources: Vec::new(),
            outputs,
        },
    };
    (project, TempDirCleanup(root))
}

pub fn project_with_unvalidated_outputs(
    name: &str,
    outputs: OutputsConfig,
) -> (Project, TempDirCleanup) {
    let root = temp_project_dir(name);
    std::fs::create_dir_all(root.join("schema")).expect("create schema dir");
    std::fs::create_dir_all(root.join("data")).expect("create data dir");
    std::fs::write(root.join("schema").join("main.cft"), "type Item {}\n").expect("write schema");
    let workbook_path = root.join("data").join("configs.xlsx");
    let mut workbook = Workbook::new();
    let sheet = workbook.add_worksheet();
    sheet.set_name("Item").expect("set sheet name");
    sheet.write_string(0, 0, "id").expect("write header");
    sheet.write_string(1, 0, "item_1").expect("write row");
    workbook.save(&workbook_path).expect("save workbook");
    let project = Project {
        config_path: root.join("coflow.yaml"),
        root_dir: root.clone(),
        config: ProjectConfig {
            schema: SchemaConfig::One(PathBuf::from("schema")),
            sources: vec![SourceConfig {
                source_type: None,
                file: Some(PathBuf::from("data/configs.xlsx")),
                dir: None,
                lark_sheet: None,
                options: BTreeMap::new(),
                sheets: vec![SheetConfig {
                    sheet: "Item".to_string(),
                    type_name: None,
                    key: None,
                    columns: BTreeMap::from([("id".to_string(), "id".to_string())]),
                }],
            }],
            outputs,
        },
    };
    (project, TempDirCleanup(root))
}

pub fn output_config(output_type: &str, dir: &str, namespace: Option<&str>) -> OutputConfig {
    OutputConfig {
        output_type: output_type.to_string(),
        dir: PathBuf::from(dir),
        namespace: namespace.map(str::to_string),
        options: BTreeMap::new(),
    }
}

pub fn assert_diagnostic_message_contains<T>(outcome: PipelineOutcome<T>, expected: &str) {
    let PipelineOutcome::Diagnostics(diagnostics) = outcome else {
        panic!("expected diagnostics containing `{expected}`");
    };
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains(expected)),
        "missing `{expected}` in diagnostics: {diagnostics:?}"
    );
}

pub fn write_key_as_enum_project(
    root: &Path,
    gene_ids: &[&str],
) -> Result<(), rust_xlsxwriter::XlsxError> {
    assert!(!gene_ids.is_empty(), "test requires at least one gene id");
    std::fs::create_dir_all(root.join("schema")).expect("create schema dir");
    std::fs::create_dir_all(root.join("data")).expect("create data dir");
    std::fs::write(
        root.join("schema").join("main.cft"),
        r#"
            @keyAsEnum("GeneId")
            type GeneConfig {}
            type BioRemainsConfig {
                gene: GeneConfig?;
            }
        "#,
    )
    .expect("write schema");
    std::fs::write(
        root.join("coflow.yaml"),
        r"schema: schema/
sources:
  - file: data/configs.xlsx
    sheets:
      - sheet: GeneConfig
        columns:
          id: id
      - sheet: BioRemainsConfig
        columns:
          id: id
          gene_id: gene
outputs:
  data:
    type: json
    dir: generated/data
  code:
    type: csharp
    dir: generated/csharp
    namespace: Game.Config
",
    )
    .expect("write config");

    let workbook_path = root.join("data").join("configs.xlsx");
    if workbook_path.exists() {
        std::fs::remove_file(&workbook_path).expect("remove old workbook");
    }

    let mut workbook = Workbook::new();
    let genes = workbook.add_worksheet();
    genes.set_name("GeneConfig")?;
    genes.write_string(0, 0, "id")?;
    for (index, id) in gene_ids.iter().enumerate() {
        let row = u32::try_from(index + 1).expect("test row index fits in u32");
        genes.write_string(row, 0, *id)?;
    }

    let remains = workbook.add_worksheet();
    remains.set_name("BioRemainsConfig")?;
    remains.write_string(0, 0, "id")?;
    remains.write_string(0, 1, "gene_id")?;
    remains.write_string(1, 0, "remains_1")?;
    remains.write_string(1, 1, format!("@GeneConfig.{}", gene_ids[0]))?;

    workbook.save(workbook_path)
}

pub fn write_renamable_key_as_enum_project(
    root: &Path,
    enum_name: &str,
) -> Result<(), rust_xlsxwriter::XlsxError> {
    std::fs::create_dir_all(root.join("schema")).expect("create schema dir");
    std::fs::create_dir_all(root.join("data")).expect("create data dir");
    std::fs::write(
        root.join("schema").join("main.cft"),
        format!(
            r#"
            @keyAsEnum("{enum_name}")
            type GeneConfig {{}}
        "#
        ),
    )
    .expect("write schema");
    std::fs::write(
        root.join("coflow.yaml"),
        r"schema: schema/
sources:
  - file: data/configs.xlsx
    sheets:
      - sheet: GeneConfig
        columns:
          id: id
outputs:
  data:
    type: json
    dir: generated/data
  code:
    type: csharp
    dir: generated/csharp
    namespace: Game.Config
",
    )
    .expect("write config");

    let workbook_path = root.join("data").join("configs.xlsx");
    if workbook_path.exists() {
        std::fs::remove_file(&workbook_path).expect("remove old workbook");
    }
    let mut workbook = Workbook::new();
    let genes = workbook.add_worksheet();
    genes.set_name("GeneConfig")?;
    genes.write_string(0, 0, "id")?;
    genes.write_string(1, 0, "Gene_Spore")?;
    workbook.save(workbook_path)
}

pub fn temp_project_dir(name: &str) -> PathBuf {
    let suffix = format!(
        "{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time")
            .as_nanos()
    );
    let root = std::env::temp_dir().join(format!("{name}-{suffix}"));
    if root.exists() {
        std::fs::remove_dir_all(&root).expect("clean temp dir");
    }
    root
}

pub fn workspace_path(path: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join(path)
}

pub fn write_key_as_enum_workbook(path: &Path) -> Result<(), rust_xlsxwriter::XlsxError> {
    let mut workbook = Workbook::new();
    let genes = workbook.add_worksheet();
    genes.set_name("GeneConfig")?;
    genes.write_string(0, 0, "id")?;
    genes.write_string(1, 0, "Gene_Spore")?;
    genes.write_string(2, 0, "Gene_Mating")?;

    let remains = workbook.add_worksheet();
    remains.set_name("BioRemainsConfig")?;
    remains.write_string(0, 0, "id")?;
    remains.write_string(0, 1, "gene_id")?;
    remains.write_string(1, 0, "remains_1")?;
    remains.write_string(1, 1, "@GeneConfig.Gene_Spore")?;

    workbook.save(path)
}

pub fn write_item_workbook(
    path: &Path,
    linked_stage: Option<&str>,
) -> Result<(), rust_xlsxwriter::XlsxError> {
    let mut workbook = Workbook::new();
    let sheet = workbook.add_worksheet();
    sheet.set_name("Item")?;
    sheet.write_string(0, 0, "id")?;
    sheet.write_string(0, 1, "name")?;
    if linked_stage.is_some() {
        sheet.write_string(0, 2, "linked_stage")?;
    }
    sheet.write_string(1, 0, "potion")?;
    sheet.write_string(1, 1, "Potion")?;
    if let Some(value) = linked_stage {
        sheet.write_string(1, 2, value)?;
    }
    workbook.save(path)
}

pub struct TempDirCleanup(pub PathBuf);

impl Drop for TempDirCleanup {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
