//! Contract 创建时分析全部默认函数、模板和检查，映像构建时只降低 IR。
use super::{
    bytecode::Program,
    compiler::{self, CompileContext},
};
use crate::schema::{
    CftConstValue as C, CftSchema, CftSchemaDefaultValue as D, CftValueType as Ty, ModuleId,
};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, sync::Arc};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ProgramKey {
    pub module: ModuleId,
    pub owner: Option<String>,
    pub offset: usize,
}
#[derive(Debug, Clone)]
pub struct CheckProgram<P = Program> {
    pub owner: Option<String>,
    pub name: String,
    pub module: ModuleId,
    pub program: Arc<P>,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ContractIr {
    pub functions: BTreeMap<ProgramKey, Arc<super::ir::Function>>,
    pub checks: Vec<CheckIr>,
}
#[derive(Debug, Clone)]
pub struct ProgramDiagnostic {
    pub module: ModuleId,
    pub path: Option<String>,
    pub span: crate::source::Span,
    pub message: String,
}
fn diagnostic(
    schema: &CftSchema,
    module: &ModuleId,
    span: crate::source::Span,
    message: String,
) -> ProgramDiagnostic {
    ProgramDiagnostic {
        module: module.clone(),
        path: schema
            .source(module)
            .map(|source| source.path.to_string_lossy().into_owned()),
        span,
        message,
    }
}
impl ContractIr {
    pub fn compile(schema: &CftSchema) -> Result<Self, ProgramDiagnostic> {
        let mut programs = Self::default();
        for meta in schema.all_types() {
            for field in meta.own_fields() {
                if let Some(default) = &field.default {
                    programs.collect_default(
                        schema,
                        default,
                        Some(meta.name.as_str()),
                        &meta.module,
                    )?;
                }
            }
            if let Some(check) = &meta.check {
                programs.check(
                    schema,
                    &check.source,
                    check.span,
                    Some(meta.name.as_str()),
                    &meta.module,
                )?;
            }
        }
        for constant in schema.all_consts() {
            programs.constant(schema, &constant.value, None, &constant.module)?;
        }
        for check in schema.all_checks() {
            programs.check(
                schema,
                &check.block.source,
                check.block.span,
                None,
                &check.module,
            )?;
        }
        Ok(programs)
    }
    fn context(schema: &CftSchema, owner: Option<&str>, module: &ModuleId) -> CompileContext {
        let owner = owner
            .and_then(|name| schema.resolve_type(name))
            .map(|meta| {
                if meta.kind == coflow_language::cft::syntax::ast::TypeKind::Data {
                    Ty::Object(meta.name.clone())
                } else {
                    Ty::RecordRef(meta.name.clone())
                }
            });
        compiler::module_context(schema, module, owner)
    }
    fn function(
        &mut self,
        schema: &CftSchema,
        source: &crate::schema::CftCallableSource,
        template: bool,
        owner: Option<&str>,
        _module: &ModuleId,
    ) -> Result<(), ProgramDiagnostic> {
        let key = ProgramKey {
            module: source.module.clone(),
            owner: owner.map(str::to_string),
            offset: source.span.start,
        };
        if self.functions.contains_key(&key) {
            return Ok(());
        }
        let context = Self::context(schema, owner, &source.module);
        let name = format!(
            "{}::<{}>",
            owner.unwrap_or("constant"),
            if template { "template" } else { "function" }
        );
        let original = schema
            .source(&source.module)
            .and_then(|file| file.source.get(source.span.start..source.span.end))
            .unwrap_or(&source.original_source);
        let mut expansion = None;
        let mut expanded = String::new();
        if !template {
            let signature = coflow_language::cft::syntax::parser::parse_type_prefix(original)
                .map_err(|e| diagnostic(schema, &source.module, source.span, format!("{e:?}")))?;
            if !matches!(
                signature.kind,
                coflow_language::cft::syntax::ast::TypeRefKind::Function(..)
            ) {
                let resolved =
                    coflow_language::cft::syntax::parser::parse_type_prefix(&source.source)
                        .map_err(|e| {
                            diagnostic(schema, &source.module, source.span, format!("{e:?}"))
                        })?;
                expanded.push_str(&source.source[..resolved.span.end]);
                expanded.push_str(&original[signature.span.end..]);
                expansion = Some((resolved.span.end, signature.span.end));
            }
        }
        let compile_source = if expansion.is_some() {
            expanded.as_str()
        } else {
            original
        };
        let mut program = if template {
            compiler::analyze_template(schema, compile_source, &name, context)
        } else {
            compiler::analyze(schema, compile_source, &name, context)
        }
        .map_err(|e| {
            let map = |offset: usize| {
                source.span.start
                    + expansion.map_or(offset, |(expanded, original)| {
                        original + offset.saturating_sub(expanded)
                    })
            };
            diagnostic(
                schema,
                &source.module,
                crate::source::Span::new(map(e.span.start), map(e.span.end)),
                e.message,
            )
        })?;
        if let Some((expanded, original)) = expansion {
            program.map_expanded_header(expanded, original);
        }
        program.locate(
            Some(source.module.clone()),
            schema
                .source(&source.module)
                .map(|s| s.path.to_string_lossy().into_owned()),
            source.span.start,
        );
        self.functions.insert(key, Arc::new(program));
        Ok(())
    }
    fn collect_default(
        &mut self,
        schema: &CftSchema,
        value: &D,
        owner: Option<&str>,
        module: &ModuleId,
    ) -> Result<(), ProgramDiagnostic> {
        match value {
            D::Function(source) => self.function(schema, source, false, owner, module)?,
            D::FormattedString(source) => self.function(schema, source, true, owner, module)?,
            D::OptionSome(value) => self.collect_default(schema, value, owner, module)?,
            D::Array(values) => {
                for value in values {
                    self.collect_default(schema, value, owner, module)?;
                }
            }
            D::Dictionary(values) => {
                for (key, value) in values {
                    self.collect_default(schema, key, owner, module)?;
                    self.collect_default(schema, value, owner, module)?;
                }
            }
            D::Object { type_name, fields } => {
                for (_, value) in fields {
                    self.collect_default(schema, value, Some(type_name), module)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
    fn constant(
        &mut self,
        schema: &CftSchema,
        value: &C,
        owner: Option<&str>,
        module: &ModuleId,
    ) -> Result<(), ProgramDiagnostic> {
        match value {
            C::Function(source) => self.function(schema, source, false, owner, module)?,
            C::FormattedString(source) => self.function(schema, source, true, owner, module)?,
            C::OptionSome(value) => self.constant(schema, value, owner, module)?,
            C::Array(values) => {
                for value in values {
                    self.constant(schema, value, owner, module)?;
                }
            }
            C::Dictionary(values) => {
                for (key, value) in values {
                    self.constant(schema, key, owner, module)?;
                    self.constant(schema, value, owner, module)?;
                }
            }
            C::Object { type_name, fields } => {
                for (_, value) in fields {
                    self.constant(schema, value, Some(type_name), module)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
    fn check(
        &mut self,
        schema: &CftSchema,
        source: &str,
        span: crate::source::Span,
        owner: Option<&str>,
        module: &ModuleId,
    ) -> Result<(), ProgramDiagnostic> {
        let source = schema
            .source(module)
            .and_then(|source| source.source.get(span.start..span.end))
            .unwrap_or(source);
        for check in coflow_language::function::parse_checks(source).map_err(|e| {
            diagnostic(
                schema,
                module,
                crate::source::Span::new(span.start + e.span.start, span.start + e.span.end),
                e.message,
            )
        })? {
            let name = check.name.clone().unwrap_or_else(|| "<anonymous>".into());
            let mut program = compiler::analyze_check(
                schema,
                &check,
                source,
                &name,
                Self::context(schema, owner, module),
            )
            .map_err(|e| {
                diagnostic(
                    schema,
                    module,
                    crate::source::Span::new(span.start + e.span.start, span.start + e.span.end),
                    e.message,
                )
            })?;
            program.locate(
                Some(module.clone()),
                schema
                    .source(module)
                    .map(|source| source.path.to_string_lossy().into_owned()),
                span.start,
            );
            self.checks.push(CheckIr {
                owner: owner.map(str::to_string),
                name,
                module: module.clone(),
                program: Arc::new(program),
            });
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckIr {
    pub owner: Option<String>,
    pub name: String,
    pub module: ModuleId,
    pub program: Arc<super::ir::Function>,
}
#[derive(Debug, Clone)]
pub struct ContractPrograms<P = Program> {
    pub functions: BTreeMap<ProgramKey, Arc<P>>,
    pub checks: Vec<CheckProgram<P>>,
}
impl ContractIr {
    pub(crate) fn validate(&self, schema: &CftSchema) -> Result<(), ProgramDiagnostic> {
        for function in self.functions.values().map(AsRef::as_ref).chain(self.checks.iter().map(|check| check.program.as_ref())) {
            function.validate_semantics(schema).map_err(|message| ProgramDiagnostic {
                module: function.module.clone().unwrap_or_else(|| ModuleId::from("<contract>")),
                path: function.path.clone(),
                span: function.body.first().map_or(crate::source::Span::default(), |node| node.span),
                message,
            })?;
        }
        Ok(())
    }
    pub(crate) fn lower(&self, optimize: bool) -> Result<ContractPrograms, ProgramDiagnostic> {
        let lower = |function: &super::ir::Function| function.lower_optimized(optimize).map(Arc::new).map_err(|message| ProgramDiagnostic {
            module: function.module.clone().unwrap_or_else(|| ModuleId::from("<contract>")),
            path: function.path.clone(), span: function.body.first().map_or(crate::source::Span::default(), |node| node.span), message,
        });
        Ok(ContractPrograms {
            functions: self.functions.iter().map(|(key, function)| Ok((key.clone(), lower(function)?))).collect::<Result<_, ProgramDiagnostic>>()?,
            checks: self.checks.iter().map(|check| Ok(CheckProgram { owner: check.owner.clone(), name: check.name.clone(), module: check.module.clone(), program: lower(&check.program)? })).collect::<Result<_, ProgramDiagnostic>>()?,
        })
    }
}

impl<P> Default for ContractPrograms<P> {
    fn default() -> Self { Self { functions: BTreeMap::new(), checks: Vec::new() } }
}
impl ContractPrograms {
    pub(crate) fn publish(self) -> Result<ContractPrograms<super::image::ValidatedProgram>, String> {
        let publish = |program: Arc<Program>| super::image::ValidatedProgram::new(Arc::unwrap_or_clone(program)).map(Arc::new);
        Ok(ContractPrograms {
            functions: self.functions.into_iter().map(|(key, program)| Ok((key, publish(program)?))).collect::<Result<_, String>>()?,
            checks: self.checks.into_iter().map(|check| Ok(CheckProgram { owner: check.owner, name: check.name, module: check.module, program: publish(check.program)? })).collect::<Result<_, String>>()?,
        })
    }
}
