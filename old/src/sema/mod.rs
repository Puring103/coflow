use crate::ast::Module;
use crate::hir::HirModule;
use crate::lexer::LexErrorKind;
use crate::parser::{parse_module, ParseErrorKind};
use crate::span::Span;

pub mod collect;
mod config_eval;
mod lower;

pub use collect::{
    BuiltinSymbols, ClassFieldInfo, ClassInfo, EnumInfo, EnumVariantInfo, GlobalEntry,
    ModuleSymbols,
};

#[derive(Debug, Clone, PartialEq)]
pub struct SemaOutput {
    pub hir: HirModule,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Diagnostic {
    Lex(LexErrorKind, Span),
    Parse(ParseErrorKind, Span),
    Sema(SemaErrorKind, Span),
}

impl Diagnostic {
    pub fn span(&self) -> Span {
        match self {
            Diagnostic::Lex(_, span) | Diagnostic::Parse(_, span) | Diagnostic::Sema(_, span) => {
                *span
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SemaErrorKind {
    DuplicateTopLevel,
    DuplicateField,
    DuplicateVariant,
    DuplicateLocal,
    UndefinedName,
    AssignToReadonly,
    BreakOutsideLoop,
    ContinueOutsideLoop,
    YieldOutsideIterFn,
    ReturnValueInIterFn,
    SelfOutsideCheck,
    CheckBlockSideEffect,
    LocalTypeLeak,
    TypeMismatch,
    UnknownType,
    RecordKeyMixed,
    DictWithoutAnnotation,
    InvalidLiteral,
    NumberOverflow,
    InvalidEscape,
    ConfigDependsOnVar,
    ConfigCircularDependency,
    ConfigNonConstant,
    ConfigCheckFailed,
    ConfigTypeMismatch,
    UnsupportedNotImplemented,
}

pub fn analyze_source(source: &str) -> SemaOutput {
    let parsed = parse_module(source);
    let mut diagnostics: Vec<Diagnostic> = parsed
        .errors
        .into_iter()
        .map(|error| match error.kind {
            ParseErrorKind::Lex(kind) => Diagnostic::Lex(kind, error.span),
            kind => Diagnostic::Parse(kind, error.span),
        })
        .collect();

    let Some(module) = parsed.module else {
        return SemaOutput {
            hir: HirModule::new(),
            diagnostics,
        };
    };

    let mut output = analyze_module(&module);
    diagnostics.append(&mut output.diagnostics);
    output.diagnostics = diagnostics;
    output
}

pub fn analyze_module(module: &Module) -> SemaOutput {
    let (symbols, mut diagnostics) = collect::collect_module_symbols(module);
    let mut hir = lower::lower_module(module, &symbols, &mut diagnostics);
    config_eval::evaluate_configs(&mut hir, &mut diagnostics);
    SemaOutput { hir, diagnostics }
}
