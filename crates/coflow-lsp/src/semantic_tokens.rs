use coflow_cft::ast::{
    Annotation, AnnotationArg, CheckExpr, CheckExprKind, CheckStmt, ConstLiteral, DefaultExpr,
    DefaultExprKind, Item, TypeRef, TypeRefKind,
};
use coflow_cft::lexer::{lex, TokenKind};
use coflow_cft::{ModuleId, Span};

use crate::position::position_from_byte;
use crate::{enum_name_exists, enum_variant_exists, LspBuild, LspDocument};

pub(crate) const SEMANTIC_TOKEN_TYPES: &[&str] = &[
    "namespace",
    "type",
    "enum",
    "enumMember",
    "property",
    "variable",
    "function",
    "keyword",
    "number",
    "string",
    "comment",
    "operator",
    "decorator",
    "parameter",
];
pub(crate) const SEMANTIC_TOKEN_MODIFIERS: &[&str] =
    &["declaration", "reference", "path", "record", "schema"];

#[cfg(test)]
pub(crate) const SEM_NAMESPACE: u32 = 0;
pub(crate) const SEM_TYPE: u32 = 1;
pub(crate) const SEM_ENUM: u32 = 2;
pub(crate) const SEM_ENUM_MEMBER: u32 = 3;
pub(crate) const SEM_PROPERTY: u32 = 4;
pub(crate) const SEM_VARIABLE: u32 = 5;
pub(crate) const SEM_FUNCTION: u32 = 6;
pub(crate) const SEM_KEYWORD: u32 = 7;
pub(crate) const SEM_NUMBER: u32 = 8;
pub(crate) const SEM_STRING: u32 = 9;
pub(crate) const SEM_COMMENT: u32 = 10;
pub(crate) const SEM_OPERATOR: u32 = 11;
pub(crate) const SEM_DECORATOR: u32 = 12;
pub(crate) const SEM_PARAMETER: u32 = 13;

pub(crate) const MOD_DECLARATION: u32 = 1 << 0;
pub(crate) const MOD_REFERENCE: u32 = 1 << 1;
pub(crate) const MOD_PATH: u32 = 1 << 2;
#[cfg(test)]
pub(crate) const MOD_RECORD: u32 = 1 << 3;
pub(crate) const MOD_SCHEMA: u32 = 1 << 4;

#[derive(Clone)]
pub(crate) struct RawSemanticToken {
    pub(crate) line: usize,
    pub(crate) character: usize,
    pub(crate) length: usize,
    pub(crate) token_type: u32,
    pub(crate) token_modifiers: u32,
}

pub(crate) fn push_semantic_span(
    source: &str,
    span: Span,
    token_type: u32,
    token_modifiers: u32,
    tokens: &mut Vec<RawSemanticToken>,
) {
    if span.end <= span.start {
        return;
    }
    let start = position_from_byte(source, span.start);
    let end = position_from_byte(source, span.end);
    if start.line != end.line || end.character <= start.character {
        return;
    }
    tokens.push(RawSemanticToken {
        line: start.line,
        character: start.character,
        length: end.character - start.character,
        token_type,
        token_modifiers,
    });
}

pub(crate) fn push_semantic_span_plain(
    source: &str,
    span: Span,
    token_type: u32,
    tokens: &mut Vec<RawSemanticToken>,
) {
    push_semantic_span(source, span, token_type, 0, tokens);
}

pub(crate) fn encode_semantic_tokens(mut tokens: Vec<RawSemanticToken>) -> Vec<u32> {
    tokens.sort_by_key(|token| (token.line, token.character, token.length));
    let mut deduped = Vec::new();
    let mut last_end = (0, 0);
    let mut has_last = false;
    for token in tokens {
        if has_last && (token.line, token.character) < last_end {
            continue;
        }
        last_end = (token.line, token.character + token.length);
        has_last = true;
        deduped.push(token);
    }

    let mut data = Vec::with_capacity(deduped.len() * 5);
    let mut previous_line = 0;
    let mut previous_character = 0;
    for token in deduped {
        let delta_line = token.line - previous_line;
        let delta_start = if delta_line == 0 {
            token.character - previous_character
        } else {
            token.character
        };
        data.push(usize_to_u32_saturating(delta_line));
        data.push(usize_to_u32_saturating(delta_start));
        data.push(usize_to_u32_saturating(token.length));
        data.push(token.token_type);
        data.push(token.token_modifiers);
        previous_line = token.line;
        previous_character = token.character;
    }
    data
}

