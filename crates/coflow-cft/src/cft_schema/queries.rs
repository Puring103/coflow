use std::collections::BTreeSet;

use crate::{
    CftAnnotation, CftAnnotationValue, CftSchema, CftSchemaTypeRef, CftType, EnumName, TypeName,
};

impl CftSchema {
    #[must_use]
    pub fn type_is_struct(&self, type_name: &str) -> bool {
        self.types
            .get(type_name)
            .is_some_and(|ty| annotation_exists(&ty.annotations, "struct"))
    }

    #[must_use]
    pub fn type_id_as_enum(&self, type_name: &str) -> Option<EnumName> {
        annotation_name_arg(&self.types.get(type_name)?.annotations, "idAsEnum")
            .map(EnumName::from_validated)
    }

    #[must_use]
    pub fn inherited_id_as_enum(&self, type_name: &str) -> Option<EnumName> {
        let mut current = Some(type_name);
        while let Some(name) = current {
            let meta = self.types.get(name)?;
            if let Some(enum_name) = annotation_name_arg(&meta.annotations, "idAsEnum") {
                return Some(EnumName::from_validated(enum_name));
            }
            current = meta.parent.as_deref();
        }
        None
    }

    #[must_use]
    pub fn is_id_as_enum(&self, enum_name: &str) -> bool {
        self.types.values().any(|ty| {
            annotation_name_arg(&ty.annotations, "idAsEnum").as_deref() == Some(enum_name)
        })
    }

    #[must_use]
    pub fn id_as_enum_names(&self) -> BTreeSet<EnumName> {
        self.types
            .values()
            .filter_map(|ty| {
                annotation_name_arg(&ty.annotations, "idAsEnum")
                    .map(EnumName::from_validated)
            })
            .collect()
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
            CftSchemaTypeRef::Enum(_) => {}
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
            CftSchemaTypeRef::Int
            | CftSchemaTypeRef::Float
            | CftSchemaTypeRef::Bool
            | CftSchemaTypeRef::String => {}
        }
    }
}

fn annotation_exists(annotations: &[CftAnnotation], name: &str) -> bool {
    annotations.iter().any(|annotation| annotation.name == name)
}

fn annotation_name_arg(annotations: &[CftAnnotation], name: &str) -> Option<String> {
    annotations
        .iter()
        .find(|annotation| annotation.name == name)
        .and_then(|annotation| annotation.args.first())
        .and_then(|arg| match arg {
            CftAnnotationValue::Name(value) => Some(value.clone()),
            _ => None,
        })
}
