use std::{collections::BTreeMap, fs, path::{Path, PathBuf}};
use serde::Serialize;
use super::{Failure, Result};
use crate::{Project, project_path, normalize_path};

#[derive(Debug, Clone, Serialize)]
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
pub struct InputSnapshot { pub id: String, pub files: Vec<InputFile> }

// 固定算法与字节编码保证跨进程游标稳定；文件身份与内容均参与快照。
pub fn fingerprint(bytes: &[u8]) -> String {
    use sha2::{Digest,Sha256};
    format!("{:x}",Sha256::digest(bytes))
}

impl InputSnapshot {
    pub fn capture(project: &Project) -> Result<Self> {
        let mut files: BTreeMap<PathBuf, InputFile> = BTreeMap::new();
        collect(project, project.config_path(), "config", "coflow.yaml", &mut files)?;
        for root in project.config().schema.paths() {
            collect(project, &project.resolve_path(root), "schema", &root.to_string_lossy(), &mut files)?;
        }
        for root in project.cfd_input_paths() {
            collect(project, &project.resolve_path(root), "data", &root.to_string_lossy(), &mut files)?;
        }
        let lock = project.root_dir().join("coflow.enum.lock.json");
        if lock.exists() { collect(project, &lock, "mapping", "coflow.enum.lock.json", &mut files)?; }
        let files: Vec<_> = files.into_values().collect();
        let mut bytes = Vec::new();
        for file in &files {
            bytes.extend_from_slice(&(file.path.len() as u64).to_le_bytes());
            bytes.extend_from_slice(file.path.as_bytes());
            bytes.extend_from_slice(&(file.contents.len() as u64).to_le_bytes());
            bytes.extend_from_slice(&file.contents);
        }
        Ok(Self { id: fingerprint(&bytes), files })
    }
    pub fn verify(&self, project: &Project) -> Result<()> {
        // 重新发现目录，新增文件与配置修改也必须使前置条件失效。
        let current = Project::open_schema_only(Some(project.config_path()))?;
        if Self::capture(&current)?.id != self.id { return Err(Failure::conflict("project inputs changed")); }
        Ok(())
    }
    pub fn file(&self, project: &Project, path: &str) -> Result<&InputFile> {
        let absolute = normalize_path(&project.resolve_path(Path::new(path)));
        self.files.iter().find(|f| normalize_path(&f.absolute) == absolute)
            .ok_or_else(|| Failure::missing(format!("source file `{path}` is not selected by the project")))
    }
}

fn collect(project: &Project, path: &Path, kind: &str, input: &str, files: &mut BTreeMap<PathBuf, InputFile>) -> Result<()> {
    if !path.exists() { return Ok(()); }
    if path.is_dir() {
        let extension = if kind == "schema" { "cft" } else { "cfd" };
        for entry in walkdir::WalkDir::new(path).follow_links(false) {
            let entry = entry.map_err(|e| Failure::from(std::io::Error::other(e)))?;
            if entry.file_type().is_file() && entry.path().extension().is_some_and(|e| e == extension) {
                collect(project, entry.path(), kind, input, files)?;
            }
        }
        return Ok(());
    }
    let absolute = crate::canonicalize_path(path)?;
    if let Some(file) = files.get_mut(&absolute) { file.inputs.push(input.replace('\\', "/")); return Ok(()); }
    let contents = fs::read(&absolute)?;
    files.insert(absolute.clone(), InputFile { path: project_path(project.root_dir(), &absolute), kind: kind.into(),
        inputs: vec![input.replace('\\', "/")], fingerprint: fingerprint(&contents), absolute, contents });
    Ok(())
}