fn usize_to_u32_saturating(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

pub(crate) fn add_comment_semantic_tokens(source: &str, tokens: &mut Vec<RawSemanticToken>) {
    let mut line_start = 0;
    for line in source.split_inclusive('\n') {
        if let Some(comment_start) = comment_start_in_line(line) {
            let start = line_start + comment_start;
            let end = line_start + line.trim_end_matches(['\r', '\n']).len();
            push_semantic_span_plain(source, Span::new(start, end), SEM_COMMENT, tokens);
        }
        line_start += line.len();
    }
}

pub(crate) fn comment_start_in_line(line: &str) -> Option<usize> {
    let mut in_string = false;
    let mut escaped = false;
    for (index, ch) in line.char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
        } else if ch == '"' {
            in_string = true;
        } else if ch == '#' {
            return Some(index);
        }
    }
    None
}

pub(crate) fn semantic_token_data(build: &LspBuild, document: &LspDocument) -> Vec<u32> {
    encode_semantic_tokens(semantic_raw_tokens(build, document))
}

pub(crate) fn semantic_raw_tokens(
    build: &LspBuild,
    document: &LspDocument,
) -> Vec<RawSemanticToken> {
    let mut tokens = Vec::new();
    add_comment_semantic_tokens(&document.source, &mut tokens);
    if let Ok(lexed) = lex(&ModuleId::new(document.module_id.clone()), &document.source) {
        for token in lexed {
            add_lex_semantic_token(&document.source, &token.kind, token.span, &mut tokens);
        }
    }
    if let Some(ast) = &document.ast {
        add_ast_semantic_tokens(build, document, ast, &mut tokens);
    }
    tokens
}

fn add_lex_semantic_token(
    source: &str,
    kind: &TokenKind,
    span: Span,
    tokens: &mut Vec<RawSemanticToken>,
) {
    let token_type = match kind {
        TokenKind::Const
        | TokenKind::Enum
        | TokenKind::Type
        | TokenKind::Abstract
        | TokenKind::Sealed
        | TokenKind::Check
        | TokenKind::When
        | TokenKind::All
        | TokenKind::Any
        | TokenKind::None
        | TokenKind::In
        | TokenKind::Is
        | TokenKind::True
        | TokenKind::False
        | TokenKind::Null => SEM_KEYWORD,
        TokenKind::Int(_) | TokenKind::UIntOverflow(_) | TokenKind::Float(_) => SEM_NUMBER,
        TokenKind::String(_) => SEM_STRING,
        TokenKind::Plus
        | TokenKind::Minus
        | TokenKind::Star
        | TokenKind::Slash
        | TokenKind::SlashSlash
        | TokenKind::Percent
        | TokenKind::StarStar
        | TokenKind::Less
        | TokenKind::Greater
        | TokenKind::Bang
        | TokenKind::Tilde
        | TokenKind::Amp
        | TokenKind::Pipe
        | TokenKind::Caret
        | TokenKind::AmpAmp
        | TokenKind::PipePipe
        | TokenKind::LessEq
        | TokenKind::GreaterEq
        | TokenKind::LessLess
        | TokenKind::GreaterGreater
        | TokenKind::EqEq
        | TokenKind::BangEq
        | TokenKind::Equal => SEM_OPERATOR,
        _ => return,
    };
    push_semantic_span_plain(source, span, token_type, tokens);
}

