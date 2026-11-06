use std::collections::{BTreeMap, BTreeSet};

use crate::api::DiagnosticSet;
use crate::data_model::{CfdEnumValue, CfdObject, CfdValue};
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
    if !schema
        .resolve_type(type_name)
        .is_some_and(coflow_language::cft::CftType::is_data)
    {
        return Err(one_mutation_error(
            "MUTATION-TYPE",
            "inline object must be data",
        ));
    }
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

/// 语义修改只补齐声明默认值，不把编辑器形状初值写成业务数据。
pub(crate) fn declared_field_default(
    schema: &CftSchema,
    field: &CftField,
) -> Result<Option<CfdValue>, DiagnosticSet> {
    let mut materializer = DefaultValueMaterializer::new(schema);
    materializer.schema_only = true;
    field.default.as_ref().map_or(Ok(None), |default| {
        materializer
            .materialize_schema_default(&field.value_type, default)
            .map(Some)
    })
}

pub(crate) fn complete_declared_defaults(
    schema: &CftSchema,
    value: &mut CfdValue,
) -> Result<(), DiagnosticSet> {
    let mut materializer = DefaultValueMaterializer::new(schema);
    materializer.schema_only = true;
    materializer.complete_value_fields(value)
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

pub(crate) fn create_record_draft_for_type(
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
    if schema_type.is_data() {
        return Err(one_mutation_error(
            "MUTATION-TYPE",
            "record draft requires table or singleton",
        ));
    }
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
    schema_only: bool,
}

impl<'a> DefaultValueMaterializer<'a> {
    const fn new(schema: &'a CftSchema) -> Self {
        Self {
            schema,
            memo: BTreeMap::new(),
            schema_only: false,
        }
    }

    fn fields_for_type(
        &mut self,
        type_name: &str,
        materialization: DefaultMaterialization,
        skip_fields: Option<&BTreeSet<String>>,
    ) -> Result<BTreeMap<FieldName, CfdValue>, DiagnosticSet> {
        ensure_type_can_materialize(self.schema, type_name)?;
        let mode = if self.schema_only {
            ValueDependencyMode::SchemaDefaults
        } else {
            dependency_mode(materialization)
        };
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
            let value = if self.schema_only {
                match &field.default {
                    Some(default) => {
                        Some(self.materialize_schema_default(&field.value_type, default)?)
                    }
                    None => None,
                }
            } else {
                match materialization {
                    DefaultMaterialization::Minimal => self.minimal_for_field(field)?,
                    DefaultMaterialization::EditableShape => self.editable_for_field(field)?,
                }
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

    fn editable_for_field(&mut self, field: &CftField) -> Result<Option<CfdValue>, DiagnosticSet> {
        if let Some(default) = field.default.as_ref() {
            return self
                .materialize_schema_default(&field.value_type, default)
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
            return match self.materialize_schema_default(&field.value_type, default) {
                Ok(value) => CreateRecordFieldDraft {
                    name: field.name.to_string(),
                    value: Some(value),
                    source: CreateFieldSource::SchemaDefault,
                    required: None,
                },
                Err(err) => required_field_draft(self.schema, field, Some(&err), None),
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
    ) -> Result<CfdValue, DiagnosticSet> {
        let loaded = coflow_model::lower_schema_default(self.schema, ty, default)
            .map_err(|message| one_mutation_error("MUTATION-DEFAULT", message))?;
        let mut value = crate::value_drafts::convert_default(self.schema, ty, loaded)
            .map_err(|message| one_mutation_error("MUTATION-DEFAULT", message))?;
        // 显式字段先进入值树，再按调用方策略补齐，避免展开被 None 等值终止的默认递归。
        self.complete_value_fields(&mut value)?;
        Ok(value)
    }

    fn complete_value_fields(&mut self, value: &mut CfdValue) -> Result<(), DiagnosticSet> {
        match value {
            CfdValue::Object(object) => {
                // 已提供的子对象也沿用当前策略；生成的新字段已递归完成，无需重复遍历。
                for value in object.fields.values_mut() {
                    self.complete_value_fields(value)?;
                }
                let provided = object.fields.keys().map(ToString::to_string).collect();
                let defaults = self.fields_for_type(
                    object.actual_type(),
                    DefaultMaterialization::EditableShape,
                    Some(&provided),
                )?;
                object.fields.extend(defaults);
            }
            CfdValue::OptionSome(value) => self.complete_value_fields(value)?,
            CfdValue::Array(values) => {
                for value in values {
                    self.complete_value_fields(value)?;
                }
            }
            CfdValue::Dict(values) => {
                for (_, value) in values {
                    self.complete_value_fields(value)?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn zero_for_ty(
        &mut self,
        ty: &CftValueType,
        materialization: DefaultMaterialization,
    ) -> Result<CfdValue, DiagnosticSet> {
        match ty {
            CftValueType::Int | CftValueType::Long => Ok(CfdValue::Int(0)),
            CftValueType::Float | CftValueType::Double => Ok(CfdValue::Float(0.0)),
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
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::panic, reason = "回归测试在值结构不符时立即失败。")]
    #![allow(clippy::expect_used)]

    use super::*;
    use coflow_language::cft::{build_schema, parse_modules, CftFile, ModuleId};

    fn schema(source: &str) -> CftSchema {
        let modules = parse_modules([CftFile::from_source(ModuleId::from("main"), source)]);
        build_schema(&modules).expect("schema")
    }

    #[test]
    fn recursive_declared_defaults_respect_explicit_overrides_in_every_creation_path() {
        let schema = schema(
            "data Node { child: Option<Node> = Some(Node { child: None }); label: string = \"leaf\"; } \
             table Holder { root: Node = {}; }",
        );
        let editable =
            default_object_for_type(&schema, "Node", DefaultMaterialization::EditableShape)
                .expect("finite editable default");
        let Some(CfdValue::OptionSome(child)) = editable.field("child") else {
            panic!("explicit child");
        };
        let CfdValue::Object(child) = child.as_ref() else {
            panic!("child object");
        };
        assert_eq!(child.field("child"), Some(&CfdValue::OptionNone));
        assert_eq!(child.field("label"), Some(&CfdValue::String("leaf".into())));

        let expected = CfdValue::Object(Box::new(editable));
        let field = schema.field("Holder", "root").expect("root field");
        assert_eq!(
            declared_field_default(&schema, field).expect("declared default"),
            Some(expected.clone())
        );
        let draft = create_record_draft_for_type(&schema, "Holder").expect("record draft");
        assert_eq!(draft.fields[0].value, Some(expected.clone()));

        let mut builder = crate::CfdDataModel::builder(&schema);
        builder.add_record(
            "holder",
            "Holder",
            std::iter::empty::<(&str, crate::LoadedValueDraft)>(),
        );
        let model = builder.build().expect("model defaults");
        let holder = model.records().next().expect("holder").1;
        assert_eq!(holder.field("root"), Some(&expected));

        let minimal = default_object_for_type(&schema, "Node", DefaultMaterialization::Minimal)
            .expect("minimal draft");
        assert!(minimal.fields.is_empty());
    }

    #[test]
    fn declared_object_drafts_keep_editable_initial_values_out_of_business_defaults() {
        fn child(value: &CfdValue) -> &CfdObject {
            match value {
                CfdValue::Object(value) => value,
                CfdValue::OptionSome(value) => child(value),
                CfdValue::Array(values) => child(&values[0]),
                CfdValue::Dict(values) => child(&values[0].1),
                _ => panic!("child object"),
            }
        }
        let schema = schema(
            r#"
            data Child { required: int; optional: int = 2; }
            table Holder {
                empty: Child = {};
                explicit: Child = Child { optional: 3 };
                option: Option<Child> = Some(Child { optional: 3 });
                array: [Child] = [Child { optional: 3 }];
                dict: {string: Child} = { "x": Child { optional: 3 } };
            }
        "#,
        );

        let draft = create_record_draft_for_type(&schema, "Holder").expect("record draft");
        for field in &draft.fields {
            let editable = child(field.value.as_ref().expect("editable value"));
            assert_eq!(field.source, CreateFieldSource::SchemaDefault);
            assert!(field.required.is_none());
            assert_eq!(editable.field("required"), Some(&CfdValue::Int(0)));
            let optional = if field.name == "empty" { 2 } else { 3 };
            assert_eq!(editable.field("optional"), Some(&CfdValue::Int(optional)));

            let schema_field = schema.field("Holder", &field.name).expect("schema field");
            let mut declared = declared_field_default(&schema, schema_field)
                .expect("declared default")
                .expect("default value");
            assert!(child(&declared).field("required").is_none());
            assert_eq!(
                child(&declared).field("optional"),
                Some(&CfdValue::Int(optional))
            );
            complete_declared_defaults(&schema, &mut declared).expect("declared completion");
            assert!(child(&declared).field("required").is_none());
        }

        // 业务加载仍报告省略的必填字段；草稿中的 0 仅服务于创建交互。
        let mut builder = crate::CfdDataModel::builder(&schema);
        builder.add_record(
            "holder",
            "Holder",
            std::iter::empty::<(&str, crate::LoadedValueDraft)>(),
        );
        let diagnostics = builder
            .build()
            .expect_err("missing required default fields");
        assert!(diagnostics
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code == crate::CfdErrorCode::MissingRequiredField));
        for field in &draft.fields {
            assert!(diagnostics.diagnostics.iter().any(|diagnostic| {
                matches!(
                    diagnostic.primary.as_ref().and_then(|label| label.path.segments.first()),
                    Some(crate::CfdPathSegment::Field(name)) if name.as_str() == field.name
                )
            }));
        }
    }

    #[test]
    fn composite_declared_defaults_match_model_materialization() {
        let schema = schema(
            r#"
            @flag enum Permission { Read = 1, Write = 2, Execute = 4 }
            table Item {}
            data Child { value: int = 1; }
            data Bundle {
                number: int = 1;
                wide: long = 2147483648;
                scale: float = 0.1;
                valid: bool = true;
                label: string = "ok";
                optional: Option<int> = Some(2);
                absent: Option<int> = None;
                items: [&Item] = [&Item::item];
                empty: [int] = [];
                index: {string: int} = { "one": 1 };
                flags: Permission = Permission::Read | Permission::Write;
                keyed: {Permission: int} = { Permission::Read: 3, Permission::Read | Permission::Write: 7 };
                synthetic_keys: {Permission: int} = { Permission::None: 0, Permission::Everything: 9 };
                empty_flags: Permission = Permission::None;
                full_flags: Permission = Permission::Everything;
                child: Child = Child { value: 5 };
                blank: Child = {};
            }
            table Holder { bundle: Bundle = {}; }
        "#,
        );
        let editable =
            default_object_for_type(&schema, "Bundle", DefaultMaterialization::EditableShape)
                .expect("composite editable defaults");
        let mut builder = crate::CfdDataModel::builder(&schema);
        builder.add_record(
            "item",
            "Item",
            std::iter::empty::<(&str, crate::LoadedValueDraft)>(),
        );
        builder.add_record(
            "holder",
            "Holder",
            std::iter::empty::<(&str, crate::LoadedValueDraft)>(),
        );
        let model = builder.build().expect("composite model defaults");
        let holder = model
            .record_by_type_id("Holder", "holder")
            .and_then(|handle| model.record(handle))
            .expect("holder");
        assert_eq!(
            holder.field("bundle"),
            Some(&CfdValue::Object(Box::new(editable)))
        );
    }

    #[test]
    fn declared_template_drafts_preserve_source_previews_until_model_evaluation() {
        fn assert_source_preview(value: &CfdValue) -> usize {
            match value {
                CfdValue::FormattedString(value) => {
                    assert_eq!(value.rendered, value.source);
                    assert!(value.source.starts_with("f\""));
                    1
                }
                CfdValue::OptionSome(value) => assert_source_preview(value),
                CfdValue::Object(value) => value.fields.values().map(assert_source_preview).sum(),
                CfdValue::Array(values) => values.iter().map(assert_source_preview).sum(),
                CfdValue::Dict(values) => values
                    .iter()
                    .map(|(_, value)| assert_source_preview(value))
                    .sum(),
                _ => 0,
            }
        }
        let schema = schema(
            r#"
            data Child { id: int = 3; label: string = f"child {self.id}"; }
            table Item {
                label: string = f"item {self.id}";
                child: Child = {};
                optional: Option<string> = Some(f"optional {self.id}");
                texts: [string] = [f"list {self.id}"];
                lookup: {string: string} = { "x": f"dict {self.id}" };
            }
        "#,
        );

        let draft = create_record_draft_for_type(&schema, "Item").expect("template draft");
        assert_eq!(
            draft
                .fields
                .iter()
                .filter_map(|field| field.value.as_ref())
                .map(assert_source_preview)
                .sum::<usize>(),
            5
        );
        let field = schema.field("Item", "label").expect("label");
        let loaded = coflow_model::lower_schema_default(
            &schema,
            &field.value_type,
            field.default.as_ref().expect("default"),
        )
        .expect("lower template");
        assert!(
            matches!(crate::value_drafts::convert(&schema, &field.value_type, loaded).expect("ordinary input"),
            CfdValue::FormattedString(value) if value.source == "f\"item {self.id}\"" && value.rendered.is_empty())
        );

        let mut builder = crate::CfdDataModel::builder(&schema);
        builder.add_record(
            "one",
            "Item",
            std::iter::empty::<(&str, crate::LoadedValueDraft)>(),
        );
        let model = builder.build().expect("evaluate templates");
        let item = model.records().next().expect("item").1;
        assert!(
            matches!(item.field("label"), Some(CfdValue::FormattedString(value)) if value.rendered == "item one")
        );
        let Some(CfdValue::Object(child)) = item.field("child") else {
            panic!("child");
        };
        assert!(
            matches!(child.field("label"), Some(CfdValue::FormattedString(value)) if value.rendered == "child 3")
        );
    }

    #[test]
    fn editable_object_omits_fields_that_require_an_explicit_choice() {
        let schema = schema(
            "table Item {} abstract data Effect {} data Damage : Effect {} \
             data Reward { item: &Item; effect: Effect; count: int; }",
        );
        let object =
            default_object_for_type(&schema, "Reward", DefaultMaterialization::EditableShape)
                .expect("editable object");

        assert!(object.field("item").is_none());
        assert!(object.field("effect").is_none());
        assert_eq!(object.field("count"), Some(&CfdValue::Int(0)));
    }

    #[test]
    fn record_draft_marks_required_reference_and_type_default_sources() {
        let schema = schema("table Item {} table Reward { item: &Item; count: int; }");
        let draft = create_record_draft_for_type(&schema, "Reward").expect("draft");
        let item = draft
            .fields
            .iter()
            .find(|field| field.name == "item")
            .expect("item");
        let count = draft
            .fields
            .iter()
            .find(|field| field.name == "count")
            .expect("count");

        assert!(item.value.is_none());
        assert!(matches!(
            item.required,
            Some(CreateRequiredInput::Ref { .. })
        ));
        assert_eq!(count.source, CreateFieldSource::TypeDefault);
    }

    #[test]
    fn record_draft_prefers_schema_defaults_over_type_defaults() {
        let schema = schema("table Item { configured: int = 7; generated: int; }");
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
        let schema = schema("table Item {} table Reward { item: &Item; count: int; }");
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
