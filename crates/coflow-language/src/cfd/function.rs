//! 函数体当前仅保存源码，源码工具共享声明关键字。
pub const CFD_FUNCTION_KEYWORDS: &[&str] = &[
    "fn", "var", "return", "if", "else", "for", "while", "break", "continue", "in", "is", "true",
    "false", "None", "Some", "self", "inf",
];
pub const CFD_FUNCTION_TYPES: &[&str] = &["int", "float", "bool", "string", "fstring"];
pub const CFD_FUNCTION_BUILTINS: &[&str] = &[];
