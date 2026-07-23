use coflow_cft::{syntax::ast::Item, Span};
use serde_json::{json, Value};

use crate::{position::range_from_span, LspDocument};

const SYMBOL_KIND_CLASS: u8 = 5;
const SYMBOL_KIND_FIELD: u8 = 8;
const SYMBOL_KIND_ENUM: u8 = 10;
const SYMBOL_KIND_CONSTANT: u8 = 14;
const SYMBOL_KIND_ENUM_MEMBER: u8 = 22;
const SYMBOL_KIND_FUNCTION: u8 = 12;

pub(crate) fn document_symbols(document: &LspDocument) -> Vec<Value> {
    let Some(ast) = &document.ast else {
        return Vec::new();
    };
    let mut symbols = Vec::new();
    for item in &ast.items {
        match item {
            Item::Const(constant) => symbols.push(document_symbol_item(
                &document.source,
                &constant.name,
                SYMBOL_KIND_CONSTANT,
                constant.span,
                constant.name_span,
                &[],
            )),
            Item::Enum(enum_def) => {
                let children = enum_def
                    .variants
                    .iter()
                    .map(|variant| {
                        document_symbol_item(
                            &document.source,
                            &variant.name,
                            SYMBOL_KIND_ENUM_MEMBER,
                            variant.span,
                            variant.name_span,
                            &[],
                        )
                    })
                    .collect::<Vec<_>>();
                symbols.push(document_symbol_item(
                    &document.source,
                    &enum_def.name,
                    SYMBOL_KIND_ENUM,
                    enum_def.span,
                    enum_def.name_span,
                    &children,
                ));
            }
            Item::Type(ty) => {
                let children = ty
                    .fields
                    .iter()
                    .map(|field| {
                        document_symbol_item(
                            &document.source,
                            &field.name,
                            SYMBOL_KIND_FIELD,
                            field.span,
                            field.name_span,
                            &[],
                        )
                    })
                    .collect::<Vec<_>>();
                symbols.push(document_symbol_item(
                    &document.source,
                    &ty.name,
                    SYMBOL_KIND_CLASS,
                    ty.span,
                    ty.name_span,
                    &children,
                ));
            }
            Item::Check(check) => symbols.push(document_symbol_item(
                &document.source,
                &check.name,
                SYMBOL_KIND_FUNCTION,
                check.span,
                check.name_span,
                &[],
            )),
        }
    }
    symbols
}

fn document_symbol_item(
    source: &str,
    name: &str,
    kind: u8,
    span: Span,
    name_span: Span,
    children: &[Value],
) -> Value {
    json!({
        "name": name,
        "kind": kind,
        "range": range_from_span(source, span),
        "selectionRange": range_from_span(source, name_span),
        "children": children
    })
}
