//! 操作输入的类型限定随候选状态维护，最终连接时校验，允许同批前向引用。
use super::{Candidate, InputValue, ModelValue, RecordTarget, Result};
use crate::{CfdPath, CfdPathSegment as P, CfdValue, LoadedValueDraft as D};
use coflow_language::cft::CftSchema;

#[derive(Debug, Clone)]
pub(crate) struct TypedReference {
    owner: RecordTarget,
    dimension: Option<(String, String)>,
    pub path: Vec<P>,
    target_type: String,
    target_key: String,
}

fn same_owner(schema: &CftSchema, left: &RecordTarget, right: &RecordTarget) -> bool {
    left.key == right.key
        && schema.inheritance_root(&left.actual_type) == schema.inheritance_root(&right.actual_type)
}

impl Candidate {
    pub(crate) fn capture_typed_references(&mut self) -> Result<()> {
        let project = self.project()?;
        let session = crate::Runtime::open_schema_session(project.clone())?;
        let Some(schema) = session.schema() else {
            return Ok(());
        };
        for file in super::InputSnapshot::capture(&project)?
            .files
            .into_iter()
            .filter(|file| file.kind == "data")
        {
            let text = String::from_utf8(file.contents)
                .map_err(|error| super::Failure::request(error.to_string()))?;
            let ast = coflow_language::cfd::parse_cfd(&text).0;
            for (record, dimension) in ast.records.iter().map(|record| (record, None)).chain(
                ast.dimensions.iter().flat_map(|block| {
                    block.records.iter().map(move |record| {
                        (
                            record,
                            Some((block.dimension.clone(), block.variant.clone())),
                        )
                    })
                }),
            ) {
                let owner = RecordTarget {
                    actual_type: record.type_name.clone(),
                    key: record.key.clone(),
                };
                for field in &record.fields {
                    let Some(meta) = schema.field(&record.type_name, &field.name) else {
                        continue;
                    };
                    let span = super::query::ast_value_span(&field.value);
                    // 局部错误不能阻止修复操作；有效的类型限定仍参与最终候选校验。
                    if let Ok(value) = crate::cfd_loader::parse_expression(
                        schema,
                        &meta.value_type,
                        &text[span.start..span.end],
                    ) {
                        collect_draft(
                            schema,
                            &value,
                            &owner,
                            &dimension,
                            vec![P::Field(field.name.clone())],
                            &mut self.typed_references,
                        )?;
                        if let Ok(mut effective) = super::values::expression(
                            schema,
                            &meta.value_type,
                            &text[span.start..span.end],
                        ) {
                            if crate::mutation::defaults::complete_declared_defaults(
                                schema,
                                &mut effective,
                            )
                            .is_ok()
                            {
                                collect_nested_defaults(
                                    schema,
                                    Some(&value),
                                    &effective,
                                    &owner,
                                    &dimension,
                                    vec![P::Field(field.name.clone())],
                                    &mut self.typed_references,
                                );
                            }
                        }
                    }
                }
                if dimension.is_none() {
                    for meta in schema
                        .resolve_type(&record.type_name)
                        .into_iter()
                        .flat_map(|ty| ty.all_fields())
                        .filter(|meta| {
                            !record
                                .fields
                                .iter()
                                .any(|field| field.name == meta.name.as_str())
                        })
                    {
                        if let Ok(Some(value)) =
                            crate::mutation::defaults::declared_field_default(schema, meta)
                        {
                            collect_default(
                                schema,
                                meta.default.as_ref(),
                                &value,
                                &owner,
                                &dimension,
                                vec![P::Field(meta.name.to_string())],
                                &mut self.typed_references,
                            );
                        }
                    }
                }
            }
        }
        Ok(())
    }