fn add_ast_semantic_tokens(
    build: &LspBuild,
    document: &LspDocument,
    ast: &coflow_cft::ast::ModuleAst,
    tokens: &mut Vec<RawSemanticToken>,
) {
    for annotation in &ast.dangling_annotations {
        add_annotation_semantic(document, annotation, tokens);
    }
    for item in &ast.items {
        match item {
            Item::Const(constant) => {
                for annotation in &constant.annotations {
                    add_annotation_semantic(document, annotation, tokens);
                }
                push_semantic_span(
                    &document.source,
                    constant.name_span,
                    SEM_VARIABLE,
                    MOD_DECLARATION | MOD_SCHEMA,
                    tokens,
                );
                if let Some(ty) = &constant.ty {
                    add_type_ref_semantic(build, document, ty, tokens);
                }
                add_const_literal_semantic(document, &constant.value, tokens);
            }
            Item::Enum(enum_def) => {
                for annotation in &enum_def.annotations {
                    add_annotation_semantic(document, annotation, tokens);
                }
                for annotation in &enum_def.dangling_annotations {
                    add_annotation_semantic(document, annotation, tokens);
                }
                push_semantic_span(
                    &document.source,
                    enum_def.name_span,
                    SEM_ENUM,
                    MOD_DECLARATION | MOD_SCHEMA,
                    tokens,
                );
                for variant in &enum_def.variants {
                    for annotation in &variant.annotations {
                        add_annotation_semantic(document, annotation, tokens);
                    }
                    push_semantic_span(
                        &document.source,
                        variant.name_span,
                        SEM_ENUM_MEMBER,
                        MOD_DECLARATION | MOD_SCHEMA,
                        tokens,
                    );
                    if let Some(value) = &variant.value {
                        push_semantic_span_plain(&document.source, value.span, SEM_NUMBER, tokens);
                    }
                }
            }
            Item::Type(ty) => {
                for annotation in &ty.annotations {
                    add_annotation_semantic(document, annotation, tokens);
                }
                for annotation in &ty.dangling_annotations {
                    add_annotation_semantic(document, annotation, tokens);
                }
                push_semantic_span(
                    &document.source,
                    ty.name_span,
                    SEM_TYPE,
                    MOD_DECLARATION | MOD_SCHEMA,
                    tokens,
                );
                if let Some(parent) = &ty.parent {
                    push_semantic_span(
                        &document.source,
                        parent.span,
                        SEM_TYPE,
                        MOD_REFERENCE | MOD_SCHEMA,
                        tokens,
                    );
                }
                for field in &ty.fields {
                    for annotation in &field.annotations {
                        add_annotation_semantic(document, annotation, tokens);
                    }
                    push_semantic_span(
                        &document.source,
                        field.name_span,
                        SEM_PROPERTY,
                        MOD_DECLARATION | MOD_SCHEMA,
                        tokens,
                    );
                    add_type_ref_semantic(build, document, &field.ty, tokens);
                    if let Some(default) = &field.default {
                        add_default_expr_semantic(document, default, tokens);
                    }
                }
                if let Some(check) = &ty.check {
                    for stmt in &check.stmts {
                        add_check_stmt_semantic(build, document, stmt, tokens);
                    }
                }
            }
        }
    }
}

fn add_annotation_semantic(
    document: &LspDocument,
    annotation: &Annotation,
    tokens: &mut Vec<RawSemanticToken>,
) {
    push_semantic_span_plain(
        &document.source,
        annotation.name_span,
        SEM_DECORATOR,
        tokens,
    );
    for arg in &annotation.args {
        match arg {
            AnnotationArg::Name(name) => {
                push_semantic_span_plain(&document.source, name.span, SEM_VARIABLE, tokens);
            }
            AnnotationArg::String(_, span) => {
                push_semantic_span_plain(&document.source, *span, SEM_STRING, tokens);
            }
            AnnotationArg::Int(_, span) | AnnotationArg::Float(_, span) => {
                push_semantic_span_plain(&document.source, *span, SEM_NUMBER, tokens);
            }
            AnnotationArg::Bool(_, span) | AnnotationArg::Null(span) => {
                push_semantic_span_plain(&document.source, *span, SEM_KEYWORD, tokens);
            }
        }
    }
}

