//! Build editor-facing `RecordRow` / `FieldCell` views over engine records.
//!
//! After spec 17, `FieldCell.value` is a `CfdValue` straight from the
//! core model — no wire-only re-encoding. Editor-derived metadata
//! (schema shape and enum integer value) is collected into
//! `FieldAnnotation` on the side.

use coflow_runtime::{
    dict_key_path_text, value_summary, FieldShapeInfo, ProjectQueries, RecordCoordinate, RecordView,
};
use coflow_runtime::{CfdRecord, CfdValue};
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;

use parking_lot::RwLock;

use crate::editor::session::Diagnostics;
use crate::editor::types::{FieldAnnotation, FieldCell, FieldDiagnostic, RecordRow};

/// 按 `(实际类型, 字段名)` 缓存 schema 派生的 [`FieldShapeInfo`]。
///
/// `field_shape` 只依赖 schema：同一项目代际内形状不变。表格、图谱与记录视图
/// 会针对每条记录的每个字段重复请求它，缓存后把 `记录数 × 字段数` 的形状重建
/// 降到一次/字段。
type ShapeCacheMap = HashMap<String, HashMap<String, Option<Arc<FieldShapeInfo>>>>;

#[derive(Debug)]
struct FieldOrder {
    names: Vec<String>,
    names_set: BTreeSet<String>,
}

#[derive(Debug, Default)]
pub struct ShapeCache {
    inner: RwLock<ShapeCacheMap>,
    field_orders: RwLock<HashMap<String, Arc<FieldOrder>>>,
}

impl ShapeCache {
    /// 查询字段形状，命中缓存时返回共享句柄；未命中时构建并写入。
    #[must_use]
    pub fn get(
        &self,
        queries: ProjectQueries<'_>,
        actual_type: &str,
        field_name: &str,
    ) -> Option<Arc<FieldShapeInfo>> {
        // 借用字符串查找，命中缓存时不为每个字段分配两个临时 String。
        if let Some(cached) = self
            .inner
            .read()
            .get(actual_type)
            .and_then(|fields| fields.get(field_name))
        {
            return cached.clone();
        }
        let shape = queries.field_shape(actual_type, field_name).map(Arc::new);
        self.inner
            .write()
            .entry(actual_type.to_string())
            .or_default()
            .insert(field_name.to_string(), shape.clone());
        shape
    }

    fn field_order(&self, queries: ProjectQueries<'_>, actual_type: &str) -> Arc<FieldOrder> {
        if let Some(order) = self.field_orders.read().get(actual_type) {
            return order.clone();
        }
        // 声明顺序只依赖 schema，同一代际内所有记录共享有序字段与成员索引。
        let names: Vec<_> = queries
            .schema_type_fields(actual_type)
            .into_iter()
            .map(|(name, _)| name)
            .collect();
        let order = Arc::new(FieldOrder {
            names_set: names.iter().cloned().collect(),
            names,
        });
        self.field_orders
            .write()
            .insert(actual_type.to_string(), order.clone());
        order
    }
}

/// Lookup context the converter consults when annotating cells.
pub struct WireContext<'a> {
    pub queries: ProjectQueries<'a>,
    pub diagnostics: &'a Diagnostics,
    pub shapes: &'a ShapeCache,
    annotations: RefCell<HashMap<(usize, AnnotationShape), Arc<FieldAnnotation>>>,
}

