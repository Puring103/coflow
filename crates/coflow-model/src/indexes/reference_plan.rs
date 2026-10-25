use crate::build::BuildSchema;
use coflow_language::cft::{CftField, CftValueType};
use std::collections::{BTreeMap, BTreeSet};

/// 同一构建只分析一次引用可达性，记录遍历不再扫描纯数值字段或纯数值对象。
pub(crate) struct ReferenceFieldPlan<'a> {
    fields: BTreeMap<&'a str, TypeFields<'a>>,
}

struct TypeFields<'a> {
    all: Vec<&'a CftField>,
    references: Vec<&'a CftField>,
}

impl<'a> ReferenceFieldPlan<'a> {
    pub(crate) fn new(schema: BuildSchema<'a>) -> Self {
        let mut dependents = BTreeMap::<&str, BTreeSet<&str>>::new();
        let mut reference_types = BTreeSet::new();
        let mut pending = Vec::new();
        for ty in schema.cft().all_types() {
            let owner = ty.name.as_str();
            // 声明为父类的对象也可能装载含引用的子类，必须沿继承链向上传播。
            if let Some(parent) = &ty.parent {
                dependents.entry(owner).or_default().insert(parent.as_str());
            }
            for field in ty.all_fields() {
                match contained_type(&field.value_type) {
                    CftValueType::RecordRef(_) => {
                        if reference_types.insert(owner) {
                            pending.push(owner);
                        }
                    }
                    CftValueType::Object(target) => {
                        dependents.entry(target.as_str()).or_default().insert(owner);
                    }
                    _ => {}
                }
            }
        }
        // 反向图的不动点支持递归对象结构；每个类型只入队一次。
        while let Some(target) = pending.pop() {
            if let Some(owners) = dependents.get(target) {
                for owner in owners {
                    if reference_types.insert(*owner) {
                        pending.push(*owner);
                    }
                }
            }
        }
        let fields = schema
            .cft()
            .all_types()
            .map(|ty| {
                let mut all = ty.all_fields().collect::<Vec<_>>();
                // 继续采用模型字段的字典序，保留引用边及诊断的稳定顺序。
                all.sort_unstable_by(|left, right| left.name.cmp(&right.name));
                let references = all
                    .iter()
                    .copied()
                    .filter(|field| match contained_type(&field.value_type) {
                        CftValueType::RecordRef(_) => true,
                        CftValueType::Object(target) => reference_types.contains(target.as_str()),
                        _ => false,
                    })
                    .collect();
                (ty.name.as_str(), TypeFields { all, references })
            })
            .collect();
        Self { fields }
    }

    pub(crate) fn fields(&self, actual_type: &str, references_only: bool) -> &[&'a CftField] {
        self.fields.get(actual_type).map_or(&[], |fields| {
            if references_only {
                &fields.references
            } else {
                &fields.all
            }
        })
    }
}

fn contained_type(mut ty: &CftValueType) -> &CftValueType {
    loop {
        match ty {
            CftValueType::Option(inner) | CftValueType::Array(inner) => ty = inner,
            CftValueType::Dict(_, value) => ty = value,
            _ => return ty,
        }
    }
}
