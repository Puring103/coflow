use std::collections::BTreeMap;
use serde::{Deserialize, Serialize};
use super::{Failure, Result, Selector};

/// 参数层只传递已解析的名字和值；所有项目语义都由 Runtime 处理。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Arguments(pub BTreeMap<String, Vec<String>>);
impl Arguments {
    pub fn get(&self, name: &str) -> Option<&str> { self.0.get(name).and_then(|v| v.first()).map(String::as_str) }
    pub fn require(&self, name: &str) -> Result<&str> { self.get(name).ok_or_else(|| Failure::request(format!("--{name} is required"))) }
    pub fn flag(&self, name: &str) -> bool { self.get(name) == Some("true") }
    pub fn all(&self, name: &str) -> Vec<String> { self.0.get(name).cloned().unwrap_or_default() }
    pub fn number(&self, name: &str, default: usize) -> Result<usize> {
        self.get(name).map_or(Ok(default), |v| v.parse().map_err(|_| Failure::request(format!("--{name} requires a nonnegative integer"))))
    }
    pub fn csv(&self, name: &str) -> Vec<String> { self.all(name).iter().flat_map(|s| s.split(',').map(str::to_string)).collect() }
    pub fn selector(&self) -> Result<Selector> {
        // 查询身份包含命令专属条件，但不包含分页位置或每页数量，也不绑定文件快照。
        let mut identity = self.0.clone();
        identity.remove("cursor");
        identity.remove("limit");
        let mut dimensions = BTreeMap::new();
        for value in self.all("dimension") {
            if let Some((name, variant)) = value.split_once('=') {
                if dimensions.insert(name.into(), variant.into()).is_some() { return Err(Failure::request(format!("dimension `{name}` is selected twice"))); }
            }
        }
        Ok(Selector { actual_type: self.get("type").map(str::to_string), include_derived: self.flag("include-derived"),
            file: self.get("file").map(|s| s.replace('\\', "/")), key: self.get("key").map(str::to_string), fields: self.csv("fields"),
            dimensions, sort: self.csv("sort"), limit: self.number("limit", 50)?, cursor: self.get("cursor").map(str::to_string),
            text: self.get("text").map(str::to_string), view: self.get("view").unwrap_or("effective").into(), filter: self.get("where").map(str::to_string),
            query_identity: Some(super::snapshot::fingerprint(&serde_json::to_vec(&identity)?)) })
    }
}
