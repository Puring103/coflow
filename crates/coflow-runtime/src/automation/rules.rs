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
        "requires_snapshot": owner == "type", "location": location, "expression": text, "dependencies": dependencies, "dimensions": info.dimensions})
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

pub fn check(session: &ReadOnlyProjectSession, args: &Arguments) -> Result<(Value, crate::DiagnosticSet)> {
    let structural = session.queries().diagnostics().as_set().clone();
    if !args.flag("rules") && args.get("rule").is_none() {
        return Ok((json!({"structure": if structural.is_empty() { "passed" } else { "failed" }, "rules": "not_executed", "complete": true}), structural));
    }
    if !structural.is_empty() { return Ok((json!({"structure": "failed", "rules": "skipped_dependency_error", "complete": false}), structural)); }
    let limits = crate::CheckLimits::default();
    let mut tasks = crate::checks::planning::plan_full_checks_with_limit(session.schema(), session.model(), limits.max_tasks)
        .map_err(|_| Failure::new(Status::Partial, "CHECK-BUDGET", "rule planning budget exhausted"))?;
    if let Some(id) = args.get("rule") {
        if !session.schema().all_check_statements().any(|s| selected(s,id)) { return Err(Failure::missing(format!("unknown rule `{id}`"))); }
        tasks.retain(|t| session.schema().check_statement(t.statement).is_some_and(|s| selected(s.info, id)));
    }
    let output = crate::execute_checks(session.schema(), session.model(), tasks, limits);
    let origins: Vec<_> = session.model().records().map(|(_,r)| r.origin.clone()).collect();
    let diagnostics = crate::checks::render::map_check_diagnostics_with_origins(Some(session.schema()), output.diagnostics().cloned().collect(), &origins);
    let exhausted = output.statistics.rejected_tasks > 0 || diagnostics.iter().any(|d| d.message.contains("budget") || d.message.contains("limit"));
    let states: Vec<_> = output.results.iter().map(|r| json!({"rule": session.schema().check_statement(r.task.statement).map(|s| identity(s.info)),
        "state": if r.diagnostics.is_empty() { "passed" } else { "failed" }, "target": format!("{:?}", r.task.target)})).collect();
    Ok((json!({"structure": "passed", "rules": if exhausted { "budget_exhausted" } else if diagnostics.is_empty() { "passed" } else { "failed" },
        "complete": !exhausted, "tasks": states, "requested_tasks": output.statistics.requested_tasks,
        "executed_tasks": output.statistics.executed_tasks, "rejected_tasks": output.statistics.rejected_tasks}), diagnostics))
}
