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

// 编译器禁止空规则，每个命名块恰好有一个索引为 0 的根语句。
pub(super) fn named_rules(schema: &CftSchema) -> impl Iterator<Item = &CheckStatementInfo> {
    schema
        .all_check_statements()
        .filter(|info| info.root_index == 0)
}

pub fn identity(info: &CheckStatementInfo) -> String {
    info.rule_id.clone()
}
pub fn selected(info: &CheckStatementInfo, id: &str) -> bool {
    info.rule_id == id
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
    let block = match &info.owner {
        CheckOwner::Project(name) => schema.resolve_check(name).map(|rule| &rule.block),
        CheckOwner::Type(name) => schema
            .resolve_type(name)
            .and_then(|ty| ty.checks.get(info.block_index)),
    };
    let span = block.map(|block| block.span);
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
    // 同一命名规则的依赖是所有根语句依赖的并集，公开身份不随语句增删改变。
    let statements: Vec<_> = schema
        .check_statements_for_rule(&info.rule_id)
        .filter_map(|id| schema.check_statement(id).map(|statement| statement.info))
        .collect();
    let dependency_set: std::collections::BTreeSet<_> = statements
        .iter()
        .flat_map(|statement| &statement.dependencies)
        .collect();
    let dependencies: Vec<_> = dependency_set
        .into_iter()
        .map(|d| match d {
            CheckDependency::Field(f) => RuleDependency::Field {
                actual_type: f.owner.to_string(),
                field: f.field.to_string(),
                scope: if statements
                    .iter()
                    .any(|statement| schema.check_dependency_is_cross_record(statement.id, d))
                {
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
    fn message_text(
        message: Option<&coflow_language::cft::CftSchemaCheckMessage>,
        source: &str,
        output: &mut Vec<String>,
    ) {
        if let Some(text) =
            message.and_then(|message| source.get(message.span.start..message.span.end))
        {
            output.push(text.to_string());
        }
    }
    fn messages(statement: &CftSchemaCheckStmt, source: &str, output: &mut Vec<String>) {
        match statement {
            CftSchemaCheckStmt::Expr { message, .. } => {
                message_text(message.as_ref(), source, output)
            }
            CftSchemaCheckStmt::Quantifier { body, message, .. }
            | CftSchemaCheckStmt::When { body, message, .. } => {
                message_text(message.as_ref(), source, output);
                for statement in body {
                    messages(statement, source, output);
                }
            }
        }
    }
    let mut rule_messages = Vec::new();
    if let Some((block, source)) = block.zip(source) {
        message_text(block.message.as_ref(), &source.source, &mut rule_messages);
        for statement in &block.stmts {
            messages(statement, &source.source, &mut rule_messages);
        }
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
        dimensions: statements
            .iter()
            .flat_map(|statement| &statement.dimensions)
            .map(ToString::to_string)
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect(),
    }
}

pub fn query(
    command: &str,
    schema: &CftSchema,
    root: &std::path::Path,
    args: &Arguments,
) -> Result<Value> {
    let mut items = Vec::new();
    for info in named_rules(schema) {
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
        let matches: Vec<_> = session.schema().check_statements_for_rule(id).collect();
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
    let exhausted = raw
        .iter()
        .any(|diagnostic| diagnostic.diagnostic.code == crate::CfdErrorCode::CheckBudgetExceeded);
    let complete = !exhausted
        && !output.results.iter().any(|result| {
            matches!(
                result.status,
                coflow_checker::CheckTaskStatus::EvaluationError
                    | coflow_checker::CheckTaskStatus::NotExecuted
            )
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
    let evaluated_values: Vec<_> = if args.flag("explain") {
        raw.iter()
            .enumerate()
            .map(|(index, diagnostic)| (index + offset, diagnostic.value_location.clone()))
            .collect()
    } else {
        Vec::new()
    };
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
    // 先按规则与目标汇总原始状态；默认路径不为每个成功任务构造重复 JSON。
    let mut grouped = Vec::<(
        &coflow_checker::CheckTaskResult,
        coflow_checker::CheckTaskStatus,
        usize,
    )>::new();
    let mut indices = std::collections::BTreeMap::new();
    for result in &output.results {
        let Some(statement) = session.schema().check_statement(result.task.statement) else {
            continue;
        };
        let key = (statement.info.rule_id.as_str(), &result.task.target);
        if let Some(index) = indices.get(&key).copied() {
            let (_, status, count) = &mut grouped[index];
            if state_rank(result.status) > state_rank(*status) {
                *status = result.status;
            }
            *count += result.diagnostics.len();
        } else {
            indices.insert(key, grouped.len());
            grouped.push((result, result.status, result.diagnostics.len()));
        }
    }
    let mut counts = std::collections::BTreeMap::from([("total", grouped.len())]);
    for (_, status, _) in &grouped {
        *counts.entry(state_name(*status)).or_default() += 1;
    }
    let mut report = json!({"structure":"passed", "rules":if exhausted {"budget_exhausted"} else if super::has_errors(&diagnostics) {"failed"} else {"passed"},
        "complete":complete, "scope": if requested.is_empty() {json!("all")} else {json!(requested)}, "rule_counts":counts,
        "diagnostic_count": diagnostics.diagnostics.len(), "requested_tasks":output.statistics.requested_tasks,
        "executed_tasks":output.statistics.executed_tasks,"rejected_tasks":output.statistics.rejected_tasks});
    if args.flag("details") {
        report["tasks"] = json!(output.results.iter().map(|result| {
            let statement = session.schema().check_statement(result.task.statement);
            json!({"rule":statement.as_ref().map(|statement| identity(statement.info)),
                "statement":statement.map(|statement| statement.info.root_index),
                "state":state_name(result.status), "target":target_json(session, &result.task.target),
                "diagnostic_count":result.diagnostics.len()})
        }).collect::<Vec<_>>());
        report["results"] = json!(grouped.into_iter().map(|(result, status, count)| {
            json!({"rule":session.schema().check_statement(result.task.statement).map(|statement| identity(statement.info)),
                "state":state_name(status), "target":target_json(session, &result.task.target), "diagnostic_count":count})
        }).collect::<Vec<_>>());
    }
    let mut validation = Validation {
        report,
        diagnostics,
        logical_locations: locations,
        explanations: Default::default(),
    };
    if args.flag("performance") {
        validation.report["performance"] = json!({"rule_work":output.statistics.work_used,"project":session.queries().execution_stats()});
    }
    if args.flag("explain") {
        explain_diagnostics(session, &mut validation);
        // 主报错位置与引用上下文不等价于求值来源；实际值只按执行器保存的游标读取。
        for (index, location) in evaluated_values {
            let Some(explanation) = validation
                .explanations
                .get_mut(&index)
                .and_then(Value::as_object_mut)
            else {
                continue;
            };
            explanation.remove("actual_value");
            let Some(location) = location else { continue };
            let Some(record) = session.model().record(location.record) else {
                continue;
            };
            let Ok(expected) = crate::write_rules::expected_type_for_cfd_path(
                session.schema(),
                record.actual_type(),
                &location.full_path().segments,
                "CLI-FIELD",
                "CLI",
            ) else {
                continue;
            };
            explanation.insert("expected_type".into(), json!(expected.to_string()));
            if let Some(value) = location.value(session.model()) {
                if let Ok(encoded) =
                    super::model_value::encode(session.schema(), &expected, Some(value))
                {
                    explanation.insert("actual_value".into(), encoded);
                }
            }
        }
    }
    Ok(validation)
}

fn state_name(status: coflow_checker::CheckTaskStatus) -> &'static str {
    use coflow_checker::CheckTaskStatus as S;
    match status {
        S::Passed => "passed",
        S::NotTriggered => "not_triggered",
        S::Failed => "failed",
        S::EvaluationError => "evaluation_error",
        S::BudgetExceeded => "budget_exhausted",
        S::NotExecuted => "not_executed",
    }
}

fn state_rank(status: coflow_checker::CheckTaskStatus) -> u8 {
    use coflow_checker::CheckTaskStatus as S;
    match status {
        S::BudgetExceeded => 6,
        S::EvaluationError => 5,
        S::NotExecuted => 4,
        S::Failed => 3,
        S::Passed => 2,
        S::NotTriggered => 1,
    }
}

fn target_json(session: &ReadOnlyProjectSession, target: &crate::CheckTarget) -> Value {
    let record = |handle| {
        session
            .model()
            .record(handle)
            .map(|record| json!({"type":record.actual_type(),"id":record.id()}))
    };
    match target {
        crate::CheckTarget::Project => json!({"kind":"project"}),
        crate::CheckTarget::Record(handle) => json!({"kind":"record","record":record(*handle)}),
        crate::CheckTarget::Data {
            record: handle,
            path,
            dimension,
        } => {
            let host = session.model().record(*handle);
            let mut full_path = dimension
                .as_ref()
                .map_or_else(crate::CfdPath::root, |(field, _)| {
                    crate::CfdPath::root().field(field)
                });
            full_path.segments.extend(path.segments.iter().cloned());
            json!({"kind":"data","record":record(*handle),"field_path":crate::format_field_path(&full_path),
                "dimension":dimension.as_ref().map(|(field,variant)| json!({"field":field,"variant":variant,
                    "dimension":host.and_then(|record| session.schema().field(record.actual_type(),field)).and_then(|field| field.dimension.as_ref()).map(|binding| binding.dimension.as_str())}))})
        }
    }
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
                if let Some(key) = &location.record_id {
                    if let Some(id) = session.model().record_by_type_id(ty, key) {
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
