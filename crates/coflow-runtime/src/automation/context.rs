use serde_json::{Value, json};
use crate::{Project, ReadOnlyProjectSession};
use super::{Arguments, InputSnapshot, Result, Failure};

pub fn query(project: &Project, session: &ReadOnlyProjectSession, snapshot: &InputSnapshot, args: &Arguments) -> Result<Value> {
    let ty = args.require("type")?;
    if session.schema().resolve_type(ty).is_none() { return Err(Failure::missing(format!("unknown type `{ty}`"))); }
    let schema = crate::Runtime::open_schema_session(project.clone())?;
    let mut selector = args.selector()?;
    selector.limit = args.number("examples", 3)?.max(1);
    let records = super::query::query_records(project,session,&selector,&snapshot.id)?;
    let mut result = json!({"schema": crate::inspect_schema(&schema,Some(ty),false), "rules": super::rules::query("rule.list",session,args)?,
        "examples": if args.number("examples",3)? == 0 { Vec::new() } else { records.items }, "dimensions": session.queries().dimensions(),
        "omitted": [], "queries": [format!("data list --type {ty}"), format!("schema inspect --type {ty} --related")]});
    let budget = args.number("budget", 16000)?;
    for name in ["examples", "rules", "dimensions"] {
        if result.to_string().chars().count() <= budget { break; }
        result.as_object_mut().map(|m| m.remove(name));
        if let Some(omitted) = result["omitted"].as_array_mut() { omitted.push(json!({"section": name, "query": match name {
            "examples" => format!("data list --type {ty}"), "rules" => format!("rule list --type {ty}"), _ => "dimension list".into() }})); }
    }
    if result.to_string().chars().count() > budget { return Err(Failure::invalid("context budget is too small for the core type declaration")); }
    Ok(result)
}
