use super::{Candidate, RecordTarget, Result};
use coflow_language::{
    cfd::{CfdField, CfdValue as A},
    cft::{CftSchema, CftValueType},
    source::Span,
};
use std::fs;

pub fn rename_references(
    candidate: &mut Candidate,
    schema: &CftSchema,
    record: &RecordTarget,
    new_id: &str,
) -> Result<()> {
    for file in candidate.data_sources()? {
        let mut text = file.text.clone();
        let ast = &file.ast;
        let mut patches = Vec::new();
        for r in ast
            .records
            .iter()
            .chain(ast.dimensions.iter().flat_map(|b| &b.records))
        {
            fields(
                schema,
                &r.type_name,
                &r.fields,
                record,
                new_id,
                &r.type_name,
                &mut patches,
            );
        }
        patches.sort_by_key(|(span, _)| std::cmp::Reverse(span.start));
        if !patches.is_empty() {
            candidate.automatic_updates.push(serde_json::json!({"kind":"references","file":crate::project_path(candidate.original.root_dir(),&candidate.unshadow(&file.absolute)?),
                "count":patches.len(),"record":record,"new_id":new_id}));
        }
        for (span, value) in patches {
            text.replace_range(span.start..span.end, &value);
        }
        if text != file.text {
            fs::write(&file.absolute, &text)?;
            candidate.refresh_source(&file.absolute, Some(text))?;
        }
    }
    rename_schema_references(candidate, schema, record, new_id)
}
fn fields(
    schema: &CftSchema,
    ty: &str,
    fields: &[CfdField],
    record: &RecordTarget,
    new_id: &str,
    host_type: &str,
    patches: &mut Vec<(Span, String)>,
) {
    for field in fields {
        if let Some(meta) = schema.field(ty, &field.name) {
            visit(
                schema,
                &meta.value_type,
                &field.value,
                record,
                new_id,
                host_type,
                patches,
            );
        }
    }
}
fn visit(
    schema: &CftSchema,
    ty: &CftValueType,
    value: &A,
    record: &RecordTarget,
    new_id: &str,
    host_type: &str,
    patches: &mut Vec<(Span, String)>,
) {
    if let CftValueType::Option(inner) = ty {
        if let A::OptionSome(value, _) = value {
            visit(schema, inner, value, record, new_id, host_type, patches);
        } else {
            visit(schema, inner, value, record, new_id, host_type, patches);
        }
        return;
    }
    match (ty, value) {
        (CftValueType::RecordRef(target), A::Ref(reference))
            if reference.id.0 == record.id && schema.is_assignable(&record.actual_type, target) =>
        {
            patches.push((reference.id.1, new_id.into()));
        }
        (CftValueType::Enum(name), A::Scalar(raw, span)) => {
            if let Some(value) = renamed_enum(schema, name, raw, record, new_id) {
                patches.push((*span, value));
            }
        }
        (CftValueType::Object(expected), A::Block(block)) => {
            let actual = block
                .type_marker
                .as_ref()
                .map_or(expected.as_str(), |(ty, _)| ty.as_str());
            fields(
                schema,
                actual,
                &block.fields,
                record,
                new_id,
                host_type,
                patches,
            );
        }
        (CftValueType::Array(inner), A::Array(items, _)) => {
            for item in items {
                visit(schema, inner, item, record, new_id, host_type, patches);
            }
        }
        (CftValueType::Dict(key_type, inner), A::Block(block)) => {
            for field in &block.fields {
                if let CftValueType::Enum(name) = key_type.as_ref() {
                    if let Some(value) = renamed_enum(schema, name, &field.name, record, new_id) {
                        patches.push((field.name_span, value));
                    }
                }
                visit(
                    schema,
                    inner,
                    &field.value,
                    record,
                    new_id,
                    host_type,
                    patches,
                );
            }
        }
        (_, A::FormattedString(formatted)) => {
            for segment in &formatted.segments {
                if let coflow_language::cfd::CfdFormatSegment::Reference(reference) = segment {
                    if reference.id.as_deref() == Some(&record.id)
                        && schema.is_assignable(
                            &record.actual_type,
                            reference.type_name.as_deref().unwrap_or(host_type),
                        )
                    {
                        let target = reference
                            .type_name
                            .as_ref()
                            .map(|target| format!("{target}::"))
                            .unwrap_or_default();
                        patches.push((
                            reference.span,
                            format!("&{target}{new_id}.{}", reference.path.join(".")),
                        ));
                    }
                }
            }
        }
        _ => {}
    }
}

