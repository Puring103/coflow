use super::{
    query::{paginate, selected_records},
    Arguments, Failure, Result,
};
use crate::{CfdRecord, ReadOnlyProjectSession};
use serde_json::Value;

#[derive(Debug, serde::Serialize, schemars::JsonSchema)]
pub struct DimensionField {
    pub source_type: String,
    pub source_field: String,
    pub is_singleton: bool,
}

#[derive(Debug, serde::Serialize, schemars::JsonSchema)]
pub struct DimensionMetadata {
    pub name: String,
    pub display_name: String,
    pub variants: Vec<String>,
    pub dir: Option<String>,
    pub files: Vec<String>,
    pub fields: Vec<DimensionField>,
}

impl From<crate::DimensionInfo> for DimensionMetadata {
    fn from(value: crate::DimensionInfo) -> Self {
        Self {
            name: value.name,
            display_name: value.display_name,
            variants: value.variants,
            dir: value.dir,
            files: value.files,
            fields: value
                .fields
                .into_iter()
                .map(|field| DimensionField {
                    source_type: field.source_type,
                    source_field: field.source_field,
                    is_singleton: field.is_singleton,
                })
                .collect(),
        }
    }
}

#[derive(Debug, serde::Serialize, schemars::JsonSchema)]
pub struct DimensionVariant {
    pub name: String,
    pub overrides: usize,
    pub locations: Vec<Option<super::diagnostics::CommandLocation>>,
}

#[derive(Debug, serde::Serialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DimensionOrigin {
    Base,
    DimensionOverride,
}

#[derive(Debug, serde::Serialize, schemars::JsonSchema)]
pub struct DimensionState {
    pub explicit: bool,
    pub effective: Option<super::ModelValue>,
    pub rendered: Option<Value>,
    pub origin: DimensionOrigin,
}

#[derive(Debug, serde::Serialize, schemars::JsonSchema)]
pub struct DimensionValue {
    #[serde(rename = "type")]
    pub actual_type: String,
    pub id: String,
    pub field: String,
    pub dimension: String,
    pub variant: String,
    pub base: Option<super::ModelValue>,
    #[serde(rename = "override")]
    pub override_value: Option<super::ModelValue>,
    #[serde(flatten)]
    pub state: DimensionState,
    pub location: Option<super::diagnostics::CommandLocation>,
}

#[derive(Debug, serde::Serialize, schemars::JsonSchema)]
#[serde(untagged)]
pub enum DimensionView {
    Variant(Box<DimensionValue>),
    Base(DimensionState),
}

impl DimensionView {
    const fn state(&self) -> &DimensionState {
        match self {
            Self::Variant(value) => &value.state,
            Self::Base(state) => state,
        }
    }
}

#[derive(Debug, serde::Serialize, schemars::JsonSchema)]
pub struct DimensionGet {
    #[serde(flatten)]
    pub value: DimensionValue,
    pub variant_exists: bool,
}

#[derive(Debug, serde::Serialize, schemars::JsonSchema)]
pub struct DimensionMissing {
    #[serde(flatten)]
    pub value: DimensionValue,
    pub label: Option<String>,
    pub description: Option<String>,
    pub variants: Vec<DimensionValue>,
}

#[derive(Debug, serde::Serialize, schemars::JsonSchema)]
pub struct DimensionComparison {
    #[serde(rename = "type")]
    pub actual_type: String,
    pub id: String,
    pub field: String,
    pub before: DimensionView,
    pub after: DimensionView,
    pub coverage_changed: bool,
    pub value_changed: bool,
    pub rendered_changed: bool,
}

pub(super) fn list(session: &ReadOnlyProjectSession) -> Vec<DimensionMetadata> {
    session
        .queries()
        .dimensions()
        .into_iter()
        .map(DimensionMetadata::from)
        .collect()
}

fn field_value(
    session: &ReadOnlyProjectSession,
    record: &CfdRecord,
    field: &str,
    dimension: &str,
    variant: &str,
    projected: &crate::CfdDataModel,
) -> Result<DimensionValue> {
    let base = record.field(field);
    let explicit = record
        .dimension_field(field)
        .filter(|f| f.dimension.as_str() == dimension)
        .and_then(|f| f.variants.get(variant));
    let effective = projected
        .record_by_type_id(record.actual_type(), record.id())
        .and_then(|id| projected.record(id))
        .and_then(|value| value.field(field));
    let expected = &session
        .schema()
        .field(record.actual_type(), field)
        .ok_or_else(|| Failure::missing("unknown dimension field"))?
        .value_type;
    // 覆盖位置使用公共定位契约，不向调用方暴露模型的 RecordOrigin 枚举。
    let location = explicit
        .and_then(|value| {
            value
                .origin
                .location_for_path(&crate::CfdPath::root().field(field))
        })
        .map(|source| {
            super::diagnostics::CommandLocation::from_source(
                &source.into(),
                Some(session.session.project.root_dir()),
            )
        });
    let encode = |value: Option<&crate::CfdValue>| {
        value
            .map(|value| super::ModelValue::from_model(session.schema(), expected, value))
            .transpose()
    };
    Ok(DimensionValue {
        actual_type: record.actual_type().to_string(),
        id: record.id().to_string(),
        field: field.into(),
        dimension: dimension.into(),
        variant: variant.into(),
        base: encode(base)?,
        override_value: encode(explicit.map(|value| value.value.as_ref()))?,
        state: DimensionState {
            explicit: explicit.is_some(),
            effective: encode(effective)?,
            rendered: effective.and_then(super::model_value::rendered),
            origin: if explicit.is_some() {
                DimensionOrigin::DimensionOverride
            } else {
                DimensionOrigin::Base
            },
        },
        location,
    })
}