/// 只记录影响注解的值结构；文本、数字等业务内容不参与 schema 投影缓存键。
#[derive(Debug, PartialEq, Eq, Hash)]
enum AnnotationShape {
    Scalar,
    Some(Box<Self>),
    Enum(i64),
    Object(String, Vec<(String, Self)>),
    Array(Vec<Self>),
    Dict(Vec<(String, Self)>),
}
impl AnnotationShape {
    fn of(value: &CfdValue) -> Self {
        match value {
            CfdValue::OptionSome(inner) => Self::Some(Box::new(Self::of(inner))),
            CfdValue::Enum(value) => Self::Enum(value.value),
            CfdValue::Object(object) => Self::Object(
                object.actual_type().to_string(),
                object
                    .fields()
                    .iter()
                    .map(|(name, value)| (name.to_string(), Self::of(value)))
                    .collect(),
            ),
            CfdValue::Array(items) => Self::Array(items.iter().map(Self::of).collect()),
            CfdValue::Dict(items) => Self::Dict(
                items
                    .iter()
                    .map(|(key, value)| (dict_key_path_text(key), Self::of(value)))
                    .collect(),
            ),
            _ => Self::Scalar,
        }
    }
}

impl<'a> WireContext<'a> {
    #[must_use]
    pub fn new(
        queries: ProjectQueries<'a>,
        diagnostics: &'a Diagnostics,
        shapes: &'a ShapeCache,
    ) -> Self {
        Self {
            queries,
            diagnostics,
            shapes,
            annotations: RefCell::new(HashMap::new()),
        }
    }

    fn field_shape(&self, actual_type: &str, field_name: &str) -> Option<Arc<FieldShapeInfo>> {
        self.shapes.get(self.queries, actual_type, field_name)
    }
}

/// Translate a [`RecordView`] into a wire [`RecordRow`].
#[must_use]
pub fn record_view_to_row(view: &RecordView<'_>, ctx: &WireContext<'_>) -> RecordRow {
    let fields = record_fields(view.record, ctx);
    let (field_index, field_summaries) = field_indexes(&fields);
    let (field_diagnostics, diagnostic_severity) =
        diagnostics_for_record(ctx.diagnostics, view.display_path, &view.coordinate);
    RecordRow {
        coordinate: view.coordinate.clone(),
        display_path: view.display_path.to_string(),
        container_index: 0,
        container_size: 1,
        fields,
        field_index,
        field_summaries,
        field_diagnostics,
        diagnostic_severity,
    }
}

pub(crate) fn diagnostics_for_record(
    diagnostics: &Diagnostics,
    file_path: &str,
    coordinate: &RecordCoordinate,
) -> (Vec<FieldDiagnostic>, Option<String>) {
    let mut fields = Vec::new();
    let mut best = None;
    for diagnostic in diagnostics.for_record(file_path, coordinate) {
        if let coflow_runtime::DiagnosticTarget::TableField { field_path, .. } = &diagnostic.target
        {
            fields.push(FieldDiagnostic {
                severity: normalized_severity(&diagnostic.severity).to_string(),
                field_path: field_path.clone(),
                message: diagnostic.display_message(),
            });
        }
        match diagnostic.severity.as_str() {
            "error" => best = Some("error"),
            "warning" if best.is_none() => best = Some("warning"),
            _ => {}
        }
    }
    (fields, best.map(str::to_string))
}

fn normalized_severity(severity: &str) -> &'static str {
    match severity {
        "error" => "error",
        "warning" => "warning",
        _ => "info",
    }
}

pub(crate) fn record_fields(record: &CfdRecord, ctx: &WireContext<'_>) -> Vec<FieldCell> {
    // `CfdRecord` stores fields in a BTreeMap for deterministic lookup, not
    // presentation. The schema retains the declared (including inherited)
    // field order, which is what users expect in the editor.
    let order = ctx.shapes.field_order(ctx.queries, record.actual_type());
    let remaining_names = record
        .fields()
        .keys()
        .map(ToString::to_string)
        .filter(|name| !order.names_set.contains(name));

    order
        .names
        .iter()
        .cloned()
        .chain(remaining_names)
        .map(|name| {
            let present = record.fields().get(name.as_str());
            let missing = present.is_none();
            let value = present.cloned().unwrap_or_else(|| {
                ctx.queries
                    .editable_field_seed(record.actual_type(), &name)
                    .unwrap_or(CfdValue::OptionNone)
            });
            FieldCell {
                name: name.clone(),
                annotation: build_annotation(record, &name, &value, ctx),
                value,
                missing,
            }
        })
        .collect()
}

