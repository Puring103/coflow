use std::collections::HashSet;

use crate::names::{camel_case, csharp_ident_error, pascal_case};
use crate::CsharpCodegenError;
use coflow_cft::CftTypeMeta;

pub(super) fn csharp_public_type_name(name: &str) -> String {
    pascal_case(name)
}

pub(super) fn csharp_public_member_name(name: &str) -> String {
    pascal_case(name)
}

pub(super) fn plural_records_var(table_name: &str) -> String {
    let base = camel_case(&pascal_case(table_name));
    if base.ends_with('s') {
        format!("{base}Rows")
    } else {
        format!("{base}s")
    }
}

pub(super) fn context_index_field_name(type_name: &str) -> String {
    format!("{type_name}Index")
}

pub(super) fn loader_reserved_local_names(ty: &CftTypeMeta) -> HashSet<String> {
    let mut out = ty
        .all_fields
        .iter()
        .map(|field| format!("has{}", csharp_public_member_name(&field.name)))
        .collect::<HashSet<_>>();
    out.insert("isTable".to_string());
    out.insert("context".to_string());
    out
}

pub(super) fn field_local_name(
    field_name: &str,
    used_names: &mut HashSet<String>,
) -> Result<String, CsharpCodegenError> {
    let candidate = camel_case(&pascal_case(field_name));
    let base_name = if csharp_ident_error(&candidate)
        .is_some_and(|reason| reason == "identifier is a C# keyword")
        || is_reserved_loader_local_name(&candidate)
    {
        format!("{candidate}Value")
    } else {
        candidate
    };
    let mut local_name = base_name.clone();
    let mut suffix = 2;
    while used_names.contains(&local_name) {
        local_name = format!("{base_name}{suffix}");
        suffix += 1;
    }

    if let Some(reason) = csharp_ident_error(&local_name) {
        return Err(CsharpCodegenError::new(format!(
            "invalid C# field local variable name `{local_name}`: {reason}"
        )));
    }

    used_names.insert(local_name.clone());
    Ok(local_name)
}

fn is_reserved_loader_local_name(value: &str) -> bool {
    matches!(
        value,
        "count"
            | "context"
            | "fieldPath"
            | "i"
            | "index"
            | "item"
            | "key"
            | "keyPath"
            | "obj"
            | "reader"
            | "result"
            | "token"
            | "typeName"
            | "value"
            | "valuePath"
    )
}
