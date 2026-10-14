use std::{collections::BTreeMap, cmp::Ordering};
use serde::{Serialize, Deserialize};
use serde_json::{Value, json};
use crate::{ReadOnlyProjectSession, CfdRecord, CfdValue, CfdPath, CfdPathSegment, Project};
use super::{Failure, Result, snapshot::fingerprint};

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct Selector {
    #[serde(rename = "type")]
    pub actual_type: Option<String>,
    pub include_derived: bool,
    pub file: Option<String>,
    pub key: Option<String>,
    pub fields: Vec<String>,
    pub dimensions: BTreeMap<String, String>,
    pub sort: Vec<String>,
    pub limit: usize,
    pub cursor: Option<String>,
    pub text: Option<String>,
    pub view: String,
    pub filter: Option<String>,
    #[serde(skip)]
    pub(crate) query_identity: Option<String>,
}
impl Default for Selector {
    fn default() -> Self { Self { actual_type: None, include_derived: false, file: None,
        key: None, fields: Vec::new(), dimensions: BTreeMap::new(), sort: Vec::new(),
        limit: 50, cursor: None, text: None, view: "effective".into(), filter: None, query_identity: None } }
}

#[derive(Debug, Serialize)]
pub struct Page<T> { pub items: Vec<T>, pub total: usize, pub truncated: bool, pub next_cursor: Option<String> }

pub(crate) fn paginate<T>(items: Vec<T>, selector: &Selector) -> Result<Page<T>> {
    if selector.limit == 0 { return Err(Failure::request("limit must be greater than zero")); }
    // 游标仅标记同一查询的读取位置，不绑定项目文件状态。
    let mut identity = selector.clone(); identity.cursor = None; identity.limit = 0;
    let query = fingerprint(&serde_json::to_vec(&(&identity, &selector.query_identity))?);
    let offset = match &selector.cursor {
        None => 0,
        Some(cursor) => {
            let parts: Vec<_> = cursor.split(':').collect();
            if parts.len() != 2 { return Err(Failure::request("malformed cursor")); }
            if parts[0] != query { return Err(Failure::request("cursor belongs to another query")); }
            parts[1].parse::<usize>().map_err(|_| Failure::request("malformed cursor offset"))?
        }
    };
    let total = items.len();
    if offset > total { return Err(Failure::request("cursor offset exceeds result count")); }
    let end = offset.saturating_add(selector.limit).min(total);
    let truncated = end < total;
    Ok(Page { items: items.into_iter().skip(offset).take(selector.limit).collect(), total, truncated,
        next_cursor: truncated.then(|| format!("{query}:{end}")) })
}

