//! CFD functions 能力。
use super::{
    byte_range, collect_function_tokens, parse_cfd, token_types, CfdValue, CftSchema,
    FunctionDocumentState, LanguageCompletion, LanguageDiagnostic, Span, TokenCollector,
    CFD_FUNCTION_BUILTINS, CFD_FUNCTION_KEYWORDS, CFD_FUNCTION_TYPES,
};

pub fn function_document(original: &str, requested_body: Option<&str>) -> FunctionDocumentState {
    let Some(parts) = function_parts(original) else {
        return FunctionDocumentState {
            source: original.into(),
            signature: "fn".into(),
            body: original.into(),
            body_range: byte_range(original, 0, original.len()),
            diagnostics: vec![LanguageDiagnostic {
                range: byte_range(original, 0, original.len().min(1)),
                severity: 1,
                source: Some("coflow-language".into()),
                message: "invalid function source".into(),
                ..Default::default()
            }],
            semantic_token_types: token_types(),
            ..Default::default()
        };
    };
    let requested_body = requested_body.unwrap_or_else(|| parts.body.trim());
    let body = if parts.multiline {
        requested_body.trim_matches('\n')
    } else {
        requested_body.trim()
    };
    let replacement = if parts.multiline {
        format!("\n{body}\n")
    } else {
        format!(" {body} ")
    };
    let source = format!("{}{}{}", parts.prefix, replacement, parts.suffix);
    let body_start = parts.prefix.len() + 1;
    let body_end = body_start + body.len();
    let mut collector = TokenCollector::new(&source);
    collect_function_tokens(Span::new(0, source.len()), &source, &mut collector);
    let diagnostics = function_body_diagnostics(&source, body_start, body);
    FunctionDocumentState {
        body_range: byte_range(&source, body_start, body_end),
        semantic_token_data: collector.into_lsp_data().data,
        completions: function_completion_items_with_locals(parts.signature, &source),
        source,
        signature: parts.signature.to_owned(),
        body: body.to_owned(),
        diagnostics,
        semantic_token_types: token_types(),
    }
}

pub(super) struct FunctionParts<'a> {
    prefix: &'a str,
    body: &'a str,
    suffix: &'a str,
    signature: &'a str,
    multiline: bool,
}

pub(super) fn function_parts(source: &str) -> Option<FunctionParts<'_>> {
    const PREFIX: &str = "__function: __EditorFunction { value: ";
    const SUFFIX: &str = "\n}";
    let document = format!("{PREFIX}{source}{SUFFIX}");
    let (ast, diagnostics) = parse_cfd(&document);
    if !diagnostics.is_empty() {
        return None;
    }
    let function = ast
        .records
        .first()?
        .fields
        .first()
        .and_then(|field| match &field.value {
            CfdValue::Function(function) => Some(function),
            _ => None,
        })?;
    let start = function.body_span.start.checked_sub(PREFIX.len())?;
    let end = function.body_span.end.checked_sub(PREFIX.len())?;
    let open = start.checked_sub(1)?;
    Some(FunctionParts {
        prefix: source.get(..start)?,
        body: source.get(start..end)?,
        suffix: source.get(end..)?,
        signature: source.get(..open)?.trim(),
        multiline: source.get(start..end)?.contains('\n') || source.contains('\n'),
    })
}

pub(super) fn function_body_diagnostics(
    function_source: &str,
    body_start: usize,
    body: &str,
) -> Vec<LanguageDiagnostic> {
    const PREFIX: &str = "__function: __EditorFunction { value: ";
    let document = format!("{PREFIX}{function_source}\n}}");
    let (_, diagnostics) = parse_cfd(&document);
    let body_document_start = PREFIX.len() + body_start;
    diagnostics
        .into_iter()
        .map(|diagnostic| {
            let relative = diagnostic.span.start.saturating_sub(body_document_start);
            let start = relative.min(body.len().saturating_sub(1));
            let end = (start
                + diagnostic
                    .span
                    .end
                    .saturating_sub(diagnostic.span.start)
                    .max(1))
            .min(body.len());
            LanguageDiagnostic {
                range: (byte_range(body, start, end)).clone(),
                severity: 1,
                source: Some(("coflow-language").to_string()),
                message: (diagnostic.message).to_string(),
                ..Default::default()
            }
        })
        .collect()
}