#[expect(
    clippy::too_many_lines,
    reason = "按语言或命令分支集中处理，保持分派与各分支语义可直接核对。"
)]
pub fn query(command: &str, session: &ReadOnlyProjectSession, args: &Arguments) -> Result<Value> {
    let name = args.require("dimension")?;
    let dimension = session
        .queries()
        .dimensions()
        .into_iter()
        .find(|d| d.name == name)
        .ok_or_else(|| Failure::missing(format!("unknown dimension `{name}`")))?;
    if command == "dimension.inspect" {
        return super::json(DimensionMetadata::from(dimension));
    }
    if command == "dimension.variants" {
        let mut declarations = std::collections::BTreeMap::<
            String,
            Vec<Option<super::diagnostics::CommandLocation>>,
        >::new();
        // 空覆盖块也是变体声明；声明位置来自源码块，不以字段覆盖位置替代。
        for (file, source) in session.session.source_data.sources() {
            let ast = coflow_language::cfd::parse_cfd(source).0;
            for block in ast
                .dimensions
                .iter()
                .filter(|block| block.dimension == name)
            {
                declarations
                    .entry(block.variant.clone())
                    .or_default()
                    .push(Some(super::schema_queries::location(
                        session.session.project.root_dir(),
                        &session
                            .session
                            .project
                            .resolve_path(std::path::Path::new(file)),
                        source,
                        block.span,
                    )));
            }
        }
        let items: Vec<_> = dimension
            .variants
            .iter()
            .map(|variant| {
                let mut count = 0;
                for (_, record) in session.model().records() {
                    for values in record
                        .dimension_fields
                        .values()
                        .filter(|v| v.dimension.as_str() == name)
                    {
                        if values.variants.contains_key(variant.as_str()) {
                            count += 1;
                        }
                    }
                }
                DimensionVariant {
                    name: variant.clone(),
                    overrides: count,
                    locations: declarations
                        .get(variant.as_str())
                        .cloned()
                        .unwrap_or_default(),
                }
            })
            .collect();
        return super::json(paginate(items, &args.selector()?)?);
    }
    let variant = if command == "dimension.diff" {
        comparison_variant(args, "to")?
    } else {
        Some(args.require("variant")?)
    };
    let from = if command == "dimension.diff" {
        comparison_variant(args, "from")?
    } else {
        None
    };
    let mut projections = std::collections::BTreeMap::new();
    let required_variants: Vec<_> = if command == "dimension.missing" {
        dimension
            .variants
            .iter()
            .map(String::as_str)
            .chain(variant)
            .collect()
    } else {
        variant.into_iter().chain(from).collect()
    };
    for variant in required_variants {
        if !projections.contains_key(variant) {
            projections.insert(
                variant.to_string(),
                super::query::project_dimensions(
                    session,
                    &std::collections::BTreeMap::from([(name.to_string(), variant.to_string())]),
                )?,
            );
        }
    }
    let selector = args.selector()?;
    if let Some(field) = args.get("field") {
        validate_bound_field(session, &selector, name, field)?;
    }
    let records = selected_records(session, &selector)?;
    let mut items = Vec::new();
    let mut comparisons = Vec::new();
    for record in &records {
        let ty = session
            .schema()
            .resolve_type(record.actual_type())
            .ok_or_else(|| Failure::missing("record type is unavailable"))?;
        for field in ty.all_fields().filter(|f| {
            f.dimension
                .as_ref()
                .is_some_and(|d| d.dimension.as_str() == name)
        }) {
            if args.get("field").is_some_and(|f| f != field.name.as_str()) {
                continue;
            }
            if command == "dimension.diff" {
                let current =
                    view_value(session, record, &field.name, name, variant, &projections)?;
                let previous = view_value(session, record, &field.name, name, from, &projections)?;
                // 覆盖状态、可写值与渲染结果分别比较，不经过 JSON 字段反读。
                let state_changed = previous.state().explicit != current.state().explicit;
                let value_changed = previous.state().effective != current.state().effective;
                let rendered_changed = previous.state().rendered != current.state().rendered;
                if state_changed || value_changed || rendered_changed {
                    comparisons.push(DimensionComparison {
                        actual_type: record.actual_type().to_string(),
                        id: record.id().to_string(),
                        field: field.name.to_string(),
                        before: previous,
                        after: current,
                        coverage_changed: state_changed,
                        value_changed,
                        rendered_changed,
                    });
                }
            } else {
                let variant = args.require("variant")?;
                let projected = projections
                    .get(variant)
                    .ok_or_else(|| Failure::missing("dimension projection is unavailable"))?;
                let current = field_value(session, record, &field.name, name, variant, projected)?;
                if command != "dimension.missing" || !current.state.explicit {
                    items.push(current);
                }
            }
        }
    }
    if command == "dimension.get" {
        let value = items
            .into_iter()
            .next()
            .ok_or_else(|| Failure::missing("record or dimension-bound field does not exist"))?;
        return super::json(DimensionGet {
            value,
            variant_exists: dimension
                .variants
                .iter()
                .any(|v| Some(v.as_str()) == variant),
        });
    }
    if command == "dimension.diff" {
        return super::json(paginate(comparisons, &selector)?);
    }
    if command == "dimension.missing" {
        let missing = items
            .into_iter()
            .map(|value| {
                let field = session
                    .schema()
                    .field(&value.actual_type, &value.field)
                    .ok_or_else(|| Failure::missing("unknown dimension field"))?;
                let record = session
                    .model()
                    .record_by_type_id(&value.actual_type, &value.id)
                    .and_then(|id| session.model().record(id))
                    .ok_or_else(|| Failure::missing("dimension record is unavailable"))?;
                // 补齐任务携带其他显式变体值，不将回退值伪装成已有翻译。
                let variants = dimension
                    .variants
                    .iter()
                    .filter(|variant| {
                        variant.as_str() != value.variant
                            && record
                                .dimension_field(&value.field)
                                .is_some_and(|field| field.variants.contains_key(variant.as_str()))
                    })
                    .map(|variant| {
                        field_value(
                            session,
                            record,
                            &value.field,
                            name,
                            variant,
                            &projections[variant],
                        )
                    })
                    .collect::<Result<Vec<_>>>()?;
                Ok(DimensionMissing {
                    value,
                    label: field
                        .display
                        .as_ref()
                        .and_then(|display| display.label.clone()),
                    description: field
                        .display
                        .as_ref()
                        .and_then(|display| display.description.clone()),
                    variants,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        return super::json(paginate(missing, &selector)?);
    }
    super::json(paginate(items, &selector)?)
}

// 基础视角使用独立开关，变体名称（包括 base）始终按原名解释。
fn comparison_variant<'a>(args: &'a Arguments, side: &str) -> Result<Option<&'a str>> {
    let flag = args.flag(&format!("{side}-base"));
    match (flag, args.get(side)) {
        (true, None) => Ok(None),
        (false, Some(variant)) => Ok(Some(variant)),
        _ => Err(Failure::request(format!(
            "specify either --{side} or --{side}-base"
        ))),
    }
}

fn view_value(
    session: &ReadOnlyProjectSession,
    record: &CfdRecord,
    field: &str,
    dimension: &str,
    variant: Option<&str>,
    projections: &std::collections::BTreeMap<String, crate::CfdDataModel>,
) -> Result<DimensionView> {
    if let Some(variant) = variant {
        let projected = projections
            .get(variant)
            .ok_or_else(|| Failure::missing("dimension projection is unavailable"))?;
        return field_value(session, record, field, dimension, variant, projected)
            .map(|value| DimensionView::Variant(Box::new(value)));
    }
    let base = record.field(field);
    let expected = &session
        .schema()
        .field(record.actual_type(), field)
        .ok_or_else(|| Failure::missing("unknown field"))?
        .value_type;
    Ok(DimensionView::Base(DimensionState {
        explicit: false,
        effective: base
            .map(|value| super::ModelValue::from_model(session.schema(), expected, value))
            .transpose()?,
        rendered: base.and_then(super::model_value::rendered),
        origin: DimensionOrigin::Base,
    }))
}

/// 精确字段筛选必须在所选声明范围绑定目标维度，空结果不能掩盖拼写错误。
pub(super) fn validate_bound_field(
    session: &ReadOnlyProjectSession,
    selector: &super::Selector,
    dimension: &str,
    field: &str,
) -> Result<()> {
    let types = super::query::selected_types(session, selector)?;
    if !types.iter().any(|ty| {
        session.schema().field(ty, field).is_some_and(|field| {
            field
                .dimension
                .as_ref()
                .is_some_and(|binding| binding.dimension.as_str() == dimension)
        })
    }) {
        return Err(Failure::missing(format!(
            "field `{field}` is not bound to dimension `{dimension}` in the selected types"
        )));
    }
    Ok(())
}
