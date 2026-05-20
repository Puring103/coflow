use crate::ast::{
    self, AssignOp, BinaryOp, Block, ElseBranch, Expr, FnBody, FnDecl, FnExpr, Ident, Item,
    LambdaExpr, Literal, Module, RecordEntry, RecordKey, StringKind, TypeExpr, UnaryOp, YieldStmt,
};
use crate::hir::{
    ClassId, FunctionId, GlobalId, HirArg, HirAssignTarget, HirCheckArm, HirClass, HirClassField,
    HirEnum, HirEnumVariant, HirExpr, HirFunction, HirGlobal, HirLocal, HirModule, HirParam,
    HirStmt, LocalId, Ty, UpvalueDesc, UpvalueId, Value, VariantId,
};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use crate::span::Span;

use super::collect::{GlobalEntry, ModuleSymbols};
use super::{Diagnostic, SemaErrorKind};

#[derive(Debug, Clone)]
struct TypedExpr {
    expr: HirExpr,
    ty: Ty,
}

#[derive(Debug, Clone)]
enum TyCtx {
    None,
    Expect(Ty),
}

struct LowerCtx<'a, 'd> {
    symbols: &'a ModuleSymbols,
    module: HirModule,
    fn_stack: Vec<FnCtx>,
    diagnostics: &'d mut Vec<Diagnostic>,
    top_level_function_ids: HashMap<usize, FunctionId>,
    in_check_block: bool,
    checking_config_without_ty: bool,
}

#[derive(Debug, Clone)]
struct FnCtx {
    fn_id: FunctionId,
    is_iter: bool,
    return_ty: Option<Ty>,
    scopes: Vec<Scope>,
    loop_depth: usize,
    locals: Vec<HirLocal>,
    upvalues: Vec<UpvalueDesc>,
    upvalue_index: HashMap<UpvalueDesc, UpvalueId>,
}

#[derive(Debug, Clone, Default)]
struct Scope {
    vars: HashMap<String, LocalId>,
}

struct FunctionLowerInput<'a> {
    is_iter: bool,
    params: &'a [ast::Param],
    return_ty: Option<Ty>,
    body: &'a FnBody,
    span: Span,
    signature: Ty,
    expected_signature: Option<&'a Ty>,
}

#[derive(Debug, Clone, Copy)]
enum ResolvedName {
    Local(LocalId),
    Upvalue(UpvalueId),
    Global(GlobalId),
    Error,
}

pub fn lower_module(
    module: &Module,
    symbols: &ModuleSymbols,
    diagnostics: &mut Vec<Diagnostic>,
) -> HirModule {
    let mut ctx = LowerCtx {
        symbols,
        module: HirModule::new(),
        fn_stack: Vec::new(),
        diagnostics,
        top_level_function_ids: HashMap::new(),
        in_check_block: false,
        checking_config_without_ty: false,
    };

    ctx.lower_enums();
    ctx.lower_classes(module);
    ctx.predeclare_top_level_functions(module);
    ctx.lower_globals(module);
    ctx.check_local_type_leaks();
    ctx.module
}

impl<'a, 'd> LowerCtx<'a, 'd> {
    fn lower_enums(&mut self) {
        self.module.enums = self
            .symbols
            .enums
            .iter()
            .map(|info| HirEnum {
                name: info.name.clone(),
                local: info.local,
                variants: info
                    .variants
                    .iter()
                    .map(|variant| HirEnumVariant {
                        name: variant.name.clone(),
                        value: variant.value,
                        span: variant.span,
                    })
                    .collect(),
                span: info.span,
            })
            .collect();
    }

    fn lower_classes(&mut self, module: &Module) {
        let class_items: Vec<_> = self
            .symbols
            .classes
            .iter()
            .map(|info| {
                let Item::Class(class) = &module.items[info.ast_index] else {
                    unreachable!("class symbol should point to class item");
                };
                (info.clone(), class.clone())
            })
            .collect();

        for (info, class) in class_items {
            let fields = class
                .fields
                .iter()
                .map(|field| {
                    let ty = self.resolve_type(&field.ty);
                    let default = field
                        .default
                        .as_ref()
                        .map(|expr| self.lower_expr(expr, TyCtx::Expect(ty.clone())).expr);
                    if let Some(default_expr) = &default {
                        let default_ty = self.infer_hir_expr_type(default_expr);
                        self.check_assignable(&ty, &default_ty, field.span);
                    }
                    HirClassField {
                        name: field.name.text.clone(),
                        ty,
                        default,
                        span: field.span,
                    }
                })
                .collect();

            let old_check = self.in_check_block;
            self.in_check_block = true;
            let checks = class
                .checks
                .iter()
                .map(|arm| {
                    let cond = self
                        .lower_expr(&arm.condition, TyCtx::Expect(Ty::Bool))
                        .expr;
                    let message = self
                        .lower_expr(&arm.message, TyCtx::Expect(Ty::String))
                        .expr;
                    HirCheckArm {
                        cond,
                        message,
                        span: arm.span,
                    }
                })
                .collect();
            self.in_check_block = old_check;

            self.module.classes.push(HirClass {
                name: info.name,
                local: info.local,
                fields,
                checks,
                span: info.span,
            });
        }
    }

    fn predeclare_top_level_functions(&mut self, module: &Module) {
        let function_items: Vec<(usize, FnDecl)> = self
            .symbols
            .globals
            .iter()
            .filter_map(|(_, entry)| match entry {
                GlobalEntry::Function { ast_index, .. } => {
                    let Item::Function(func) = &module.items[*ast_index] else {
                        unreachable!("function symbol should point to function item");
                    };
                    Some((*ast_index, func.clone()))
                }
                _ => None,
            })
            .collect();

        for (ast_index, func) in function_items {
            let return_ty = if func.iter {
                None
            } else {
                func.return_type.as_ref().map(|ty| self.resolve_type(ty))
            };
            let signature = self.function_signature(&func.params, return_ty.clone(), func.iter);
            let fn_id = FunctionId(self.module.functions.len());
            self.module.functions.push(HirFunction {
                is_iter: func.iter,
                params: Vec::new(),
                return_ty,
                locals: Vec::new(),
                upvalues: Vec::new(),
                body: Vec::new(),
                span: func.span,
                signature,
            });
            self.top_level_function_ids.insert(ast_index, fn_id);
        }
    }

    fn lower_globals(&mut self, module: &Module) {
        let mut globals = Vec::new();

        for (name, entry) in self.symbols.globals.clone() {
            match entry {
                GlobalEntry::Config { id, ast_index, .. } => {
                    let Item::Config(config) = &module.items[ast_index] else {
                        unreachable!("config symbol should point to config item");
                    };
                    let ty = config.ty.as_ref().map(|ty| self.resolve_type(ty));
                    let old_without_ty = self.checking_config_without_ty;
                    self.checking_config_without_ty = ty.is_none();
                    let value = self
                        .lower_expr(&config.value, ty.clone().map_or(TyCtx::None, TyCtx::Expect))
                        .expr;
                    self.checking_config_without_ty = old_without_ty;
                    let value = if let Some(expected) = &ty {
                        let actual = self.infer_hir_expr_type(&value);
                        let span = value.span();
                        self.coerce_annotated_expr(value, expected, &actual, span)
                    } else {
                        value
                    };
                    if let Some(expected) = &ty {
                        let actual = self.infer_hir_expr_type(&value);
                        self.check_assignable(expected, &actual, config.span);
                    }
                    globals.push(HirGlobal::Config {
                        id,
                        name,
                        ty,
                        value,
                        span: config.span,
                    });
                }
                GlobalEntry::Var { id, ast_index, .. } => {
                    let Item::Var(var) = &module.items[ast_index] else {
                        unreachable!("var symbol should point to var item");
                    };
                    let ty = var.ty.as_ref().map(|ty| self.resolve_type(ty));
                    let init = var.init.as_ref().map(|expr| {
                        let lowered =
                            self.lower_expr(expr, ty.clone().map_or(TyCtx::None, TyCtx::Expect));
                        if let Some(expected) = &ty {
                            let span = lowered.expr.span();
                            self.coerce_annotated_expr(lowered.expr, expected, &lowered.ty, span)
                        } else {
                            lowered.expr
                        }
                    });
                    globals.push(HirGlobal::Var {
                        id,
                        name,
                        local: var.local,
                        ty,
                        init,
                        span: var.span,
                    });
                }
                GlobalEntry::Function { id, ast_index, .. } => {
                    let Item::Function(func) = &module.items[ast_index] else {
                        unreachable!("function symbol should point to function item");
                    };
                    let fn_id = self
                        .top_level_function_ids
                        .get(&ast_index)
                        .copied()
                        .expect("top-level function should be predeclared");
                    self.lower_fn_decl_into(func, fn_id);
                    globals.push(HirGlobal::Function {
                        id,
                        name,
                        local: func.local,
                        fn_id,
                        span: func.span,
                    });
                }
                GlobalEntry::Class { id, class_id, .. } => {
                    globals.push(HirGlobal::Class {
                        id,
                        class_id,
                        span: self
                            .symbols
                            .classes
                            .get(class_id.0)
                            .map_or(Span { start: 0, end: 0 }, |class| class.span),
                    });
                }
                GlobalEntry::Enum { id, enum_id, .. } => {
                    globals.push(HirGlobal::Enum {
                        id,
                        enum_id,
                        span: self
                            .symbols
                            .enums
                            .get(enum_id.0)
                            .map_or(Span { start: 0, end: 0 }, |enum_info| enum_info.span),
                    });
                }
                GlobalEntry::Import { id, import_id, .. } => {
                    let info = &self.symbols.imports[import_id];
                    globals.push(HirGlobal::Import {
                        id,
                        name,
                        span: info.span,
                    });
                }
                GlobalEntry::Builtin { id, builtin_id } => {
                    globals.push(HirGlobal::Builtin {
                        id,
                        builtin_id,
                        name,
                        span: Span { start: 0, end: 0 },
                    });
                }
            }
        }

        globals.sort_by_key(HirGlobal::id);
        self.module.globals = globals;
    }