pub(super) fn function_completion_items(signature: &str) -> Vec<LanguageCompletion> {
    let mut items = CFD_FUNCTION_KEYWORDS
        .iter()
        .filter(|label| **label != "build")
        .map(|label| LanguageCompletion {
            label: (label).to_string(),
            kind: Some(14),
            ..Default::default()
        })
        .chain(CFD_FUNCTION_TYPES.iter().map(|label| LanguageCompletion {
            label: (label).to_string(),
            kind: Some(7),
            ..Default::default()
        }))
        .collect::<Vec<_>>();
    items.push(LanguageCompletion {
        label: ("build").to_string(),
        kind: Some(14),
        insert_text: Some(("build ${1:Type} as ${2:b} {\n\t${3}\n}").to_string()),
        insert_text_format: Some(2),
        detail: Some(("局部构造，正常结束自动冻结").to_string()),
        ..Default::default()
    });
    items.extend(
        CFD_FUNCTION_BUILTINS
            .iter()
            .map(|label| LanguageCompletion {
                label: (label).to_string(),
                kind: Some(2),
                insert_text: Some((format!("{label}(${{1}})")).to_string()),
                insert_text_format: Some(2),
                ..Default::default()
            }),
    );
    items.extend(
        function_parameter_names(signature)
            .into_iter()
            .map(|label| LanguageCompletion {
                label: (label).to_string(),
                kind: Some(6),
                detail: Some(("function parameter").to_string()),
                ..Default::default()
            }),
    );
    items
}

pub(super) fn function_completion_items_with_locals(
    signature: &str,
    source: &str,
) -> Vec<LanguageCompletion> {
    let mut items = function_completion_items(signature);
    items.extend(
        function_local_names(source)
            .into_iter()
            .map(|label| LanguageCompletion {
                label: (label).to_string(),
                kind: Some(6),
                detail: Some(("local variable").to_string()),
                ..Default::default()
            }),
    );
    items
}

pub(crate) fn function_source_completion_items_at(
    source: &str,
    relative_offset: usize,
    schema: Option<&CftSchema>,
) -> Option<Vec<LanguageCompletion>> {
    let parts = function_parts(source)?;
    (relative_offset > parts.prefix.len()).then(|| {
        function_scoped_completions(
            parts.signature,
            source.get(..relative_offset).unwrap_or(source),
            schema,
        )
    })
}

pub(super) fn function_scoped_completions(
    signature: &str,
    prefix: &str,
    schema: Option<&CftSchema>,
) -> Vec<LanguageCompletion> {
    if let Some(items) =
        schema.and_then(|schema| super::builder_completion::members(signature, prefix, schema))
    {
        return items;
    }
    let mut scopes = vec![Vec::<String>::new()];
    let mut builder = None;
    let mut declaration = None;
    let tokens = coflow_language::lexical::tokenize_lossless(prefix)
        .into_iter()
        .filter(|token| !token.is_trivia());
    for token in tokens {
        let text = token.text(prefix);
        if let Some(kind) = declaration.take() {
            if text
                .chars()
                .next()
                .is_some_and(|ch| ch == '_' || ch.is_alphabetic())
            {
                if kind {
                    builder = Some(text.to_string());
                } else {
                    scopes.last_mut().unwrap().push(text.to_string());
                }
                continue;
            }
        }
        match text {
            "var" => declaration = Some(false),
            "as" => declaration = Some(true),
            "{" => scopes.push(builder.take().into_iter().collect()),
            "}" if scopes.len() > 1 => {
                scopes.pop();
            }
            _ => {}
        }
    }
    let mut items = function_completion_items(signature);
    let mut seen = std::collections::BTreeSet::new();
    // builder 只属于随后的构造块；关闭作用域和光标之后的声明不进入候选列表。
    for name in scopes.into_iter().rev().flatten() {
        if seen.insert(name.clone()) {
            items.push(LanguageCompletion {
                label: (name).to_string(),
                kind: Some(6),
                detail: Some(("local variable").to_string()),
                ..Default::default()
            });
        }
    }
    items
}

pub(super) fn function_local_names(source: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut tokens = coflow_language::lexical::tokenize_lossless(source)
        .into_iter()
        .filter(|token| !token.is_trivia())
        .map(|token| token.text(source));
    while let Some(token) = tokens.next() {
        if token == "var" || token == "as" {
            if let Some(name) = tokens.next() {
                if !names.iter().any(|existing| existing == name) {
                    names.push(name.to_string());
                }
            }
        }
    }
    names
}

pub(super) fn function_parameter_names(signature: &str) -> Vec<&str> {
    let Some(open) = signature.find('(') else {
        return Vec::new();
    };
    let mut depth = 0_usize;
    let mut close = None;
    for (relative, character) in signature[open..].char_indices() {
        match character {
            '(' => depth = depth.saturating_add(1),
            ')' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    close = Some(open + relative);
                    break;
                }
            }
            _ => {}
        }
    }
    let Some(parameters) = close.and_then(|close| signature.get(open + 1..close)) else {
        return Vec::new();
    };
    let mut names = Vec::new();
    let mut parameter_start = 0;
    depth = 0;
    for (index, character) in parameters
        .char_indices()
        .chain(std::iter::once((parameters.len(), ',')))
    {
        match character {
            '(' | '[' | '{' | '<' => depth = depth.saturating_add(1),
            ')' | ']' | '}' | '>' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                if let Some(name) = parameters[parameter_start..index]
                    .split_once(':')
                    .map(|(name, _)| name.trim())
                    .filter(|name| !name.is_empty())
                {
                    names.push(name);
                }
                parameter_start = index + character.len_utf8();
            }
            _ => {}
        }
    }
    names
}