fn renamed_enum(
    schema: &CftSchema,
    name: &str,
    raw: &str,
    record: &RecordTarget,
    new_id: &str,
) -> Option<String> {
    if schema.inherited_id_as_enum(&record.actual_type)?.as_str() != name {
        return None;
    }
    let (prefix, key) = raw
        .rsplit_once("::")
        .map_or((None, raw), |(prefix, key)| (Some(prefix), key));
    if key != record.id {
        return None;
    }
    Some(prefix.map_or_else(
        || new_id.to_string(),
        |prefix| format!("{prefix}::{new_id}"),
    ))
}

fn rename_schema_references(
    candidate: &mut Candidate,
    schema: &CftSchema,
    record: &RecordTarget,
    new_id: &str,
) -> Result<()> {
    use coflow_language::cft::syntax::ast::Item;
    let session = candidate.schema_session()?;
    for (_, module) in session.modules().modules() {
        let Some(ast) = module.ast() else {
            continue;
        };
        let mut patches = Vec::new();
        for item in &ast.items {
            match item {
                Item::Type(ty) => {
                    for field in &ty.fields {
                        if let Some(default) = &field.default {
                            visit_default(
                                schema,
                                default,
                                module.source(),
                                &ty.name,
                                record,
                                new_id,
                                &mut patches,
                            );
                        }
                    }
                    for check in &ty.checks {
                        rename_rule_members(schema, check, record, new_id, &mut patches);
                    }
                }
                Item::Const(constant) => visit_default(
                    schema,
                    &constant.value,
                    module.source(),
                    &record.actual_type,
                    record,
                    new_id,
                    &mut patches,
                ),
                Item::Check(check) => {
                    rename_rule_members(schema, &check.block, record, new_id, &mut patches);
                }
                _ => {}
            }
        }
        if patches.is_empty() {
            continue;
        }
        patches.sort_by_key(|(span, _)| std::cmp::Reverse(span.start));
        let mut text = module.source().to_string();
        candidate.automatic_updates.push(serde_json::json!({"kind":"schema_references","file":crate::project_path(candidate.original.root_dir(),&candidate.unshadow(module.path())?),
            "count":patches.len(),"record":record,"new_id":new_id}));
        for (span, value) in patches {
            text.replace_range(span.start..span.end, &value);
        }
        fs::write(module.path(), &text)?;
        candidate.refresh_source(module.path(), Some(text))?;
    }
    Ok(())
}

fn rename_rule_members(
    schema: &CftSchema,
    block: &coflow_language::cft::syntax::ast::CheckBlock,
    record: &RecordTarget,
    new_id: &str,
    patches: &mut Vec<(Span, String)>,
) {
    use coflow_language::cft::syntax::{
        ast::{CheckExpr, CheckExprKind},
        CheckVisitor,
    };
    struct Visitor<'a> {
        schema: &'a CftSchema,
        record: &'a RecordTarget,
        new_id: &'a str,
        patches: &'a mut Vec<(Span, String)>,
    }
    impl CheckVisitor for Visitor<'_> {
        type Error = std::convert::Infallible;
        fn visit_expr(&mut self, expression: &CheckExpr) -> std::result::Result<(), Self::Error> {
            if let CheckExprKind::StaticPath(path) = &expression.kind {
                if let Some(name) = self.schema.inherited_id_as_enum(&self.record.actual_type) {
                    if let Some((member, types)) = path.segments.split_last() {
                        if member.name == self.record.id
                            && types
                                .iter()
                                .map(|part| part.name.as_str())
                                .collect::<Vec<_>>()
                                .join("::")
                                == name.as_str()
                        {
                            self.patches.push((member.span, self.new_id.into()));
                        }
                    }
                }
            }
            self.walk_expr(expression)
        }
    }
    let _ = Visitor {
        schema,
        record,
        new_id,
        patches,
    }
    .visit_block(block);
}