    fn lower_fn_decl(&mut self, func: &FnDecl) -> FunctionId {
        let return_ty = if func.iter {
            if func.return_type.is_some() {
                self.push(SemaErrorKind::TypeMismatch, func.span);
            }
            None
        } else {
            func.return_type.as_ref().map(|ty| self.resolve_type(ty))
        };
        let signature = self.function_signature(&func.params, return_ty.clone(), func.iter);
        self.lower_function_like(FunctionLowerInput {
            is_iter: func.iter,
            params: &func.params,
            return_ty,
            body: &func.body,
            span: func.span,
            signature,
            expected_signature: None,
        })
    }

    fn lower_fn_decl_into(&mut self, func: &FnDecl, fn_id: FunctionId) {
        let return_ty = if func.iter {
            if func.return_type.is_some() {
                self.push(SemaErrorKind::TypeMismatch, func.span);
            }
            None
        } else {
            func.return_type.as_ref().map(|ty| self.resolve_type(ty))
        };
        let signature = self.function_signature(&func.params, return_ty.clone(), func.iter);
        self.lower_function_like_into_id(
            fn_id,
            FunctionLowerInput {
                is_iter: func.iter,
                params: &func.params,
                return_ty,
                body: &func.body,
                span: func.span,
                signature,
                expected_signature: None,
            },
        );
    }

    fn lower_fn_expr(&mut self, func: &FnExpr, ctx: TyCtx) -> TypedExpr {
        let expected_sig = match ctx {
            TyCtx::Expect(Ty::FunctionSig { params, return_ty }) => Some((params, *return_ty)),
            _ => None,
        };
        let return_ty = if func.iter {
            if func.return_type.is_some() {
                self.push(SemaErrorKind::TypeMismatch, func.span);
            }
            None
        } else if let Some(ret) = &func.return_type {
            Some(self.resolve_type(ret))
        } else {
            expected_sig
                .as_ref()
                .map(|(_, return_ty)| return_ty.clone())
        };
        let signature =
            self.infer_function_signature(&func.params, return_ty.clone(), func.iter, expected_sig);
        let fn_id = self.lower_function_like(FunctionLowerInput {
            is_iter: func.iter,
            params: &func.params,
            return_ty,
            body: &func.body,
            span: func.span,
            signature: signature.clone(),
            expected_signature: Some(&signature),
        });
        TypedExpr {
            expr: HirExpr::Closure {
                fn_id,
                span: func.span,
            },
            ty: signature,
        }
    }

    fn lower_lambda(&mut self, lambda: &LambdaExpr, ctx: TyCtx) -> TypedExpr {
        let expected_sig = match ctx {
            TyCtx::Expect(Ty::FunctionSig { params, return_ty }) => Some((params, *return_ty)),
            _ => None,
        };
        let return_ty = if let Some(ret) = &lambda.return_type {
            Some(self.resolve_type(ret))
        } else {
            expected_sig
                .as_ref()
                .map(|(_, return_ty)| return_ty.clone())
        };
        let signature =
            self.infer_function_signature(&lambda.params, return_ty.clone(), false, expected_sig);
        let fn_id = self.lower_function_like(FunctionLowerInput {
            is_iter: false,
            params: &lambda.params,
            return_ty,
            body: &lambda.body,
            span: lambda.span,
            signature: signature.clone(),
            expected_signature: Some(&signature),
        });
        TypedExpr {
            expr: HirExpr::Closure {
                fn_id,
                span: lambda.span,
            },
            ty: signature,
        }
    }

    fn lower_function_like(&mut self, input: FunctionLowerInput<'_>) -> FunctionId {
        let fn_id = FunctionId(self.module.functions.len());
        self.module.functions.push(HirFunction {
            is_iter: input.is_iter,
            params: Vec::new(),
            return_ty: input.return_ty.clone(),
            locals: Vec::new(),
            upvalues: Vec::new(),
            body: Vec::new(),
            span: input.span,
            signature: input.signature.clone(),
        });
        self.lower_function_like_into_id(fn_id, input);
        fn_id
    }

    fn lower_function_like_into_id(&mut self, fn_id: FunctionId, input: FunctionLowerInput<'_>) {
        if let Some(function) = self.module.functions.get_mut(fn_id.0) {
            function.is_iter = input.is_iter;
            function.return_ty = input.return_ty.clone();
            function.span = input.span;
            function.signature = input.signature.clone();
        }

        let mut fn_ctx = FnCtx {
            fn_id,
            is_iter: input.is_iter,
            return_ty: input.return_ty.clone(),
            scopes: vec![Scope::default()],
            loop_depth: 0,
            locals: Vec::new(),
            upvalues: Vec::new(),
            upvalue_index: HashMap::new(),
        };

        let mut hir_params = Vec::new();
        for (idx, param) in input.params.iter().enumerate() {
            let inferred_ty = param
                .ty
                .as_ref()
                .map(|ty| self.resolve_type(ty))
                .or_else(|| match input.expected_signature {
                    Some(Ty::FunctionSig { params, .. }) => params.get(idx).cloned(),
                    _ => None,
                });
            if let (Some(Ty::FunctionSig { params, .. }), Some(annotated)) =
                (input.expected_signature, inferred_ty.as_ref())
            {
                if let Some(expected) = params.get(idx) {
                    self.check_assignable(expected, annotated, param.span);
                }
            }

            let local_id = LocalId(fn_ctx.locals.len());
            fn_ctx.locals.push(HirLocal {
                name: param.name.text.clone(),
                ty: inferred_ty.clone(),
                is_captured: false,
                span: param.span,
            });
            fn_ctx
                .scopes
                .last_mut()
                .expect("function should have root scope")
                .vars
                .insert(param.name.text.clone(), local_id);
            let default = param.default.as_ref().map(|default| {
                let lowered = self.lower_expr_with_ctx(
                    &mut fn_ctx,
                    default,
                    inferred_ty.clone().map_or(TyCtx::None, TyCtx::Expect),
                );
                if let Some(expected) = &inferred_ty {
                    if !is_assignable(expected, &lowered.ty) {
                        self.push(SemaErrorKind::TypeMismatch, default.span());
                    }
                    HirExpr::TypeGuard {
                        expr: Box::new(lowered.expr),
                        ty: expected.clone(),
                        span: default.span(),
                    }
                } else {
                    lowered.expr
                }
            });
            hir_params.push(HirParam {
                local_id,
                name: param.name.text.clone(),
                ty: inferred_ty,
                default,
                span: param.span,
            });
        }

        self.fn_stack.push(fn_ctx);
        let mut body = match input.body {
            FnBody::Block(block) => self.lower_block(block),
            FnBody::Expr(expr) => {
                let lowered = self.lower_expr(
                    expr,
                    input.return_ty.clone().map_or(TyCtx::None, TyCtx::Expect),
                );
                let expr = if let Some(expected) = &input.return_ty {
                    let span = lowered.expr.span();
                    self.coerce_annotated_expr(lowered.expr, expected, &lowered.ty, span)
                } else {
                    lowered.expr
                };
                let span = expr.span();
                vec![HirStmt::Return {
                    value: Some(expr),
                    span,
                }]
            }
        };

        let mut guards = Vec::new();
        if let Some(current) = self.fn_stack.last() {
            for param in &hir_params {
                if let Some(ty) = &param.ty {
                    guards.push(HirStmt::Local {
                        id: param.local_id,
                        init: Some(HirExpr::TypeGuard {
                            expr: Box::new(HirExpr::Local {
                                id: param.local_id,
                                span: param.span,
                            }),
                            ty: ty.clone(),
                            span: param.span,
                        }),
                        span: param.span,
                    });
                }
            }
            debug_assert_eq!(current.fn_id, fn_id);
        }
        guards.append(&mut body);

        let finished = self.fn_stack.pop().expect("function stack should pop");
        let function = self
            .module
            .functions
            .get_mut(fn_id.0)
            .expect("function id should exist");
        function.params = hir_params;
        function.return_ty = input.return_ty;
        function.locals = finished.locals;
        function.upvalues = finished.upvalues;
        function.body = guards;
        function.signature = input.signature;
    }

    fn lower_block(&mut self, block: &Block) -> Vec<HirStmt> {
        self.push_scope();
        let mut stmts = Vec::new();
        for stmt in &block.stmts {
            stmts.extend(self.lower_stmt(stmt));
        }
        self.pop_scope();
        stmts
    }

