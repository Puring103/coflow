use super::{snapshot::fingerprint, Failure, Result};
use crate::{CfdPath, CfdPathSegment, CfdRecord, CfdValue, Project, ReadOnlyProjectSession};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{cmp::Ordering, collections::BTreeMap};

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct Selector {
    #[serde(rename = "type")]
    pub actual_type: Option<String>,
    pub include_derived: bool,
    pub file: Option<String>,
    pub id: Option<String>,
    pub fields: Vec<String>,
    pub dimensions: BTreeMap<String, String>,
    pub sort: Vec<String>,
    #[schemars(range(min = 1))]
    pub limit: usize,
    pub cursor: Option<String>,
    pub text: Option<String>,
    pub view: String,
    pub filter: Option<String>,
    #[serde(skip)]
    pub(crate) query_identity: Option<String>,
    #[serde(skip)]
    pub(crate) revision: Option<String>,
}
impl Default for Selector {
    fn default() -> Self {
        Self {
            actual_type: None,
            include_derived: false,
            file: None,
            id: None,
            fields: Vec::new(),
            dimensions: BTreeMap::new(),
            sort: Vec::new(),
            limit: 50,
            cursor: None,
            text: None,
            view: "effective".into(),
            filter: None,
            query_identity: None,
            revision: None,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Page<T> {
    pub items: Vec<T>,
    pub total: usize,
    pub truncated: bool,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct FieldSource {
    pub origin: String,
    pub location: Option<super::diagnostics::CommandLocation>,
    pub assigned: bool,
    pub expression: Option<String>,
    pub effective: Option<super::ModelValue>,
    pub reachable: bool,
}

#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct RecordResult {
    #[serde(rename = "type")]
    pub actual_type: String,
    pub id: String,
    pub file: Option<String>,
    pub rendered: BTreeMap<String, Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fields: Option<BTreeMap<String, Option<super::ModelValue>>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub origins: Option<BTreeMap<String, FieldSource>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<BTreeMap<String, FieldSource>>,
}

pub(crate) fn paginate<T>(items: Vec<T>, selector: &Selector) -> Result<Page<T>> {
    if selector.limit == 0 {
        return Err(Failure::request("limit must be greater than zero"));
    }
    // 查询身份与数据修订分别校验；调整页大小不影响游标，但数据变化必须重查。
    let mut identity = selector.clone();
    identity.cursor = None;
    identity.limit = 0;
    let query = fingerprint(&serde_json::to_vec(&(&identity, &selector.query_identity))?);
    let revision = selector
        .revision
        .as_deref()
        .ok_or_else(|| Failure::request("pagination requires a captured project revision"))?;
    let offset = match &selector.cursor {
        None => 0,
        Some(cursor) => {
            let parts: Vec<_> = cursor.split(':').collect();
            if parts.len() != 3 {
                return Err(Failure::request("malformed cursor"));
            }
            if parts[0] != query {
                return Err(Failure::request("cursor belongs to another query"));
            }
            if parts[1] != revision {
                return Err(Failure::conflict("cursor revision changed; restart the query without --cursor")
                    .with_result(serde_json::json!({"revision": revision,
                        "precondition": {"kind": "cursor_revision", "expected": parts[1], "actual": revision}})));
            }
            parts[2]
                .parse::<usize>()
                .map_err(|_| Failure::request("malformed cursor offset"))?
        }
    };
    let total = items.len();
    if offset > total {
        return Err(Failure::request("cursor offset exceeds result count"));
    }
    let end = offset.saturating_add(selector.limit).min(total);
    let truncated = end < total;
    Ok(Page {
        items: items
            .into_iter()
            .skip(offset)
            .take(selector.limit)
            .collect(),
        total,
        truncated,
        next_cursor: truncated.then(|| format!("{query}:{revision}:{end}")),
    })
}

#[expect(
    clippy::too_many_lines,
    reason = "按语言或命令分支集中处理，保持分派与各分支语义可直接核对。"
)]
pub(crate) fn selected_records(
    session: &ReadOnlyProjectSession,
    selector: &Selector,
) -> Result<Vec<CfdRecord>> {
    if selector.limit == 0 {
        return Err(Failure::request("limit must be greater than zero"));
    }
    if !["source", "effective", "both"].contains(&selector.view.as_str()) {
        return Err(Failure::request("view must be source, effective or both"));
    }
    let types = selected_types(session, selector)?;
    let range = record_range(session, selector)?;
    let filters = selector
        .filter
        .as_deref()
        .map(|expression| super::filter::compile(session, &types, &range, expression))
        .transpose()?;
    for field in &selector.fields {
        if field != "id" {
            range_field_types(session, &types, &range, field)?;
        }
    }
    // 空类型范围仍按声明验证；有记录时使用对象实际类型，精确 key 不受范围外对象影响。
    for sort in &selector.sort {
        let field = sort.strip_prefix('-').unwrap_or(sort);
        if field.is_empty() || field.starts_with('-') {
            return Err(Failure::request(
                "sort requires a field path with at most one descending prefix",
            ));
        }
        if field == "id" || field == "type" {
            continue;
        }
        let mut common = None;
        for mut expected in range_field_types(session, &types, &range, field)? {
            while let coflow_language::cft::CftValueType::Option(inner) = expected {
                expected = *inner;
            }

            if !matches!(
                expected,
                T::Int
                    | T::Long
                    | T::Float
                    | T::Double
                    | T::Bool
                    | T::String
                    | T::Enum(_)
                    | T::RecordRef(_)
            ) {
                return Err(Failure::request(format!(
                    "field `{field}` cannot be sorted"
                )));
            }
            if common
                .as_ref()
                .is_some_and(|previous| previous != &expected)
            {
                return Err(Failure::request(format!(
                    "sort field `{field}` has incompatible types across the selected range"
                )));
            }
            common = Some(expected);
        }
    }
    let mut records = Vec::new();
    for record in range {
        if let Some(text) = &selector.text {
            if !record.id().contains(text)
                && !record
                    .fields()
                    .values()
                    .any(|v| crate::value_summary(v).contains(text))
            {
                continue;
            }
        }
        if let Some(filters) = &filters {
            let filter = filters
                .get(&(record.actual_type().to_string(), record.id().to_string()))
                .ok_or_else(|| Failure::request("filter record is outside the selected range"))?;
            if !filter.matches(&record) {
                continue;
            }
        }
        records.push(record);
    }
    // 排序键只解析一次；任何读取错误都在排序前返回，不能被比较器吞掉。
    let mut keyed = records
        .into_iter()
        .map(|record| {
            let keys = selector
                .sort
                .iter()
                .map(|sort| {
                    let name = sort.strip_prefix('-').unwrap_or(sort);
                    match name {
                        "id" => Ok(Some(CfdValue::String(record.id().into()))),
                        "type" => Ok(Some(CfdValue::String(record.actual_type().into()))),
                        _ => Ok(value_at(session, &record, name)?.cloned()),
                    }
                })
                .collect::<Result<Vec<_>>>()?;
            Ok((record, keys))
        })
        .collect::<Result<Vec<_>>>()?;
    keyed.sort_by(|(a, a_keys), (b, b_keys)| {
        for ((a_value, b_value), sort) in a_keys.iter().zip(b_keys).zip(&selector.sort) {
            let order = compare_values(a_value.as_ref(), b_value.as_ref());
            let order = if sort.starts_with('-') {
                order.reverse()
            } else {
                order
            };
            if order != Ordering::Equal {
                return order;
            }
        }
        a.actual_type()
            .cmp(b.actual_type())
            .then(a.id().cmp(b.id()))
    });
    Ok(keyed.into_iter().map(|(record, _)| record).collect())
}

/// 先确定类型、文件和 key 范围，再绑定字段路径和过滤条件，最后进行内容筛选。
pub(crate) fn record_range(
    session: &ReadOnlyProjectSession,
    selector: &Selector,
) -> Result<Vec<CfdRecord>> {
    let dimensions = session.queries().dimensions();
    for name in selector.dimensions.keys() {
        if !dimensions.iter().any(|dimension| &dimension.name == name) {
            return Err(Failure::missing(format!("unknown dimension `{name}`")));
        }
    }
    let projected = project_dimensions(session, &selector.dimensions)?;
    let requested = selector.file.as_ref().map(|file| {
        crate::normalized_path_identity(
            &session
                .session
                .project
                .resolve_path(std::path::Path::new(file)),
        )
    });
    let mut source_identities = BTreeMap::<String, String>::new();
    Ok(projected
        .records()
        .filter(|(_, record)| {
            if selector.actual_type.as_ref().is_some_and(|ty| {
                record.actual_type() != ty
                    && !(selector.include_derived
                        && session.schema().is_assignable(record.actual_type(), ty))
            }) {
                return false;
            }
            if selector.id.as_ref().is_some_and(|key| record.id() != key) {
                return false;
            }
            if requested.is_some() {
                let project = &session.session.project;
                if session
                    .queries()
                    .file_for_record(record.actual_type(), record.id())
                    .is_none_or(|source| {
                        let identity =
                            source_identities
                                .entry(source.to_string())
                                .or_insert_with(|| {
                                    crate::normalized_path_identity(
                                        &project.resolve_path(std::path::Path::new(source)),
                                    )
                                });
                        Some(identity.as_str()) != requested.as_deref()
                    })
                {
                    return false;
                }
            }
            true
        })
        .map(|(_, record)| record.clone())
        .collect())
}

/// 路径的合法性取决于查询范围；无样本类型不能绕过声明检查。
pub(crate) fn range_field_types(
    session: &ReadOnlyProjectSession,
    types: &[String],
    records: &[CfdRecord],
    field: &str,
) -> Result<Vec<coflow_language::cft::CftValueType>> {
    super::execute::public_path(field)?;
    let mut expected = Vec::new();
    for ty in types {
        let records = records
            .iter()
            .filter(|record| record.actual_type() == ty)
            .collect::<Vec<_>>();
        if records.is_empty() {
            let path = resolved_path(session, ty, field)?;
            expected.push(crate::write_rules::expected_type_for_cfd_path(
                session.schema(),
                ty,
                &path,
                "CLI-FIELD",
                "CLI",
            )?);
        } else {
            for record in records {
                let path = resolved_record_path(session, record, field)?;
                let ty = crate::write_rules::expected_type_for_record_path(
                    session.schema(),
                    record,
                    &path,
                    "CLI-FIELD",
                    "CLI",
                )?;
                if !expected.contains(&ty) {
                    expected.push(ty);
                }
            }
        }
    }
    Ok(expected)
}

pub(crate) fn project_dimensions(
    session: &ReadOnlyProjectSession,
    dimensions: &BTreeMap<String, String>,
) -> Result<crate::CfdDataModel> {
    session
        .model()
        .project_dimensions(session.schema(), dimensions)
        .map_err(|diagnostics| {
            let origins = session
                .model()
                .records()
                .map(|(_, record)| record.origin.clone())
                .collect::<Vec<_>>();
            Failure::from(crate::map_diagnostics_with_origins(diagnostics, &origins))
        })
}

pub(crate) fn selected_types(
    session: &ReadOnlyProjectSession,
    selector: &Selector,
) -> Result<Vec<String>> {
    if let Some(name) = &selector.actual_type {
        if session.schema().resolve_type(name).is_none() {
            return Err(Failure::missing(format!("unknown type `{name}`")));
        }
    }
    Ok(session
        .schema()
        .all_types()
        .filter(|ty| {
            selector.actual_type.as_ref().map_or_else(
                || !ty.is_abstract && !ty.is_data(),
                |name| {
                    ty.name.as_str() == name
                        || selector.include_derived
                            && !ty.is_abstract
                            && !ty.is_data()
                            && session.schema().is_assignable(&ty.name, name)
                },
            )
        })
        .map(|ty| ty.name.to_string())
        .collect())
}

#[expect(
    clippy::float_cmp,
    reason = "精度校验需要精确比较舍入前后的值，近似比较会接受无法表示的输入。"
)]
pub(crate) fn compare_values(a: Option<&CfdValue>, b: Option<&CfdValue>) -> Ordering {
    match (a, b) {
        (None, None) | (Some(CfdValue::OptionNone), Some(CfdValue::OptionNone)) => Ordering::Equal,
        (None, _) => Ordering::Less,
        (_, None) => Ordering::Greater,
        (Some(CfdValue::OptionNone), _) => Ordering::Less,
        (_, Some(CfdValue::OptionNone)) => Ordering::Greater,
        (Some(CfdValue::OptionSome(a)), Some(CfdValue::OptionSome(b))) => {
            compare_values(Some(a), Some(b))
        }
        (Some(CfdValue::OptionSome(a)), b) => compare_values(Some(a), b),
        (a, Some(CfdValue::OptionSome(b))) => compare_values(a, Some(b)),
        (Some(CfdValue::Int(a)), Some(CfdValue::Int(b))) => a.cmp(b),
        (Some(CfdValue::Float(a)), Some(CfdValue::Float(b))) => {
            if a == b {
                Ordering::Equal
            } else {
                a.total_cmp(b)
            }
        }
        (Some(CfdValue::String(a)), Some(CfdValue::String(b))) => a.cmp(b),
        (Some(CfdValue::FormattedString(a)), Some(CfdValue::FormattedString(b))) => {
            a.rendered.cmp(&b.rendered)
        }
        (Some(CfdValue::String(a)), Some(CfdValue::FormattedString(b))) => a.cmp(&b.rendered),
        (Some(CfdValue::FormattedString(a)), Some(CfdValue::String(b))) => a.rendered.cmp(b),
        (Some(CfdValue::Bool(a)), Some(CfdValue::Bool(b))) => a.cmp(b),
        (Some(CfdValue::Enum(a)), Some(CfdValue::Enum(b))) => {
            a.enum_name.cmp(&b.enum_name).then(a.value.cmp(&b.value))
        }
        (Some(CfdValue::Ref(a)), Some(CfdValue::Ref(b))) => a.cmp(b),
        (Some(a), Some(b)) => crate::value_summary(a).cmp(&crate::value_summary(b)),
    }
}