    pub(crate) fn replace_typed_references(
        &mut self,
        schema: &CftSchema,
        owner: &RecordTarget,
        dimension: Option<(String, String)>,
        path: &[P],
        input: Option<&InputValue>,
        expected: &coflow_language::cft::CftValueType,
    ) -> Result<()> {
        self.typed_references.retain(|reference| {
            !(same_owner(schema, &reference.owner, owner)
                && reference.dimension == dimension
                && reference.path.starts_with(path))
        });
        if let Some(input) = input {
            match input {
                InputValue::Expression { source } => {
                    let draft = crate::cfd_loader::parse_expression(schema, expected, source)?;
                    collect_draft(
                        schema,
                        &draft,
                        owner,
                        &dimension,
                        path.to_vec(),
                        &mut self.typed_references,
                    )?;
                    let mut value = super::values::convert(schema, expected, draft.clone())?;
                    crate::mutation::defaults::complete_declared_defaults(schema, &mut value)?;
                    collect_nested_defaults(
                        schema,
                        Some(&draft),
                        &value,
                        owner,
                        &dimension,
                        path.to_vec(),
                        &mut self.typed_references,
                    );
                }
                InputValue::Model { value } => {
                    let mut resolved = value.resolve(schema, expected)?;
                    collect_model(
                        value,
                        &resolved,
                        owner,
                        &dimension,
                        path.to_vec(),
                        &mut self.typed_references,
                    );
                    let supplied = supplied_shape(&resolved);
                    crate::mutation::defaults::complete_declared_defaults(schema, &mut resolved)?;
                    collect_nested_defaults(
                        schema,
                        Some(&supplied),
                        &resolved,
                        owner,
                        &dimension,
                        path.to_vec(),
                        &mut self.typed_references,
                    );
                }
            }
        }
        Ok(())
    }

    pub(crate) fn rename_typed_references(
        &mut self,
        schema: &CftSchema,
        record: &RecordTarget,
        new_key: &str,
    ) {
        for reference in &mut self.typed_references {
            if same_owner(schema, &reference.owner, record) {
                reference.owner.key = new_key.into();
            }
            if reference.target_key == record.key
                && schema.is_assignable(&record.actual_type, &reference.target_type)
            {
                reference.target_key = new_key.into();
            }
        }
    }

    pub(crate) fn validate_typed_references(
        &self,
        session: &crate::ReadOnlyProjectSession,
    ) -> Vec<(crate::Diagnostic, crate::DiagnosticLogicalLocation)> {
        let mut diagnostics = Vec::new();
        for reference in &self.typed_references {
            let Some(owner) = session
                .model()
                .lookup_assignable(
                    session.schema(),
                    &reference.owner.actual_type,
                    &reference.owner.key,
                )
                .and_then(|id| session.model().record(id))
            else {
                continue;
            };
            let mut value = if let Some((dimension, variant)) = &reference.dimension {
                let Some(P::Field(field)) = reference.path.first() else {
                    continue;
                };
                owner
                    .dimension_field(field)
                    .filter(|values| values.dimension.as_str() == dimension)
                    .and_then(|values| values.variants.get(variant.as_str()))
                    .map(|value| &value.value)
                    .and_then(|value| nested_value(value, &reference.path[1..]))
            } else {
                owner.value_at_path(&CfdPath {
                    segments: reference.path.clone(),
                })
            };
            while let Some(CfdValue::OptionSome(inner)) = value {
                value = Some(inner);
            }
            if !matches!(value, Some(CfdValue::Ref(key)) if key.as_str() == reference.target_key) {
                continue;
            }
            if session
                .model()
                .lookup_assignable(
                    session.schema(),
                    &reference.target_type,
                    &reference.target_key,
                )
                .is_none()
            {
                let mut diagnostic = crate::Diagnostic::error(
                    "REF-001",
                    "REF",
                    format!(
                        "typed reference `&{}::{}` has no assignable target",
                        reference.target_type, reference.target_key
                    ),
                );
                if let Some(location) = owner.origin.location_for_path(&CfdPath {
                    segments: reference.path.clone(),
                }) {
                    diagnostic = diagnostic.with_primary(crate::Label {
                        location: location.into(),
                        message: None,
                    });
                }
                diagnostics.push((
                    diagnostic,
                    crate::DiagnosticLogicalLocation {
                        actual_type: Some(owner.actual_type().into()),
                        record_key: Some(owner.key().into()),
                        field_path: Some(reference.path.iter().fold(
                            String::new(),
                            |mut path, segment| {
                                match segment {
                                    P::Field(name) => {
                                        if !path.is_empty() {
                                            path.push('.');
                                        }
                                        path.push_str(name);
                                    }
                                    P::Index(index) => path.push_str(&format!("[{index}]")),
                                    P::DictKey(key) => path.push_str(&format!("[{key}]")),
                                }
                                path
                            },
                        )),
                    },
                ));
            }
        }
        diagnostics
    }