fn field_indexes(fields: &[FieldCell]) -> (BTreeMap<String, usize>, BTreeMap<String, String>) {
    let mut index = BTreeMap::new();
    let mut summaries = BTreeMap::new();
    for (idx, field) in fields.iter().enumerate() {
        index.insert(field.name.clone(), idx);
        summaries.insert(
            field.name.clone(),
            if field.missing {
                String::new()
            } else {
                value_summary(&field.value)
            },
        );
    }
    (index, summaries)
}

fn build_annotation(
    host: &CfdRecord,
    field_name: &str,
    value: &CfdValue,
    ctx: &WireContext<'_>,
) -> Option<Arc<FieldAnnotation>> {
    let declared_shape = ctx.field_shape(host.actual_type(), field_name);
    let key = (
        declared_shape
            .as_ref()
            .map_or(0, |shape| Arc::as_ptr(shape) as usize),
        AnnotationShape::of(value),
    );
    if let Some(annotation) = ctx.annotations.borrow().get(&key) {
        return Some(annotation.clone());
    }
    let annotation = annotation_for_value(value, ctx, declared_shape.as_deref());
    // Synthesized dimension records expose a `default` slot that mirrors the
    // source record's value. Writing into it isn't blocked at the engine
    // layer, but the editor renders it as read-only to steer users to the
    // source record instead.
    if annotation.is_empty() {
        None
    } else {
        let annotation = Arc::new(annotation);
        ctx.annotations.borrow_mut().insert(key, annotation.clone());
        Some(annotation)
    }
}

#[must_use]
pub fn annotation_for_draft_field(
    actual_type: &str,
    field_name: &str,
    value: &CfdValue,
    ctx: &WireContext<'_>,
) -> Option<FieldAnnotation> {
    let declared_shape = ctx.field_shape(actual_type, field_name);
    let annotation = annotation_for_value(value, ctx, declared_shape.as_deref());
    if annotation.is_empty() {
        None
    } else {
        Some(annotation)
    }
}

