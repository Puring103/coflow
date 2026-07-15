use coflow_cft::syntax::ast::Annotation;

pub(crate) const KEYWORDS: &[(&str, &str)] = &[
    ("const", "Define a compile-time constant."),
    ("enum", "Define an enum."),
    ("type", "Define a schema type."),
    ("abstract", "Mark a type as non-instantiable."),
    ("sealed", "Prevent a type from being inherited."),
    ("check", "Start a validation block inside a type."),
    ("when", "Run nested checks only when the condition is true."),
    ("all", "Require every collection item to pass."),
    ("any", "Require at least one collection item to pass."),
    ("none", "Require no collection item to pass."),
    ("in", "Bind a quantifier variable to a collection."),
    ("is", "Check the runtime type or null value."),
];

pub(crate) const PRIMITIVE_TYPES: &[(&str, &str)] = &[
    ("int", "64-bit integer."),
    ("float", "64-bit floating point number."),
    ("bool", "Boolean value."),
    ("string", "String value."),
];

pub(crate) const LITERALS: &[(&str, &str)] = &[
    ("true", "Boolean true."),
    ("false", "Boolean false."),
    ("null", "Nullable value."),
];

pub(crate) const BUILTIN_FUNCTIONS: &[(&str, &str)] = &[
    (
        "len",
        "value.len(): return the number of items in an array or dict.",
    ),
    (
        "contains",
        "value.contains(val): test array element or dict key presence.",
    ),
    (
        "isUnique",
        "value.isUnique(): true when supported scalar elements are unique.",
    ),
    (
        "min",
        "value.min(): minimum value in a non-empty int, float, or enum array.",
    ),
    (
        "max",
        "value.max(): maximum value in a non-empty int, float, or enum array.",
    ),
    ("sum", "value.sum(): sum an int or float array."),
    ("keys", "value.keys(): return dict keys as an array."),
    ("values", "value.values(): return dict values as an array."),
    (
        "matches",
        "value.matches(pat): regex match with a string literal pattern.",
    ),
];

pub(crate) const ANNOTATIONS: &[AnnotationCompletion] = &[
    AnnotationCompletion {
        label: "@struct",
        insert_text: "@struct",
        detail: "type annotation",
        documentation: "Generate a value type. The target must be a sealed type.",
    },
    AnnotationCompletion {
        label: "@flag",
        insert_text: "@flag",
        detail: "enum annotation",
        documentation: "Mark an enum as bit flags. Non-zero values must be powers of two.",
    },
    AnnotationCompletion {
        label: "@idAsEnum",
        insert_text: "@idAsEnum(${1:EnumName})",
        detail: "type annotation",
        documentation: "Fill an empty enum placeholder from this type's record keys.",
    },
];

pub(crate) struct AnnotationCompletion {
    pub(crate) label: &'static str,
    pub(crate) insert_text: &'static str,
    pub(crate) detail: &'static str,
    pub(crate) documentation: &'static str,
}

pub(crate) fn annotation_documentation(
    annotation: &Annotation,
) -> Option<(&'static str, &'static str)> {
    let label = format!("@{}", annotation.name);
    ANNOTATIONS
        .iter()
        .find(|item| item.label == label)
        .map(|item| (item.label, item.documentation))
}

pub(crate) fn static_documentation(text: &str) -> Option<&'static str> {
    KEYWORDS
        .iter()
        .chain(PRIMITIVE_TYPES)
        .chain(LITERALS)
        .chain(BUILTIN_FUNCTIONS)
        .find_map(|(label, documentation)| (*label == text).then_some(*documentation))
        .or_else(|| {
            ANNOTATIONS
                .iter()
                .find(|annotation| annotation.label == text)
                .map(|annotation| annotation.documentation)
        })
}

pub(crate) fn is_builtin_name(name: &str) -> bool {
    KEYWORDS
        .iter()
        .chain(PRIMITIVE_TYPES)
        .chain(LITERALS)
        .chain(BUILTIN_FUNCTIONS)
        .any(|(label, _)| *label == name)
}