/// 只能经捕获并复核输入快照的读取边界调用，现有会话不能绑定随后捕获的磁盘修订。
pub(crate) fn query_records(
    project: &Project,
    session: &ReadOnlyProjectSession,
    selector: &Selector,
) -> Result<Page<Value>> {
    let records = selected_records(session, selector)?;
    let mut sources = RecordSources::default();
    // 全局选择与排序先完成，但只有当前页需要物化字段、渲染值和来源。
    let page = paginate(records, selector)?;
    let items = page
        .items
        .iter()
        .map(|record| record_json_with_sources(project, session, record, selector, &mut sources))
        .collect::<Result<Vec<_>>>()?;
    Ok(Page {
        items,
        total: page.total,
        truncated: page.truncated,
        next_cursor: page.next_cursor,
    })
}

pub(crate) fn record_json(
    project: &Project,
    session: &ReadOnlyProjectSession,
    record: &CfdRecord,
    selector: &Selector,
) -> Result<Value> {
    record_json_with_sources(
        project,
        session,
        record,
        selector,
        &mut RecordSources::default(),
    )
}

#[derive(Default)]
pub(crate) struct RecordSources {
    files: BTreeMap<std::path::PathBuf, std::sync::Arc<ParsedRecordSource>>,
    lines: BTreeMap<std::path::PathBuf, (crate::LineIndex, String)>,
}
struct ParsedRecordSource {
    source: String,
    ast: coflow_language::cfd::CfdAst,
    records: BTreeMap<(String, String), usize>,
    dimensions: BTreeMap<(String, String, String, String), (usize, usize)>,
}
impl RecordSources {
    fn location(
        &mut self,
        root: &std::path::Path,
        path: &std::path::Path,
        source: &str,
        span: coflow_language::source::Span,
    ) -> super::diagnostics::CommandLocation {
        // 每份源码只扫描一次行索引，位置查询不再从文件头重复扫描。
        let (index, display) = self.lines.entry(path.to_path_buf()).or_insert_with(|| {
            (
                crate::LineIndex::new(source),
                crate::project_path(root, path),
            )
        });
        let range = index.range(source, span.start, span.end);
        super::diagnostics::CommandLocation {
            path: display.clone(),
            config_path: None,
            range: Some(super::diagnostics::CommandRange {
                start: super::diagnostics::CommandPosition {
                    line: range.start.line,
                    character: range.start.character,
                },
                end: super::diagnostics::CommandPosition {
                    line: range.end.line,
                    character: range.end.character,
                },
            }),
        }
    }
    fn load(&mut self, path: &std::path::Path) -> Result<std::sync::Arc<ParsedRecordSource>> {
        if let Some(source) = self.files.get(path) {
            return Ok(source.clone());
        }
        let source = std::fs::read_to_string(path)?;
        let ast = coflow_language::cfd::parse_cfd(&source).0;
        let records = ast
            .records
            .iter()
            .enumerate()
            .map(|(index, record)| ((record.type_name.clone(), record.id.clone()), index))
            .collect();
        let dimensions = ast
            .dimensions
            .iter()
            .enumerate()
            .flat_map(|(block_index, block)| {
                block
                    .records
                    .iter()
                    .enumerate()
                    .map(move |(record_index, record)| {
                        (
                            (
                                block.dimension.clone(),
                                block.variant.clone(),
                                record.type_name.clone(),
                                record.id.clone(),
                            ),
                            (block_index, record_index),
                        )
                    })
            })
            .collect();
        let parsed = std::sync::Arc::new(ParsedRecordSource {
            source,
            ast,
            records,
            dimensions,
        });
        self.files.insert(path.to_path_buf(), parsed.clone());
        Ok(parsed)
    }
}