    pub(crate) fn collection_typed_references(
        &mut self,
        schema: &CftSchema,
        owner: &RecordTarget,
        path: &[P],
        operation: &super::Operation,
    ) -> Vec<TypedReference> {
        let mut retained = Vec::new();
        for reference in &self.typed_references {
            if !same_owner(schema, &reference.owner, owner)
                || reference.dimension.is_some()
                || !reference.path.starts_with(path)
            {
                continue;
            }
            let mut reference = reference.clone();
            if let Some(P::Index(index)) = reference.path.get_mut(path.len()) {
                match operation {
                    super::Operation::ArrayInsert {
                        index: inserted, ..
                    } if *index >= *inserted => *index += 1,
                    super::Operation::ArrayRemove { index: removed, .. } => {
                        if *index == *removed {
                            continue;
                        }
                        if *index > *removed {
                            *index -= 1;
                        }
                    }
                    super::Operation::ArrayMove { from, to, .. } => {
                        if *index == *from {
                            *index = *to;
                        } else if from < to && *index > *from && *index <= *to {
                            *index -= 1;
                        } else if to < from && *index >= *to && *index < *from {
                            *index += 1;
                        }
                    }
                    _ => {}
                }
            }
            retained.push(reference);
        }
        retained
    }
}

fn nested_value<'a>(value: &'a CfdValue, path: &[P]) -> Option<&'a CfdValue> {
    if path.is_empty() {
        return Some(value);
    }
    if let CfdValue::OptionSome(value) = value {
        return nested_value(value, path);
    }
    let next = match (&path[0], value) {
        (P::Field(name), CfdValue::Object(value)) => value.field(name),
        (P::Index(index), CfdValue::Array(values)) => values.get(*index),
        (P::DictKey(key), CfdValue::Dict(values)) => values
            .iter()
            .find(|(candidate, _)| crate::data_model::format_cfd_dict_key(candidate) == *key)
            .map(|(_, value)| value),
        _ => None,
    }?;
    nested_value(next, &path[1..])
}

fn child(path: &[P], segment: P) -> Vec<P> {
    let mut path = path.to_vec();
    path.push(segment);
    path
}

