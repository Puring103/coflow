use super::{Arguments, Failure, Result, Status};
use crate::ReadOnlyProjectSession;
use coflow_language::cft::{
    CftSchema, CftSchemaCheckStmt, CheckDependency, CheckOwner, CheckStatementInfo,
};
use serde_json::{json, Value};

#[derive(Debug, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RuleOwnerKind {
    Project,
    Type,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DependencyScope {
    Exact,
    Conservative,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RuleDependency {
    Field {
        #[serde(rename = "type")]
        actual_type: String,
        field: String,
        scope: DependencyScope,
    },
    RecordSet {
        #[serde(rename = "type")]
        actual_type: String,
        scope: DependencyScope,
    },
}

#[derive(Debug, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct RuleDescription {
    pub id: String,
    pub owner_kind: RuleOwnerKind,
    pub owner: String,
    pub location: Option<super::diagnostics::CommandLocation>,
    pub expression: Option<String>,
    pub messages: Vec<String>,
    pub dependencies: Vec<RuleDependency>,
    pub dimensions: Vec<String>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct RuleQuery {
    pub rules: Vec<RuleDescription>,
}

pub fn identity(info: &CheckStatementInfo) -> String {
    match &info.owner {
        CheckOwner::Project(name) => format!("{name}#{}", info.root_index),
        CheckOwner::Type(name) => format!("{name}#{}", info.root_index),
    }
}
pub fn selected(info: &CheckStatementInfo, id: &str) -> bool {
    identity(info) == id || matches!(&info.owner, CheckOwner::Project(name) if name.as_str() == id)
}

pub fn describe(
    schema: &CftSchema,
    info: &CheckStatementInfo,
    root: &std::path::Path,
) -> RuleDescription {
    let (owner, module) = match &info.owner {
        CheckOwner::Project(name) => (
            RuleOwnerKind::Project,
            schema.resolve_check(name).map(|r| &r.module),
        ),
        CheckOwner::Type(name) => (
            RuleOwnerKind::Type,
            schema.resolve_type(name).map(|r| &r.module),
        ),
    };
    let span = schema.check_statement(info.id).map(|r| match r.statement {
        CftSchemaCheckStmt::Expr { span, .. }
        | CftSchemaCheckStmt::Quantifier { span, .. }
        | CftSchemaCheckStmt::When { span, .. } => *span,
    });
    let source = module.and_then(|m| schema.source(m));
    let text = source
        .zip(span)
        .and_then(|(s, span)| s.source.get(span.start..span.end));
    // 规则位置与诊断使用同一公共契约，路径相对项目根，范围使用 UTF-16 行列。
    let location = source.zip(span).map(|(s, span)| {
        let range = crate::TextRange::from_byte_offsets(&s.source, span.start, span.end);
        super::diagnostics::CommandLocation::from_source(
            &crate::SourceLocation::FileSpan {
                path: s.path.clone(),
                start_line: range.start.line,
                start_character: range.start.character,
                end_line: range.end.line,
                end_character: range.end.character,
            },
            Some(root),
        )
    });
    let dependencies: Vec<_> = info
        .dependencies
        .iter()
        .map(|d| match d {
            CheckDependency::Field(f) => RuleDependency::Field {
                actual_type: f.owner.to_string(),
                field: f.field.to_string(),
                scope: if schema.check_dependency_is_cross_record(info.id, d) {
                    DependencyScope::Conservative
                } else {
                    DependencyScope::Exact
                },
            },
            CheckDependency::RecordSet(ty) => RuleDependency::RecordSet {
                actual_type: ty.to_string(),
                scope: DependencyScope::Conservative,
            },
        })
        .collect();
    // 量词和条件块内的消息同样属于该规则，保留源码表达式以表达插值消息。
    fn messages(statement: &CftSchemaCheckStmt, source: &str, output: &mut Vec<String>) {
        match statement {
            CftSchemaCheckStmt::Expr {
                message: Some(message),
                ..
            } => {
                if let Some(text) = source.get(message.span.start..message.span.end) {
                    output.push(text.to_string());
                }
            }
            CftSchemaCheckStmt::Quantifier { body, .. } | CftSchemaCheckStmt::When { body, .. } => {
                for statement in body {
                    messages(statement, source, output);
                }
            }
            _ => {}
        }
    }
    let mut rule_messages = Vec::new();
    if let Some((statement, source)) = schema.check_statement(info.id).zip(source) {
        messages(statement.statement, &source.source, &mut rule_messages);
    }
    RuleDescription {
        id: identity(info),
        owner_kind: owner,
        owner: match &info.owner {
            CheckOwner::Type(n) => n.to_string(),
            CheckOwner::Project(n) => n.to_string(),
        },
        location,
        expression: text.map(str::to_string),
        messages: rule_messages,
        dependencies,
        dimensions: info.dimensions.iter().map(ToString::to_string).collect(),
    }
}

pub fn query(
    command: &str,
    schema: &CftSchema,
    root: &std::path::Path,
    args: &Arguments,
) -> Result<Value> {
    let mut items = Vec::new();
    for info in schema.all_check_statements() {
        if args.get("type").is_some_and(
            |ty| matches!(&info.owner, CheckOwner::Type(owner) if !schema.is_assignable(ty, owner)),
        ) {
            continue;
        }
        if command == "rule.inspect" && !selected(info, args.require("rule")?) {
            continue;
        }
        items.push(describe(schema, info, root));
    }
    if command == "rule.inspect" && items.is_empty() {
        return Err(Failure::missing("rule does not exist"));
    }
    super::json(RuleQuery { rules: items })
}

/// 校验报告与诊断一起返回，执行完整性决定是否允许写入。
#[derive(Debug)]
pub(crate) struct Validation {
    pub report: Value,
    pub diagnostics: crate::DiagnosticSet,
    pub logical_locations: std::collections::BTreeMap<usize, crate::DiagnosticLogicalLocation>,
    pub explanations: std::collections::BTreeMap<usize, Value>,
}

impl Validation {
    pub fn status(&self) -> Status {
        if self.report["rules"] == "budget_exhausted" {
            Status::Partial
        } else if super::has_errors(&self.diagnostics) {
            Status::Invalid
        } else if self.report["complete"] == false {
            Status::Partial
        } else {
            Status::Success
        }
    }
    pub fn passed(&self) -> bool {
        self.status() == Status::Success
    }
}

pub(crate) fn check(session: &ReadOnlyProjectSession, args: &Arguments) -> Result<Validation> {
    check_with_limits(session, args, crate::CheckLimits::default())
}

pub(crate) fn check_schema(session: &crate::ProjectSchemaSession, args: &Arguments) -> Validation {
    let diagnostics = session.diagnostics().as_set().clone();
    let passed = session.schema().is_some() && !super::has_errors(&diagnostics);
    let explanations = if args.flag("explain") {
        diagnostics
            .diagnostics
            .iter()
            .enumerate()
            .filter_map(|(index, diagnostic)| {
                super::explanations::for_diagnostic(diagnostic)
                    .map(|explanation| (index, explanation))
            })
            .collect()
    } else {
        Default::default()
    };
    Validation {
        report: json!({"schema": if passed { "passed" } else { "failed" }, "structure":"not_executed", "rules":"not_executed",
            "complete":passed,"diagnostic_count":diagnostics.diagnostics.len()}),
        diagnostics,
        logical_locations: Default::default(),
        explanations,
    }
}

pub(crate) fn check_with_limits(
    session: &ReadOnlyProjectSession,
    args: &Arguments,
    limits: crate::CheckLimits,
) -> Result<Validation> {
    let store = session.queries().diagnostics();
    let structural = store.as_set().clone();
    let mut locations = (0..structural.diagnostics.len())
        .filter_map(|index| {
            store
                .logical_location(index)
                .cloned()
                .map(|location| (index, location))
        })
        .collect::<std::collections::BTreeMap<_, _>>();
    let requested = args.all("rule");
    if args.flag("rules") && !requested.is_empty() {
        return Err(Failure::request(
            "--rules and --rule are mutually exclusive",
        ));
    }
    let mut selected = std::collections::BTreeSet::new();
    for id in &requested {
        let matches: Vec<_> = session
            .schema()
            .all_check_statements()
            .filter(|info| self::selected(info, id))
            .map(|info| info.id)
            .collect();
        if matches.is_empty() {
            return Err(Failure::missing(format!("unknown rule `{id}`")));
        }
        selected.extend(matches);
    }
    let required = args.flag("rules") || !requested.is_empty();
    if super::has_errors(&structural) || !required {
        let mut validation = Validation {
            report: json!({"structure": if super::has_errors(&structural) { "failed" } else { "passed" },
                "rules": if required { "skipped_dependency_error" } else { "not_executed" },
                "complete": !super::has_errors(&structural), "scope": requested,
                "diagnostic_count": structural.diagnostics.len()}),
            diagnostics: structural,
            logical_locations: locations,
            explanations: Default::default(),
        };
        if args.flag("explain") {
            explain_diagnostics(session, &mut validation);
        }
        return Ok(validation);
    }
    let tasks = match crate::checks::planning::plan_selected_checks_with_limit(
        session.schema(),
        session.model(),
        limits.max_tasks,
        (!requested.is_empty()).then_some(&selected),
    ) {
        Ok(tasks) => tasks,
        Err(_) => {
            let mut diagnostics = structural;
            diagnostics.push(crate::Diagnostic::error(
                "CHECK-BUDGET",
                "CHECK",
                "rule planning budget exhausted",
            ));
            return Ok(Validation {
                report: json!({"structure": "passed", "rules": "budget_exhausted", "complete": false,
                "scope": requested, "diagnostic_count": diagnostics.diagnostics.len(), "executed_tasks": 0}),
                diagnostics,
                logical_locations: locations,
                explanations: Default::default(),
            });
        }
    };
    let output = crate::execute_checks(session.schema(), session.model(), tasks, limits);
    let raw: Vec<_> = output.diagnostics().cloned().collect();
    let exhausted = output.statistics.rejected_tasks > 0
        || raw.iter().any(|diagnostic| {
            diagnostic.diagnostic.code == crate::CfdErrorCode::CheckBudgetExceeded
        });
    let origins: Vec<_> = session
        .model()
        .records()
        .map(|(_, record)| record.origin.clone())
        .collect();
    let logical = crate::load::logical_locations_from_cfd(
        &crate::CfdDiagnostics::new(
            raw.iter()
                .map(|diagnostic| diagnostic.diagnostic.clone())
                .collect(),
        ),
        |id| session.model().record(id).map(crate::CfdRecord::coordinate),
    );
    let offset = structural.diagnostics.len();
    locations.extend(
        logical
            .into_iter()
            .map(|(index, location)| (index + offset, location)),
    );
    let mut diagnostics = structural;
    diagnostics.extend(crate::checks::render::map_check_diagnostics_with_origins(
        Some(session.schema()),
        raw,
        &origins,
    ));
    let states: Vec<_> = output.results.iter().map(|result| json!({
        "rule": session.schema().check_statement(result.task.statement).map(|statement| identity(statement.info)),
        "state": if result.diagnostics.iter().any(|diagnostic| diagnostic.diagnostic.code == crate::CfdErrorCode::CheckBudgetExceeded) { "budget_exhausted" }
            else if result.diagnostics.is_empty() { "passed" } else { "failed" },
        "target": match result.task.target { crate::CheckTarget::Project => json!({"kind":"project"}),
            crate::CheckTarget::Record(id) => json!({"kind":"record", "record": session.model().record(id).map(|record| json!({"type":record.actual_type(),"key":record.key()}))}) }
    })).collect();
    let mut validation = Validation {
        report: json!({"structure":"passed", "rules":if exhausted {"budget_exhausted"} else if super::has_errors(&diagnostics) {"failed"} else {"passed"},
            "complete":!exhausted, "scope": if requested.is_empty() {json!("all")} else {json!(requested)}, "tasks":states,
            "diagnostic_count": diagnostics.diagnostics.len(), "requested_tasks":output.statistics.requested_tasks,
            "executed_tasks":output.statistics.executed_tasks,"rejected_tasks":output.statistics.rejected_tasks}),
        diagnostics,
        logical_locations: locations,
        explanations: Default::default(),
    };
    if args.flag("performance") {
        validation.report["performance"] = json!({"rule_work":output.statistics.work_used,"project":session.queries().execution_stats()});
    }
    if args.flag("explain") {
        explain_diagnostics(session, &mut validation);
    }
    Ok(validation)
}

pub(super) fn explain_diagnostics(session: &ReadOnlyProjectSession, validation: &mut Validation) {
    for (index, diagnostic) in validation.diagnostics.iter().enumerate() {
        let Some(mut explanation) = super::explanations::for_diagnostic(diagnostic) else {
            continue;
        };
        if let Some(location) = validation.logical_locations.get(&index) {
            if let (Some(ty), Some(path)) = (&location.actual_type, &location.field_path) {
                if let Ok(segments) = super::query::parse_path(path) {
                    if let Ok(expected) = crate::write_rules::expected_type_for_cfd_path(
                        session.schema(),
                        ty,
                        &segments,
                        "CLI-FIELD",
                        "CLI",
                    ) {
                        explanation["expected_type"] = json!(expected.to_string());
                    }
                }
                if let Some(key) = &location.record_key {
                    if let Some(id) = session.model().record_by_type_key(ty, key) {
                        if let Some(value) = session.model().record(id).and_then(|record| {
                            super::query::value_at(session, record, path).ok().flatten()
                        }) {
                            if let Ok(segments) = super::query::resolved_path(session, ty, path) {
                                if let Ok(expected) = crate::write_rules::expected_type_for_cfd_path(
                                    session.schema(),
                                    ty,
                                    &segments,
                                    "CLI-FIELD",
                                    "CLI",
                                ) {
                                    if let Ok(encoded) = super::model_value::encode(
                                        session.schema(),
                                        &expected,
                                        Some(value),
                                    ) {
                                        explanation["actual_value"] = encoded;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        validation.explanations.insert(index, explanation);
    }
}
