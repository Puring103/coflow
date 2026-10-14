use serde_json::Value;
use crate::{ReadOnlyProjectSession, CfdRecord};
use super::{Arguments, Result, Failure, query::{selected_records, paginate}};

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
        Self { name: value.name, display_name: value.display_name, variants: value.variants,
            dir: value.dir, files: value.files, fields: value.fields.into_iter().map(|field| DimensionField {
                source_type: field.source_type, source_field: field.source_field, is_singleton: field.is_singleton,
            }).collect() }
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
pub enum DimensionOrigin { Base, DimensionOverride }

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
    pub key: String,
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
pub enum DimensionView { Variant(DimensionValue), Base(DimensionState) }

impl DimensionView {
    fn state(&self) -> &DimensionState {
        match self { Self::Variant(value) => &value.state, Self::Base(state) => state }
    }
}

#[derive(Debug, serde::Serialize, schemars::JsonSchema)]
pub struct DimensionGet {
    #[serde(flatten)]
    pub value: DimensionValue,
    pub variant_exists: bool,
}

#[derive(Debug, serde::Serialize, schemars::JsonSchema)]
pub struct DimensionComparison {
    #[serde(rename = "type")]
    pub actual_type: String,
    pub key: String,
    pub field: String,
    pub before: DimensionView,
    pub after: DimensionView,
    pub coverage_changed: bool,
    pub value_changed: bool,
    pub rendered_changed: bool,
}

pub(super) fn list(session: &ReadOnlyProjectSession) -> Vec<DimensionMetadata> {
    session.queries().dimensions().into_iter().map(DimensionMetadata::from).collect()
}

fn field_value(session: &ReadOnlyProjectSession, record: &CfdRecord, field: &str, dimension: &str, variant: &str) -> Result<DimensionValue> {
    let base = record.field(field);
    let explicit = record.dimension_field(field).filter(|f| f.dimension.as_str() == dimension)
        .and_then(|f| f.variants.get(variant));
    let effective = explicit.map(|v| &v.value).or(base);
    let expected = &session.schema().field(record.actual_type(), field).ok_or_else(|| Failure::missing("unknown dimension field"))?.value_type;
    // 覆盖位置使用公共定位契约，不向调用方暴露模型的 RecordOrigin 枚举。
    let location = explicit.and_then(|value| value.origin.location_for_path(&crate::CfdPath::root().field(field)))
        .map(|source| super::diagnostics::CommandLocation::from_source(&source.into(), Some(session.session.project.root_dir())));
    let encode = |value: Option<&crate::CfdValue>| value.map(|value| super::ModelValue::from_model(session.schema(), expected, value)).transpose();
    Ok(DimensionValue { actual_type: record.actual_type().to_string(), key: record.key().to_string(), field: field.into(),
        dimension: dimension.into(), variant: variant.into(), base: encode(base)?, override_value: encode(explicit.map(|value| &value.value))?,
        state: DimensionState { explicit: explicit.is_some(), effective: encode(effective)?, rendered: effective.and_then(super::model_value::rendered),
            origin: if explicit.is_some() { DimensionOrigin::DimensionOverride } else { DimensionOrigin::Base } }, location })
}