pub(crate) fn selected_records(session: &ReadOnlyProjectSession, selector: &Selector) -> Result<Vec<CfdRecord>> {
    if !["source", "effective", "both"].contains(&selector.view.as_str()) { return Err(Failure::request("view must be source, effective or both")); }
    let types = selected_types(session, selector)?;
    let filters = selector.filter.as_deref().map(|expression| super::filter::compile(session, &types, expression)).transpose()?;
    for field in &selector.fields { if field != "key" { super::execute::public_path(field)?; } }
    // 验证范围取自 Schema，不能让空项目或后续筛选掩盖不合法的投影与排序字段。
    for ty in &types {
        for field in &selector.fields { if field != "key" { validate_field(session, ty, field)?; } }
    }
    for sort in &selector.sort {
        let field = sort.strip_prefix('-').unwrap_or(sort);
        if field.is_empty() || field.starts_with('-') { return Err(Failure::request("sort requires a field path with at most one descending prefix")); }
        if field == "key" || field == "type" { continue; }
        let mut common = None;
        for ty in &types {
            let path = resolved_path(session, ty, field)?;
            let mut expected = crate::write_rules::expected_type_for_cfd_path(session.schema(), ty, &path, "CLI-SORT", "CLI")
                .map_err(|diagnostics| Failure { status: super::Status::Error, error_category: Some(super::ErrorCategory::Request), diagnostics, result: None })?;
            while let coflow_language::cft::CftValueType::Option(inner) = expected { expected = *inner; }
            use coflow_language::cft::CftValueType as T;
            if expected == T::FormattedString { expected = T::String; }
            if !matches!(expected, T::Int | T::Float | T::Bool | T::String | T::Enum(_) | T::RecordRef(_)) {
                return Err(Failure::request(format!("field `{ty}.{field}` cannot be sorted")));
            }
            if common.as_ref().is_some_and(|previous| previous != &expected) {
                return Err(Failure::request(format!("sort field `{field}` has incompatible types across the selected range")));
            }
            common = Some(expected);
        }
    }
    let dimensions = session.queries().dimensions();
    for name in selector.dimensions.keys() {
        if !dimensions.iter().any(|d| &d.name == name) { return Err(Failure::missing(format!("unknown dimension `{name}`"))); }
    }
    let mut records = Vec::new();
    for (_, record) in session.model().records() {
        if selector.actual_type.as_ref().is_some_and(|ty| record.actual_type() != ty &&
            !(selector.include_derived && session.schema().is_assignable(record.actual_type(), ty))) { continue; }
        if selector.key.as_ref().is_some_and(|key| record.key() != key) { continue; }
        if selector.file.as_ref().is_some_and(|file| session.queries().file_for_record(record.actual_type(), record.key()) != Some(file.as_str())) { continue; }
        let mut record = record.clone();
        for (name, values) in &record.dimension_fields {
            if let Some(value) = selector.dimensions.get(values.dimension.as_str()).and_then(|v| values.variants.get(v.as_str())) {
                record.object.fields.insert(name.clone(), value.value.clone());
            }
        }
        if let Some(text) = &selector.text {
            if !record.key().contains(text) && !record.fields().values().any(|v| crate::value_summary(v).contains(text)) { continue; }
        }
        if let Some(filters) = &filters {
            let filter = filters.get(record.actual_type()).ok_or_else(|| Failure::request("filter type is outside the selected range"))?;
            if !filter.matches(&record) { continue; }
        }
        records.push(record);
    }
    // 排序键只解析一次；任何读取错误都在排序前返回，不能被比较器吞掉。
    let mut keyed = records.into_iter().map(|record| {
        let keys = selector.sort.iter().map(|sort| {
            let name = sort.strip_prefix('-').unwrap_or(sort);
            match name {
                "key" => Ok(Some(CfdValue::String(record.key().into()))),
                "type" => Ok(Some(CfdValue::String(record.actual_type().into()))),
                _ => Ok(value_at(session, &record, name)?.cloned()),
            }
        }).collect::<Result<Vec<_>>>()?;
        Ok((record, keys))
    }).collect::<Result<Vec<_>>>()?;
    keyed.sort_by(|(a, a_keys), (b, b_keys)| {
        for ((a_value, b_value), sort) in a_keys.iter().zip(b_keys).zip(&selector.sort) {
            let order = compare_values(a_value.as_ref(), b_value.as_ref());
            let order = if sort.starts_with('-') { order.reverse() } else { order };
            if order != Ordering::Equal { return order; }
        }
        a.actual_type().cmp(b.actual_type()).then(a.key().cmp(b.key()))
    });
    Ok(keyed.into_iter().map(|(record, _)| record).collect())
}

pub(crate) fn selected_types(session: &ReadOnlyProjectSession, selector: &Selector) -> Result<Vec<String>> {
    if let Some(name) = &selector.actual_type {
        if session.schema().resolve_type(name).is_none() { return Err(Failure::missing(format!("unknown type `{name}`"))); }
    }
    Ok(session.schema().all_types().filter(|ty| {
        if let Some(name) = &selector.actual_type {
            ty.name.as_str() == name || selector.include_derived && !ty.is_abstract && !ty.is_data() && session.schema().is_assignable(&ty.name, name)
        } else { !ty.is_abstract && !ty.is_data() }
    }).map(|ty|ty.name.to_string()).collect())
}

