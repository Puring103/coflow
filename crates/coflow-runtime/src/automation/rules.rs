use serde_json::{Value, json};
use coflow_language::cft::{CftSchema, CheckOwner, CheckStatementInfo, CheckDependency, CftSchemaCheckStmt};
use crate::ReadOnlyProjectSession;
use super::{Arguments, Result, Failure, Status};

pub fn identity(info: &CheckStatementInfo) -> String {
    match &info.owner {
        CheckOwner::Project(name) => format!("{name}#{}", info.root_index),
        CheckOwner::Type(name) => format!("{name}#{}", info.root_index),
    }
}
pub fn selected(info: &CheckStatementInfo, id: &str) -> bool {
    identity(info) == id || matches!(&info.owner, CheckOwner::Project(name) if name.as_str() == id)
}

pub fn describe(schema: &CftSchema, info: &CheckStatementInfo) -> Value {
    let (owner, module) = match &info.owner {
        CheckOwner::Project(name) => ("project", schema.resolve_check(name).map(|r| &r.module)),
        CheckOwner::Type(name) => ("type", schema.resolve_type(name).map(|r| &r.module)),
    };
    let span = schema.check_statement(info.id).map(|r| match r.statement {
        CftSchemaCheckStmt::Expr { span, .. } | CftSchemaCheckStmt::Quantifier { span, .. } | CftSchemaCheckStmt::When { span, .. } => *span
    });
    let source = module.and_then(|m| schema.source(m));
    let text = source.zip(span).and_then(|(s, span)| s.source.get(span.start..span.end));
    let location = source.zip(span).map(|(s, span)| super::schema_queries::location(&s.path, &s.source, span));
    let dependencies: Vec<_> = info.dependencies.iter().map(|d| match d {
        CheckDependency::Field(f) => json!({"kind": "field", "type": f.owner, "field": f.field,
            "scope": if schema.check_dependency_is_cross_record(info.id, d) { "conservative" } else { "exact" }}),
        CheckDependency::RecordSet(ty) => json!({"kind": "record_set", "type": ty, "scope": "conservative"}),
    }).collect();
    json!({"id": identity(info), "owner_kind": owner, "owner": match &info.owner { CheckOwner::Type(n) => n.as_str(), CheckOwner::Project(n) => n.as_str() },
        "location": location, "expression": text, "dependencies": dependencies, "dimensions": info.dimensions})
}

pub fn query(command: &str, session: &ReadOnlyProjectSession, args: &Arguments) -> Result<Value> {
    let mut items = Vec::new();
    for info in session.schema().all_check_statements() {
        if args.get("type").is_some_and(|ty| matches!(&info.owner, CheckOwner::Type(owner) if !session.schema().is_assignable(ty, owner))) { continue; }
        if command == "rule.inspect" && !selected(info, args.require("rule")?) { continue; }
        items.push(describe(session.schema(), info));
    }
    if command == "rule.inspect" && items.is_empty() { return Err(Failure::missing("rule does not exist")); }
    Ok(json!({"rules": items}))
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
        if self.report["rules"] == "budget_exhausted" { Status::Partial }
        else if super::has_errors(&self.diagnostics) { Status::Invalid }
        else if self.report["complete"] == false { Status::Partial }
        else { Status::Success }
    }
    pub fn passed(&self) -> bool { self.status() == Status::Success }
}

pub(crate) fn check(session: &ReadOnlyProjectSession, args: &Arguments) -> Result<Validation> {
    check_with_limits(session, args, crate::CheckLimits::default())
}