pub fn query(command: &str, session: &ReadOnlyProjectSession, args: &Arguments) -> Result<Value> {
    let name = args.require("dimension")?;
    let dimension = session.queries().dimensions().into_iter().find(|d| d.name == name)
        .ok_or_else(|| Failure::missing(format!("unknown dimension `{name}`")))?;
    if command == "dimension.inspect" { return super::json(DimensionMetadata::from(dimension)); }
    if command == "dimension.variants" {
        let items: Vec<_> = dimension.variants.iter().map(|variant| {
            let mut count = 0;
            let mut locations = Vec::new();
            for (_, record) in session.model().records() {
                for values in record.dimension_fields.values().filter(|v| v.dimension.as_str() == name) {
                    if let Some(value) = values.variants.get(variant.as_str()) {
                        count += 1;
                        // 未知来源保留 null，已知来源统一为 CLI 的路径与 UTF-16 行列位置。
                        locations.push(value.origin.location_for_path(&crate::CfdPath::root()).map(|source|
                            super::diagnostics::CommandLocation::from_source(&source.into(), Some(session.session.project.root_dir()))));
                    }
                }
            }
            DimensionVariant { name: variant.clone(), overrides: count, locations }
        }).collect();
        return super::json(paginate(items, &args.selector()?)?);
    }
    let variant = if command == "dimension.diff" {
        comparison_variant(args, "to")?
    } else { Some(args.require("variant")?) };
    let from = if command == "dimension.diff" { comparison_variant(args, "from")? } else { None };
    let selector = args.selector()?;
    let records = selected_records(session, &selector)?;
    let mut items = Vec::new();
    let mut comparisons = Vec::new();
    for record in &records {
        let ty = session.schema().resolve_type(record.actual_type()).ok_or_else(|| Failure::missing("record type is unavailable"))?;
        for field in ty.all_fields().filter(|f| f.dimension.as_ref().is_some_and(|d| d.dimension.as_str() == name)) {
            if args.get("field").is_some_and(|f| f != field.name.as_str()) { continue; }
            if command == "dimension.diff" {
                let current = view_value(session, record, &field.name, name, variant)?;
                let previous = view_value(session, record, &field.name, name, from)?;
                // 覆盖状态、可写值与渲染结果分别比较，不经过 JSON 字段反读。
                let state_changed = previous.state().explicit != current.state().explicit;
                let value_changed = previous.state().effective != current.state().effective;
                let rendered_changed = previous.state().rendered != current.state().rendered;
                if state_changed || value_changed || rendered_changed { comparisons.push(DimensionComparison {
                    actual_type: record.actual_type().to_string(), key: record.key().to_string(), field: field.name.to_string(),
                    before: previous, after: current, coverage_changed: state_changed, value_changed, rendered_changed }); }
            } else {
                let current = field_value(session, record, &field.name, name, args.require("variant")?)?;
                if command != "dimension.missing" || !current.state.explicit { items.push(current); }
            }
        }
    }
    if command == "dimension.get" {
        let value = items.into_iter().next().ok_or_else(|| Failure::missing("record or dimension-bound field does not exist"))?;
        return super::json(DimensionGet { value, variant_exists: dimension.variants.iter().any(|v| Some(v.as_str()) == variant) });
    }
    if command == "dimension.diff" { return super::json(paginate(comparisons, &selector)?); }
    super::json(paginate(items, &selector)?)
}

// 基础视角使用独立开关，变体名称（包括 base）始终按原名解释。
fn comparison_variant<'a>(args: &'a Arguments, side: &str) -> Result<Option<&'a str>> {
    let flag = args.flag(&format!("{side}-base"));
    match (flag, args.get(side)) {
        (true, None) => Ok(None),
        (false, Some(variant)) => Ok(Some(variant)),
        _ => Err(Failure::request(format!("specify either --{side} or --{side}-base"))),
    }
}

fn view_value(session: &ReadOnlyProjectSession, record: &CfdRecord, field: &str, dimension: &str, variant: Option<&str>) -> Result<DimensionView> {
    if let Some(variant) = variant { return field_value(session, record, field, dimension, variant).map(DimensionView::Variant); }
    let base = record.field(field);
    let expected = &session.schema().field(record.actual_type(), field).ok_or_else(|| Failure::missing("unknown field"))?.value_type;
    Ok(DimensionView::Base(DimensionState { explicit: false,
        effective: base.map(|value| super::ModelValue::from_model(session.schema(), expected, value)).transpose()?,
        rendered: base.and_then(super::model_value::rendered), origin: DimensionOrigin::Base }))
}
