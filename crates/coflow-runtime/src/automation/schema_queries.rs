pub use super::schema_inspection::inspect;
use super::{Failure, Result};
use crate::{Project, ProjectSchemaSession};
use coflow_language::cft::CftValueType;
use serde_json::{json, Value};
use std::path::Path;

#[derive(Debug, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SchemaUseKind {
    Declaration,
    TypeReference,
    DefaultValue,
    RuleExpression,
    Data,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct SchemaUse {
    pub kind: SchemaUseKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub record: Option<super::RecordTarget>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dimension: Option<super::ReferenceDimension>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location: Option<super::diagnostics::CommandLocation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rule: Option<super::RuleDescription>,
}

#[derive(Debug, serde::Serialize, schemars::JsonSchema)]
pub struct SchemaReferences {
    pub symbol: String,
    pub references: Vec<SchemaUse>,
    pub total: usize,
    pub truncated: bool,
    pub next_cursor: Option<String>,
}

pub fn location(
    root: &Path,
    path: &Path,
    source: &str,
    span: coflow_language::source::Span,
) -> super::diagnostics::CommandLocation {
    let range = crate::TextRange::from_byte_offsets(source, span.start, span.end);
    super::diagnostics::CommandLocation::from_source(
        &crate::SourceLocation::FileSpan {
            path: path.to_path_buf(),
            start_line: range.start.line,
            start_character: range.start.character,
            end_line: range.end.line,
            end_character: range.end.character,
        },
        Some(root),
    )
}

pub fn mentions(ty: &CftValueType, symbol: &str) -> bool {
    match ty {
        CftValueType::Object(n) | CftValueType::RecordRef(n) => n.as_str() == symbol,
        CftValueType::Enum(n) => n.as_str() == symbol,
        CftValueType::Array(t) | CftValueType::Option(t) => mentions(t, symbol),
        CftValueType::Dict(k, v) => mentions(k, symbol) || mentions(v, symbol),
        _ => false,
    }
}

