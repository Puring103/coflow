use std::collections::{BTreeMap, BTreeSet};

use crate::api::DiagnosticSet;
use crate::data_model::{CfdDictKey, CfdEnumValue, CfdObject, CfdValue};
use crate::RecordKey;
use coflow_language::cft::{
    CftField, CftSchema, CftSchemaDefaultValue, CftValueType, FieldName, TypeName,
    ValueDependencyMode,
};

use super::{
    one_mutation_error, CreateFieldSource, CreateRecordDraft, CreateRecordFieldDraft,
    CreateRequiredInput, DefaultMaterialization,
};

pub(super) fn default_object_for_type(
    schema: &CftSchema,
    type_name: &str,
    materialization: DefaultMaterialization,
) -> Result<CfdObject, DiagnosticSet> {
    ensure_type_can_materialize(schema, type_name)?;
    let schema_type = schema.resolve_type(type_name).ok_or_else(|| {
        one_mutation_error("MUTATION-TYPE", format!("unknown type `{type_name}`"))
    })?;
    let mut materializer = DefaultValueMaterializer::new(schema);
    let fields = materializer.fields_for_type(type_name, materialization, None)?;
    Ok(CfdObject::new(schema_type.name.clone(), fields))
}

pub fn default_value_for_value_type(
    schema: &CftSchema,
    ty: &CftValueType,
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
    DefaultValueMaterializer::new(schema)
        .fields_for_type(type_name, materialization, Some(provided_names))
        .map(|fields| {
            fields
                .into_iter()
                .map(|(name, value)| (name.to_string(), value))
                .collect()
        })
}

pub(super) fn create_record_draft_for_type(
    schema: &CftSchema,
    type_name: &str,
) -> Result<CreateRecordDraft, DiagnosticSet> {
    ensure_type_can_materialize(schema, type_name)?;
    let Some(schema_type) = schema.resolve_type(type_name) else {
        return Err(one_mutation_error(
            "MUTATION-TYPE",
            format!("unknown type `{type_name}`"),
        ));
    };
    let mut materializer = DefaultValueMaterializer::new(schema);
    let fields = schema_type
        .all_fields()
        .map(|field| materializer.create_field_draft(field))
        .collect();
    Ok(CreateRecordDraft {
        actual_type: type_name.to_string(),
        fields,
    })
}

