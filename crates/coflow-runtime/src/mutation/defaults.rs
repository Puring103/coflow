use std::collections::{BTreeMap, BTreeSet};

use coflow_api::DiagnosticSet;
use coflow_cft::{
    CftFieldMeta, CftSchemaDefaultValue, CftSchemaTypeRef, CftSchema, ValueDependencyMode,
};
use coflow_data_model::{CfdEnumValue, CfdObject, CfdRecord, CfdValue, RecordOrigin};

use super::{
    non_nullable, one_mutation_error, CreateFieldSource, CreateRecordDraft, CreateRecordFieldDraft,
    CreateRequiredInput, DefaultMaterialization,
};

pub(super) fn default_record_for_type(
    schema: &CftSchema,
    type_name: &str,
    materialization: DefaultMaterialization,
) -> Result<CfdRecord, DiagnosticSet> {
    ensure_type_can_materialize(schema, type_name)?;
    let mut materializer = DefaultValueMaterializer::new(schema);
    let fields = materializer.fields_for_type(type_name, materialization, None)?;
    Ok(CfdRecord {
        key: String::new(),
        object: CfdObject::new(type_name, fields),
        origin: RecordOrigin::None,
    })
}

pub fn default_value_for_type_ref(
    schema: &CftSchema,
    ty: &CftSchemaTypeRef,
    materialization: DefaultMaterialization,
) -> Result<CfdValue, DiagnosticSet> {
    DefaultValueMaterializer::new(schema).zero_for_ty(ty, materialization)
}

pub(super) fn default_missing_fields_for_type(
    schema: &CftSchema,
    type_name: &str,
    materialization: DefaultMaterialization,
    provided_names: &BTreeSet<String>,
) -> Result<BTreeMap<String, CfdValue>, DiagnosticSet> {
    DefaultValueMaterializer::new(schema).fields_for_type(
        type_name,
        materialization,
        Some(provided_names),
    )
}

pub(super) fn create_record_draft_for_type(
    schema: &CftSchema,
    type_name: &str,
) -> Result<CreateRecordDraft, DiagnosticSet> {
    ensure_type_can_materialize(schema, type_name)?;
    let Some(schema_type) = schema.type_meta(type_name) else {
        return Err(one_mutation_error(
            "MUTATION-TYPE",
            format!("unknown type `{type_name}`"),
        ));
    };
    let mut materializer = DefaultValueMaterializer::new(schema);
    let fields = schema
        .full_fields(&schema_type.name)
        .unwrap_or(&[])
        .iter()
        .map(|field| materializer.create_field_draft(field))
        .collect();
    Ok(CreateRecordDraft {
        actual_type: type_name.to_string(),
        fields,
    })
}

struct DefaultValueMaterializer<'a> {
    schema: &'a CftSchema,
    memo: BTreeMap<(ValueDependencyMode, String), BTreeMap<String, CfdValue>>,
}

impl<'a> DefaultValueMaterializer<'a> {
    const fn new(schema: &'a CftSchema) -> Self {
        Self {
            schema,
            memo: BTreeMap::new(),
        }
    }

    fn fields_for_type(
        &mut self,
        type_name: &str,
        materialization: DefaultMaterialization,
        skip_fields: Option<&BTreeSet<String>>,
    ) -> Result<BTreeMap<String, CfdValue>, DiagnosticSet> {
        ensure_type_can_materialize(self.schema, type_name)?;
        let mode = dependency_mode(materialization);
        let memo_key = (mode, type_name.to_string());
        if skip_fields.is_none() {
            if let Some(fields) = self.memo.get(&memo_key) {
                return Ok(fields.clone());
            }
            self.ensure_acyclic(type_name, mode)?;
        }

        let Some(schema_type) = self.schema.type_meta(type_name) else {
            return Err(one_mutation_error(
                "MUTATION-TYPE",
                format!("unknown type `{type_name}`"),
            ));
        };
        let mut fields = BTreeMap::new();
        for field in self.schema.full_fields(&schema_type.name).unwrap_or(&[]) {
            if skip_fields.is_some_and(|skip| skip.contains(&field.name)) {
                continue;
            }
            let value = match materialization {
                DefaultMaterialization::Minimal => self.minimal_for_field(field)?,
                DefaultMaterialization::EditableShape => Some(self.value_for_ty(
                    &field.ty_ref,
                    field.default.as_ref(),
                    materialization,
                )?),
            };
            if let Some(value) = value {
                fields.insert(field.name.clone(), value);
            }
        }

        if skip_fields.is_none() {
            self.memo.insert(memo_key, fields.clone());
        }
        Ok(fields)
    }

