//! 编辑器只读预览：以同代际模型构建运行时，模板源码仍归模型持有。
use std::{collections::BTreeMap, sync::Arc};

use coflow_core::{
    contract::Contract,
    runtime::{Runtime, Value, ValueId},
    schema::CftSchema,
};

use crate::{CfdDataModel, CfdPathSegment, CfdRecord, CfdValue};

#[derive(Debug)]
pub struct TemplatePreview {
    runtime: Runtime,
}

impl TemplatePreview {
    pub(crate) fn new(schema: &CftSchema, model: &CfdDataModel) -> Result<Self, String> {
        let contract = Arc::new(Contract::new(schema.clone()).map_err(|e| e.to_string())?);
        let runtime = Runtime::from_model(contract, model.clone(), Default::default())
            .map_err(|e| e.message)?;
        Ok(Self { runtime })
    }

    /// 路径编码与前端 JSON.stringify(fieldPath) 一致；包括编辑器补出的默认字段。
    pub fn record<'a>(
        &self,
        record: &CfdRecord,
        fields: impl IntoIterator<Item = (&'a str, &'a CfdValue)>,
    ) -> BTreeMap<String, Result<String, String>> {
        let mut previews = BTreeMap::new();
        let root = self.runtime.record(record.actual_type(), record.key());
        for (name, value) in fields {
            if !contains_template(value) {
                continue;
            }
            let path = vec![CfdPathSegment::Field(name.to_string())];
            let id = root
                .as_ref()
                .map_err(ToString::to_string)
                .and_then(|root| self.runtime.field(*root, name).map_err(|e| e.to_string()));
            self.collect_result(value, id, &path, &mut previews);
        }
        previews
    }

    /// 维度视图以字段值为路径根；默认值和每个显式变体分别求值。
    pub fn dimension(
        &self,
        record: &CfdRecord,
        field: &str,
        value: &CfdValue,
        variant: Option<&str>,
    ) -> BTreeMap<String, Result<String, String>> {
        let mut previews = BTreeMap::new();
        if !contains_template(value) {
            return previews;
        }
        let id = self
            .runtime
            .record(record.actual_type(), record.key())
            .map_err(|error| error.to_string())
            .and_then(|root| {
                self.runtime
                    .field(root, field)
                    .map_err(|error| error.to_string())
            })
            .and_then(|dimension| {
                match variant {
                    Some(variant) => self.runtime.dimension_variant(dimension, variant),
                    None => self.runtime.dimension_default(dimension),
                }
                .map_err(|error| error.to_string())
            });
        self.collect_result(value, id, &[], &mut previews);
        previews
    }

    fn collect_result(
        &self,
        value: &CfdValue,
        id: Result<ValueId, String>,
        path: &[CfdPathSegment],
        out: &mut BTreeMap<String, Result<String, String>>,
    ) {
        match value {
            CfdValue::FormattedString(_) => {
                // 只通过 VM 对 fstring 插值；普通 string 保持原文，失败显示诊断。
                out.insert(
                    serde_json::to_string(path).expect("serializable path"),
                    id.and_then(|id| self.runtime.read_text(id).map_err(|e| e.to_string())),
                );
            }
            CfdValue::OptionSome(inner) => self.collect_result(inner, id, path, out),
            CfdValue::Object(object) => {
                for (name, child) in object.fields() {
                    if !contains_template(child) {
                        continue;
                    }
                    let mut path = path.to_vec();
                    path.push(CfdPathSegment::Field(name.to_string()));
                    let child_id = id
                        .as_ref()
                        .map_err(Clone::clone)
                        .and_then(|id| self.runtime.field(*id, name).map_err(|e| e.to_string()));
                    self.collect_result(child, child_id, &path, out);
                }
            }
            CfdValue::Array(items) => {
                let array = id
                    .and_then(|id| self.runtime.stored_value(id).map_err(|e| e.to_string()))
                    .and_then(|stored| match stored.as_ref() {
                        Value::Array(array) => Ok(array.clone()),
                        _ => Err("expected array".to_string()),
                    });
                for (index, child) in items.iter().enumerate() {
                    if !contains_template(child) {
                        continue;
                    }
                    let mut path = path.to_vec();
                    path.push(CfdPathSegment::Index(index));
                    let child_id = array.as_ref().map_err(Clone::clone).and_then(|array| {
                        array
                            .get(index)
                            .ok_or_else(|| format!("missing array index {index}"))
                    });
                    self.collect_result(child, child_id, &path, out);
                }
            }
            CfdValue::Dict(items) => {
                let dict = id
                    .and_then(|id| self.runtime.stored_value(id).map_err(|e| e.to_string()))
                    .and_then(|stored| match stored.as_ref() {
                        Value::Dict(dict) => Ok(dict.clone()),
                        _ => Err("expected dict".to_string()),
                    });
                for (index, (key, child)) in items.iter().enumerate() {
                    if !contains_template(child) {
                        continue;
                    }
                    let mut path = path.to_vec();
                    path.push(CfdPathSegment::DictKey(crate::dict_key_path_text(key)));
                    let child_id = dict.as_ref().map_err(Clone::clone).and_then(|dict| {
                        dict.get_index(index)
                            .map(|(_, (_, id))| *id)
                            .ok_or_else(|| format!("missing dict entry {index}"))
                    });
                    self.collect_result(child, child_id, &path, out);
                }
            }
            _ => {}
        }
    }
}

pub fn contains_template(value: &CfdValue) -> bool {
    match value {
        CfdValue::FormattedString(_) => true,
        CfdValue::OptionSome(inner) => contains_template(inner),
        CfdValue::Object(object) => object.fields().values().any(contains_template),
        CfdValue::Array(items) => items.iter().any(contains_template),
        CfdValue::Dict(items) => items.iter().any(|(_, value)| contains_template(value)),
        _ => false,
    }
}