#[expect(
    clippy::too_many_lines,
    reason = "按语言或命令分支集中处理，保持分派与各分支语义可直接核对。"
)]
pub(crate) fn record_json_with_sources(
    project: &Project,
    session: &ReadOnlyProjectSession,
    record: &CfdRecord,
    selector: &Selector,
    cache: &mut RecordSources,
) -> Result<Value> {
    if !["source", "effective", "both"].contains(&selector.view.as_str()) {
        return Err(Failure::request("view must be source, effective or both"));
    }
    let mut values = BTreeMap::new();
    let mut sources = BTreeMap::new();
    let mut rendered = BTreeMap::new();
    let file = session
        .queries()
        .file_for_record(record.actual_type(), record.id());
    let parsed = file
        .map(|file| cache.load(&project.resolve_path(std::path::Path::new(file))))
        .transpose()?;
    let text = parsed.as_ref().map(|parsed| parsed.source.as_str());
    let declaration = parsed.as_ref().and_then(|parsed| {
        parsed
            .records
            .get(&(record.actual_type().into(), record.id().into()))
            .and_then(|index| parsed.ast.records.get(*index))
    });
    let fields: Vec<_> = if selector.fields.is_empty() {
        record.fields().keys().map(ToString::to_string).collect()
    } else {
        selector.fields.clone()
    };
    for field in fields {
        if field == "id" {
            continue;
        }
        let path = resolved_record_path(session, record, &field)?;
        let value = record.value_at_path(&CfdPath {
            segments: path.clone(),
        });
        let expected = crate::write_rules::expected_type_for_record_path(
            session.schema(),
            record,
            &path,
            "CLI-PATH",
            "CLI",
        )?;
        let encoded = value
            .map(|value| super::ModelValue::from_model(session.schema(), &expected, value))
            .transpose()?;
        values.insert(field.clone(), encoded.clone());
        if let Some(value) = value.and_then(super::model_value::rendered) {
            rendered.insert(field.clone(), value);
        }
        let top = path.first().and_then(|s| {
            if let CfdPathSegment::Field(n) = s {
                Some(n)
            } else {
                None
            }
        });
        let assignment = top
            .and_then(|name| declaration.and_then(|r| r.fields.iter().find(|f| &f.name == name)));
        let assigned_value =
            assignment.and_then(|assignment| ast_value_at(&assignment.value, &path[1..]));
        let mut assigned = assigned_value.is_some();
        let mut location = assigned_value
            .zip(text)
            .zip(file)
            .map(|((value, text), file)| {
                cache.location(
                    project.root_dir(),
                    &project.resolve_path(std::path::Path::new(file)),
                    text,
                    ast_value_span(value),
                )
            });
        let override_value = top
            .and_then(|n| record.dimension_fields.get(n.as_str()))
            .and_then(|d| {
                selector
                    .dimensions
                    .get(d.dimension.as_str())
                    .and_then(|v| d.variants.get(v.as_str()))
            });
        let source_field = if assignment.is_none() {
            top.and_then(|name| session.schema().field(record.actual_type(), name))
        } else {
            declared_field_at(session, record, &path)
        };
        let mut origin = if override_value.is_some() {
            "dimension_override"
        } else if assigned_value.is_some() {
            "explicit_base"
        } else if source_field.is_some_and(|f| f.default.is_some()) {
            "schema_default"
        } else {
            "explicit_base"
        };
        if origin == "schema_default" {
            if let Some(f) = source_field {
                location = session
                    .schema()
                    .resolve_type(&f.declaring_type)
                    .and_then(|t| session.schema().source(&t.module))
                    .map(|s| cache.location(project.root_dir(), &s.path, &s.source, f.span));
            }
        }
        let mut expression = assigned_value
            .zip(text)
            .and_then(|(value, text)| {
                let span = ast_value_span(value);
                text.get(span.start..span.end)
            })
            .map(str::to_string);
        if let Some(override_value) = override_value {
            // 有效维度值的来源必须跟随覆盖文件，不能仍指向基础赋值。
            location = override_value
                .origin
                .location_for_path(&CfdPath::root())
                .map(|source| {
                    super::diagnostics::CommandLocation::from_source(
                        &source.into(),
                        Some(project.root_dir()),
                    )
                });
            expression = None;
            assigned = path.len() == 1;
            if let Some(source_path) = override_value.origin.local_path() {
                let parsed = cache.load(source_path)?;
                let dimension = top
                    .and_then(|field| record.dimension_field(field))
                    .map(|field| field.dimension.as_str());
                let variant = dimension.and_then(|dimension| selector.dimensions.get(dimension));
                let entry = dimension
                    .zip(variant)
                    .and_then(|(dimension, variant)| {
                        parsed
                            .dimensions
                            .get(&(
                                dimension.into(),
                                variant.clone(),
                                record.actual_type().into(),
                                record.id().into(),
                            ))
                            .or_else(|| {
                                parsed
                                    .dimensions
                                    .iter()
                                    .find(|((name, value, ty, key), _)| {
                                        name == dimension
                                            && value == variant
                                            && key == record.id()
                                            && session
                                                .schema()
                                                .is_assignable(record.actual_type(), ty)
                                    })
                                    .map(|(_, indices)| indices)
                            })
                    })
                    .and_then(|(block, entry)| {
                        parsed
                            .ast
                            .dimensions
                            .get(*block)
                            .and_then(|block| block.records.get(*entry))
                    });
                if let Some(assignment) = entry
                    .and_then(|entry| entry.fields.iter().find(|entry| Some(&entry.name) == top))
                {
                    let assigned_value = ast_value_at(&assignment.value, &path[1..]);
                    assigned = assigned_value.is_some();
                    if let Some(value) = assigned_value {
                        let span = ast_value_span(value);
                        location = Some(cache.location(
                            project.root_dir(),
                            source_path,
                            &parsed.source,
                            span,
                        ));
                        expression = parsed.source.get(span.start..span.end).map(str::to_string);
                    }
                }
            }
            if !assigned && path.len() > 1 && value.is_some() {
                if let Some(declared) = declared_field_at(session, record, &path)
                    .filter(|field| field.default.is_some())
                {
                    origin = "schema_default";
                    location = session
                        .schema()
                        .resolve_type(&declared.declaring_type)
                        .and_then(|ty| session.schema().source(&ty.module))
                        .map(|source| {
                            cache.location(
                                project.root_dir(),
                                &source.path,
                                &source.source,
                                declared.span,
                            )
                        });
                }
            }
        }
        if value.is_none() {
            // 空态父对象下的路径没有字段值，不能伪装成子字段的默认值来源。
            origin = "unreachable";
            location = None;
            expression = None;
            assigned = false;
        }
        sources.insert(
            field,
            FieldSource {
                origin: origin.into(),
                location,
                assigned,
                expression,
                effective: encoded,
                reachable: value.is_some(),
            },
        );
    }
    let output = RecordResult {
        actual_type: record.actual_type().to_string(),
        id: record.id().to_string(),
        file: file.map(str::to_string),
        rendered,
        fields: (selector.view != "source").then_some(values),
        origins: (selector.view != "source").then(|| sources.clone()),
        source: (selector.view != "effective").then_some(sources),
    };
    super::json(output)
}

