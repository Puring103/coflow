//! 编辑中的构造块允许语法尚未闭合；声明类型仍由语言解析器和 schema 解析。
use super::*;
use coflow_language::function::{parse_expression, parse_function, BuildSource, Expr, ExprKind};
use std::collections::BTreeMap;

#[derive(Clone)]
struct Local { ty: Option<CftValueType>, builder: bool }
type Scope = BTreeMap<String, Local>;
fn lookup<'a>(scopes: &'a [Scope], name: &str) -> Option<&'a Local> {
    scopes.iter().rev().find_map(|scope| scope.get(name))
}
fn expression_type(expr: &Expr, schema: &CftSchema, scopes: &[Scope]) -> Option<CftValueType> {
    match &expr.kind {
        ExprKind::Name(name) => lookup(scopes, name)?.ty.clone(),
        ExprKind::Field { value, name } => {
            let CftValueType::Object(ty) = expression_type(value, schema, scopes)? else { return None; };
            Some(schema.field(&ty, name)?.value_type.clone())
        }
        ExprKind::Index { value, .. } => match expression_type(value, schema, scopes)? {
            CftValueType::Array(ty) | CftValueType::Dict(_, ty) => Some(*ty), _ => None,
        },
        ExprKind::Object { type_name, .. } => Some(CftValueType::Object(schema.resolve_type(type_name)?.name.clone())),
        _ => None,
    }
}
pub(super) fn members(signature: &str, prefix: &str, schema: &CftSchema) -> Option<Vec<Value>> {
    let tokens = coflow_language::lexical::tokenize_lossless(prefix).into_iter()
        .filter(|token| !token.is_trivia()).collect::<Vec<_>>();
    let text = |index: usize| tokens.get(index).map(|token| token.text(prefix)).unwrap_or("");
    let end = tokens.len().checked_sub(1)?;
    let dot = if text(end) == "." { end } else if end > 0 && text(end - 1) == "." { end - 1 } else { return None; };
    let receiver = text(dot.checked_sub(1)?);
    let mut scopes = vec![Scope::new()];
    if let Ok(function) = parse_function(&format!("{signature} {{}}")) {
        for (name, ty) in function.parameters { scopes[0].insert(name, Local { ty: schema.resolve_type_ref(&ty).ok(), builder: false }); }
    }
    let mut pending = BTreeMap::<usize, (String, Local)>::new();
    for index in 0..dot {
        match text(index) {
            "var" => {
                let name = text(index + 1);
                let ty = if text(index + 2) == ":" {
                    (index + 3..dot).find(|i| text(*i) == "=").and_then(|equal| {
                        let start = tokens.get(index + 3)?.span.start;
                        let end = tokens[equal].span.start;
                        let function = parse_function(&format!("fn(value: {}) -> int {{}}", &prefix[start..end])).ok()?;
                        schema.resolve_type_ref(&function.parameters[0].1).ok()
                    })
                } else { None };
                scopes.last_mut()?.insert(name.into(), Local { ty, builder: false });
            }
            "build" => {
                // 整个头部交回正式解析器，避免另造一份集合/命名类型文法。
                let Some(as_index) = (index + 1..dot).find(|i| text(*i) == "as") else { continue; };
                if text(as_index + 2) != "{" { continue; }
                let head = &prefix[tokens[index].span.start..tokens[as_index + 2].span.start];
                let Ok(expr) = parse_expression(&format!("{head} {{}}")) else { continue; };
                let ExprKind::Build { source, binding, .. } = expr.kind else { continue; };
                let ty = match source { BuildSource::Type(ty) => schema.resolve_type_ref(&ty).ok(), BuildSource::Value(value) => expression_type(&value, schema, &scopes) };
                pending.insert(as_index + 2, (binding, Local { ty, builder: true }));
            }
            "{" => { let mut scope = Scope::new(); if let Some((name, local)) = pending.remove(&index) { scope.insert(name, local); } scopes.push(scope); }
            "}" if scopes.len() > 1 => { scopes.pop(); }
            _ => {},
        }
    }
    let local = lookup(&scopes, receiver)?;
    if !local.builder { return None; }
    let mut items = Vec::new();
    match local.ty.as_ref()? {
        CftValueType::Object(name) => {
            for field in schema.resolve_type(name)?.all_fields() {
                items.push(json!({ "label": field.name, "kind": 5, "detail": field.value_type.display_label() }));
            }
        }
        CftValueType::Array(inner) => {
            items.push(json!({ "label": "append", "kind": 2, "detail": format!("append(value: {})", inner.display_label()), "insertText": "append(${1:value})", "insertTextFormat": 2 }));
            items.push(json!({ "label": "remove", "kind": 2, "detail": "remove(index: int)", "insertText": "remove(${1:index})", "insertTextFormat": 2 }));
        }
        CftValueType::Dict(key, _) => items.push(json!({ "label": "remove", "kind": 2, "detail": format!("remove(key: {})", key.display_label()), "insertText": "remove(${1:key})", "insertTextFormat": 2 })),
        _ => {},
    }
    Some(items)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn typed_builder_members_follow_source_types_and_scope() {
        use coflow_core::schema::{build_schema, parse_modules, CftFile, ModuleId};
        let schema = build_schema(&parse_modules([CftFile::from_source(ModuleId::from("builders"), "data Item { count: int; title: string; }")])).unwrap();
        let labels = |signature: &str, prefix: &str| members(signature, prefix, &schema).map(|items| items.into_iter().map(|item| item["label"].as_str().unwrap().to_owned()).collect::<Vec<_>>());
        assert_eq!(labels("fn() -> [int]", "fn() -> [int] { build [int] as b { b."), Some(vec!["append".into(), "remove".into()]));
        assert_eq!(labels("fn() -> {string: int}", "fn() -> {string: int} { build {string: int} as b { b.re"), Some(vec!["remove".into()]));
        assert_eq!(labels("fn() -> Item", "fn() -> Item { build Item as b { b."), Some(vec!["count".into(), "title".into()]));
        assert_eq!(labels("fn(input: [int]) -> [int]", "fn(input: [int]) -> [int] { build (input) as b { b."), Some(vec!["append".into(), "remove".into()]));
        assert_eq!(labels("fn() -> [int]", "fn() -> [int] { var original: [int] = []; build (original) as b { b."), Some(vec!["append".into(), "remove".into()]));
        assert!(labels("fn() -> int", "fn() -> int { build [int] as b { } b.").is_none());
        assert!(labels("fn() -> int", "fn() -> int { build [int] as b { var b: int = 0; b.").is_none());
    }
}