fn add_type_ref_semantic(
    build: &LspBuild,
    document: &LspDocument,
    ty: &TypeRef,
    tokens: &mut Vec<RawSemanticToken>,
) {
    match &ty.kind {
        TypeRefKind::Int | TypeRefKind::Float | TypeRefKind::Bool | TypeRefKind::String => {
            push_semantic_span(
                &document.source,
                ty.span,
                SEM_TYPE,
                MOD_REFERENCE | MOD_SCHEMA,
                tokens,
            );
        }
        TypeRefKind::Named(name) => {
            let token_type = if enum_name_exists(build, name) {
                SEM_ENUM
            } else {
                SEM_TYPE
            };
            push_semantic_span(
                &document.source,
                ty.span,
                token_type,
                MOD_REFERENCE | MOD_SCHEMA,
                tokens,
            );
        }
        TypeRefKind::Array(inner) | TypeRefKind::Nullable(inner) => {
            add_type_ref_semantic(build, document, inner, tokens);
        }
        TypeRefKind::Ref(inner) => add_type_ref_semantic(build, document, inner, tokens),
        TypeRefKind::Dict(key, value) => {
            add_type_ref_semantic(build, document, key, tokens);
            add_type_ref_semantic(build, document, value, tokens);
        }
    }
}

fn add_const_literal_semantic(
    document: &LspDocument,
    literal: &ConstLiteral,
    tokens: &mut Vec<RawSemanticToken>,
) {
    match literal {
        ConstLiteral::Int(_, span) | ConstLiteral::Float(_, span) => {
            push_semantic_span_plain(&document.source, *span, SEM_NUMBER, tokens);
        }
        ConstLiteral::Bool(_, span) => {
            push_semantic_span_plain(&document.source, *span, SEM_KEYWORD, tokens);
        }
        ConstLiteral::String(_, span) => {
            push_semantic_span_plain(&document.source, *span, SEM_STRING, tokens);
        }
    }
}

fn add_default_expr_semantic(
    document: &LspDocument,
    expr: &DefaultExpr,
    tokens: &mut Vec<RawSemanticToken>,
) {
    match &expr.kind {
        DefaultExprKind::Int(_) | DefaultExprKind::Float(_) => {
            push_semantic_span_plain(&document.source, expr.span, SEM_NUMBER, tokens);
        }
        DefaultExprKind::Bool(_) | DefaultExprKind::Null => {
            push_semantic_span_plain(&document.source, expr.span, SEM_KEYWORD, tokens);
        }
        DefaultExprKind::String(_) => {
            push_semantic_span_plain(&document.source, expr.span, SEM_STRING, tokens);
        }
        DefaultExprKind::Name(name) => {
            push_semantic_span(
                &document.source,
                name.span,
                SEM_VARIABLE,
                MOD_REFERENCE | MOD_SCHEMA,
                tokens,
            );
        }
        DefaultExprKind::EnumVariant { enum_name, variant } => {
            push_semantic_span(
                &document.source,
                enum_name.span,
                SEM_ENUM,
                MOD_REFERENCE | MOD_SCHEMA,
                tokens,
            );
            push_semantic_span(
                &document.source,
                variant.span,
                SEM_ENUM_MEMBER,
                MOD_REFERENCE | MOD_SCHEMA,
                tokens,
            );
        }
        DefaultExprKind::Array(items) => {
            for item in items {
                add_default_expr_semantic(document, item, tokens);
            }
        }
        DefaultExprKind::Object(fields) => {
            for (name, value) in fields {
                push_semantic_span(
                    &document.source,
                    name.span,
                    SEM_PROPERTY,
                    MOD_DECLARATION | MOD_SCHEMA,
                    tokens,
                );
                add_default_expr_semantic(document, value, tokens);
            }
        }
    }
}

fn add_check_stmt_semantic(
    build: &LspBuild,
    document: &LspDocument,
    stmt: &CheckStmt,
    tokens: &mut Vec<RawSemanticToken>,
) {
    match stmt {
        CheckStmt::Expr(expr) => add_check_expr_semantic(build, document, expr, tokens),
        CheckStmt::Quantifier {
            binding,
            collection,
            body,
            ..
        } => {
            push_semantic_span(
                &document.source,
                binding.span,
                SEM_PARAMETER,
                MOD_DECLARATION,
                tokens,
            );
            add_check_expr_semantic(build, document, collection, tokens);
            for stmt in body {
                add_check_stmt_semantic(build, document, stmt, tokens);
            }
        }
        CheckStmt::When {
            condition, body, ..
        } => {
            add_check_expr_semantic(build, document, condition, tokens);
            for stmt in body {
                add_check_stmt_semantic(build, document, stmt, tokens);
            }
        }
    }
}

