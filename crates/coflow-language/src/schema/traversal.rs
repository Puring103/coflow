//! schema 代次共享的值树可达性；继承和递归通过反向图不动点处理。
use super::{CftField, CftSchema, CftValueType, FieldName, TypeName};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug)]
pub(super) struct ValueTraversalPlan {
    fields: BTreeMap<TypeName, TypeFields>,
}
#[derive(Debug)]
struct TypeFields {
    all: Vec<FieldName>,
    references: Vec<FieldName>,
    checks: Vec<FieldName>,
}

impl ValueTraversalPlan {
    pub(super) fn compile(schema: &CftSchema) -> Self {
        let mut dependents = BTreeMap::<&str, BTreeSet<&str>>::new();
        let mut refs = BTreeSet::new();
        let mut checks = BTreeSet::new();
        for ty in schema.all_types() {
            let owner = ty.name.as_str();
            if let Some(parent) = &ty.parent {
                dependents.entry(owner).or_default().insert(parent.as_str());
            }
            if schema
                .check_statements_for_actual_type(owner)
                .next()
                .is_some()
            {
                checks.insert(owner);
            }
            for field in ty.all_fields() {
                match contained_type(&field.value_type) {
                    CftValueType::RecordRef(_) => {
                        refs.insert(owner);
                    }
                    CftValueType::Object(target) => {
                        dependents.entry(target.as_str()).or_default().insert(owner);
                    }
                    _ => {}
                }
            }
        }
        for reachable in [&mut refs, &mut checks] {
            let mut pending: Vec<_> = reachable.iter().copied().collect();
            while let Some(target) = pending.pop() {
                if let Some(owners) = dependents.get(target) {
                    for owner in owners {
                        if reachable.insert(*owner) {
                            pending.push(*owner);
                        }
                    }
                }
            }
        }
        let fields = schema
            .all_types()
            .map(|ty| {
                let mut all: Vec<_> = ty.all_fields().collect();
                all.sort_unstable_by(|a, b| a.name.cmp(&b.name));
                let references = all
                    .iter()
                    .filter(|f| match contained_type(&f.value_type) {
                        CftValueType::RecordRef(_) => true,
                        CftValueType::Object(target) => refs.contains(target.as_str()),
                        _ => false,
                    })
                    .map(|f| f.name.clone())
                    .collect();
                let checks = all
                    .iter()
                    .filter(|f| match contained_type(&f.value_type) {
                        CftValueType::Object(target) => checks.contains(target.as_str()),
                        _ => false,
                    })
                    .map(|f| f.name.clone())
                    .collect();
                (
                    ty.name.clone(),
                    TypeFields {
                        all: all.iter().map(|f| f.name.clone()).collect(),
                        references,
                        checks,
                    },
                )
            })
            .collect();
        Self { fields }
    }
}
impl CftSchema {
    /// 字段顺序与模型的字典序一致，跨代次复用 schema 计划。
    pub fn reference_fields<'a>(
        &'a self,
        actual_type: &'a str,
        references_only: bool,
    ) -> impl Iterator<Item = &'a CftField> {
        self.traversal
            .get_or_init(|| ValueTraversalPlan::compile(self))
            .fields
            .get(actual_type)
            .into_iter()
            .flat_map(move |fields| {
                if references_only {
                    &fields.references
                } else {
                    &fields.all
                }
            })
            .filter_map(move |name| self.field(actual_type, name))
    }
    /// 只枚举可能含有内联 data check 的字段，引用目标由独立记录任务负责。
    pub fn data_check_fields<'a>(
        &'a self,
        actual_type: &'a str,
    ) -> impl Iterator<Item = &'a CftField> {
        self.traversal
            .get_or_init(|| ValueTraversalPlan::compile(self))
            .fields
            .get(actual_type)
            .into_iter()
            .flat_map(|fields| &fields.checks)
            .filter_map(move |name| self.field(actual_type, name))
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

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use crate::cft::{build_schema, parse_modules, CftFile, ModuleId};
    #[test]
    fn traversal_plan_covers_recursive_and_polymorphic_children() {
        let schema = build_schema(&parse_modules([CftFile::from_source(ModuleId::from("main"), r"
            table Target {}
            abstract data Base {}
            data Checked : Base { next: Option<Base> = None; link: Option<&Target> = None; check Valid { true; } }
            data Wrapper { children: [Base] = []; numbers: [int] = []; }
            table Owner { wrapped: Wrapper; link: Option<&Target> = None; numbers: [int] = []; }
        ")])).expect("schema");
        let names = |ty| {
            schema
                .data_check_fields(ty)
                .map(|f| f.name.to_string())
                .collect::<Vec<_>>()
        };
        assert_eq!(names("Owner"), vec!["wrapped"]);
        assert_eq!(names("Wrapper"), vec!["children"]);
        assert_eq!(names("Checked"), vec!["next"]);
        assert_eq!(
            schema
                .reference_fields("Owner", true)
                .map(|f| f.name.to_string())
                .collect::<Vec<_>>(),
            vec!["link", "wrapped"]
        );
        let cloned = schema.clone();
        assert!(std::sync::Arc::ptr_eq(&schema.traversal, &cloned.traversal));
    }
}