struct DefaultValueMaterializer<'a> {
    schema: &'a CftSchema,
    memo: BTreeMap<(ValueDependencyMode, TypeName), BTreeMap<FieldName, CfdValue>>,
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
    ) -> Result<BTreeMap<FieldName, CfdValue>, DiagnosticSet> {
        ensure_type_can_materialize(self.schema, type_name)?;
        let mode = dependency_mode(materialization);
        let Some(schema_type) = self.schema.resolve_type(type_name) else {
            return Err(one_mutation_error(
                "MUTATION-TYPE",
                format!("unknown type `{type_name}`"),
            ));
        };
        let memo_key = (mode, schema_type.name.clone());
        if skip_fields.is_none() {
            if let Some(fields) = self.memo.get(&memo_key) {
                return Ok(fields.clone());
            }
            self.ensure_acyclic(type_name, mode)?;
        }

        let mut fields = BTreeMap::new();
        for field in schema_type.all_fields() {
            if skip_fields.is_some_and(|skip| skip.contains(field.name.as_str())) {
                continue;
            }
            let value = match materialization {
                DefaultMaterialization::Minimal => self.minimal_for_field(field)?,
                DefaultMaterialization::EditableShape => self.editable_for_field(field)?,
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

    fn minimal_for_field(&mut self, field: &CftField) -> Result<Option<CfdValue>, DiagnosticSet> {
        if field.default.is_some() {
            return Ok(None);
        }
        match &field.value_type {
            CftValueType::Option(_) => Ok(Some(CfdValue::OptionNone)),
            CftValueType::RecordRef(_) => Ok(None),
            CftValueType::Object(name) => {
                let fields = self.fields_for_type(name, DefaultMaterialization::Minimal, None)?;
                Ok(Some(CfdValue::Object(Box::new(CfdObject::new(
                    name.clone(),
                    fields,
                )))))
            }
            _ => self
                .zero_for_ty(&field.value_type, DefaultMaterialization::Minimal)
                .map(Some),
        }
    }

    fn editable_for_field(
        &mut self,
        field: &CftField,
    ) -> Result<Option<CfdValue>, DiagnosticSet> {
        if let Some(default) = field.default.as_ref() {
            return self
                .materialize_schema_default(
                    &field.value_type,
                    default,
                    DefaultMaterialization::EditableShape,
                )
                .map(Some);
        }
        // 引用和抽象 object 没有无歧义的类型默认值；省略后由编辑器暴露缺失状态。
        let requires_choice = match &field.value_type {
            CftValueType::RecordRef(_) => true,
            CftValueType::Object(name) => self
                .schema
                .resolve_type(name)
                .is_some_and(|meta| meta.is_abstract),
            _ => false,
        };
        if requires_choice {
            return Ok(None);
        }
        self.zero_for_ty(&field.value_type, DefaultMaterialization::EditableShape)
            .map(Some)
    }

    fn create_field_draft(&mut self, field: &CftField) -> CreateRecordFieldDraft {
        if let Some(default) = field.default.as_ref() {
            return match self.materialize_schema_default(
                &field.value_type,
                default,
                DefaultMaterialization::EditableShape,
            ) {
                Ok(value) => CreateRecordFieldDraft {
                    name: field.name.to_string(),
                    value: Some(value),
                    source: CreateFieldSource::SchemaDefault,
                    required: None,
                },
                Err(err) => {
                    required_field_draft(self.schema, field, Some(&err), None)
                }
            };
        }

        if matches!(field.value_type, CftValueType::RecordRef(_)) {
            return required_field_draft(self.schema, field, None, None);
        }

        match self.minimal_for_field(field) {
            Ok(Some(value)) => CreateRecordFieldDraft {
                name: field.name.to_string(),
                value: Some(value),
                source: CreateFieldSource::TypeDefault,
                required: None,
            },
            Ok(None) => CreateRecordFieldDraft {
                name: field.name.to_string(),
                value: None,
                source: CreateFieldSource::TypeDefault,
                required: None,
            },
            Err(err) => required_field_draft(self.schema, field, Some(&err), None),
        }
    }

    fn materialize_schema_default(
        &mut self,
        ty: &CftValueType,
        default: &CftSchemaDefaultValue,
        materialization: DefaultMaterialization,
    ) -> Result<CfdValue, DiagnosticSet> {
        match default {
            CftSchemaDefaultValue::OptionNone => Ok(CfdValue::OptionNone),
            CftSchemaDefaultValue::OptionSome(value) => match ty {
                CftValueType::Option(inner) => self
                    .materialize_schema_default(inner, value, materialization)
                    .map(|value| CfdValue::OptionSome(Box::new(value))),
                _ => self.zero_for_ty(ty, materialization),
            },
            CftSchemaDefaultValue::ResultOk(value) => match ty {
                CftValueType::Result(ok, _) => self
                    .materialize_schema_default(ok, value, materialization)
                    .map(|value| CfdValue::ResultOk(Box::new(value))),
                _ => self.zero_for_ty(ty, materialization),
            },
            CftSchemaDefaultValue::ResultErr(value) => match ty {
                CftValueType::Result(_, error) => self
                    .materialize_schema_default(error, value, materialization)
                    .map(|value| CfdValue::ResultErr(Box::new(value))),
                _ => self.zero_for_ty(ty, materialization),
            },
            CftSchemaDefaultValue::Int(value) => Ok(CfdValue::Int(*value)),
            CftSchemaDefaultValue::Float(value) => Ok(CfdValue::Float(*value)),
            CftSchemaDefaultValue::Bool(value) => Ok(CfdValue::Bool(*value)),
            CftSchemaDefaultValue::String(value) => Ok(CfdValue::String(value.clone())),
            CftSchemaDefaultValue::FormattedString(source) => {
                Ok(CfdValue::FormattedString(crate::data_model::CfdFormattedString {
                    source: source.clone(),
                    rendered: source.clone(),
                }))
            }
            CftSchemaDefaultValue::Function(source) => {
                Ok(CfdValue::Function(crate::data_model::CfdFunction {
                    source: source.clone(),
                }))
            }
            CftSchemaDefaultValue::Enum {
                enum_name,
                variant,
                value,
            } => {
                let mut enum_value = self
                    .schema
                    .enum_value_from_int(enum_name, *value)
                    .map_or_else(
                        || CfdEnumValue {
                            enum_name: enum_name.clone(),
                            variant: Some(variant.clone()),
                            value: *value,
                        },
                        Into::into,
                    );
                if self
                    .schema
                    .resolve_enum(enum_name)
                    .is_some_and(|schema_enum| schema_enum.is_flag)
                {
                    enum_value.variant = None;
                }
                Ok(CfdValue::Enum(enum_value))
            }
            CftSchemaDefaultValue::EmptyArray => Ok(CfdValue::Array(Vec::new())),
            CftSchemaDefaultValue::EmptyObject => match ty {
                CftValueType::Object(name) => {
                    let fields = self.fields_for_type(name, materialization, None)?;
                    Ok(CfdValue::Object(Box::new(CfdObject::new(
                        name.clone(),
                        fields,
                    ))))
                }
                CftValueType::Dict(_, _) => Ok(CfdValue::Dict(Vec::new())),
                _ => self.zero_for_ty(ty, materialization),
            },
            CftSchemaDefaultValue::Array(values) => match ty {
                CftValueType::Array(inner) => values
                    .iter()
                    .map(|value| self.materialize_schema_default(inner, value, materialization))
                    .collect::<Result<Vec<_>, _>>()
                    .map(CfdValue::Array),
                _ => self.zero_for_ty(ty, materialization),
            },
            CftSchemaDefaultValue::Dictionary(entries) => match ty {
                CftValueType::Dict(key_type, value_type) => {
                    let mut values = Vec::with_capacity(entries.len());
                    for (key, value) in entries {
                        let key = self.materialize_schema_default(key_type, key, materialization)?;
                        let key = match key {
                            CfdValue::Int(value) => CfdDictKey::Int(value),
                            CfdValue::String(value) => CfdDictKey::String(value),
                            CfdValue::Enum(value) => CfdDictKey::Enum(value),
                            _ => return self.zero_for_ty(ty, materialization),
                        };
                        let value = self.materialize_schema_default(
                            value_type,
                            value,
                            materialization,
                        )?;
                        values.push((key, value));
                    }
                    Ok(CfdValue::Dict(values))
                }
                _ => self.zero_for_ty(ty, materialization),
            },
            CftSchemaDefaultValue::Object { type_name, fields } => match ty {
                CftValueType::Object(expected)
                    if self.schema.is_assignable(type_name, expected) => {
                    let mut values = self.fields_for_type(type_name, materialization, None)?;
                    for (name, value) in fields {
                        let Some(field) = self.schema.field(type_name, name) else {
                            return self.zero_for_ty(ty, materialization);
                        };
                        let value = self.materialize_schema_default(
                            &field.value_type,
                            value,
                            materialization,
                        )?;
                        values.insert(name.clone(), value);
                    }
                    Ok(CfdValue::Object(Box::new(CfdObject::new(
                        type_name.clone(),
                        values,
                    ))))
                }
                _ => self.zero_for_ty(ty, materialization),
            },
            CftSchemaDefaultValue::RecordReference { type_name, key } => match ty {
                CftValueType::RecordRef(expected)
                    if self.schema.is_assignable(type_name, expected) => RecordKey::new(key)
                    .map(CfdValue::Ref)
                    .map_err(|error| one_mutation_error("MUTATION-DEFAULT", error.to_string())),
                _ => self.zero_for_ty(ty, materialization),
            },
        }
    }

    fn zero_for_ty(
        &mut self,
        ty: &CftValueType,
        materialization: DefaultMaterialization,
    ) -> Result<CfdValue, DiagnosticSet> {
        match ty {
            CftValueType::Int => Ok(CfdValue::Int(0)),
            CftValueType::Float => Ok(CfdValue::Float(0.0)),
            CftValueType::Bool => Ok(CfdValue::Bool(false)),
            CftValueType::String => Ok(CfdValue::String(String::new())),
            CftValueType::Option(_) => Ok(CfdValue::OptionNone),
            CftValueType::RecordRef(_) => Err(one_mutation_error(
                "MUTATION-DEFAULT",
                format!("no implicit editable default exists for `{ty}`"),
            )),
            CftValueType::Array(_) => Ok(CfdValue::Array(Vec::new())),
            CftValueType::Dict(_, _) => Ok(CfdValue::Dict(Vec::new())),
            CftValueType::Enum(name) => {
                let schema_enum = self.schema.resolve_enum(name);
                let value = schema_enum.and_then(|enm| enm.variants.first());
                let is_flag = schema_enum.is_some_and(|enm| enm.is_flag);
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
                            variant: (!is_flag).then(|| variant.name.clone()),
                            value: variant.value,
                        })
                    },
                ))
            }
            CftValueType::Object(name) => {
                let fields = self.fields_for_type(name, materialization, None)?;
                Ok(CfdValue::Object(Box::new(CfdObject::new(
                    name.clone(),
                    fields,
                ))))
            }
            CftValueType::Result(_, _) | CftValueType::Function(_, _) | CftValueType::Unit => {
                Err(one_mutation_error(
                    "MUTATION-DEFAULT",
                    format!("no implicit editable default exists for `{ty}`"),
                ))
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
    field: &CftField,
    err: Option<&DiagnosticSet>,
    value: Option<CfdValue>,
) -> CreateRecordFieldDraft {
    CreateRecordFieldDraft {
        name: field.name.to_string(),
        value,
        source: CreateFieldSource::RequiredInput,
        required: Some(required_input_for_field(schema, field, err)),
    }
}