pub fn references(
    project: &Project,
    session: &ProjectSchemaSession,
    args: &super::Arguments,
    data: &crate::ReadOnlyProjectSession,
) -> Result<Value> {
    let symbol = args.require("symbol")?;
    let schema = session
        .schema()
        .ok_or_else(|| Failure::missing("schema is unavailable"))?;
    let field = symbol
        .rsplit_once('.')
        .and_then(|(owner, name)| schema.field(owner, name));
    let rule = schema
        .all_check_statements()
        .any(|info| super::rules::selected(info, symbol));
    if schema.resolve_type(symbol).is_none()
        && schema.resolve_enum(symbol).is_none()
        && schema.resolve_const(symbol).is_none()
        && field.is_none()
        && !rule
    {
        return Err(Failure::missing(format!("unknown symbol `{symbol}`")));
    }
    let mut items = Vec::new();
    if let Some(field) = field {
        let source = schema
            .resolve_type(&field.declaring_type)
            .and_then(|owner| schema.source(&owner.module));
        items.push(json!({"kind": "declaration", "symbol": format!("{}.{}", field.declaring_type, field.name),
            "location": source.map(|source| location(project.root_dir(), &source.path, &source.source, field.span))}));
    }
    if let Some(declaration) = schema.resolve_enum(symbol) {
        let source = schema.source(&declaration.module);
        items.push(json!({"kind": "declaration", "symbol": symbol, "location": source.map(|source| location(project.root_dir(), &source.path, &source.source, declaration.span))}));
    }
    if let Some(declaration) = schema.resolve_const(symbol) {
        let source = schema.source(&declaration.module);
        items.push(json!({"kind": "declaration", "symbol": symbol, "location": source.map(|source| location(project.root_dir(), &source.path, &source.source, declaration.span))}));
    }
    // 已折叠的常量与枚举表达式从 AST 恢复语义身份和逐处源码位置。
    for (_, module) in session.modules().modules() {
        let Some(ast) = module.ast() else {
            continue;
        };
        for item in &ast.items {
            use coflow_language::cft::syntax::ast::Item;
            let mut uses = Vec::new();
            match item {
                Item::Type(ty) => {
                    for field in &ty.fields {
                        if let Some(default) = &field.default {
                            default_static_paths(default, &mut uses);
                        }
                        for (_, span) in uses.drain(..).filter(|(name, _)| {
                            name == symbol && schema.resolve_const(name).is_some()
                        }) {
                            items.push(json!({"kind":"default_value","symbol":format!("{}.{}",ty.name,field.name),
                                "location":location(project.root_dir(),module.path(),module.source(),span)}));
                        }
                    }
                    if let Some(block) = &ty.check {
                        check_static_paths(
                            &block.stmts,
                            &mut uses,
                            &schema
                                .resolve_type(&ty.name)
                                .map(|ty| {
                                    ty.all_fields()
                                        .map(|field| field.name.to_string())
                                        .chain(std::iter::once("id".into()))
                                        .collect()
                                })
                                .unwrap_or_default(),
                        );
                    }
                }
                Item::Const(constant) => {
                    default_static_paths(&constant.value, &mut uses);
                    for (_, span) in uses
                        .drain(..)
                        .filter(|(name, _)| static_symbol_matches(schema, name, symbol))
                    {
                        items.push(json!({"kind":"default_value","symbol":constant.name,
                            "location":location(project.root_dir(),module.path(),module.source(),span)}));
                    }
                }
                Item::Check(check) => {
                    check_static_paths(&check.block.stmts, &mut uses, &Default::default())
                }
                _ => {}
            }
            for (_, span) in uses
                .into_iter()
                .filter(|(name, _)| static_symbol_matches(schema, name, symbol))
            {
                items.push(json!({"kind":"rule_expression","location":location(project.root_dir(),module.path(),module.source(),span)}));
            }
        }
    }

    for constant in schema
        .all_consts()
        .filter(|constant| mentions(&constant.value_type, symbol))
    {
        items.push(json!({"kind":"type_reference","symbol":constant.name,"location":schema.source(&constant.module)
            .map(|source|location(project.root_dir(),&source.path,&source.source,constant.span))}));
    }
    for ty in schema.all_types() {
        let source = schema.source(&ty.module);
        if ty.name.as_str() == symbol {
            items.push(json!({"kind": "declaration", "symbol": ty.name, "location": source.map(|s| location(project.root_dir(), &s.path, &s.source, ty.span))}));
        }
        if ty.parent.as_ref().is_some_and(|p| p.as_str() == symbol) {
            items.push(json!({"kind": "type_reference", "symbol": ty.name, "location": source.map(|s| location(project.root_dir(), &s.path, &s.source, ty.span))}));
        }
        for field in ty.all_fields().filter(|f| f.declaring_type == ty.name) {
            if mentions(&field.value_type, symbol) {
                items.push(json!({"kind": "type_reference", "symbol": format!("{}.{}",ty.name,field.name), "location": source.map(|s| location(project.root_dir(), &s.path, &s.source, field.span))}));
            }
            if field
                .default
                .as_ref()
                .is_some_and(|value| default_mentions(value, symbol))
            {
                items.push(json!({"kind": "default_value", "symbol": format!("{}.{}", ty.name, field.name),
                    "location": source.map(|source| location(project.root_dir(), &source.path, &source.source, field.span))}));
            }
        }
    }
    for info in schema.all_check_statements() {
        if rule && super::rules::selected(info, symbol) {
            items.push(json!({"kind": "declaration", "symbol": super::rules::identity(info), "rule": super::rules::describe(schema, info, project.root_dir())}));
        }
        if info.dependencies.iter().any(|d| match d {
            coflow_language::cft::CheckDependency::Field(f) => {
                f.owner.as_str() == symbol
                    || field.is_some_and(|target| {
                        schema.field(&f.owner, &f.field).is_some_and(|dependency| {
                            dependency.declaring_type == target.declaring_type
                                && dependency.name == target.name
                        })
                    })
            }
            coflow_language::cft::CheckDependency::RecordSet(t) => t.as_str() == symbol,
        }) {
            items.push(json!({"kind": "rule_expression", "rule": super::rules::describe(schema,info,project.root_dir())}));
        }
    }
    // 使用源码赋值树定位 CFD 引用；有效模型中的默认值不能伪装成显式使用。
    for (file, source) in data.session.source_data.sources() {
        let ast = coflow_language::cfd::parse_cfd(source).0;
        let path = project.resolve_path(Path::new(file));
        for (record, dimension) in
            ast.records
                .iter()
                .map(|record| (record, None))
                .chain(ast.dimensions.iter().flat_map(|block| {
                    block.records.iter().map(move |record| {
                        (
                            record,
                            Some(super::ReferenceDimension {
                                dimension: block.dimension.clone(),
                                variant: block.variant.clone(),
                            }),
                        )
                    })
                }))
        {
            if schema.resolve_type(&record.type_name).is_none() {
                continue;
            }
            let coordinate = super::RecordTarget {
                actual_type: record.type_name.clone(),
                key: record.key.clone(),
            };
            if record.type_name == symbol {
                items.push(
                    json!({"kind":"data","record":coordinate,"dimension":dimension,
                "location":location(project.root_dir(),&path,source,record.type_span)}),
                );
            }
            for assignment in &record.fields {
                let Some(meta) = schema.field(&record.type_name, &assignment.name) else {
                    continue;
                };
                if field.is_some_and(|target| {
                    target.declaring_type == meta.declaring_type && target.name == meta.name
                }) {
                    items.push(json!({"kind":"data","record":coordinate,"field":assignment.name,"dimension":dimension,
                        "location":location(project.root_dir(),&path,source,assignment.span)}));
                }
                source_value_uses(
                    schema,
                    &assignment.value,
                    &meta.value_type,
                    symbol,
                    field,
                    &coordinate,
                    &assignment.name,
                    dimension.as_ref(),
                    project.root_dir(),
                    &path,
                    source,
                    &mut items,
                );
            }
        }
    }
    let items = items
        .into_iter()
        .map(serde_json::from_value)
        .collect::<std::result::Result<Vec<SchemaUse>, _>>()?;
    let page = super::query::paginate(items, &args.selector()?)?;
    super::json(SchemaReferences {
        symbol: symbol.into(),
        references: page.items,
        total: page.total,
        truncated: page.truncated,
        next_cursor: page.next_cursor,
    })
}