    fn lower_stmt(&mut self, stmt: &ast::Stmt) -> Vec<HirStmt> {
        match stmt {
            ast::Stmt::Function(func) => {
                let local_id = self.declare_local(&func.name, None);
                let fn_id = self.lower_fn_decl(func);
                vec![HirStmt::Local {
                    id: local_id,
                    init: Some(HirExpr::Closure {
                        fn_id,
                        span: func.span,
                    }),
                    span: func.span,
                }]
            }
            ast::Stmt::Var(var) => {
                let ty = var.ty.as_ref().map(|ty| self.resolve_type(ty));
                let local_id = self.declare_local(&var.name, ty.clone());
                let init = var.init.as_ref().map(|expr| {
                    let lowered =
                        self.lower_expr(expr, ty.clone().map_or(TyCtx::None, TyCtx::Expect));
                    if let Some(expected) = &ty {
                        let span = lowered.expr.span();
                        self.coerce_annotated_expr(lowered.expr, expected, &lowered.ty, span)
                    } else {
                        lowered.expr
                    }
                });
                vec![HirStmt::Local {
                    id: local_id,
                    init,
                    span: var.span,
                }]
            }
            ast::Stmt::Assign(assign) => {
                let target = self.lower_assign_target(&assign.target);
                let target_ty = self.assign_target_ty(&target);
                let lowered = self.lower_expr(
                    &assign.value,
                    target_ty.clone().map_or(TyCtx::None, TyCtx::Expect),
                );
                let value = target_ty.as_ref().map_or(lowered.expr.clone(), |expected| {
                    let span = lowered.expr.span();
                    self.coerce_annotated_expr(lowered.expr, expected, &lowered.ty, span)
                });
                if assign.op == AssignOp::NullCoalesce {
                    let assign_stmt = HirStmt::Assign {
                        target: target.clone(),
                        op: AssignOp::Assign,
                        value,
                        span: assign.span,
                    };
                    let cond = self
                        .assign_target_read_expr(&target, assign.span)
                        .map(|expr| HirExpr::Binary {
                            op: BinaryOp::Eq,
                            lhs: Box::new(expr),
                            rhs: Box::new(HirExpr::Const {
                                value: Value::Null,
                                span: assign.span,
                            }),
                            span: assign.span,
                        });
                    vec![HirStmt::If {
                        cond: cond.unwrap_or(HirExpr::Error(assign.span)),
                        then_: vec![assign_stmt],
                        else_: None,
                        span: assign.span,
                    }]
                } else {
                    vec![HirStmt::Assign {
                        target,
                        op: assign.op,
                        value,
                        span: assign.span,
                    }]
                }
            }
            ast::Stmt::Expr(expr) => vec![HirStmt::Expr(self.lower_expr(expr, TyCtx::None).expr)],
            ast::Stmt::If(if_stmt) => {
                let cond = self
                    .lower_expr(&if_stmt.condition, TyCtx::Expect(Ty::Bool))
                    .expr;
                let then_ = self.lower_block(&if_stmt.then_block);
                let else_ = if_stmt.else_branch.as_ref().map(|branch| match branch {
                    ElseBranch::If(stmt) => self.lower_stmt(&ast::Stmt::If((**stmt).clone())),
                    ElseBranch::Block(block) => self.lower_block(block),
                });
                vec![HirStmt::If {
                    cond,
                    then_,
                    else_,
                    span: if_stmt.span,
                }]
            }
            ast::Stmt::While(while_stmt) => {
                let cond = self
                    .lower_expr(&while_stmt.condition, TyCtx::Expect(Ty::Bool))
                    .expr;
                self.with_loop(|this| {
                    vec![HirStmt::While {
                        cond,
                        body: this.lower_block(&while_stmt.body),
                        span: while_stmt.span,
                    }]
                })
            }
            ast::Stmt::Until(until_stmt) => {
                let inner = self
                    .lower_expr(&until_stmt.condition, TyCtx::Expect(Ty::Bool))
                    .expr;
                let cond = HirExpr::Unary {
                    op: UnaryOp::Not,
                    expr: Box::new(inner),
                    span: until_stmt.condition.span(),
                };
                self.with_loop(|this| {
                    vec![HirStmt::While {
                        cond,
                        body: this.lower_block(&until_stmt.body),
                        span: until_stmt.span,
                    }]
                })
            }
            ast::Stmt::Loop(loop_stmt) => self.with_loop(|this| {
                vec![HirStmt::Loop {
                    body: this.lower_block(&loop_stmt.body),
                    span: loop_stmt.span,
                }]
            }),
            ast::Stmt::ForIn(for_stmt) => {
                let iter = self.lower_expr(&for_stmt.iterable, TyCtx::None).expr;
                self.with_loop(|this| {
                    this.push_scope();
                    let item = this.declare_local(&for_stmt.item, None);
                    let body = this.lower_block(&for_stmt.body);
                    this.pop_scope();
                    if let HirExpr::Range {
                        start,
                        end,
                        inclusive,
                        ..
                    } = iter
                    {
                        vec![HirStmt::ForRange {
                            var: item,
                            start: *start,
                            end: *end,
                            inclusive,
                            body,
                            span: for_stmt.span,
                        }]
                    } else {
                        vec![HirStmt::ForIn {
                            item,
                            iter,
                            body,
                            span: for_stmt.span,
                        }]
                    }
                })
            }
            ast::Stmt::Break(span) => {
                if self.current_loop_depth() == 0 {
                    self.push(SemaErrorKind::BreakOutsideLoop, *span);
                }
                vec![HirStmt::Break(*span)]
            }
            ast::Stmt::Continue(span) => {
                if self.current_loop_depth() == 0 {
                    self.push(SemaErrorKind::ContinueOutsideLoop, *span);
                }
                vec![HirStmt::Continue(*span)]
            }
            ast::Stmt::Return(ret) => {
                let is_iter = self.fn_stack.last().is_some_and(|func| func.is_iter);
                if is_iter && ret.value.is_some() {
                    self.push(SemaErrorKind::ReturnValueInIterFn, ret.span);
                }
                let expected = self.fn_stack.last().and_then(|func| func.return_ty.clone());
                let value = ret.value.as_ref().map(|expr| {
                    let lowered =
                        self.lower_expr(expr, expected.clone().map_or(TyCtx::None, TyCtx::Expect));
                    if let Some(expected) = &expected {
                        let span = lowered.expr.span();
                        self.coerce_annotated_expr(lowered.expr, expected, &lowered.ty, span)
                    } else {
                        lowered.expr
                    }
                });
                vec![HirStmt::Return {
                    value,
                    span: ret.span,
                }]
            }
            ast::Stmt::Throw(throw) => {
                let value = self.lower_expr(&throw.value, TyCtx::None).expr;
                vec![HirStmt::Throw {
                    value,
                    span: throw.span,
                }]
            }
            ast::Stmt::TryCatch(try_catch) => {
                let try_ = self.lower_block(&try_catch.try_block);
                self.push_scope();
                let err = self.declare_local(&try_catch.error_name, Some(Ty::Any));
                let catch_ = self.lower_block(&try_catch.catch_block);
                self.pop_scope();
                vec![HirStmt::TryCatch {
                    try_,
                    err,
                    catch_,
                    span: try_catch.span,
                }]
            }
            ast::Stmt::Yield(yield_stmt) => {
                if !self.fn_stack.last().is_some_and(|func| func.is_iter) {
                    let span = match yield_stmt {
                        YieldStmt::Value { span, .. } | YieldStmt::From { span, .. } => *span,
                    };
                    self.push(SemaErrorKind::YieldOutsideIterFn, span);
                }
                match yield_stmt {
                    YieldStmt::Value { value, span } => vec![HirStmt::Yield {
                        value: self.lower_expr(value, TyCtx::None).expr,
                        span: *span,
                    }],
                    YieldStmt::From { value, span } => vec![HirStmt::YieldFrom {
                        value: self.lower_expr(value, TyCtx::None).expr,
                        span: *span,
                    }],
                }
            }
        }
    }

    fn lower_assign_target(&mut self, target: &ast::AssignTarget) -> HirAssignTarget {
        if self.in_check_block {
            self.push(SemaErrorKind::CheckBlockSideEffect, target.span());
        }
        match target {
            ast::AssignTarget::Name(name) => match self.resolve_name(name) {
                ResolvedName::Local(id) => HirAssignTarget::Local(id),
                ResolvedName::Upvalue(id) => HirAssignTarget::Upvalue(id),
                ResolvedName::Global(id) => {
                    if self
                        .symbols
                        .get_global(&name.text)
                        .is_some_and(GlobalEntry::readonly)
                    {
                        self.push(SemaErrorKind::AssignToReadonly, name.span);
                    }
                    HirAssignTarget::Global(id)
                }
                ResolvedName::Error => HirAssignTarget::Global(GlobalId(usize::MAX)),
            },
            ast::AssignTarget::Field {
                object,
                field,
                span,
            } => {
                if let Expr::Name(base) = object.as_ref() {
                    if matches!(
                        self.symbols.get_global(&base.text),
                        Some(
                            GlobalEntry::Enum { .. }
                                | GlobalEntry::Class { .. }
                                | GlobalEntry::Import { .. }
                        )
                    ) {
                        self.push(SemaErrorKind::AssignToReadonly, *span);
                    }
                }
                HirAssignTarget::Field {
                    obj: Box::new(self.lower_expr(object, TyCtx::None).expr),
                    field: field.text.clone(),
                    span: *span,
                }
            }
            ast::AssignTarget::Index {
                object,
                index,
                span,
            } => HirAssignTarget::Index {
                obj: Box::new(self.lower_expr(object, TyCtx::None).expr),
                index: Box::new(self.lower_expr(index, TyCtx::None).expr),
                span: *span,
            },
        }
    }