    fn ensure_acyclic(
        &self,
        type_name: &str,
        mode: ValueDependencyMode,
    ) -> Result<(), DiagnosticSet> {
        let Some(result) = self
            .schema
            .value_dependencies()
            .materialization_order(type_name, mode)
        else {
            return Err(one_mutation_error(
                "MUTATION-TYPE",
                format!("unknown type `{type_name}`"),
            ));
        };
        match result {
            Ok(_) => Ok(()),
            Err(cycle) => Err(one_mutation_error(
                "MUTATION-DEFAULT",
                format!("default materialization dependency cycle: {cycle}"),
            )),
        }
    }

    fn minimal_for_field(
        &mut self,
        field: &CftFieldMeta,
    ) -> Result<Option<CfdValue>, DiagnosticSet> {
        if field.default.is_some() {
            return Ok(None);
        }
        match &field.ty_ref {
            CftSchemaTypeRef::Nullable(_) => Ok(Some(CfdValue::Null)),
            CftSchemaTypeRef::Ref(name) => Err(one_mutation_error(
                "MUTATION-DEFAULT",
                format!(
                    "field `{}` of type `&{name}` has no schema default; provide an explicit value",
                    field.name
                ),
            )),
            CftSchemaTypeRef::Named(name) if self.schema.has_type(name) => {
                let fields = self.fields_for_type(name, DefaultMaterialization::Minimal, None)?;
                Ok(Some(CfdValue::Object(Box::new(CfdObject::new(
                    name.clone(),
                    fields,
                )))))
            }
            CftSchemaTypeRef::Named(name) if self.schema.is_schema_enum(name) => self
                .zero_for_ty(&field.ty_ref, DefaultMaterialization::Minimal)
                .map(Some),
            CftSchemaTypeRef::Named(name) => Err(one_mutation_error(
                "MUTATION-DEFAULT",
                format!(
                    "field `{}` of type `{name}` has no schema default; provide an explicit value",
                    field.name
                ),
            )),
            _ => self
                .zero_for_ty(&field.ty_ref, DefaultMaterialization::Minimal)
                .map(Some),
        }
    }

    fn create_field_draft(&mut self, field: &CftFieldMeta) -> CreateRecordFieldDraft {
        if let Some(default) = field.default.as_ref() {
            return match self.materialize_schema_default(
                &field.ty_ref,
                default,
                DefaultMaterialization::EditableShape,
            ) {
                Ok(value) => CreateRecordFieldDraft {
                    name: field.name.clone(),
                    value: Some(value),
                    source: CreateFieldSource::SchemaDefault,
                    required: None,
                },
                Err(err) => {
                    required_field_draft(self.schema, field, Some(&err), Some(CfdValue::Null))
                }
            };
        }

        match self.minimal_for_field(field) {
            Ok(Some(value)) => CreateRecordFieldDraft {
                name: field.name.clone(),
                value: Some(value),
                source: CreateFieldSource::TypeSeed,
                required: None,
            },
            Ok(None) => CreateRecordFieldDraft {
                name: field.name.clone(),
                value: None,
                source: CreateFieldSource::TypeSeed,
                required: None,
            },
            Err(err) => required_field_draft(self.schema, field, Some(&err), Some(CfdValue::Null)),
        }
    }

    fn value_for_ty(
        &mut self,
        ty: &CftSchemaTypeRef,
        declared_default: Option<&CftSchemaDefaultValue>,
        materialization: DefaultMaterialization,
    ) -> Result<CfdValue, DiagnosticSet> {
        if let Some(default) = declared_default {
            return self.materialize_schema_default(ty, default, materialization);
        }
        self.zero_for_ty(ty, materialization)
    }

    fn materialize_schema_default(
        &mut self,
        ty: &CftSchemaTypeRef,
        default: &CftSchemaDefaultValue,
        materialization: DefaultMaterialization,
    ) -> Result<CfdValue, DiagnosticSet> {
        match default {
            CftSchemaDefaultValue::Null => Ok(CfdValue::Null),
            CftSchemaDefaultValue::Int(value) => Ok(CfdValue::Int(*value)),
            CftSchemaDefaultValue::Float(value) => Ok(CfdValue::Float(*value)),
            CftSchemaDefaultValue::Bool(value) => Ok(CfdValue::Bool(*value)),
            CftSchemaDefaultValue::String(value) => Ok(CfdValue::String(value.clone())),
            CftSchemaDefaultValue::Enum {
                enum_name,
                variant,
                value,
            } => Ok(CfdValue::Enum(
                self.schema
                    .enum_value_from_int(enum_name, *value)
                    .map_or_else(
                        || CfdEnumValue {
                            enum_name: enum_name.clone(),
                            variant: Some(variant.clone()),
                            value: *value,
                        },
                        |value| CfdEnumValue {
                            enum_name: value.enum_name,
                            variant: value.variant,
                            value: value.value,
                        },
                    ),
            )),
            CftSchemaDefaultValue::EmptyArray => Ok(CfdValue::Array(Vec::new())),
            CftSchemaDefaultValue::EmptyObject => match non_nullable(ty) {
                CftSchemaTypeRef::Named(name) if self.schema.has_type(name) => {
                    let fields = self.fields_for_type(name, materialization, None)?;
                    Ok(CfdValue::Object(Box::new(CfdObject::new(
                        name.clone(),
                        fields,
                    ))))
                }
                CftSchemaTypeRef::Dict(_, _) => Ok(CfdValue::Dict(Vec::new())),
                _ => self.zero_for_ty(ty, materialization),
            },
        }
    }

