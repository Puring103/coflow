use coflow_cft::{CftAnnotation, CftAnnotationValue};

pub(crate) fn has_annotation(annotations: &[CftAnnotation], name: &str) -> bool {
    annotations.iter().any(|annotation| annotation.name == name)
}

pub(crate) fn annotation_name_arg(annotations: &[CftAnnotation], name: &str) -> Option<String> {
    annotations
        .iter()
        .find(|annotation| annotation.name == name)
        .and_then(|annotation| annotation.args.first())
        .and_then(|arg| match arg {
            CftAnnotationValue::Name(name) => Some(name.clone()),
            _ => None,
        })
}

pub(crate) fn display_annotation(annotations: &[CftAnnotation]) -> Option<String> {
    annotations
        .iter()
        .find(|annotation| annotation.name == "display")
        .and_then(|annotation| annotation.args.first())
        .and_then(|arg| match arg {
            CftAnnotationValue::String(value) => Some(value.clone()),
            _ => None,
        })
}

pub(crate) fn pascal_case(name: &str) -> String {
    let mut out = String::new();
    let mut upper = true;
    for ch in name.chars() {
        if matches!(ch, '_' | '-' | ' ') {
            upper = true;
            continue;
        }
        if upper {
            out.extend(ch.to_uppercase());
            upper = false;
        } else {
            out.push(ch);
        }
    }
    out
}

pub(crate) fn camel_case(name: &str) -> String {
    let pascal = pascal_case(name);
    let mut chars = pascal.chars();
    let Some(first) = chars.next() else {
        return String::new();
    };
    first.to_lowercase().collect::<String>() + chars.as_str()
}

pub(crate) fn pluralize(name: &str) -> String {
    if name.ends_with('s') {
        format!("{name}es")
    } else {
        format!("{name}s")
    }
}

pub(crate) fn ref_property_name(field_name: &str, target: &str) -> String {
    for suffix in ["_id", "Id", "ID"] {
        if let Some(prefix) = field_name.strip_suffix(suffix) {
            if !prefix.is_empty() {
                return pascal_case(prefix);
            }
        }
    }
    pascal_case(target)
}

pub(crate) fn index_var_name(type_name: &str) -> String {
    format!("_{}Index", camel_case(type_name))
}

pub(crate) fn index_param_name(type_name: &str) -> String {
    format!("{}Index", camel_case(type_name))
}

pub(crate) fn multi_index_var_name(type_name: &str, field_name: &str) -> String {
    format!(
        "_{}By{}",
        camel_case(&pluralize(type_name)),
        pascal_case(field_name)
    )
}

pub(crate) fn format_float(value: f64) -> String {
    let mut text = value.to_string();
    if !text.contains('.') && !text.contains('e') && !text.contains('E') {
        text.push_str(".0");
    }
    text.push('f');
    text
}

pub(crate) fn escape_csharp_string(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}
