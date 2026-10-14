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
}
impl Default for Selector {
    fn default() -> Self { Self { actual_type: None, include_derived: false, file: None,
        key: None, fields: Vec::new(), dimensions: BTreeMap::new(), sort: Vec::new(),
        limit: 50, cursor: None, text: None, view: "effective".into(), filter: None } }
}

#[derive(Debug, Serialize)]
pub struct Page<T> { pub items: Vec<T>, pub total: usize, pub truncated: bool, pub next_cursor: Option<String> }

pub(crate) fn paginate<T>(items: Vec<T>, selector: &Selector) -> Result<Page<T>> {
    if selector.limit == 0 { return Err(Failure::request("limit must be greater than zero")); }
    // 游标仅标记同一查询的读取位置，不绑定项目文件状态。
    let mut identity = selector.clone(); identity.cursor = None; identity.limit = 0;
    let query = fingerprint(&serde_json::to_vec(&identity)?);
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
    if let Some(ty) = &selector.actual_type {
        if session.schema().resolve_type(ty).is_none() { return Err(Failure::missing(format!("unknown type `{ty}`"))); }
        for field in &selector.fields {
            if field != "key" { validate_field(session, ty, field)?; }
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
        if let Some(filter) = &selector.filter {
            if !super::filter::matches(session, &record, filter)? { continue; }
        }
        records.push(record);
    }
    // 排序只使用语义值，整数之间不经浮点转换，避免大整数失去顺序。
    for sort in &selector.sort {
        let path = sort.trim_start_matches('-');
        if path != "key" && path != "type" {
            for record in &records { validate_field(session, record.actual_type(), path)?; }
        }
    }
    records.sort_by(|a, b| {
        for sort in &selector.sort {
            let name = sort.trim_start_matches('-');
            let order = match name {
                "key" => a.key().cmp(b.key()), "type" => a.actual_type().cmp(b.actual_type()),
                _ => compare_values(value_at(a, name).ok().flatten(), value_at(b, name).ok().flatten()),
            };
            let order = if sort.starts_with('-') { order.reverse() } else { order };
            if order != Ordering::Equal { return order; }
        }
        a.actual_type().cmp(b.actual_type()).then(a.key().cmp(b.key()))
    });
    Ok(records)
}

pub(crate) fn compare_values(a: Option<&CfdValue>, b: Option<&CfdValue>) -> Ordering {
    match (a, b) {
        (Some(CfdValue::Int(a)), Some(CfdValue::Int(b))) => a.cmp(b),
        (Some(CfdValue::Float(a)), Some(CfdValue::Float(b))) => a.total_cmp(b),
        (Some(CfdValue::String(a)), Some(CfdValue::String(b))) => a.cmp(b),
        (Some(CfdValue::Bool(a)), Some(CfdValue::Bool(b))) => a.cmp(b),
        (Some(CfdValue::Enum(a)), Some(CfdValue::Enum(b))) => a.enum_name.cmp(&b.enum_name).then(a.value.cmp(&b.value)),
        (Some(CfdValue::OptionSome(a)), Some(CfdValue::OptionSome(b))) => compare_values(Some(a), Some(b)),
        (None, None) => Ordering::Equal, (None, _) => Ordering::Less, (_, None) => Ordering::Greater,
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
    let file = session.queries().file_for_record(record.actual_type(), record.key());
    let text = file.map(|file| std::fs::read_to_string(project.resolve_path(std::path::Path::new(file)))).transpose()?;
    let ast = text.as_ref().map(|s| coflow_language::cfd::parse_cfd(s).0);
    let declaration = ast.as_ref().and_then(|ast| ast.records.iter().find(|r|r.key == record.key() && r.type_name == record.actual_type()));
    let fields: Vec<_> = if selector.fields.is_empty() { record.fields().keys().map(ToString::to_string).collect() } else { selector.fields.clone() };
    for field in fields {
        if field == "key" { continue; }
        let path = parse_path(&field)?;
        let value = record.value_at_path(&CfdPath { segments: path.clone() });
        values.insert(field.clone(), super::json(value)?);
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
        sources.insert(field, json!({"origin": origin, "location": location, "assigned": assignment.is_some(), "expression": expression, "effective": value}));
    }
    let mut output = json!({"type": record.actual_type(), "key": record.key(), "file": file});
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
    let path = parse_path(field)?;
    crate::write_rules::expected_type_for_cfd_path(session.schema(), ty, &path, "CLI-FIELD", "CLI")?;
    Ok(())
}

pub(crate) fn value_at<'a>(record: &'a CfdRecord, field: &str) -> Result<Option<&'a CfdValue>> {
    Ok(record.value_at_path(&CfdPath { segments: parse_path(field)? }))
}

pub fn parse_path(source: &str) -> Result<Vec<CfdPathSegment>> {
    let mut chars = source.char_indices().peekable();
    let mut out = Vec::new();
    let mut field = String::new();
    while let Some((_, ch)) = chars.next() {
        match ch {
            '.' => { if !field.is_empty() { out.push(CfdPathSegment::Field(std::mem::take(&mut field))); } else if out.is_empty() { return Err(Failure::request("empty field path segment")); } }
            '[' => {
                if !field.is_empty() { out.push(CfdPathSegment::Field(std::mem::take(&mut field))); }
                let mut key = String::new(); let mut quoted = false; let mut escaped = false; let mut closed = false;
                for (_, c) in chars.by_ref() {
                    if c == ']' && !quoted { closed = true; break; }
                    if c == '"' && !escaped { quoted = !quoted; }
                    escaped = c == '\\' && !escaped;
                    key.push(c);
                }
                if !closed { return Err(Failure::request("unclosed field path subscript")); }
                let key = key.trim();
                if key.starts_with('"') { let _: String = serde_json::from_str(key)?; out.push(CfdPathSegment::DictKey(key.into())); }
                else if let Ok(index) = key.parse() { out.push(CfdPathSegment::Index(index)); }
                else { out.push(CfdPathSegment::DictKey(key.into())); }
            }
            _ => field.push(ch),
        }
    }
    if !field.is_empty() { out.push(CfdPathSegment::Field(field)); }
    if out.is_empty() { return Err(Failure::request("field path is empty")); }
    Ok(out)
}