fn collect_draft(
    schema: &CftSchema,
    value: &D,
    owner: &RecordTarget,
    dimension: &Option<(String, String)>,
    path: Vec<P>,
    output: &mut Vec<TypedReference>,
) -> Result<()> {
    match value {
        D::TypedRecordRef { type_name, key } => output.push(TypedReference {
            owner: owner.clone(),
            dimension: dimension.clone(),
            path,
            target_type: type_name.clone(),
            target_key: key.clone(),
        }),
        D::OptionSome(value) => collect_draft(schema, value, owner, dimension, path, output)?,
        D::Object { fields, .. } => {
            for (field, value) in fields {
                collect_draft(
                    schema,
                    value,
                    owner,
                    dimension,
                    child(&path, P::Field(field.clone())),
                    output,
                )?;
            }
        }
        D::Array(values) => {
            for (index, value) in values.iter().enumerate() {
                collect_draft(
                    schema,
                    value,
                    owner,
                    dimension,
                    child(&path, P::Index(index)),
                    output,
                )?;
            }
        }
        D::Dict(values) => {
            for (key, value) in values {
                let key = super::values::dict_key(schema, key.clone())?;
                collect_draft(
                    schema,
                    value,
                    owner,
                    dimension,
                    child(
                        &path,
                        P::DictKey(crate::data_model::format_cfd_dict_key(&key)),
                    ),
                    output,
                )?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn collect_model(
    input: &ModelValue,
    value: &CfdValue,
    owner: &RecordTarget,
    dimension: &Option<(String, String)>,
    path: Vec<P>,
    output: &mut Vec<TypedReference>,
) {
    match (input, value) {
        (ModelValue::Ref { type_name, key }, CfdValue::Ref(_)) => output.push(TypedReference {
            owner: owner.clone(),
            dimension: dimension.clone(),
            path,
            target_type: type_name.clone(),
            target_key: key.clone(),
        }),
        (ModelValue::Some { value: input }, CfdValue::OptionSome(value)) => {
            collect_model(input, value, owner, dimension, path, output)
        }
        (ModelValue::Object { fields, .. }, CfdValue::Object(value)) => {
            for (name, input) in fields {
                if let Some(value) = value.field(name) {
                    collect_model(
                        input,
                        value,
                        owner,
                        dimension,
                        child(&path, P::Field(name.clone())),
                        output,
                    );
                }
            }
        }
        (ModelValue::Array { items }, CfdValue::Array(values)) => {
            for (index, (input, value)) in items.iter().zip(values).enumerate() {
                collect_model(
                    input,
                    value,
                    owner,
                    dimension,
                    child(&path, P::Index(index)),
                    output,
                );
            }
        }
        (ModelValue::Dict { entries }, CfdValue::Dict(values)) => {
            for (input, (key, value)) in entries.iter().zip(values) {
                collect_model(
                    &input.value,
                    value,
                    owner,
                    dimension,
                    child(
                        &path,
                        P::DictKey(crate::data_model::format_cfd_dict_key(key)),
                    ),
                    output,
                );
            }
        }
        _ => {}
    }
}

/// 原来由声明提供的引用在默认对象物化后仍保留其类型限制。
fn collect_default(
    schema: &CftSchema,
    default: Option<&coflow_language::cft::CftSchemaDefaultValue>,
    value: &CfdValue,
    owner: &RecordTarget,
    dimension: &Option<(String, String)>,
    path: Vec<P>,
    output: &mut Vec<TypedReference>,
) {
    use coflow_language::cft::CftSchemaDefaultValue as S;
    match (default, value) {
        (Some(S::RecordReference { type_name, key }), CfdValue::Ref(_)) => {
            output.push(TypedReference {
                owner: owner.clone(),
                dimension: dimension.clone(),
                path,
                target_type: type_name.to_string(),
                target_key: key.clone(),
            })
        }
        (default, CfdValue::OptionSome(value)) => collect_default(
            schema,
            default.and_then(|default| {
                if let S::OptionSome(value) = default {
                    Some(value.as_ref())
                } else {
                    None
                }
            }),
            value,
            owner,
            dimension,
            path,
            output,
        ),
        (default, CfdValue::Object(object)) => {
            for (name, value) in &object.fields {
                let explicit = default.and_then(|default| {
                    if let S::Object { fields, .. } = default {
                        fields
                            .iter()
                            .find(|(field, _)| field == name)
                            .map(|(_, value)| value)
                    } else {
                        None
                    }
                });
                let declared = explicit.or_else(|| {
                    schema
                        .field(object.actual_type(), name)
                        .and_then(|field| field.default.as_ref())
                });
                collect_default(
                    schema,
                    declared,
                    value,
                    owner,
                    dimension,
                    child(&path, P::Field(name.to_string())),
                    output,
                );
            }
        }
        (default, CfdValue::Array(values)) => {
            for (index, value) in values.iter().enumerate() {
                let declared = default.and_then(|default| {
                    if let S::Array(values) = default {
                        values.get(index)
                    } else {
                        None
                    }
                });
                collect_default(
                    schema,
                    declared,
                    value,
                    owner,
                    dimension,
                    child(&path, P::Index(index)),
                    output,
                );
            }
        }
        (default, CfdValue::Dict(values)) => {
            for (index, (key, value)) in values.iter().enumerate() {
                let declared = default.and_then(|default| {
                    if let S::Dictionary(values) = default {
                        values.get(index).map(|(_, value)| value)
                    } else {
                        None
                    }
                });
                collect_default(
                    schema,
                    declared,
                    value,
                    owner,
                    dimension,
                    child(
                        &path,
                        P::DictKey(crate::data_model::format_cfd_dict_key(key)),
                    ),
                    output,
                );
            }
        }
        _ => {}
    }
}

fn collect_nested_defaults(
    schema: &CftSchema,
    input: Option<&D>,
    value: &CfdValue,
    owner: &RecordTarget,
    dimension: &Option<(String, String)>,
    path: Vec<P>,
    output: &mut Vec<TypedReference>,
) {
    match value {
        CfdValue::OptionSome(value) => collect_nested_defaults(
            schema,
            input.map(|input| {
                if let D::OptionSome(value) = input {
                    value.as_ref()
                } else {
                    input
                }
            }),
            value,
            owner,
            dimension,
            path,
            output,
        ),
        CfdValue::Object(object) => {
            for (name, value) in &object.fields {
                let input = input.and_then(|input| {
                    if let D::Object { fields, .. } = input {
                        fields.get(name.as_str())
                    } else {
                        None
                    }
                });
                let next = child(&path, P::Field(name.to_string()));
                if input.is_some() {
                    collect_nested_defaults(schema, input, value, owner, dimension, next, output);
                } else {
                    collect_default(
                        schema,
                        schema
                            .field(object.actual_type(), name)
                            .and_then(|field| field.default.as_ref()),
                        value,
                        owner,
                        dimension,
                        next,
                        output,
                    );
                }
            }
        }
        CfdValue::Array(values) => {
            for (index, value) in values.iter().enumerate() {
                let input = input.and_then(|input| {
                    if let D::Array(values) = input {
                        values.get(index)
                    } else {
                        None
                    }
                });
                collect_nested_defaults(
                    schema,
                    input,
                    value,
                    owner,
                    dimension,
                    child(&path, P::Index(index)),
                    output,
                );
            }
        }
        CfdValue::Dict(values) => {
            for (index, (key, value)) in values.iter().enumerate() {
                let input = input.and_then(|input| {
                    if let D::Dict(values) = input {
                        values.get(index).map(|(_, value)| value)
                    } else {
                        None
                    }
                });
                collect_nested_defaults(
                    schema,
                    input,
                    value,
                    owner,
                    dimension,
                    child(
                        &path,
                        P::DictKey(crate::data_model::format_cfd_dict_key(key)),
                    ),
                    output,
                );
            }
        }
        _ => {}
    }
}

// 形状只标记调用方实际提供的成员；缺失成员的来源仍是声明默认值。
fn supplied_shape(value: &CfdValue) -> D {
    match value {
        CfdValue::OptionSome(value) => D::OptionSome(Box::new(supplied_shape(value))),
        CfdValue::Object(object) => D::Object {
            actual_type: Some(object.actual_type().into()),
            fields: object
                .fields
                .iter()
                .map(|(name, value)| (name.to_string(), supplied_shape(value)))
                .collect(),
        },
        CfdValue::Array(values) => D::Array(values.iter().map(supplied_shape).collect()),
        CfdValue::Dict(values) => D::Dict(
            values
                .iter()
                .map(|(_, value)| (crate::LoadedDictKeyDraft::Int(0), supplied_shape(value)))
                .collect(),
        ),
        _ => D::Bool(false),
    }
}