    fn lower_expr(&mut self, expr: &Expr, ctx: TyCtx) -> TypedExpr {
        if self.in_check_block && matches!(expr, Expr::Call(_)) {
            self.push(SemaErrorKind::CheckBlockSideEffect, expr.span());
        }

        match expr {
            Expr::Literal(literal) => self.lower_literal(literal),
            Expr::Name(name) => {
                if name.text == "self" {
                    self.push(SemaErrorKind::SelfOutsideCheck, name.span);
                    return TypedExpr {
                        expr: HirExpr::Error(name.span),
                        ty: Ty::Error,
                    };
                }
                match self.resolve_name(name) {
                    ResolvedName::Local(id) => {
                        let ty = self.local_ty(id).unwrap_or(Ty::Any);
                        TypedExpr {
                            expr: HirExpr::Local {
                                id,
                                span: name.span,
                            },
                            ty,
                        }
                    }
                    ResolvedName::Upvalue(id) => TypedExpr {
                        expr: HirExpr::Upvalue {
                            id,
                            span: name.span,
                        },
                        ty: Ty::Any,
                    },
                    ResolvedName::Global(id) => {
                        let ty = self.global_ty(id).unwrap_or(Ty::Any);
                        TypedExpr {
                            expr: HirExpr::Global {
                                id,
                                span: name.span,
                            },
                            ty,
                        }
                    }
                    ResolvedName::Error => TypedExpr {
                        expr: HirExpr::Error(name.span),
                        ty: Ty::Error,
                    },
                }
            }
            Expr::Array(array) => {
                let element_ctx = match &ctx {
                    TyCtx::Expect(Ty::Array(element)) => TyCtx::Expect((**element).clone()),
                    _ => TyCtx::None,
                };
                let mut elements = Vec::new();
                let mut element_tys = Vec::new();
                for element in &array.elements {
                    let lowered = self.lower_expr(element, element_ctx.clone());
                    element_tys.push(lowered.ty.clone());
                    elements.push(lowered.expr);
                }
                let element_ty = infer_common_ty(&element_tys).unwrap_or(Ty::Any);
                let array_ty = Ty::Array(Box::new(element_ty));
                let expr = HirExpr::Array {
                    elements,
                    span: array.span,
                };
                self.coerce_typed(expr, array_ty, ctx, array.span)
            }
            Expr::Record(record) => self.lower_record(record, ctx),
            Expr::Fn(func) => self.lower_fn_expr(func, ctx),
            Expr::Lambda(lambda) => self.lower_lambda(lambda, ctx),
            Expr::Range(range) => {
                let start = self.lower_expr(&range.start, TyCtx::Expect(Ty::Int)).expr;
                let end = self.lower_expr(&range.end, TyCtx::Expect(Ty::Int)).expr;
                TypedExpr {
                    expr: HirExpr::Range {
                        start: Box::new(start),
                        end: Box::new(end),
                        inclusive: range.inclusive,
                        span: range.span,
                    },
                    ty: Ty::Iterator,
                }
            }
            Expr::Unary(unary) => {
                let lowered = self.lower_expr(&unary.expr, TyCtx::None);
                let ty = match unary.op {
                    UnaryOp::Not => Ty::Bool,
                    UnaryOp::Neg | UnaryOp::BitNot => lowered.ty,
                };
                TypedExpr {
                    expr: HirExpr::Unary {
                        op: unary.op,
                        expr: Box::new(lowered.expr),
                        span: unary.span,
                    },
                    ty,
                }
            }
            Expr::Binary(binary) => {
                if binary.op == BinaryOp::NullCoalesce {
                    let left = self.lower_expr(&binary.lhs, TyCtx::None);
                    let right = self.lower_expr(&binary.rhs, ctx.clone());
                    return TypedExpr {
                        expr: HirExpr::NullCoalesce {
                            left: Box::new(left.expr),
                            right: Box::new(right.expr),
                            span: binary.span,
                        },
                        ty: merge_ty(&left.ty, &right.ty),
                    };
                }
                if binary.op == BinaryOp::And
                    && is_comparison_expr(&binary.lhs)
                    && is_comparison_expr(&binary.rhs)
                {
                    let mut exprs = Vec::new();
                    collect_and_chain(binary, self, &mut exprs);
                    return TypedExpr {
                        expr: HirExpr::AndChain {
                            exprs,
                            span: binary.span,
                        },
                        ty: Ty::Bool,
                    };
                }
                let lhs = self.lower_expr(&binary.lhs, TyCtx::None);
                let rhs = self.lower_expr(&binary.rhs, TyCtx::None);
                let ty = binary_result_ty(binary.op, &lhs.ty, &rhs.ty);
                let expr = HirExpr::Binary {
                    op: binary.op,
                    lhs: Box::new(lhs.expr),
                    rhs: Box::new(rhs.expr),
                    span: binary.span,
                };
                self.coerce_typed(expr, ty, ctx, binary.span)
            }
            Expr::Call(call) => {
                if self.in_check_block {
                    self.push(SemaErrorKind::CheckBlockSideEffect, call.span);
                }
                let callee = self.lower_expr(&call.callee, TyCtx::None);
                let expected_params = match &callee.ty {
                    Ty::FunctionSig { params, .. } => Some(params.clone()),
                    _ => None,
                };
                let mut args = Vec::new();
                for (idx, arg) in call.args.iter().enumerate() {
                    if arg.name.is_some() && expected_params.is_some() {
                        self.push(SemaErrorKind::TypeMismatch, arg.span);
                    }
                    let arg_ctx = expected_params
                        .as_ref()
                        .and_then(|params| params.get(idx))
                        .cloned()
                        .map_or(TyCtx::None, TyCtx::Expect);
                    let lowered = self.lower_expr(&arg.value, arg_ctx);
                    if let Some(params) = &expected_params {
                        if let Some(expected) = params.get(idx) {
                            self.check_assignable(expected, &lowered.ty, arg.span);
                        }
                    }
                    args.push(HirArg {
                        name: arg.name.as_ref().map(|name| name.text.clone()),
                        value: lowered.expr,
                        span: arg.span,
                    });
                }
                let ty = match &callee.ty {
                    Ty::FunctionSig { params, return_ty } => {
                        if args.len() != params.len() {
                            self.push(SemaErrorKind::TypeMismatch, call.span);
                        }
                        (**return_ty).clone()
                    }
                    Ty::Function | Ty::Any | Ty::Error => Ty::Any,
                    _ => {
                        self.push(SemaErrorKind::TypeMismatch, call.span);
                        Ty::Error
                    }
                };
                let expr = HirExpr::Call {
                    callee: Box::new(callee.expr),
                    args,
                    span: call.span,
                };
                self.coerce_typed(expr, ty, ctx, call.span)
            }
            Expr::Field(field) => {
                if self.in_check_block && is_name(&field.object, "self") {
                    return TypedExpr {
                        expr: HirExpr::SelfField {
                            name: field.field.text.clone(),
                            span: field.span,
                        },
                        ty: Ty::Any,
                    };
                }
                if let Expr::Name(base) = field.object.as_ref() {
                    if let Some(result) = self.try_lower_path_field(base, &field.field, field.span)
                    {
                        return result;
                    }
                }
                let obj = self.lower_expr(&field.object, TyCtx::None);
                let ty = match &obj.ty {
                    Ty::Class(class_id) => self
                        .class_field_ty(*class_id, &field.field.text)
                        .unwrap_or_else(|| {
                            self.push(SemaErrorKind::UndefinedName, field.field.span);
                            Ty::Error
                        }),
                    Ty::Any | Ty::Error => Ty::Any,
                    _ => Ty::Any,
                };
                let expr = HirExpr::Field {
                    obj: Box::new(obj.expr),
                    field: field.field.text.clone(),
                    span: field.span,
                };
                self.coerce_typed(expr, ty, ctx, field.span)
            }
            Expr::OptionalField(field) => {
                let obj = self.lower_expr(&field.object, TyCtx::None);
                TypedExpr {
                    expr: HirExpr::OptField {
                        obj: Box::new(obj.expr),
                        field: field.field.text.clone(),
                        span: field.span,
                    },
                    ty: Ty::Any,
                }
            }
            Expr::Index(index) => {
                let obj = self.lower_expr(&index.object, TyCtx::None);
                let index_expr = self.lower_expr(&index.index, TyCtx::None);
                let ty = match &obj.ty {
                    Ty::Array(element) => (**element).clone(),
                    Ty::Dict(_, value) => (**value).clone(),
                    Ty::Any | Ty::Error => Ty::Any,
                    _ => Ty::Any,
                };
                let expr = HirExpr::Index {
                    obj: Box::new(obj.expr),
                    index: Box::new(index_expr.expr),
                    span: index.span,
                };
                self.coerce_typed(expr, ty, ctx, index.span)
            }
            Expr::OptionalIndex(index) => {
                let obj = self.lower_expr(&index.object, TyCtx::None);
                let index_expr = self.lower_expr(&index.index, TyCtx::None);
                TypedExpr {
                    expr: HirExpr::OptIndex {
                        obj: Box::new(obj.expr),
                        index: Box::new(index_expr.expr),
                        span: index.span,
                    },
                    ty: Ty::Any,
                }
            }
            Expr::If(if_expr) => {
                let cond = self
                    .lower_expr(&if_expr.condition, TyCtx::Expect(Ty::Bool))
                    .expr;
                let then_expr = self.lower_expr(&if_expr.then_expr, ctx.clone());
                let else_expr = self.lower_expr(&if_expr.else_expr, ctx.clone());
                let ty = merge_ty(&then_expr.ty, &else_expr.ty);
                TypedExpr {
                    expr: HirExpr::If {
                        cond: Box::new(cond),
                        then_expr: Box::new(then_expr.expr),
                        else_expr: Box::new(else_expr.expr),
                        span: if_expr.span,
                    },
                    ty,
                }
            }
        }
    }

