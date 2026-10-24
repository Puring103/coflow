use super::{Failure, Result};
use crate::{normalize_path, project_path, Project};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct InputFile {
    pub path: String,
    pub kind: String,
    pub inputs: Vec<String>,
    pub fingerprint: String,
    #[serde(skip)]
    pub absolute: PathBuf,
    #[serde(skip)]
    pub contents: Vec<u8>,
}

#[derive(Debug, Clone, Serialize)]
pub struct InputSnapshot {
    pub files: Vec<InputFile>,
}

// 固定算法与字节编码保证跨进程游标稳定；文件身份与内容均参与快照。
pub fn fingerprint(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

impl InputSnapshot {
    /// 修订绑定输入身份与原始字节，排序采用相对路径，使跨进程结果稳定。
    #[must_use]
    pub fn revision(&self) -> String {
        let mut files = self.files.iter().collect::<Vec<_>>();
        files.sort_by(|a, b| (&a.path, &a.kind).cmp(&(&b.path, &b.kind)));
        let mut digest = Sha256::new();
        digest.update(b"coflow-project-revision-v1\0");
        for file in files {
            for part in [
                file.path.as_bytes(),
                file.kind.as_bytes(),
                file.contents.as_slice(),
            ] {
                digest.update((part.len() as u64).to_le_bytes());
                digest.update(part);
            }
        }
        format!("{:x}", digest.finalize())
    }
    /// # Errors
    ///
    /// 预期修订与当前快照不一致时返回冲突。
    pub fn require_revision(&self, expected: Option<&str>) -> Result<()> {
        if let Some(expected) = expected {
            let actual = self.revision();
            if expected != actual {
                return Err(Failure::conflict("project revision changed; read the current target and submit a new change after reviewing it")
                    .with_result(serde_json::json!({"written": false, "revision": actual,
                        "precondition": {"kind": "revision", "expected": expected, "actual": actual}})));
            }
        }
        Ok(())
    }
    /// # Errors
    ///
    /// 无法发现项目输入或读取文件时返回错误。
    pub fn capture(project: &Project) -> Result<Self> {
        let mut files: BTreeMap<PathBuf, InputFile> = BTreeMap::new();
        collect(
            project,
            project.config_path(),
            "config",
            "coflow.yaml",
            &mut files,
        )?;
        for root in project.config().schema.paths() {
            collect(
                project,
                &project.resolve_path(root),
                "schema",
                &root.to_string_lossy(),
                &mut files,
            )?;
        }
        for root in project.cfd_input_paths() {
            collect(
                project,
                &project.resolve_path(root),
                "data",
                &root.to_string_lossy(),
                &mut files,
            )?;
        }
        let lock = project.root_dir().join("coflow.enum.lock.json");
        if lock.exists() {
            collect(
                project,
                &lock,
                "mapping",
                "coflow.enum.lock.json",
                &mut files,
            )?;
        }
        let files: Vec<_> = files.into_values().collect();
        Ok(Self { files })
    }
    /// # Errors
    ///
    /// 无法读取项目或输入身份、内容发生变化时返回错误。
    pub fn verify(&self, project: &Project) -> Result<()> {
        // 重新发现目录，新增文件与配置修改也必须使前置条件失效。
        let current = Project::open_schema_only(Some(project.config_path()))?;
        self.verify_scope(&current)
    }
    pub(crate) fn verify_scope(&self, project: &Project) -> Result<()> {
        // 原始与候选输入范围分别重新发现，候选新增的输入同样检查外部变化。
        let current = Self::capture(project)?;
        if current.files.len() != self.files.len()
            || current
                .files
                .iter()
                .zip(&self.files)
                .any(|(a, b)| a.absolute != b.absolute || a.contents != b.contents)
        {
            return Err(Failure::conflict("project inputs changed; read the current target and submit a new change after reviewing it")
                .with_result(serde_json::json!({"written": false, "revision": current.revision(),
                    "precondition": {"kind": "revision", "expected": self.revision(), "actual": current.revision()}})));
        }
        Ok(())
    }
    /// # Errors
    ///
    /// 指定文件不属于项目输入范围时返回未找到错误。
    pub fn file(&self, project: &Project, path: &str) -> Result<&InputFile> {
        let absolute = normalize_path(&project.resolve_path(Path::new(path)));
        self.files
            .iter()
            .find(|f| normalize_path(&f.absolute) == absolute)
            .ok_or_else(|| {
                Failure::missing(format!(
                    "source file `{path}` is not selected by the project"
                ))
            })
    }
}

fn collect(
    project: &Project,
    path: &Path,
    kind: &str,
    input: &str,
    files: &mut BTreeMap<PathBuf, InputFile>,
) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }
    if path.is_dir() {
        let extension = if kind == "schema" { "cft" } else { "cfd" };
        for entry in walkdir::WalkDir::new(path).follow_links(false) {
            let entry = entry.map_err(|e| Failure::from(std::io::Error::other(e)))?;
            if entry.file_type().is_file()
                && entry.path().extension().is_some_and(|e| e == extension)
            {
                collect(project, entry.path(), kind, input, files)?;
            }
        }
        return Ok(());
    }
    let absolute = crate::canonicalize_path(path)?;
    if let Some(file) = files.get_mut(&absolute) {
        file.inputs.push(input.replace('\\', "/"));
        return Ok(());
    }
    let contents = fs::read(&absolute)?;
    files.insert(
        absolute.clone(),
        InputFile {
            path: project_path(project.root_dir(), &absolute),
            kind: kind.into(),
            inputs: vec![input.replace('\\', "/")],
            fingerprint: fingerprint(&contents),
            absolute,
            contents,
        },
    );
    Ok(())
}