pub(crate) fn compare_values(a: Option<&CfdValue>, b: Option<&CfdValue>) -> Ordering {
    match (a, b) {
        (None, None) => Ordering::Equal, (None, _) => Ordering::Less, (_, None) => Ordering::Greater,
        (Some(CfdValue::OptionNone), Some(CfdValue::OptionNone)) => Ordering::Equal,
        (Some(CfdValue::OptionNone), _) => Ordering::Less, (_, Some(CfdValue::OptionNone)) => Ordering::Greater,
        (Some(CfdValue::OptionSome(a)), Some(CfdValue::OptionSome(b))) => compare_values(Some(a), Some(b)),
        (Some(CfdValue::OptionSome(a)), b) => compare_values(Some(a), b),
        (a, Some(CfdValue::OptionSome(b))) => compare_values(a, Some(b)),
        (Some(CfdValue::Int(a)), Some(CfdValue::Int(b))) => a.cmp(b),
        (Some(CfdValue::Float(a)), Some(CfdValue::Float(b))) => if a == b { Ordering::Equal } else { a.total_cmp(b) },
        (Some(CfdValue::String(a)), Some(CfdValue::String(b))) => a.cmp(b),
        (Some(CfdValue::FormattedString(a)), Some(CfdValue::FormattedString(b))) => a.rendered.cmp(&b.rendered),
        (Some(CfdValue::String(a)), Some(CfdValue::FormattedString(b))) => a.cmp(&b.rendered),
        (Some(CfdValue::FormattedString(a)), Some(CfdValue::String(b))) => a.rendered.cmp(b),
        (Some(CfdValue::Bool(a)), Some(CfdValue::Bool(b))) => a.cmp(b),
        (Some(CfdValue::Enum(a)), Some(CfdValue::Enum(b))) => a.enum_name.cmp(&b.enum_name).then(a.value.cmp(&b.value)),
        (Some(CfdValue::Ref(a)), Some(CfdValue::Ref(b))) => a.cmp(b),
        (Some(a), Some(b)) => crate::value_summary(a).cmp(&crate::value_summary(b)),
    }
}

pub fn query_records(project: &Project, session: &ReadOnlyProjectSession, selector: &Selector) -> Result<Page<Value>> {
    let records = selected_records(session, selector)?;
    let items = records.iter().map(|record| record_json(project, session, record, selector)).collect::<Result<Vec<_>>>()?;
    paginate(items, selector)
}

pub(crate) fn record_json(project: &Project, session: &ReadOnlyProjectSession, record: &CfdRecord, selector: &Selector) -> Result<Value> {
    if !["source", "effective", "both"].contains(&selector.view.as_str()) { return Err(Failure::request("view must be source, effective or both")); }
    let mut values = serde_json::Map::new();
    let mut sources = serde_json::Map::new();
    let mut rendered = serde_json::Map::new();
    let file = session.queries().file_for_record(record.actual_type(), record.key());
    let text = file.map(|file| std::fs::read_to_string(project.resolve_path(std::path::Path::new(file)))).transpose()?;
    let ast = text.as_ref().map(|s| coflow_language::cfd::parse_cfd(s).0);
    let declaration = ast.as_ref().and_then(|ast| ast.records.iter().find(|r|r.key == record.key() && r.type_name == record.actual_type()));
    let fields: Vec<_> = if selector.fields.is_empty() { record.fields().keys().map(ToString::to_string).collect() } else { selector.fields.clone() };
    for field in fields {
        if field == "key" { continue; }
        let path = resolved_path(session, record.actual_type(), &field)?;
        let value = record.value_at_path(&CfdPath { segments: path.clone() });
        let expected = crate::write_rules::expected_type_for_record_path(session.schema(), record, &path, "CLI-PATH", "CLI")?;
        let encoded = super::model_value::encode(session.schema(), &expected, value)?;
        values.insert(field.clone(), encoded.clone());
        if let Some(value) = value.and_then(super::model_value::rendered) { rendered.insert(field.clone(), value); }
        let top = path.first().and_then(|s| if let CfdPathSegment::Field(n) = s { Some(n) } else { None });
        let assignment = top.and_then(|name|declaration.and_then(|r|r.fields.iter().find(|f|&f.name==name)));
        let mut location = assignment.zip(text.as_deref()).zip(file).map(|((f,text),file)|super::schema_queries::location(&project.resolve_path(std::path::Path::new(file)),text,f.span));
        let override_value = top.and_then(|n| record.dimension_fields.get(n.as_str())).and_then(|d|
            selector.dimensions.get(d.dimension.as_str()).and_then(|v| d.variants.get(v.as_str())));
        let origin = if override_value.is_some() { "dimension_override" }
            else if assignment.is_some() { "explicit_base" }
            else if top.and_then(|n| session.schema().field(record.actual_type(), n)).is_some_and(|f| f.default.is_some()) { "schema_default" }
            else { "option_default" };
        if origin == "schema_default" {
            if let Some(f)=top.and_then(|name|session.schema().field(record.actual_type(),name)) {
                location=session.schema().resolve_type(&f.declaring_type).and_then(|t|session.schema().source(&t.module)).map(|s|super::schema_queries::location(&s.path,&s.source,f.span));
            }
        }
        let expression=assignment.zip(text.as_deref()).and_then(|(f,text)|{let span=ast_value_span(&f.value);text.get(span.start..span.end)});
        sources.insert(field, json!({"origin": origin, "location": location, "assigned": assignment.is_some(), "expression": expression, "effective": encoded, "reachable": value.is_some()}));
    }
    let mut output = json!({"type": record.actual_type(), "key": record.key(), "file": file});
    output["rendered"] = Value::Object(rendered);
    if selector.view != "source" { output["fields"] = Value::Object(values); output["origins"] = Value::Object(sources.clone()); }
    if selector.view != "effective" {
        output["source"] = Value::Object(sources);
    }
    Ok(output)
}