    fn lower_expr_with_ctx(&mut self, fn_ctx: &mut FnCtx, expr: &Expr, ctx: TyCtx) -> TypedExpr {
        self.fn_stack.push(fn_ctx.clone());
        let lowered = self.lower_expr(expr, ctx);
        *fn_ctx = self.fn_stack.pop().expect("temporary function context");
        lowered
    }

    fn lower_record(&mut self, record: &ast::RecordLiteral, ctx: TyCtx) -> TypedExpr {
        match ctx.clone() {
            TyCtx::Expect(Ty::Class(class_id)) => {
                self.lower_object_record(record, Some(class_id), ctx)
            }
            TyCtx::Expect(Ty::Dict(key_ty, value_ty)) => {
                self.lower_dict_record(record, Some((*key_ty, *value_ty)), ctx)
            }
            TyCtx::Expect(Ty::Any) | TyCtx::None => {
                let mut has_ident = false;
                let mut has_string = false;
                let mut has_spread = false;
                for entry in &record.entries {
                    match entry {
                        RecordEntry::Field { key, .. } => match key {
                            RecordKey::Ident(_) => has_ident = true,
                            RecordKey::String(_) => has_string = true,
                        },
                        RecordEntry::Spread { .. } => has_spread = true,
                    }
                }
                if has_ident && has_string {
                    self.push(SemaErrorKind::RecordKeyMixed, record.span);
                    return TypedExpr {
                        expr: HirExpr::Error(record.span),
                        ty: Ty::Error,
                    };
                }
                if has_string && !has_ident && !has_spread {
                    if self.checking_config_without_ty {
                        self.push(SemaErrorKind::DictWithoutAnnotation, record.span);
                    }
                    self.lower_dict_record(record, None, ctx)
                } else {
                    self.lower_object_record(record, None, ctx)
                }
            }
            TyCtx::Expect(other) => {
                let typed = self.lower_object_record(record, None, TyCtx::None);
                self.check_assignable(&other, &typed.ty, record.span);
                typed
            }
        }
    }

    fn lower_object_record(
        &mut self,
        record: &ast::RecordLiteral,
        class: Option<ClassId>,
        ctx: TyCtx,
    ) -> TypedExpr {
        let mut fields = Vec::new();
        let mut spreads = Vec::new();
        let mut seen = HashSet::new();
        for entry in &record.entries {
            match entry {
                RecordEntry::Field { key, value, span } => {
                    let key_name = match key {
                        RecordKey::Ident(ident) => ident.text.clone(),
                        RecordKey::String(string) => self.decode_string(string).unwrap_or_default(),
                    };
                    if !seen.insert(key_name.clone()) {
                        self.push(SemaErrorKind::DuplicateField, *span);
                    }
                    let field_ty =
                        class.and_then(|class_id| self.class_field_ty(class_id, &key_name));
                    if class.is_some() && field_ty.is_none() {
                        self.push(SemaErrorKind::TypeMismatch, *span);
                    }
                    let lowered =
                        self.lower_expr(value, field_ty.clone().map_or(TyCtx::None, TyCtx::Expect));
                    if let Some(expected) = &field_ty {
                        self.check_assignable(expected, &lowered.ty, *span);
                    }
                    fields.push((key_name, lowered.expr));
                }
                RecordEntry::Spread { expr, .. } => {
                    spreads.push(self.lower_expr(expr, TyCtx::None).expr);
                }
            }
        }

        if let Some(class_id) = class {
            let class_fields = self
                .module
                .classes
                .get(class_id.0)
                .map(|class| class.fields.clone())
                .unwrap_or_default();
            for field in &class_fields {
                if !seen.contains(&field.name) && field.default.is_none() {
                    self.push(SemaErrorKind::TypeMismatch, field.span);
                }
            }
        }

        let ty = class.map_or(Ty::Any, Ty::Class);
        let expr = HirExpr::Object {
            class,
            fields,
            spreads,
            span: record.span,
        };
        self.coerce_typed(expr, ty, ctx, record.span)
    }

    fn lower_dict_record(
        &mut self,
        record: &ast::RecordLiteral,
        expected: Option<(Ty, Ty)>,
        ctx: TyCtx,
    ) -> TypedExpr {
        let mut entries = Vec::new();
        let mut value_tys = Vec::new();
        for entry in &record.entries {
            match entry {
                RecordEntry::Field { key, value, span } => {
                    let key_expr = match key {
                        RecordKey::Ident(ident) => HirExpr::Const {
                            value: Value::String(Rc::from(ident.text.as_str())),
                            span: ident.span,
                        },
                        RecordKey::String(string) => match self.decode_string(string) {
                            Ok(value) => HirExpr::Const {
                                value: Value::String(Rc::from(value.as_str())),
                                span: string.span,
                            },
                            Err(_) => HirExpr::Error(string.span),
                        },
                    };
                    if let Some((key_ty, _)) = &expected {
                        self.check_assignable(key_ty, &Ty::String, *span);
                    }
                    let value_ctx = expected
                        .as_ref()
                        .map(|(_, value_ty)| TyCtx::Expect(value_ty.clone()))
                        .unwrap_or(TyCtx::None);
                    let lowered = self.lower_expr(value, value_ctx);
                    if let Some((_, expected_value_ty)) = &expected {
                        self.check_assignable(expected_value_ty, &lowered.ty, *span);
                    }
                    value_tys.push(lowered.ty.clone());
                    entries.push((key_expr, lowered.expr));
                }
                RecordEntry::Spread { span, .. } => {
                    self.push(SemaErrorKind::TypeMismatch, *span);
                }
            }
        }
        let (key_ty, value_ty) = expected
            .unwrap_or_else(|| (Ty::String, infer_common_ty(&value_tys).unwrap_or(Ty::Any)));
        let ty = Ty::Dict(Box::new(key_ty), Box::new(value_ty));
        let expr = HirExpr::Dict {
            entries,
            span: record.span,
        };
        self.coerce_typed(expr, ty, ctx, record.span)
    }

    fn lower_literal(&mut self, literal: &Literal) -> TypedExpr {
        match literal {
            Literal::Int { raw, span } => match parse_int_literal(raw) {
                Ok(value) => TypedExpr {
                    expr: HirExpr::Const {
                        value: Value::Int(value),
                        span: *span,
                    },
                    ty: Ty::Int,
                },
                Err(kind) => {
                    self.push(kind, *span);
                    TypedExpr {
                        expr: HirExpr::Error(*span),
                        ty: Ty::Error,
                    }
                }
            },
            Literal::Float { raw, span } => match raw.replace('_', "").parse::<f64>() {
                Ok(value) => TypedExpr {
                    expr: HirExpr::Const {
                        value: Value::Float(value),
                        span: *span,
                    },
                    ty: Ty::Float,
                },
                Err(_) => {
                    self.push(SemaErrorKind::InvalidLiteral, *span);
                    TypedExpr {
                        expr: HirExpr::Error(*span),
                        ty: Ty::Error,
                    }
                }
            },
            Literal::String(string) => match self.decode_string(string) {
                Ok(value) => TypedExpr {
                    expr: HirExpr::Const {
                        value: Value::String(Rc::from(value.as_str())),
                        span: string.span,
                    },
                    ty: Ty::String,
                },
                Err(kind) => {
                    self.push(kind, string.span);
                    TypedExpr {
                        expr: HirExpr::Error(string.span),
                        ty: Ty::Error,
                    }
                }
            },
            Literal::Bool { value, span } => TypedExpr {
                expr: HirExpr::Const {
                    value: Value::Bool(*value),
                    span: *span,
                },
                ty: Ty::Bool,
            },
            Literal::Null { span } => TypedExpr {
                expr: HirExpr::Const {
                    value: Value::Null,
                    span: *span,
                },
                ty: Ty::Null,
            },
        }
    }

    fn try_lower_path_field(
        &mut self,
        base: &Ident,
        field: &Ident,
        span: Span,
    ) -> Option<TypedExpr> {
        match self.symbols.get_global(&base.text) {
            Some(GlobalEntry::Enum { enum_id, .. }) => {
                let enum_info = &self.symbols.enums[enum_id.0];
                if let Some((idx, _)) = enum_info
                    .variants
                    .iter()
                    .enumerate()
                    .find(|(_, variant)| variant.name == field.text)
                {
                    Some(TypedExpr {
                        expr: HirExpr::Variant {
                            enum_id: *enum_id,
                            variant: VariantId(idx),
                            span,
                        },
                        ty: Ty::Enum(*enum_id),
                    })
                } else {
                    self.push(SemaErrorKind::UndefinedName, field.span);
                    Some(TypedExpr {
                        expr: HirExpr::Error(span),
                        ty: Ty::Error,
                    })
                }
            }
            Some(GlobalEntry::Import { .. }) => {
                self.push(SemaErrorKind::UnsupportedNotImplemented, span);
                Some(TypedExpr {
                    expr: HirExpr::Error(span),
                    ty: Ty::Error,
                })
            }
            _ => None,
        }
    }

    fn resolve_name(&mut self, name: &Ident) -> ResolvedName {
        if name.text == "self" {
            self.push(SemaErrorKind::SelfOutsideCheck, name.span);
            return ResolvedName::Error;
        }

        if let Some((depth, local_id)) = self.find_local_in_stack(&name.text) {
            let current_depth = self.fn_stack.len().saturating_sub(1);
            if depth == current_depth {
                return ResolvedName::Local(local_id);
            }
            let upvalue = self.capture_upvalue(depth, local_id);
            return ResolvedName::Upvalue(upvalue);
        }

        if let Some(entry) = self.symbols.get_global(&name.text) {
            return ResolvedName::Global(entry.id());
        }

        self.push(SemaErrorKind::UndefinedName, name.span);
        ResolvedName::Error
    }