pub(crate) fn check_with_limits(session: &ReadOnlyProjectSession, args: &Arguments, limits: crate::CheckLimits) -> Result<Validation> {
    let store = session.queries().diagnostics();
    let structural = store.as_set().clone();
    let mut locations = (0..structural.diagnostics.len()).filter_map(|index|
        store.logical_location(index).cloned().map(|location| (index, location))).collect::<std::collections::BTreeMap<_, _>>();
    let requested = args.all("rule");
    if args.flag("rules") && !requested.is_empty() { return Err(Failure::request("--rules and --rule are mutually exclusive")); }
    let mut selected = std::collections::BTreeSet::new();
    for id in &requested {
        let matches: Vec<_> = session.schema().all_check_statements().filter(|info| self::selected(info, id)).map(|info| info.id).collect();
        if matches.is_empty() { return Err(Failure::missing(format!("unknown rule `{id}`"))); }
        selected.extend(matches);
    }
    let required = args.flag("rules") || !requested.is_empty();
    if super::has_errors(&structural) || !required {
        let mut validation = Validation {
            report: json!({"structure": if super::has_errors(&structural) { "failed" } else { "passed" },
                "rules": if required { "skipped_dependency_error" } else { "not_executed" },
                "complete": !super::has_errors(&structural), "scope": requested,
                "diagnostic_count": structural.diagnostics.len()}),
            diagnostics: structural, logical_locations: locations, explanations: Default::default(),
        };
        if args.flag("explain") { explain_diagnostics(session, &mut validation); }
        return Ok(validation);
    }
    let tasks = match crate::checks::planning::plan_selected_checks_with_limit(session.schema(), session.model(), limits.max_tasks,
        (!requested.is_empty()).then_some(&selected)) {
        Ok(tasks) => tasks,
        Err(_) => {
            let mut diagnostics = structural;
            diagnostics.push(crate::Diagnostic::error("CHECK-BUDGET", "CHECK", "rule planning budget exhausted"));
            return Ok(Validation { report: json!({"structure": "passed", "rules": "budget_exhausted", "complete": false,
                "scope": requested, "diagnostic_count": diagnostics.diagnostics.len(), "executed_tasks": 0}),
                diagnostics, logical_locations: locations, explanations: Default::default() });
        }
    };
    let output = crate::execute_checks(session.schema(), session.model(), tasks, limits);
    let raw: Vec<_> = output.diagnostics().cloned().collect();
    let exhausted = output.statistics.rejected_tasks > 0 || raw.iter().any(|diagnostic|
        diagnostic.diagnostic.code == crate::CfdErrorCode::CheckBudgetExceeded);
    let origins: Vec<_> = session.model().records().map(|(_, record)| record.origin.clone()).collect();
    let logical = crate::load::logical_locations_from_cfd(&crate::CfdDiagnostics::new(raw.iter().map(|diagnostic|diagnostic.diagnostic.clone()).collect()),
        |id| session.model().record(id).map(crate::CfdRecord::coordinate));
    let offset = structural.diagnostics.len();
    locations.extend(logical.into_iter().map(|(index, location)| (index + offset, location)));
    let mut diagnostics = structural;
    diagnostics.extend(crate::checks::render::map_check_diagnostics_with_origins(Some(session.schema()), raw, &origins));
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
        diagnostics, logical_locations:locations, explanations:Default::default(),
    };
    if args.flag("performance") { validation.report["performance"] = json!({"rule_work":output.statistics.work_used,"project":session.queries().execution_stats()}); }
    if args.flag("explain") { explain_diagnostics(session, &mut validation); }
    Ok(validation)
}

fn explain_diagnostics(session: &ReadOnlyProjectSession, validation: &mut Validation) {
    for (index, diagnostic) in validation.diagnostics.iter().enumerate() {
        let mut explanation = super::explain(&diagnostic.code).ok().unwrap_or_else(|| json!({"code":diagnostic.code}));
        if let Some(location) = validation.logical_locations.get(&index) {
            if let (Some(ty), Some(path)) = (&location.actual_type, &location.field_path) {
                if let Ok(segments) = super::query::parse_path(path) {
                    if let Ok(expected) = crate::write_rules::expected_type_for_cfd_path(session.schema(), ty, &segments, "CLI-FIELD", "CLI") {
                        explanation["expected_type"] = json!(expected.to_string());
                    }
                }
                if let Some(key) = &location.record_key {
                    if let Some(id) = session.model().record_by_type_key(ty, key) {
                        if let Some(value) = session.model().record(id).and_then(|record| super::query::value_at(session, record, path).ok().flatten()) {
                            explanation["actual_value"] = serde_json::to_value(value).ok().unwrap_or(Value::Null);
                        }
                    }
                }
            }
        }
        validation.explanations.insert(index, explanation);
    }
}