fn visit_default(
    schema: &CftSchema,
    expression: &coflow_language::cft::syntax::ast::DefaultExpr,
    source: &str,
    host_type: &str,
    record: &RecordTarget,
    new_id: &str,
    patches: &mut Vec<(Span, String)>,
) {
    use coflow_language::cft::syntax::ast::DefaultExprKind as E;
    match &expression.kind {
        E::RecordReference(path) => {
            if let Some((key, types)) = path.segments.split_last() {
                let target = types
                    .iter()
                    .map(|part| part.name.as_str())
                    .collect::<Vec<_>>()
                    .join("::");
                if key.name == record.id && schema.is_assignable(&record.actual_type, &target) {
                    patches.push((key.span, new_id.into()));
                }
            }
        }
        E::StaticPath(path) => {
            if let Some(name) = schema.inherited_id_as_enum(&record.actual_type) {
                if let Some((key, types)) = path.segments.split_last() {
                    if key.name == record.id
                        && types
                            .iter()
                            .map(|part| part.name.as_str())
                            .collect::<Vec<_>>()
                            .join("::")
                            == name.as_str()
                    {
                        patches.push((key.span, new_id.into()));
                    }
                }
            }
        }
        E::FormattedString(_) => {
            if let Some(text) = source.get(expression.span.start..expression.span.end) {
                let prefix = "value: Value { field: ";
                let ast = coflow_language::cfd::parse_cfd(&format!("{prefix}{text} }}")).0;
                if let Some(value) = ast
                    .records
                    .first()
                    .and_then(|record| record.fields.first())
                    .map(|field| &field.value)
                {
                    let mut local = Vec::new();
                    visit(
                        schema,
                        &CftValueType::String,
                        value,
                        record,
                        new_id,
                        host_type,
                        &mut local,
                    );
                    patches.extend(local.into_iter().map(|(span, value)| {
                        (
                            Span::new(
                                span.start - prefix.len() + expression.span.start,
                                span.end - prefix.len() + expression.span.start,
                            ),
                            value,
                        )
                    }));
                }
            }
        }
        E::OptionSome(value) => {
            visit_default(schema, value, source, host_type, record, new_id, patches);
        }
        E::Array(values) => {
            for value in values {
                visit_default(schema, value, source, host_type, record, new_id, patches);
            }
        }
        E::Object(fields) | E::TypedObject { fields, .. } => {
            for (_, value) in fields {
                visit_default(schema, value, source, host_type, record, new_id, patches);
            }
        }
        E::Dictionary(values) => {
            for (key, value) in values {
                visit_default(schema, key, source, host_type, record, new_id, patches);
                visit_default(schema, value, source, host_type, record, new_id, patches);
            }
        }
        E::BitExpr { lhs, rhs, .. } => {
            visit_default(schema, lhs, source, host_type, record, new_id, patches);
            visit_default(schema, rhs, source, host_type, record, new_id, patches);
        }
        _ => {}
    }
}

pub fn rename_mapping(
    candidate: &mut Candidate,
    schema: &CftSchema,
    record: &RecordTarget,
    new_id: &str,
) -> Result<()> {
    let Some(name) = schema.inherited_id_as_enum(&record.actual_type) else {
        return Ok(());
    };
    let path = candidate
        .project()?
        .root_dir()
        .join("coflow.enum.lock.json");
    if !path.exists() {
        return Ok(());
    }
    let mut values: serde_json::Value = serde_json::from_slice(&fs::read(&path)?)?;
    if let Some(entries) = values
        .get_mut(name.as_str())
        .and_then(serde_json::Value::as_object_mut)
    {
        if let Some(value) = entries.remove(&record.id) {
            entries.insert(new_id.into(), value);
            fs::write(&path, serde_json::to_vec_pretty(&values)?)?;
            candidate.automatic_updates.push(serde_json::json!({"kind":"stable_mapping","file":"coflow.enum.lock.json","record":record,"new_id":new_id}));
        }
    }
    Ok(())
}