fn declared_field_at<'a>(
    session: &'a ReadOnlyProjectSession,
    record: &CfdRecord,
    path: &[CfdPathSegment],
) -> Option<&'a coflow_language::cft::CftField> {
    let mut metadata = None;
    for (index, segment) in path.iter().enumerate() {
        let CfdPathSegment::Field(name) = segment else {
            continue;
        };
        let actual = if index == 0 {
            record.actual_type()
        } else {
            let mut parent = record.value_at_path(&CfdPath {
                segments: path[..index].to_vec(),
            })?;
            while let CfdValue::OptionSome(value) = parent {
                parent = value;
            }
            let CfdValue::Object(object) = parent else {
                return metadata;
            };
            object.actual_type()
        };
        metadata = session.schema().field(actual, name);
    }
    metadata
}

fn ast_value_at<'a>(
    value: &'a coflow_language::cfd::CfdValue,
    path: &[CfdPathSegment],
) -> Option<&'a coflow_language::cfd::CfdValue> {
    use coflow_language::cfd::CfdValue as V;
    if path.is_empty() {
        return Some(value);
    }
    if let V::OptionSome(value, _) = value {
        return ast_value_at(value, path);
    }
    let child = match (&path[0], value) {
        (CfdPathSegment::Field(name), V::Block(block)) => block
            .fields
            .iter()
            .find(|field| field.name == *name)
            .map(|field| &field.value),
        (CfdPathSegment::Index(index), V::Array(values, _)) => values.get(*index),
        (CfdPathSegment::DictKey(key), V::Block(block)) => {
            let string = serde_json::from_str::<String>(key).ok();
            block
                .fields
                .iter()
                .find(|field| {
                    string.as_ref().is_some_and(|key| field.name == *key)
                        || field.name == *key
                        || field.name == key.replace('.', "::")
                        || key
                            .rsplit_once('.')
                            .is_some_and(|(_, member)| field.name == member)
                })
                .map(|field| &field.value)
        }
        _ => None,
    }?;
    ast_value_at(child, &path[1..])
}