pub(crate) fn ast_value_span(value:&coflow_language::cfd::CfdValue)->coflow_language::source::Span {
    let mut span=value.span();
    if let coflow_language::cfd::CfdValue::Block(block)=value {if let Some((_,marker))=&block.type_marker{span.start=marker.start;}}
    span
}

pub(crate) fn validate_field(session: &ReadOnlyProjectSession, ty: &str, field: &str) -> Result<()> {
    let path = resolved_path(session, ty, field)?;
    crate::write_rules::expected_type_for_cfd_path(session.schema(), ty, &path, "CLI-FIELD", "CLI")?;
    Ok(())
}

pub(crate) fn value_at<'a>(session: &ReadOnlyProjectSession, record: &'a CfdRecord, field: &str) -> Result<Option<&'a CfdValue>> {
    Ok(record.value_at_path(&CfdPath { segments: resolved_path(session, record.actual_type(), field)? }))
}

pub(crate) fn resolved_path(session: &ReadOnlyProjectSession, ty: &str, field: &str) -> Result<Vec<CfdPathSegment>> {
    super::operations::internal_path(session.schema(), ty, &super::execute::public_path(field)?)
}

pub fn parse_path(source: &str) -> Result<Vec<CfdPathSegment>> {
    let mut offset = 0;
    let mut out = Vec::new();
    let bytes = source.as_bytes();
    // 每个字段、分隔符和下标都消费完整语法，禁止把损坏路径解释成另一条合法路径。
    loop {
        let start = offset;
        while offset < bytes.len() && bytes[offset] != b'.' && bytes[offset] != b'[' { offset += 1; }
        let field = &source[start..offset];
        coflow_language::cft::FieldName::new(field).map_err(|e|Failure::request(e.to_string()))?;
        out.push(CfdPathSegment::Field(field.into()));
        while offset < bytes.len() && bytes[offset] == b'[' {
            offset += 1;
            let start = offset;
            let mut quoted = false;
            let mut escaped = false;
            while offset < bytes.len() {
                let ch = bytes[offset];
                if ch == b']' && !quoted { break; }
                if ch == b'"' && !escaped { quoted = !quoted; }
                escaped = ch == b'\\' && !escaped;
                offset += 1;
            }
            if offset == bytes.len() { return Err(Failure::request("unclosed field path subscript")); }
            let key = source[start..offset].trim();
            if key.is_empty() { return Err(Failure::request("empty field path subscript")); }
            if key.starts_with('"') {
                let _: String = serde_json::from_str(key)?;
                out.push(CfdPathSegment::DictKey(key.into()));
            } else if let Ok(index) = key.parse::<usize>() { out.push(CfdPathSegment::Index(index)); }
            else { out.push(CfdPathSegment::DictKey(key.into())); }
            offset += 1;
        }
        if offset == bytes.len() { return Ok(out); }
        if bytes[offset] != b'.' { return Err(Failure::request("field path segments require a dot separator")); }
        offset += 1;
        if offset == bytes.len() { return Err(Failure::request("field path has a trailing separator")); }
    }
}