    fn zero_for_ty(
        &mut self,
        ty: &CftSchemaTypeRef,
        materialization: DefaultMaterialization,
    ) -> Result<CfdValue, DiagnosticSet> {
        match ty {
            CftSchemaTypeRef::Int => Ok(CfdValue::Int(0)),
            CftSchemaTypeRef::Float => Ok(CfdValue::Float(0.0)),
            CftSchemaTypeRef::Bool => Ok(CfdValue::Bool(false)),
            CftSchemaTypeRef::String => Ok(CfdValue::String(String::new())),
            CftSchemaTypeRef::Ref(_) | CftSchemaTypeRef::Nullable(_) => Ok(CfdValue::Null),
            CftSchemaTypeRef::Array(_) => Ok(CfdValue::Array(Vec::new())),
            CftSchemaTypeRef::Dict(_, _) => Ok(CfdValue::Dict(Vec::new())),
            CftSchemaTypeRef::Named(name) if self.schema.is_schema_enum(name) => {
                let value = self
                    .schema
                    .enum_meta(name)
                    .and_then(|enm| enm.all_variants.first());
                Ok(value.map_or_else(
                    || {
                        CfdValue::Enum(CfdEnumValue {
                            enum_name: name.clone(),
                            variant: None,
                            value: 0,
                        })
                    },
                    |variant| {
                        CfdValue::Enum(CfdEnumValue {
                            enum_name: name.clone(),
                            variant: Some(variant.name.clone()),
                            value: variant.value,
                        })
                    },
                ))
            }
            CftSchemaTypeRef::Named(name) => {
                let fields = self.fields_for_type(name, materialization, None)?;
                Ok(CfdValue::Object(Box::new(CfdObject::new(
                    name.clone(),
                    fields,
                ))))
            }
        }
    }
}

const fn dependency_mode(materialization: DefaultMaterialization) -> ValueDependencyMode {
    match materialization {
        DefaultMaterialization::Minimal => ValueDependencyMode::Minimal,
        DefaultMaterialization::EditableShape => ValueDependencyMode::EditableShape,
    }
}

fn required_field_draft(
    schema: &CftSchema,
    field: &CftFieldMeta,
    err: Option<&DiagnosticSet>,
    value: Option<CfdValue>,
) -> CreateRecordFieldDraft {
    CreateRecordFieldDraft {
        name: field.name.clone(),
        value,
        source: CreateFieldSource::RequiredInput,
        required: Some(required_input_for_field(schema, field, err)),
    }
}

fn required_input_for_field(
    schema: &CftSchema,
    field: &CftFieldMeta,
    err: Option<&DiagnosticSet>,
) -> CreateRequiredInput {
    match non_nullable(&field.ty_ref) {
        CftSchemaTypeRef::Ref(target_type) => CreateRequiredInput::Ref {
            target_type: target_type.clone(),
        },
        CftSchemaTypeRef::Named(expected_type)
            if schema
                .type_meta(expected_type)
                .is_some_and(|meta| meta.is_abstract) =>
        {
            CreateRequiredInput::AbstractObject {
                expected_type: expected_type.clone(),
                concrete_types: schema
                    .concrete_assignable_types(expected_type)
                    .unwrap_or_default(),
            }
        }
        CftSchemaTypeRef::Named(type_name)
            if err.is_some_and(|err| {
                err.iter()
                    .any(|diagnostic| diagnostic.message.contains("dependency cycle"))
            }) =>
        {
            CreateRequiredInput::RecursiveObject {
                type_name: type_name.clone(),
            }
        }
        _ => CreateRequiredInput::Unsupported {
            message: err.and_then(|err| err.iter().next()).map_or_else(
                || format!("field `{}` requires an explicit value", field.name),
                |diagnostic| diagnostic.message.clone(),
            ),
        },
    }
}

fn ensure_type_can_materialize(
    schema: &CftSchema,
    type_name: &str,
) -> Result<(), DiagnosticSet> {
    let Some(schema_type) = schema.type_meta(type_name) else {
        return Err(one_mutation_error(
            "MUTATION-TYPE",
            format!("unknown type `{type_name}`"),
        ));
    };
    if schema_type.is_abstract {
        return Err(one_mutation_error(
            "MUTATION-DEFAULT",
            format!("abstract object type `{type_name}` cannot be default materialized"),
        ));
    }
    if schema_type.is_singleton {
        return Err(one_mutation_error(
            "MUTATION-DEFAULT",
            format!("singleton object type `{type_name}` cannot be default materialized"),
        ));
    }
    Ok(())
}