#[allow(clippy::too_many_arguments)]
fn source_value_uses(
    schema: &coflow_language::cft::CftSchema,
    value: &coflow_language::cfd::CfdValue,
    expected: &CftValueType,
    symbol: &str,
    target_field: Option<&coflow_language::cft::CftField>,
    record: &super::RecordTarget,
    field_path: &str,
    dimension: Option<&super::ReferenceDimension>,
    root: &Path,
    file: &Path,
    source: &str,
    items: &mut Vec<Value>,
) {
    use coflow_language::cfd::CfdValue as V;
    let mut push = |span| {
        items.push(
            json!({"kind":"data","record":record,"field":field_path,"dimension":dimension,
        "location":location(root,file,source,span)}),
        )
    };
    match (expected, value) {
        (CftValueType::Option(_), V::OptionNone(_)) => {}
        (CftValueType::Option(inner), V::OptionSome(value, _)) => source_value_uses(
            schema,
            value,
            inner,
            symbol,
            target_field,
            record,
            field_path,
            dimension,
            root,
            file,
            source,
            items,
        ),
        (CftValueType::Option(inner), value) => source_value_uses(
            schema,
            value,
            inner,
            symbol,
            target_field,
            record,
            field_path,
            dimension,
            root,
            file,
            source,
            items,
        ),
        (CftValueType::Array(inner), V::Array(values, _)) => {
            for (index, value) in values.iter().enumerate() {
                source_value_uses(
                    schema,
                    value,
                    inner,
                    symbol,
                    target_field,
                    record,
                    &format!("{field_path}[{index}]"),
                    dimension,
                    root,
                    file,
                    source,
                    items,
                );
            }
        }
        (CftValueType::Object(name), V::Block(block)) => {
            let actual = block
                .type_marker
                .as_ref()
                .map_or(name.as_str(), |(name, _)| name.as_str());
            if actual == symbol {
                push(
                    block
                        .type_marker
                        .as_ref()
                        .map_or(block.span, |(_, span)| *span),
                );
            }
            for assignment in &block.fields {
                let Some(meta) = schema.field(actual, &assignment.name) else {
                    continue;
                };
                let nested_path = format!("{field_path}.{}", assignment.name);
                if target_field.is_some_and(|target| {
                    target.declaring_type == meta.declaring_type && target.name == meta.name
                }) {
                    items.push(json!({"kind":"data","record":record,"field":nested_path,"dimension":dimension,
                        "location":location(root,file,source,assignment.span)}));
                }
                source_value_uses(
                    schema,
                    &assignment.value,
                    &meta.value_type,
                    symbol,
                    target_field,
                    record,
                    &nested_path,
                    dimension,
                    root,
                    file,
                    source,
                    items,
                );
            }
        }
        (CftValueType::Dict(key, inner), V::Block(block)) => {
            for assignment in &block.fields {
                if matches!(key.as_ref(),CftValueType::Enum(name) if name.as_str()==symbol) {
                    items.push(json!({"kind":"data","record":record,"field":field_path,"dimension":dimension,
                    "location":location(root,file,source,assignment.name_span)}));
                }
                source_value_uses(
                    schema,
                    &assignment.value,
                    inner,
                    symbol,
                    target_field,
                    record,
                    &format!("{field_path}[{}]", assignment.name),
                    dimension,
                    root,
                    file,
                    source,
                    items,
                );
            }
        }
        (CftValueType::Enum(name), _) if name.as_str() == symbol => push(value.span()),
        (CftValueType::RecordRef(name), V::Ref(reference))
            if reference
                .type_name
                .as_ref()
                .map_or(name.as_str(), |(name, _)| name.as_str())
                == symbol =>
        {
            push(reference.span)
        }
        (_, V::FormattedString(formatted)) => {
            for segment in &formatted.segments {
                if let coflow_language::cfd::CfdFormatSegment::Reference(reference) = segment {
                    let owner = reference
                        .type_name
                        .as_deref()
                        .unwrap_or(&record.actual_type);
                    if owner == symbol
                        || target_field.is_some_and(|target| {
                            reference
                                .path
                                .first()
                                .and_then(|name| schema.field(owner, name))
                                .is_some_and(|meta| {
                                    meta.declaring_type == target.declaring_type
                                        && meta.name == target.name
                                })
                        })
                    {
                        push(reference.span);
                    }
                }
            }
        }
        _ => {}
    }
}