#[allow(clippy::too_many_lines)]
fn add_check_expr_semantic(
    build: &LspBuild,
    document: &LspDocument,
    expr: &CheckExpr,
    tokens: &mut Vec<RawSemanticToken>,
) {
    match &expr.kind {
        CheckExprKind::Int(_) | CheckExprKind::Float(_) => {
            push_semantic_span_plain(&document.source, expr.span, SEM_NUMBER, tokens);
        }
        CheckExprKind::Bool(_) | CheckExprKind::Null => {
            push_semantic_span_plain(&document.source, expr.span, SEM_KEYWORD, tokens);
        }
        CheckExprKind::String(_) => {
            push_semantic_span_plain(&document.source, expr.span, SEM_STRING, tokens);
        }
        CheckExprKind::Name(_) => {
            push_semantic_span(
                &document.source,
                expr.span,
                SEM_VARIABLE,
                MOD_REFERENCE,
                tokens,
            );
        }
        CheckExprKind::Field { expr, name } => {
            if let CheckExprKind::Name(enum_name) = &expr.kind {
                if enum_variant_exists(build, enum_name, &name.name) {
                    push_semantic_span(
                        &document.source,
                        expr.span,
                        SEM_ENUM,
                        MOD_REFERENCE | MOD_SCHEMA,
                        tokens,
                    );
                    push_semantic_span(
                        &document.source,
                        name.span,
                        SEM_ENUM_MEMBER,
                        MOD_REFERENCE | MOD_SCHEMA,
                        tokens,
                    );
                    return;
                }
            }
            add_check_expr_semantic(build, document, expr, tokens);
            push_semantic_span(
                &document.source,
                name.span,
                SEM_PROPERTY,
                MOD_REFERENCE | MOD_PATH | MOD_SCHEMA,
                tokens,
            );
        }
        CheckExprKind::Index { expr, index } => {
            add_check_expr_semantic(build, document, expr, tokens);
            add_check_expr_semantic(build, document, index, tokens);
        }
        CheckExprKind::Is { expr, predicate } => {
            add_check_expr_semantic(build, document, expr, tokens);
            match predicate {
                coflow_cft::ast::TypePredicate::Type(name) => {
                    push_semantic_span(
                        &document.source,
                        name.span,
                        SEM_TYPE,
                        MOD_REFERENCE | MOD_SCHEMA,
                        tokens,
                    );
                }
                coflow_cft::ast::TypePredicate::Null(span) => {
                    push_semantic_span_plain(&document.source, *span, SEM_KEYWORD, tokens);
                }
            }
        }
        CheckExprKind::Call { name, args } => {
            let token_type = if enum_name_exists(build, &name.name) {
                SEM_ENUM
            } else {
                SEM_FUNCTION
            };
            let modifiers = if token_type == SEM_ENUM {
                MOD_REFERENCE | MOD_SCHEMA
            } else {
                MOD_REFERENCE
            };
            push_semantic_span(&document.source, name.span, token_type, modifiers, tokens);
            for arg in args {
                add_check_expr_semantic(build, document, arg, tokens);
            }
        }
        CheckExprKind::MethodCall {
            receiver,
            name,
            args,
        } => {
            add_check_expr_semantic(build, document, receiver, tokens);
            push_semantic_span(
                &document.source,
                name.span,
                SEM_FUNCTION,
                MOD_REFERENCE,
                tokens,
            );
            for arg in args {
                add_check_expr_semantic(build, document, arg, tokens);
            }
        }
        CheckExprKind::BinOp { lhs, rhs, .. } => {
            add_check_expr_semantic(build, document, lhs, tokens);
            add_check_expr_semantic(build, document, rhs, tokens);
        }
        CheckExprKind::Unary { expr, .. } => {
            add_check_expr_semantic(build, document, expr, tokens);
        }
        CheckExprKind::CmpChain { first, rest } => {
            add_check_expr_semantic(build, document, first, tokens);
            for (_, expr) in rest {
                add_check_expr_semantic(build, document, expr, tokens);
            }
        }
    }
}