fn annotation_for_value(
    value: &CfdValue,
    ctx: &WireContext<'_>,
    declared_shape: Option<&FieldShapeInfo>,
) -> FieldAnnotation {
    let wrapped = match value {
        CfdValue::OptionSome(inner) => declared_shape
            .and_then(|shape| shape.option_inner.as_deref())
            .map(|shape| (inner.as_ref(), shape)),
        _ => None,
    };
    if let Some((inner, inner_shape)) = wrapped {
        let mut annotation = annotation_for_value(inner, ctx, Some(inner_shape));
        if let Some(outer) = declared_shape {
            annotation.declared_type = Some(outer.display_label.clone());
            annotation.label.clone_from(&outer.label);
            annotation.description.clone_from(&outer.description);
            annotation.nullable = outer.nullable;
        }
        return annotation;
    }
    let mut annotation = FieldAnnotation::default();
    if let Some(shape) = declared_shape {
        annotation.declared_type = Some(shape.display_label.clone());
        annotation.label.clone_from(&shape.label);
        annotation.description.clone_from(&shape.description);
        annotation
            .ref_target_type
            .clone_from(&shape.ref_target_type);
        annotation.enum_type.clone_from(&shape.enum_type);
        annotation.enum_is_flag = shape.enum_is_flag;
        annotation.nullable = shape.nullable;
        annotation
            .polymorphic_types
            .clone_from(&shape.polymorphic_types);
        annotation.object_type.clone_from(&shape.object_type);
        annotation.field_order.clone_from(&shape.field_order);
        // Preload the element template when the declared type is a
        // collection. Filled here (not only in the Array/Dict arms below) so
        // a nullable / empty collection still carries the template the
        // editor needs to add its first element.
        if let Some(item_shape) = shape.collection_item.as_deref() {
            annotation.item_annotation = Some(Box::new(annotation_template(item_shape)));
        }
        if let Some(key_shape) = shape.collection_key.as_deref() {
            annotation.key_annotation = Some(Box::new(annotation_template(key_shape)));
        }
    }
    match value {
        CfdValue::Enum(enum_value) => {
            annotation.enum_int_value = Some(enum_value.value);
        }
        CfdValue::Object(object) => {
            annotation.object_type = Some(object.actual_type().to_string());
            annotation.field_order = ctx.queries.type_field_names(object.actual_type());
            for (name, child) in object.fields() {
                let child_shape = ctx.field_shape(object.actual_type(), name.as_str());
                let child_annotation = annotation_for_value(child, ctx, child_shape.as_deref());
                if !child_annotation.is_empty() {
                    annotation
                        .children
                        .insert(name.to_string(), child_annotation);
                }
            }
            // Editable object 草稿会省略需要用户选择的字段；仍发送其 schema 注解，
            // 使前端可以合成缺失行并提供引用或具体类型选择器。
            for name in &annotation.field_order {
                if object.field(name).is_some() || annotation.children.contains_key(name) {
                    continue;
                }
                if let Some(shape) = ctx.field_shape(object.actual_type(), name) {
                    annotation
                        .children
                        .insert(name.clone(), annotation_template(&shape));
                }
            }
        }
        CfdValue::Array(items) => {
            let item_shape = declared_shape.and_then(|shape| shape.collection_item.as_deref());
            for (idx, child) in items.iter().enumerate() {
                let child_annotation = annotation_for_value(child, ctx, item_shape);
                if !child_annotation.is_empty() {
                    annotation
                        .children
                        .insert(idx.to_string(), child_annotation);
                }
            }
        }
        CfdValue::Dict(entries) => {
            let item_shape = declared_shape.and_then(|shape| shape.collection_item.as_deref());
            for (key, child) in entries {
                let key_text = dict_key_path_text(key);
                let child_annotation = annotation_for_value(child, ctx, item_shape);
                if !child_annotation.is_empty() {
                    annotation.children.insert(key_text, child_annotation);
                }
            }
        }
        _ => {}
    }
    annotation
}

/// Produce a minimal template `FieldAnnotation` describing the elements of
/// a collection (array item / dict value). The editor consumes this when it
/// needs the element's declared type / ref target / enum type to add a new
/// entry into an empty or nullable collection.
fn annotation_template(item_shape: &FieldShapeInfo) -> FieldAnnotation {
    let mut ann = FieldAnnotation {
        declared_type: Some(item_shape.display_label.clone()),
        ref_target_type: item_shape.ref_target_type.clone(),
        enum_type: item_shape.enum_type.clone(),
        enum_is_flag: item_shape.enum_is_flag,
        nullable: item_shape.nullable,
        polymorphic_types: item_shape.polymorphic_types.clone(),
        object_type: item_shape.object_type.clone(),
        field_order: item_shape.field_order.clone(),
        ..FieldAnnotation::default()
    };
    if let Some(inner) = item_shape.collection_item.as_deref() {
        ann.item_annotation = Some(Box::new(annotation_template(inner)));
    }
    if let Some(key) = item_shape.collection_key.as_deref() {
        ann.key_annotation = Some(Box::new(annotation_template(key)));
    }
    ann
}

#[cfg(test)]
mod tests {
    use coflow_runtime::value_summary;
    use coflow_runtime::CfdValue;

    #[test]
    fn string_summary_preserves_ascii_truncation_behavior() {
        let value = "abcdefghijklmnopqrstuvwxyz0123456789ABCDE";

        assert_eq!(
            value_summary(&CfdValue::String(value.to_string())),
            "abcdefghijklmnopqrstuvwxyz0123456789AB..."
        );
    }

    #[test]
    fn string_summary_truncates_at_utf8_boundary() {
        let value = "婆".repeat(20);
        let expected = format!("{}...", "婆".repeat(12));

        assert_eq!(value_summary(&CfdValue::String(value)), expected);
    }
}