    fn find_local_in_stack(&self, name: &str) -> Option<(usize, LocalId)> {
        for (depth, func) in self.fn_stack.iter().enumerate().rev() {
            for scope in func.scopes.iter().rev() {
                if let Some(local_id) = scope.vars.get(name) {
                    return Some((depth, *local_id));
                }
            }
        }
        None
    }

    fn capture_upvalue(&mut self, owner_depth: usize, local_id: LocalId) -> UpvalueId {
        let mut desc = UpvalueDesc::Local(local_id);
        if let Some(owner) = self.fn_stack.get_mut(owner_depth) {
            if let Some(local) = owner.locals.get_mut(local_id.0) {
                local.is_captured = true;
            }
        }
        for depth in owner_depth + 1..self.fn_stack.len() {
            let func = &mut self.fn_stack[depth];
            let upvalue_id = if let Some(id) = func.upvalue_index.get(&desc) {
                *id
            } else {
                let id = UpvalueId(func.upvalues.len());
                func.upvalues.push(desc);
                func.upvalue_index.insert(desc, id);
                id
            };
            desc = UpvalueDesc::Upvalue(upvalue_id);
        }
        match desc {
            UpvalueDesc::Upvalue(id) => id,
            UpvalueDesc::Local(_) => unreachable!("current function locals should not be captured"),
        }
    }

    fn resolve_type(&mut self, ty: &TypeExpr) -> Ty {
        match ty {
            TypeExpr::Name(path) => {
                if path.segments.len() != 1 {
                    self.push(SemaErrorKind::UnknownType, path.span);
                    return Ty::Error;
                }
                let name = &path.segments[0].text;
                match name.as_str() {
                    "int" => Ty::Int,
                    "float" => Ty::Float,
                    "bool" => Ty::Bool,
                    "string" => Ty::String,
                    "null" => Ty::Null,
                    "any" => Ty::Any,
                    _ => match self.symbols.get_global(name) {
                        Some(GlobalEntry::Class { class_id, .. }) => Ty::Class(*class_id),
                        Some(GlobalEntry::Enum { enum_id, .. }) => Ty::Enum(*enum_id),
                        _ => {
                            self.push(SemaErrorKind::UnknownType, path.span);
                            Ty::Error
                        }
                    },
                }
            }
            TypeExpr::Array { element, .. } => Ty::Array(Box::new(self.resolve_type(element))),
            TypeExpr::Dict { key, value, .. } => Ty::Dict(
                Box::new(self.resolve_type(key)),
                Box::new(self.resolve_type(value)),
            ),
            TypeExpr::Function {
                params, return_ty, ..
            } => {
                if let Some(params) = params {
                    Ty::FunctionSig {
                        params: params
                            .iter()
                            .map(|param| self.resolve_type(param))
                            .collect(),
                        return_ty: Box::new(
                            return_ty
                                .as_ref()
                                .map(|return_ty| self.resolve_type(return_ty))
                                .unwrap_or(Ty::Any),
                        ),
                    }
                } else {
                    Ty::Function
                }
            }
        }
    }

    fn function_signature(
        &mut self,
        params: &[ast::Param],
        return_ty: Option<Ty>,
        is_iter: bool,
    ) -> Ty {
        if is_iter {
            return Ty::FunctionSig {
                params: params
                    .iter()
                    .filter(|param| param.default.is_none())
                    .map(|param| {
                        param
                            .ty
                            .as_ref()
                            .map(|ty| self.resolve_type(ty))
                            .unwrap_or(Ty::Any)
                    })
                    .collect(),
                return_ty: Box::new(Ty::Iterator),
            };
        }
        Ty::FunctionSig {
            params: params
                .iter()
                .filter(|param| param.default.is_none())
                .map(|param| {
                    param
                        .ty
                        .as_ref()
                        .map(|ty| self.resolve_type(ty))
                        .unwrap_or(Ty::Any)
                })
                .collect(),
            return_ty: Box::new(return_ty.unwrap_or(Ty::Any)),
        }
    }

    fn infer_function_signature(
        &mut self,
        params: &[ast::Param],
        return_ty: Option<Ty>,
        is_iter: bool,
        expected: Option<(Vec<Ty>, Ty)>,
    ) -> Ty {
        if is_iter {
            return self.function_signature(params, return_ty, true);
        }
        let expected_params = expected.as_ref().map(|(params, _)| params.as_slice());
        let params_ty = params
            .iter()
            .filter(|param| param.default.is_none())
            .enumerate()
            .map(|(idx, param)| {
                param
                    .ty
                    .as_ref()
                    .map(|ty| self.resolve_type(ty))
                    .or_else(|| expected_params.and_then(|params| params.get(idx).cloned()))
                    .unwrap_or(Ty::Any)
            })
            .collect();
        Ty::FunctionSig {
            params: params_ty,
            return_ty: Box::new(
                return_ty
                    .or_else(|| expected.map(|(_, return_ty)| return_ty))
                    .unwrap_or(Ty::Any),
            ),
        }
    }

    fn check_assignable(&mut self, expected: &Ty, actual: &Ty, span: Span) -> bool {
        let ok = is_assignable(expected, actual);
        if !ok {
            self.push(SemaErrorKind::TypeMismatch, span);
        }
        ok
    }

    fn coerce_typed(&mut self, expr: HirExpr, ty: Ty, ctx: TyCtx, span: Span) -> TypedExpr {
        match ctx {
            TyCtx::Expect(expected) => {
                let expr = self.coerce_expr(expr, &expected, &ty, span);
                TypedExpr { expr, ty: expected }
            }
            TyCtx::None => TypedExpr { expr, ty },
        }
    }

    fn coerce_expr(&mut self, expr: HirExpr, expected: &Ty, actual: &Ty, span: Span) -> HirExpr {
        if matches!(expected, Ty::Error) || matches!(actual, Ty::Error) {
            return expr;
        }
        if is_assignable(expected, actual) {
            if should_guard(expected, actual) {
                HirExpr::TypeGuard {
                    expr: Box::new(expr),
                    ty: expected.clone(),
                    span,
                }
            } else {
                expr
            }
        } else {
            self.push(SemaErrorKind::TypeMismatch, span);
            expr
        }
    }

    fn coerce_annotated_expr(
        &mut self,
        expr: HirExpr,
        expected: &Ty,
        actual: &Ty,
        span: Span,
    ) -> HirExpr {
        if matches!(expected, Ty::Error) || matches!(actual, Ty::Error) {
            return expr;
        }
        if is_assignable(expected, actual) {
            HirExpr::TypeGuard {
                expr: Box::new(expr),
                ty: expected.clone(),
                span,
            }
        } else {
            self.push(SemaErrorKind::TypeMismatch, span);
            expr
        }
    }

    fn infer_hir_expr_type(&self, expr: &HirExpr) -> Ty {
        match expr {
            HirExpr::Const { value, .. } => value_ty(value),
            HirExpr::Local { id, .. } => self.local_ty(*id).unwrap_or(Ty::Any),
            HirExpr::Global { id, .. } => self.global_ty(*id).unwrap_or(Ty::Any),
            HirExpr::Variant { enum_id, .. } => Ty::Enum(*enum_id),
            HirExpr::Closure { fn_id, .. } => self
                .module
                .functions
                .get(fn_id.0)
                .map(|func| func.signature.clone())
                .unwrap_or(Ty::Function),
            HirExpr::Array { elements, .. } => Ty::Array(Box::new(
                infer_common_ty(
                    &elements
                        .iter()
                        .map(|expr| self.infer_hir_expr_type(expr))
                        .collect::<Vec<_>>(),
                )
                .unwrap_or(Ty::Any),
            )),
            HirExpr::Dict { entries, .. } => Ty::Dict(
                Box::new(Ty::String),
                Box::new(
                    infer_common_ty(
                        &entries
                            .iter()
                            .map(|(_, value)| self.infer_hir_expr_type(value))
                            .collect::<Vec<_>>(),
                    )
                    .unwrap_or(Ty::Any),
                ),
            ),
            HirExpr::Object { class, .. } => class.map_or(Ty::Any, Ty::Class),
            HirExpr::Range { .. } => Ty::Iterator,
            HirExpr::TypeGuard { ty, .. } => ty.clone(),
            HirExpr::Binary { op, lhs, rhs, .. } => binary_result_ty(
                *op,
                &self.infer_hir_expr_type(lhs),
                &self.infer_hir_expr_type(rhs),
            ),
            HirExpr::Unary { op, expr, .. } => match op {
                UnaryOp::Not => Ty::Bool,
                UnaryOp::Neg | UnaryOp::BitNot => self.infer_hir_expr_type(expr),
            },
            HirExpr::NullCoalesce { left, right, .. } => merge_ty(
                &self.infer_hir_expr_type(left),
                &self.infer_hir_expr_type(right),
            ),
            HirExpr::If {
                then_expr,
                else_expr,
                ..
            } => merge_ty(
                &self.infer_hir_expr_type(then_expr),
                &self.infer_hir_expr_type(else_expr),
            ),
            HirExpr::AndChain { .. } => Ty::Bool,
            HirExpr::SelfField { .. }
            | HirExpr::Upvalue { .. }
            | HirExpr::Call { .. }
            | HirExpr::Field { .. }
            | HirExpr::OptField { .. }
            | HirExpr::Index { .. }
            | HirExpr::OptIndex { .. } => Ty::Any,
            HirExpr::Error(_) => Ty::Error,
        }
    }

