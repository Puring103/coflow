//! 源码工具共享函数关键字；执行语义由核心编译器负责。
pub const CFD_FUNCTION_KEYWORDS: &[&str] = &[
    "fn", "var", "return", "if", "else", "for", "while", "break", "continue", "in", "is", "true",
    "false", "None", "Some", "self", "inf", "build", "as",
];
pub const CFD_FUNCTION_TYPES: &[&str] = &["int", "float", "bool", "string", "fstring"];
pub const CFD_FUNCTION_BUILTINS: &[&str] = &[];