pub(crate) fn ast_value_span(
    value: &coflow_language::cfd::CfdValue,
) -> coflow_language::source::Span {
    let mut span = value.span();
    if let coflow_language::cfd::CfdValue::Block(block) = value {
        if let Some((_, marker)) = &block.type_marker {
            span.start = marker.start;
        }
    }
    span
}

pub(crate) fn value_at<'a>(
    session: &ReadOnlyProjectSession,
    record: &'a CfdRecord,
    field: &str,
) -> Result<Option<&'a CfdValue>> {
    Ok(record.value_at_path(&CfdPath {
        segments: resolved_record_path(session, record, field)?,
    }))
}

pub(crate) fn resolved_record_path(
    session: &ReadOnlyProjectSession,
    record: &CfdRecord,
    field: &str,
) -> Result<Vec<CfdPathSegment>> {
    super::operations::internal_path_for_record(
        session.schema(),
        record,
        &super::execute::public_path(field)?,
        Vec::new(),
    )
}

pub(crate) fn resolved_path(
    session: &ReadOnlyProjectSession,
    ty: &str,
    field: &str,
) -> Result<Vec<CfdPathSegment>> {
    super::operations::internal_path(session.schema(), ty, &super::execute::public_path(field)?)
}

pub fn parse_path(source: &str) -> Result<Vec<CfdPathSegment>> {
    let mut offset = 0;
    let mut out = Vec::new();
    let bytes = source.as_bytes();
    // 每个字段、分隔符和下标都消费完整语法，禁止把损坏路径解释成另一条合法路径。
    loop {
        let start = offset;
        while offset < bytes.len() && bytes[offset] != b'.' && bytes[offset] != b'[' {
            offset += 1;
        }
        let field = &source[start..offset];
        coflow_language::cft::FieldName::new(field).map_err(|e| Failure::request(e.to_string()))?;
        out.push(CfdPathSegment::Field(field.into()));
        while offset < bytes.len() && bytes[offset] == b'[' {
            offset += 1;
            let start = offset;
            let mut quoted = false;
            let mut escaped = false;
            while offset < bytes.len() {
                let ch = bytes[offset];
                if ch == b']' && !quoted {
                    break;
                }
                if ch == b'"' && !escaped {
                    quoted = !quoted;
                }
                escaped = ch == b'\\' && !escaped;
                offset += 1;
            }
            if offset == bytes.len() {
                return Err(Failure::request("unclosed field path subscript"));
            }
            let key = source[start..offset].trim();
            if key.is_empty() {
                return Err(Failure::request("empty field path subscript"));
            }
            if key.starts_with('"') {
                let _: String = serde_json::from_str(key)?;
                out.push(CfdPathSegment::DictKey(key.into()));
            } else if let Ok(index) = key.parse::<usize>() {
                out.push(CfdPathSegment::Index(index));
            } else {
                out.push(CfdPathSegment::DictKey(key.into()));
            }
            offset += 1;
        }
        if offset == bytes.len() {
            return Ok(out);
        }
        if bytes[offset] != b'.' {
            return Err(Failure::request(
                "field path segments require a dot separator",
            ));
        }
        offset += 1;
        if offset == bytes.len() {
            return Err(Failure::request("field path has a trailing separator"));
        }
    }
}

use coflow_language::cft::CftValueType as T;