fn default_mentions(value: &coflow_language::cft::CftSchemaDefaultValue, symbol: &str) -> bool {
    use coflow_language::cft::CftSchemaDefaultValue as V;
    // 遍历已解析的类型身份，字符串内容与同名标识符不构成符号引用。
    match value {
        V::Enum { enum_name, .. } => enum_name.as_str() == symbol,
        V::RecordReference { type_name, .. } => type_name.as_str() == symbol,
        V::Object { type_name, fields } => {
            type_name.as_str() == symbol
                || fields
                    .iter()
                    .any(|(_, value)| default_mentions(value, symbol))
        }
        V::OptionSome(value) => default_mentions(value, symbol),
        V::Array(values) => values.iter().any(|value| default_mentions(value, symbol)),
        V::Dictionary(entries) => entries
            .iter()
            .any(|(key, value)| default_mentions(key, symbol) || default_mentions(value, symbol)),
        _ => false,
    }
}

pub(super) fn default_static_paths(
    expression: &coflow_language::cft::syntax::ast::DefaultExpr,
    uses: &mut Vec<(String, coflow_language::source::Span)>,
) {
    use coflow_language::cft::syntax::ast::DefaultExprKind as E;
    // 编译会折叠常量值；源码 AST 保留实际常量路径和精确位置，字符串不参与匹配。
    match &expression.kind {
        E::StaticPath(path) => uses.push((path.canonical(), path.span)),
        E::OptionSome(value) => default_static_paths(value, uses),
        E::BitExpr { lhs, rhs, .. } => {
            default_static_paths(lhs, uses);
            default_static_paths(rhs, uses);
        }
        E::Array(values) => {
            for value in values {
                default_static_paths(value, uses);
            }
        }
        E::Object(fields) | E::TypedObject { fields, .. } => {
            for (_, value) in fields {
                default_static_paths(value, uses);
            }
        }
        E::Dictionary(entries) => {
            for (key, value) in entries {
                default_static_paths(key, uses);
                default_static_paths(value, uses);
            }
        }
        _ => {}
    }
}

fn static_symbol_matches(
    schema: &coflow_language::cft::CftSchema,
    name: &str,
    symbol: &str,
) -> bool {
    name == symbol && schema.resolve_const(name).is_some()
        || name.rsplit_once("::").is_some_and(|(owner, variant)| {
            owner == symbol
                && schema.resolve_enum(owner).is_some_and(|declaration| {
                    declaration
                        .variants
                        .iter()
                        .any(|item| item.name.as_str() == variant)
                })
        })
}