fn required_input_for_field(
    schema: &CftSchema,
    field: &CftField,
    err: Option<&DiagnosticSet>,
) -> CreateRequiredInput {
    match &field.value_type {
        CftValueType::RecordRef(target_type) => CreateRequiredInput::Ref {
            target_type: target_type.to_string(),
        },
        CftValueType::Object(expected_type)
            if schema
                .resolve_type(expected_type)
                .is_some_and(|meta| meta.is_abstract) =>
        {
            CreateRequiredInput::AbstractObject {
                expected_type: expected_type.to_string(),
                concrete_types: schema
                    .concrete_assignable_types(expected_type)
                    .unwrap_or_default()
                    .into_iter()
                    .map(|name| name.to_string())
                    .collect(),
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

fn ensure_type_can_materialize(schema: &CftSchema, type_name: &str) -> Result<(), DiagnosticSet> {
    let Some(schema_type) = schema.resolve_type(type_name) else {
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

#[cfg(test)]
mod tests {
    use super::*;
    use coflow_language::cft::{build_schema, parse_modules, CftDimensionInputs, CftFile, ModuleId};

    fn schema(source: &str) -> CftSchema {
        let modules = parse_modules([CftFile::from_source(ModuleId::from("main"), source)]);
        build_schema(&modules, &CftDimensionInputs::default()).expect("schema")
    }

    #[test]
    fn editable_object_omits_fields_that_require_an_explicit_choice() {
        let schema = schema(
            "type Item {} abstract type Effect {} type Damage : Effect {} \
             type Reward { item: &Item; effect: Effect; count: int; }",
        );
        let object = default_object_for_type(
            &schema,
            "Reward",
            DefaultMaterialization::EditableShape,
        )
        .expect("editable object");

        assert!(object.field("item").is_none());
        assert!(object.field("effect").is_none());
        assert_eq!(object.field("count"), Some(&CfdValue::Int(0)));
    }

    #[test]
    fn record_draft_marks_required_reference_and_type_default_sources() {
        let schema = schema("type Item {} type Reward { item: &Item; count: int; }");
        let draft = create_record_draft_for_type(&schema, "Reward").expect("draft");
        let item = draft.fields.iter().find(|field| field.name == "item").expect("item");
        let count = draft.fields.iter().find(|field| field.name == "count").expect("count");

        assert!(item.value.is_none());
        assert!(matches!(item.required, Some(CreateRequiredInput::Ref { .. })));
        assert_eq!(count.source, CreateFieldSource::TypeDefault);
    }

    #[test]
    fn record_draft_prefers_schema_defaults_over_type_defaults() {
        let schema = schema("type Item { configured: int = 7; generated: int; }");
        let draft = create_record_draft_for_type(&schema, "Item").expect("draft");
        let configured = draft
            .fields
            .iter()
            .find(|field| field.name == "configured")
            .expect("configured");
        let generated = draft
            .fields
            .iter()
            .find(|field| field.name == "generated")
            .expect("generated");

        assert_eq!(configured.value, Some(CfdValue::Int(7)));
        assert_eq!(configured.source, CreateFieldSource::SchemaDefault);
        assert_eq!(generated.value, Some(CfdValue::Int(0)));
        assert_eq!(generated.source, CreateFieldSource::TypeDefault);
    }

    #[test]
    fn minimal_insert_fields_omit_required_references() {
        let schema = schema("type Item {} type Reward { item: &Item; count: int; }");
        let fields = default_missing_fields_for_type(
            &schema,
            "Reward",
            DefaultMaterialization::Minimal,
            &BTreeSet::new(),
        )
        .expect("minimal fields");

        assert!(!fields.contains_key("item"));
        assert_eq!(fields.get("count"), Some(&CfdValue::Int(0)));
    }
}
