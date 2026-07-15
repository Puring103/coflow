use std::collections::BTreeSet;

use crate::{CftSchema, CftSchemaTypeRef, CftType, EnumName, TypeName};

impl CftSchema {
    #[must_use]
    pub fn inherited_id_as_enum(&self, type_name: &str) -> Option<EnumName> {
        let mut current = Some(type_name);
        while let Some(name) = current {
            let meta = self.types.get(name)?;
            if let Some(enum_name) = &meta.id_as_enum {
                return Some(enum_name.clone());
            }
            current = meta.parent.as_deref();
        }
        None
    }

    #[must_use]
    pub fn is_id_as_enum(&self, enum_name: &str) -> bool {
        self.type_by_id_as_enum.contains_key(enum_name)
    }

    #[must_use]
    pub fn id_as_enum_names(&self) -> BTreeSet<EnumName> {
        self.type_by_id_as_enum.keys().cloned().collect()
    }

    #[must_use]
    pub fn ref_target_names(&self) -> Vec<TypeName> {
        let mut out = BTreeSet::new();
        for ty in self.types.values() {
            let mut visited = BTreeSet::new();
            self.collect_ref_targets_for_type(ty, &mut out, &mut visited);
        }
        out.into_iter().collect()
    }

    fn collect_ref_targets_for_type(
        &self,
        ty: &CftType,
        out: &mut BTreeSet<TypeName>,
        visited: &mut BTreeSet<TypeName>,
    ) {
        if !visited.insert(ty.name.clone()) {
            return;
        }
        for field in &ty.all_fields {
            self.collect_ref_targets_in_type(&field.ty_ref, out, visited);
        }
    }

    fn collect_ref_targets_in_type(
        &self,
        ty: &CftSchemaTypeRef,
        out: &mut BTreeSet<TypeName>,
        visited: &mut BTreeSet<TypeName>,
    ) {
        match ty {
            CftSchemaTypeRef::Object(name) => {
                if let Some(meta) = self.types.get(name) {
                    self.collect_ref_targets_for_type(meta, out, visited);
                }
            }
            CftSchemaTypeRef::RecordRef(name) => {
                out.insert(name.clone());
            }
            CftSchemaTypeRef::Array(inner) | CftSchemaTypeRef::Nullable(inner) => {
                self.collect_ref_targets_in_type(inner, out, visited);
            }
            CftSchemaTypeRef::Dict(_, value) => {
                self.collect_ref_targets_in_type(value, out, visited);
            }
            CftSchemaTypeRef::Enum(_)
            | CftSchemaTypeRef::Int
            | CftSchemaTypeRef::Float
            | CftSchemaTypeRef::Bool
            | CftSchemaTypeRef::String => {}
        }
    }
}