    fn global_ty(&self, id: GlobalId) -> Option<Ty> {
        let (_, entry) = self.symbols.globals.get(id.0)?;
        match entry {
            GlobalEntry::Config { ty, .. } | GlobalEntry::Var { ty, .. } => {
                ty.as_ref().map(|ty| self.resolve_type_readonly(ty))
            }
            GlobalEntry::Function { fn_id, .. } if fn_id.0 != usize::MAX => self
                .module
                .functions
                .get(fn_id.0)
                .map(|func| func.signature.clone()),
            GlobalEntry::Function { ast_index, .. } => {
                let fn_id = self.top_level_function_ids.get(ast_index)?;
                self.module
                    .functions
                    .get(fn_id.0)
                    .map(|func| func.signature.clone())
            }
            GlobalEntry::Class { class_id, .. } => Some(Ty::Class(*class_id)),
            GlobalEntry::Enum { enum_id, .. } => Some(Ty::Enum(*enum_id)),
            GlobalEntry::Builtin { .. } => Some(Ty::Function),
            GlobalEntry::Import { .. } => Some(Ty::Any),
        }
    }

    fn resolve_type_readonly(&self, ty: &TypeExpr) -> Ty {
        match ty {
            TypeExpr::Name(path) => {
                if path.segments.len() != 1 {
                    return Ty::Error;
                }
                match path.segments[0].text.as_str() {
                    "int" => Ty::Int,
                    "float" => Ty::Float,
                    "bool" => Ty::Bool,
                    "string" => Ty::String,
                    "null" => Ty::Null,
                    "any" => Ty::Any,
                    name => match self.symbols.get_global(name) {
                        Some(GlobalEntry::Class { class_id, .. }) => Ty::Class(*class_id),
                        Some(GlobalEntry::Enum { enum_id, .. }) => Ty::Enum(*enum_id),
                        _ => Ty::Error,
                    },
                }
            }
            TypeExpr::Array { element, .. } => {
                Ty::Array(Box::new(self.resolve_type_readonly(element)))
            }
            TypeExpr::Dict { key, value, .. } => Ty::Dict(
                Box::new(self.resolve_type_readonly(key)),
                Box::new(self.resolve_type_readonly(value)),
            ),
            TypeExpr::Function {
                params, return_ty, ..
            } => params
                .as_ref()
                .map_or(Ty::Function, |params| Ty::FunctionSig {
                    params: params
                        .iter()
                        .map(|param| self.resolve_type_readonly(param))
                        .collect(),
                    return_ty: Box::new(
                        return_ty
                            .as_ref()
                            .map(|return_ty| self.resolve_type_readonly(return_ty))
                            .unwrap_or(Ty::Any),
                    ),
                }),
        }
    }

    fn local_ty(&self, id: LocalId) -> Option<Ty> {
        self.fn_stack
            .last()
            .and_then(|func| func.locals.get(id.0))
            .and_then(|local| local.ty.clone())
    }

    fn class_field_ty(&self, class_id: ClassId, field_name: &str) -> Option<Ty> {
        self.module
            .classes
            .get(class_id.0)?
            .fields
            .iter()
            .find_map(|field| (field.name == field_name).then_some(field.ty.clone()))
    }

    fn decode_string(&mut self, string: &ast::StringLiteral) -> Result<String, SemaErrorKind> {
        decode_string_literal(string)
    }

    fn push_scope(&mut self) {
        if let Some(func) = self.fn_stack.last_mut() {
            func.scopes.push(Scope::default());
        }
    }

    fn pop_scope(&mut self) {
        if let Some(func) = self.fn_stack.last_mut() {
            func.scopes.pop();
        }
    }

    fn declare_local(&mut self, name: &Ident, ty: Option<Ty>) -> LocalId {
        if self.fn_stack.is_empty() {
            self.push(SemaErrorKind::UndefinedName, name.span);
            return LocalId(usize::MAX);
        }
        let duplicate = self
            .fn_stack
            .last()
            .and_then(|func| func.scopes.last())
            .is_some_and(|scope| scope.vars.contains_key(&name.text));
        if duplicate {
            self.push(SemaErrorKind::DuplicateLocal, name.span);
        }
        let func = self
            .fn_stack
            .last_mut()
            .expect("function stack should be non-empty");
        let id = LocalId(func.locals.len());
        func.locals.push(HirLocal {
            name: name.text.clone(),
            ty,
            is_captured: false,
            span: name.span,
        });
        if let Some(scope) = func.scopes.last_mut() {
            scope.vars.insert(name.text.clone(), id);
        }
        id
    }

    fn current_loop_depth(&self) -> usize {
        self.fn_stack.last().map_or(0, |func| func.loop_depth)
    }

    fn with_loop(&mut self, f: impl FnOnce(&mut Self) -> Vec<HirStmt>) -> Vec<HirStmt> {
        if let Some(func) = self.fn_stack.last_mut() {
            func.loop_depth += 1;
        }
        let result = f(self);
        if let Some(func) = self.fn_stack.last_mut() {
            func.loop_depth = func.loop_depth.saturating_sub(1);
        }
        result
    }

    fn assign_target_read_expr(&self, target: &HirAssignTarget, span: Span) -> Option<HirExpr> {
        Some(match target {
            HirAssignTarget::Local(id) => HirExpr::Local { id: *id, span },
            HirAssignTarget::Upvalue(id) => HirExpr::Upvalue { id: *id, span },
            HirAssignTarget::Global(id) => HirExpr::Global { id: *id, span },
            HirAssignTarget::Field { obj, field, span } => HirExpr::Field {
                obj: obj.clone(),
                field: field.clone(),
                span: *span,
            },
            HirAssignTarget::Index { obj, index, span } => HirExpr::Index {
                obj: obj.clone(),
                index: index.clone(),
                span: *span,
            },
        })
    }

    fn assign_target_ty(&self, target: &HirAssignTarget) -> Option<Ty> {
        match target {
            HirAssignTarget::Local(id) => self.local_ty(*id),
            HirAssignTarget::Upvalue(id) => self.upvalue_ty(*id),
            HirAssignTarget::Global(id) => self.global_ty(*id),
            HirAssignTarget::Field { obj, field, .. } => {
                match self.infer_hir_expr_type(obj.as_ref()) {
                    Ty::Class(class_id) => self.class_field_ty(class_id, field),
                    Ty::Any | Ty::Error => Some(Ty::Any),
                    _ => None,
                }
            }
            HirAssignTarget::Index { obj, .. } => match self.infer_hir_expr_type(obj.as_ref()) {
                Ty::Array(element) => Some(*element),
                Ty::Dict(_, value) => Some(*value),
                Ty::Any | Ty::Error => Some(Ty::Any),
                _ => None,
            },
        }
    }

    fn upvalue_ty(&self, id: UpvalueId) -> Option<Ty> {
        self.fn_stack
            .len()
            .checked_sub(1)
            .and_then(|depth| self.upvalue_ty_at(depth, id))
    }

    fn upvalue_ty_at(&self, depth: usize, id: UpvalueId) -> Option<Ty> {
        let desc = *self.fn_stack.get(depth)?.upvalues.get(id.0)?;
        match desc {
            UpvalueDesc::Local(local_id) => depth
                .checked_sub(1)
                .and_then(|owner_depth| self.fn_stack.get(owner_depth))
                .and_then(|func| func.locals.get(local_id.0))
                .and_then(|local| local.ty.clone()),
            UpvalueDesc::Upvalue(parent_id) => depth
                .checked_sub(1)
                .and_then(|parent_depth| self.upvalue_ty_at(parent_depth, parent_id)),
        }
    }

    fn check_local_type_leaks(&mut self) {
        let globals = self.module.globals.clone();
        for global in globals {
            match global {
                HirGlobal::Config { ty, span, .. }
                | HirGlobal::Var {
                    ty,
                    span,
                    local: false,
                    ..
                } if ty.as_ref().is_some_and(|ty| self.ty_mentions_local(ty)) => {
                    self.push(SemaErrorKind::LocalTypeLeak, span);
                }
                HirGlobal::Function {
                    local: false,
                    fn_id,
                    span,
                    ..
                } => {
                    if let Some(func) = self.module.functions.get(fn_id.0) {
                        let leaks_params = func.params.iter().any(|param| {
                            param
                                .ty
                                .as_ref()
                                .is_some_and(|ty| self.ty_mentions_local(ty))
                        });
                        let leaks_return = func
                            .return_ty
                            .as_ref()
                            .is_some_and(|ty| self.ty_mentions_local(ty));
                        if leaks_params || leaks_return {
                            self.push(SemaErrorKind::LocalTypeLeak, span);
                        }
                    }
                }
                HirGlobal::Class { class_id, span, .. } => {
                    if let Some(class) = self.module.classes.get(class_id.0) {
                        if !class.local
                            && class
                                .fields
                                .iter()
                                .any(|field| self.ty_mentions_local(&field.ty))
                        {
                            self.push(SemaErrorKind::LocalTypeLeak, span);
                        }
                    }
                }
                _ => {}
            }
        }
    }

    fn ty_mentions_local(&self, ty: &Ty) -> bool {
        match ty {
            Ty::Class(id) => self
                .module
                .classes
                .get(id.0)
                .is_some_and(|class| class.local),
            Ty::Enum(id) => self.module.enums.get(id.0).is_some_and(|enum_| enum_.local),
            Ty::Array(element) => self.ty_mentions_local(element),
            Ty::Dict(key, value) => self.ty_mentions_local(key) || self.ty_mentions_local(value),
            Ty::FunctionSig { params, return_ty } => {
                params.iter().any(|param| self.ty_mentions_local(param))
                    || self.ty_mentions_local(return_ty)
            }
            _ => false,
        }
    }