fn check_static_paths(
    statements: &[coflow_language::cft::syntax::ast::CheckStmt],
    uses: &mut Vec<(String, coflow_language::source::Span)>,
    shadowed: &std::collections::BTreeSet<String>,
) {
    use coflow_language::cft::syntax::ast::{CheckFormatSegment, CheckMessageKind, CheckStmt as S};
    for statement in statements {
        match statement {
            S::Expr {
                condition, message, ..
            } => {
                check_expression_paths(condition, uses, shadowed);
                if let Some(message) = message {
                    if let CheckMessageKind::Formatted(segments) = &message.kind {
                        for segment in segments {
                            if let CheckFormatSegment::Expr(expression) = segment {
                                check_expression_paths(expression, uses, shadowed);
                            }
                        }
                    }
                }
            }
            S::Quantifier {
                collection,
                body,
                bindings,
                ..
            } => {
                check_expression_paths(collection, uses, shadowed);
                let mut scope = shadowed.clone();
                scope.extend(bindings.iter().map(|binding| binding.name.clone()));
                check_static_paths(body, uses, &scope);
            }
            S::When {
                condition, body, ..
            } => {
                check_expression_paths(condition, uses, shadowed);
                let mut scope = shadowed.clone();
                condition_bindings(condition, &mut scope);
                check_static_paths(body, uses, &scope);
            }
        }
    }
}
fn check_expression_paths(
    expression: &coflow_language::cft::syntax::ast::CheckExpr,
    uses: &mut Vec<(String, coflow_language::source::Span)>,
    shadowed: &std::collections::BTreeSet<String>,
) {
    use coflow_language::cft::syntax::ast::{CheckExprKind as E, CheckFormatSegment};
    match &expression.kind {
        E::StaticPath(path) => uses.push((path.canonical(), path.span)),
        E::Name(name) if !shadowed.contains(name) => uses.push((name.clone(), expression.span)),
        E::FormattedString(segments) => {
            for segment in segments {
                if let CheckFormatSegment::Expr(expression) = segment {
                    check_expression_paths(expression, uses, shadowed);
                }
            }
        }
        E::Field { expr, .. } | E::Is { expr, .. } | E::Unary { expr, .. } => {
            check_expression_paths(expr, uses, shadowed)
        }
        E::Index { expr, index } => {
            check_expression_paths(expr, uses, shadowed);
            check_expression_paths(index, uses, shadowed);
        }
        E::BinOp {
            op: coflow_language::cft::syntax::ast::BinOp::And,
            lhs,
            rhs,
        } => {
            check_expression_paths(lhs, uses, shadowed);
            let mut scope = shadowed.clone();
            condition_bindings(lhs, &mut scope);
            check_expression_paths(rhs, uses, &scope);
        }
        E::BinOp { lhs, rhs, .. } => {
            check_expression_paths(lhs, uses, shadowed);
            check_expression_paths(rhs, uses, shadowed);
        }
        E::Call { args, .. } => {
            for expression in args {
                check_expression_paths(expression, uses, shadowed);
            }
        }
        E::MethodCall { receiver, args, .. } => {
            check_expression_paths(receiver, uses, shadowed);
            for expression in args {
                check_expression_paths(expression, uses, shadowed);
            }
        }
        E::CmpChain { first, rest } => {
            check_expression_paths(first, uses, shadowed);
            for (_, expression) in rest {
                check_expression_paths(expression, uses, shadowed);
            }
        }
        _ => {}
    }
}

/// 与编译器一致，仅肯定成立的 Some 模式与 && 链把绑定带入后续表达式。
fn condition_bindings(
    expression: &coflow_language::cft::syntax::ast::CheckExpr,
    scope: &mut std::collections::BTreeSet<String>,
) {
    use coflow_language::cft::syntax::ast::{BinOp, CheckExprKind as E, TypePredicate};
    match &expression.kind {
        E::Is {
            predicate: TypePredicate::Some { binding, .. },
            ..
        } => {
            scope.insert(binding.name.clone());
        }
        E::BinOp {
            op: BinOp::And,
            lhs,
            rhs,
        } => {
            condition_bindings(lhs, scope);
            condition_bindings(rhs, scope);
        }
        _ => {}
    }
}