    fn push(&mut self, kind: SemaErrorKind, span: Span) {
        self.diagnostics.push(Diagnostic::Sema(kind, span));
    }
}

fn collect_and_chain(binary: &ast::BinaryExpr, ctx: &mut LowerCtx<'_, '_>, out: &mut Vec<HirExpr>) {
    if binary.op == BinaryOp::And
        && is_comparison_expr(&binary.lhs)
        && is_comparison_expr(&binary.rhs)
    {
        if let Expr::Binary(lhs) = binary.lhs.as_ref() {
            collect_and_chain(lhs, ctx, out);
        }
        if let Expr::Binary(rhs) = binary.rhs.as_ref() {
            collect_and_chain(rhs, ctx, out);
        }
    } else {
        out.push(
            ctx.lower_expr(&Expr::Binary(binary.clone()), TyCtx::Expect(Ty::Bool))
                .expr,
        );
    }
}

fn is_comparison_expr(expr: &Expr) -> bool {
    matches!(
        expr,
        Expr::Binary(binary)
            if matches!(
                binary.op,
                BinaryOp::Eq
                    | BinaryOp::NotEq
                    | BinaryOp::Lt
                    | BinaryOp::LtEq
                    | BinaryOp::Gt
                    | BinaryOp::GtEq
                    | BinaryOp::In
                    | BinaryOp::NotIn
            )
    )
}

fn is_name(expr: &Expr, name: &str) -> bool {
    matches!(expr, Expr::Name(ident) if ident.text == name)
}

trait AstSpan {
    fn span(&self) -> Span;
}

impl AstSpan for Expr {
    fn span(&self) -> Span {
        match self {
            Expr::Literal(literal) => match literal {
                Literal::Int { span, .. }
                | Literal::Float { span, .. }
                | Literal::Bool { span, .. }
                | Literal::Null { span } => *span,
                Literal::String(string) => string.span,
            },
            Expr::Name(name) => name.span,
            Expr::Array(array) => array.span,
            Expr::Record(record) => record.span,
            Expr::Fn(func) => func.span,
            Expr::Lambda(lambda) => lambda.span,
            Expr::Range(range) => range.span,
            Expr::Unary(unary) => unary.span,
            Expr::Binary(binary) => binary.span,
            Expr::Call(call) => call.span,
            Expr::Field(field) => field.span,
            Expr::OptionalField(field) => field.span,
            Expr::Index(index) => index.span,
            Expr::OptionalIndex(index) => index.span,
            Expr::If(if_expr) => if_expr.span,
        }
    }
}

trait AssignTargetSpan {
    fn span(&self) -> Span;
}

impl AssignTargetSpan for ast::AssignTarget {
    fn span(&self) -> Span {
        match self {
            ast::AssignTarget::Name(name) => name.span,
            ast::AssignTarget::Field { span, .. } | ast::AssignTarget::Index { span, .. } => *span,
        }
    }
}

fn value_ty(value: &Value) -> Ty {
    match value {
        Value::Null => Ty::Null,
        Value::Bool(_) => Ty::Bool,
        Value::Int(_) => Ty::Int,
        Value::Float(_) => Ty::Float,
        Value::String(_) => Ty::String,
        Value::Array(values) => Ty::Array(Box::new(
            infer_common_ty(&values.iter().map(value_ty).collect::<Vec<_>>()).unwrap_or(Ty::Any),
        )),
        Value::Dict(entries) => Ty::Dict(
            Box::new(Ty::String),
            Box::new(
                infer_common_ty(
                    &entries
                        .iter()
                        .map(|(_, value)| value_ty(value))
                        .collect::<Vec<_>>(),
                )
                .unwrap_or(Ty::Any),
            ),
        ),
        Value::Object { class, .. } => class.map_or(Ty::Any, Ty::Class),
        Value::Range { .. } => Ty::Iterator,
        Value::EnumVariant(enum_id, _) => Ty::Enum(*enum_id),
        Value::Closure(_) => Ty::FunctionSig {
            params: Vec::new(),
            return_ty: Box::new(Ty::Any),
        },
    }
}

fn parse_int_literal(raw: &str) -> Result<i64, SemaErrorKind> {
    let stripped = raw.replace('_', "");
    let (digits, radix) = if let Some(rest) = stripped
        .strip_prefix("0x")
        .or_else(|| stripped.strip_prefix("0X"))
    {
        (rest, 16)
    } else if let Some(rest) = stripped
        .strip_prefix("0b")
        .or_else(|| stripped.strip_prefix("0B"))
    {
        (rest, 2)
    } else if let Some(rest) = stripped
        .strip_prefix("0o")
        .or_else(|| stripped.strip_prefix("0O"))
    {
        (rest, 8)
    } else {
        (stripped.as_str(), 10)
    };
    i64::from_str_radix(digits, radix).map_err(|error| {
        if error.kind() == &std::num::IntErrorKind::PosOverflow {
            SemaErrorKind::NumberOverflow
        } else {
            SemaErrorKind::InvalidLiteral
        }
    })
}

fn decode_string_literal(string: &ast::StringLiteral) -> Result<String, SemaErrorKind> {
    let raw = string.raw.as_str();
    let body = match string.kind {
        StringKind::Normal => raw.strip_prefix('"').and_then(|s| s.strip_suffix('"')),
        StringKind::Raw => raw.strip_prefix("r\"").and_then(|s| s.strip_suffix('"')),
        StringKind::Multiline => raw
            .strip_prefix("\"\"\"")
            .and_then(|s| s.strip_suffix("\"\"\"")),
        StringKind::RawMultiline => raw
            .strip_prefix("r\"\"\"")
            .and_then(|s| s.strip_suffix("\"\"\"")),
    }
    .ok_or(SemaErrorKind::InvalidLiteral)?;

    if matches!(string.kind, StringKind::Raw | StringKind::RawMultiline) {
        return Ok(body.to_string());
    }

    let mut out = String::new();
    let mut chars = body.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        let escaped = chars.next().ok_or(SemaErrorKind::InvalidEscape)?;
        match escaped {
            '"' => out.push('"'),
            '\\' => out.push('\\'),
            'n' => out.push('\n'),
            'r' => out.push('\r'),
            't' => out.push('\t'),
            _ => return Err(SemaErrorKind::InvalidEscape),
        }
    }
    Ok(out)
}

fn binary_result_ty(op: BinaryOp, lhs: &Ty, rhs: &Ty) -> Ty {
    match op {
        BinaryOp::Eq
        | BinaryOp::NotEq
        | BinaryOp::Lt
        | BinaryOp::LtEq
        | BinaryOp::Gt
        | BinaryOp::GtEq
        | BinaryOp::And
        | BinaryOp::Or
        | BinaryOp::In
        | BinaryOp::NotIn => Ty::Bool,
        BinaryOp::Add => {
            if matches!(lhs, Ty::String) || matches!(rhs, Ty::String) {
                Ty::String
            } else if matches!(lhs, Ty::Float) || matches!(rhs, Ty::Float) {
                Ty::Float
            } else {
                Ty::Int
            }
        }
        BinaryOp::Div => Ty::Float,
        BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Rem | BinaryOp::Pow => {
            if matches!(lhs, Ty::Float) || matches!(rhs, Ty::Float) {
                Ty::Float
            } else {
                Ty::Int
            }
        }
        BinaryOp::IntDiv
        | BinaryOp::BitAnd
        | BinaryOp::BitOr
        | BinaryOp::BitXor
        | BinaryOp::Shl
        | BinaryOp::Shr => Ty::Int,
        BinaryOp::NullCoalesce => merge_ty(lhs, rhs),
    }
}

fn merge_ty(lhs: &Ty, rhs: &Ty) -> Ty {
    if lhs == rhs {
        lhs.clone()
    } else if matches!(lhs, Ty::Error) {
        rhs.clone()
    } else if matches!(rhs, Ty::Error) {
        lhs.clone()
    } else {
        Ty::Any
    }
}

fn infer_common_ty(types: &[Ty]) -> Option<Ty> {
    let first = types.first()?.clone();
    if types.iter().all(|ty| *ty == first) {
        Some(first)
    } else {
        Some(Ty::Any)
    }
}

fn is_assignable(expected: &Ty, actual: &Ty) -> bool {
    if matches!(expected, Ty::Error) || matches!(actual, Ty::Error) {
        return true;
    }
    if expected == actual {
        return true;
    }
    match (expected, actual) {
        (Ty::Any, _) => true,
        (_, Ty::Any) => true,
        (Ty::Function, Ty::Function | Ty::FunctionSig { .. }) => true,
        (Ty::FunctionSig { .. }, Ty::Function) => true,
        (
            Ty::FunctionSig {
                params: expected_params,
                return_ty: expected_return,
            },
            Ty::FunctionSig {
                params: actual_params,
                return_ty: actual_return,
            },
        ) => expected_params == actual_params && expected_return == actual_return,
        (Ty::Array(expected), Ty::Array(actual)) => is_assignable(expected, actual),
        (Ty::Dict(expected_key, expected_value), Ty::Dict(actual_key, actual_value)) => {
            is_assignable(expected_key, actual_key) && is_assignable(expected_value, actual_value)
        }
        _ => false,
    }
}

fn should_guard(expected: &Ty, actual: &Ty) -> bool {
    matches!(actual, Ty::Any) && !matches!(expected, Ty::Any | Ty::Error)
        || matches!((expected, actual), (Ty::FunctionSig { .. }, Ty::Function))
}
