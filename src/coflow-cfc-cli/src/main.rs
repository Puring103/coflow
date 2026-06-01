use coflow_cfd::{
    BuildErrors, CfdContainer, CfdModuleResult, CfdValue, CfdValueRef, CheckError, ModuleId,
    ParseErrors,
};
use coflow_cft::{CftContainer, CftSchemaEnum, CftSchemaType, ParseErrors as CftParseErrors};
use coflow_cfd::CfdResult;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

fn main() {
    match run(env::args().collect()) {
        Ok(()) => {}
        Err(err) => {
            eprintln!("{err}");
            std::process::exit(1);
        }
    }
}

fn run(args: Vec<String>) -> Result<(), String> {
    let Some(command) = args.get(1).map(String::as_str) else {
        return Err(usage());
    };
    match command {
        "check" => {
            let (json, dir) = parse_check_args(&args)?;
            if json {
                return check_json(&dir);
            }
            let loaded = load_dir(&dir)?;
            report_checks(&loaded.cfd, &loaded.result)?;
            println!("ok");
            Ok(())
        }
        "get" => {
            let dir = required_arg(&args, 2, "missing dir")?;
            let module = required_arg(&args, 3, "missing module")?;
            let path = required_arg(&args, 4, "missing path")?;
            let loaded = load_dir(&dir)?;
            report_checks(&loaded.cfd, &loaded.result)?;
            let module_id = ModuleId::new(module);
            let module_result = loaded
                .result
                .module(&module_id)
                .ok_or_else(|| format!("unknown module `{module_id}`"))?;
            let value = select_root_path(module_result, &path)?;
            println!("{}", GraphRenderer::new().render(&value));
            Ok(())
        }
        "type" => {
            let dir = required_arg(&args, 2, "missing dir")?;
            let name = required_arg(&args, 3, "missing type name")?;
            let loaded = load_dir(&dir)?;
            print_definition(loaded.cfd.type_ctx(), &name)
        }
        "-h" | "--help" | "help" => {
            println!("{}", usage());
            Ok(())
        }
        other => Err(format!("unknown command `{other}`\n\n{}", usage())),
    }
}

fn required_arg(args: &[String], index: usize, message: &str) -> Result<String, String> {
    args.get(index).cloned().ok_or_else(|| message.to_string())
}

fn usage() -> String {
    "usage:
  cfc check [--json] <dir>
  cfc get <dir> <module> <path>
  cfc type <dir> <type-name>"
        .to_string()
}

fn parse_check_args(args: &[String]) -> Result<(bool, String), String> {
    match (args.get(2).map(String::as_str), args.get(3)) {
        (Some("--json"), Some(dir)) => Ok((true, dir.clone())),
        (Some("--json"), None) => Err("missing dir".to_string()),
        (Some(dir), None) => Ok((false, dir.to_string())),
        (Some(dir), Some(_)) => Err(format!("unexpected extra argument after `{dir}`")),
        (None, _) => Err("missing dir".to_string()),
    }
}

struct LoadedGraph {
    cfd: CfdContainer,
    result: CfdResult,
}

fn load_dir(dir: &str) -> Result<LoadedGraph, String> {
    let root = fs::canonicalize(dir)
        .map_err(|err| format!("failed to resolve `{dir}`: {err}"))?;

    let mut cft = CftContainer::new();
    // Register all .cft files first
    for entry in collect_files(&root, "cft")? {
        let module_id = file_to_module_id(&root, &entry);
        let source = fs::read_to_string(&entry)
            .map_err(|err| format!("failed to read `{}`: {err}", entry.display()))?;
        cft.add_module(ModuleId::new(module_id), source)
            .map_err(|err| format_cft_parse_errors(&entry, &err))?;
    }

    let mut cfd = CfdContainer::new(cft);
    // Register all .cfd files
    for entry in collect_files(&root, "cfd")? {
        let module_id = file_to_module_id(&root, &entry);
        let source = fs::read_to_string(&entry)
            .map_err(|err| format!("failed to read `{}`: {err}", entry.display()))?;
        cfd.add_module(ModuleId::new(module_id), source)
            .map_err(|err| format_cfd_parse_errors(&entry, &err))?;
    }

    let result = cfd.build_all().map_err(|err| format_build_errors(&err))?;
    Ok(LoadedGraph { cfd, result })
}

fn collect_files(root: &Path, ext: &str) -> Result<Vec<PathBuf>, String> {
    let mut files = Vec::new();
    collect_files_recursive(root, ext, &mut files)
        .map_err(|err| format!("failed to walk `{}`: {err}", root.display()))?;
    files.sort();
    Ok(files)
}

fn collect_files_recursive(dir: &Path, ext: &str, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_files_recursive(&path, ext, out)?;
        } else if path.extension().and_then(|e| e.to_str()) == Some(ext) {
            out.push(path);
        }
    }
    Ok(())
}

fn file_to_module_id(root: &Path, file: &Path) -> String {
    let rel = file.strip_prefix(root).unwrap_or(file);
    let without_ext = rel.with_extension("");
    without_ext.to_string_lossy().replace('\\', "/")
}

fn check_json(dir: &str) -> Result<(), String> {
    let loaded = match load_dir(dir) {
        Ok(loaded) => loaded,
        Err(err) => {
            println!("[{}]", single_diagnostic_json(dir, "build", "Build", &err));
            return Err("check failed".to_string());
        }
    };
    let errors = loaded.cfd.check(&loaded.result);
    println!("{}", check_errors_json(dir, &errors));
    if errors.is_empty() {
        Ok(())
    } else {
        Err("check failed".to_string())
    }
}

fn format_cft_parse_errors(file: &Path, errors: &CftParseErrors) -> String {
    errors
        .errors
        .iter()
        .map(|e| format!("{}: {:?}: {}", file.display(), e.kind, e.message))
        .collect::<Vec<_>>()
        .join("\n")
}

fn format_cfd_parse_errors(file: &Path, errors: &ParseErrors) -> String {
    errors
        .errors
        .iter()
        .map(|e| format!("{}: {:?}: {}", file.display(), e.kind, e.message))
        .collect::<Vec<_>>()
        .join("\n")
}

fn format_build_errors(errors: &BuildErrors) -> String {
    errors
        .errors
        .iter()
        .map(|e| format!("{:?}: {}", e.kind, e.message))
        .collect::<Vec<_>>()
        .join("\n")
}

fn format_check_errors(errors: &[CheckError]) -> String {
    errors
        .iter()
        .map(|e| format!("{:?}: {}", e.kind, e.message))
        .collect::<Vec<_>>()
        .join("\n")
}

fn report_checks(cfd: &CfdContainer, result: &CfdResult) -> Result<(), String> {
    let errors = cfd.check(result);
    if errors.is_empty() {
        Ok(())
    } else {
        Err(format_check_errors(&errors))
    }
}

fn check_errors_json(file: &str, errors: &[CheckError]) -> String {
    let items = errors
        .iter()
        .map(|error| {
            diagnostic_json(
                error.module.as_deref().unwrap_or(file),
                "check",
                "Check",
                &error.message,
                error.span,
            )
        })
        .collect::<Vec<_>>();
    format!("[{}]", items.join(","))
}

fn single_diagnostic_json(file: &str, stage: &str, kind: &str, message: &str) -> String {
    diagnostic_json(file, stage, kind, message, None)
}

fn diagnostic_json(
    file: &str,
    stage: &str,
    kind: &str,
    message: &str,
    span: Option<coflow_cfd::Span>,
) -> String {
    let span = span.map_or_else(|| "null".to_string(), span_json);
    format!(
        "{{\"file\":{},\"stage\":{},\"kind\":{},\"message\":{},\"span\":{}}}",
        json_string(file),
        json_string(stage),
        json_string(kind),
        json_string(message),
        span
    )
}

fn span_json(span: coflow_cfd::Span) -> String {
    format!("{{\"start\":{},\"end\":{}}}", span.start, span.end)
}

fn json_string(value: &str) -> String {
    let mut out = String::from("\"");
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if ch.is_control() => out.push_str(&format!("\\u{:04x}", u32::from(ch))),
            ch => out.push(ch),
        }
    }
    out.push('"');
    out
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Segment {
    Field(String),
    Index(usize),
}

fn select_root_path(module: &CfdModuleResult, path: &str) -> Result<CfdValueRef, String> {
    let (root, segments) = parse_value_path(path)?;
    let mut value = module
        .get(&root)
        .ok_or_else(|| format!("unknown data node `{root}`"))?;
    for segment in segments {
        value = select_segment(value, &segment)?;
    }
    Ok(value)
}

fn parse_value_path(path: &str) -> Result<(String, Vec<Segment>), String> {
    let mut chars = path.char_indices().peekable();
    let root = read_ident(path, &mut chars)?;
    let mut segments = Vec::new();
    while let Some((_, ch)) = chars.peek().copied() {
        match ch {
            '.' => {
                chars.next();
                segments.push(Segment::Field(read_ident(path, &mut chars)?));
            }
            '[' => {
                chars.next();
                let start = chars
                    .peek()
                    .map(|(index, _)| *index)
                    .ok_or_else(|| format!("unterminated index in `{path}`"))?;
                while let Some((_, ch)) = chars.peek() {
                    if *ch == ']' {
                        break;
                    }
                    chars.next();
                }
                let end = chars
                    .peek()
                    .map(|(index, _)| *index)
                    .ok_or_else(|| format!("unterminated index in `{path}`"))?;
                chars.next();
                let index = path[start..end]
                    .parse::<usize>()
                    .map_err(|_| format!("index must be a nonneg integer in `{path}`"))?;
                segments.push(Segment::Index(index));
            }
            _ => return Err(format!("unexpected `{ch}` in path `{path}`")),
        }
    }
    Ok((root, segments))
}

fn read_ident(
    path: &str,
    chars: &mut std::iter::Peekable<std::str::CharIndices<'_>>,
) -> Result<String, String> {
    let start = chars
        .peek()
        .map(|(index, _)| *index)
        .ok_or_else(|| format!("expected identifier in `{path}`"))?;
    while let Some((_, ch)) = chars.peek() {
        if *ch == '.' || *ch == '[' || *ch == ']' {
            break;
        }
        chars.next();
    }
    let end = chars.peek().map_or(path.len(), |(index, _)| *index);
    if start == end {
        Err(format!("expected identifier in `{path}`"))
    } else {
        Ok(path[start..end].to_string())
    }
}

fn select_segment(value: CfdValueRef, segment: &Segment) -> Result<CfdValueRef, String> {
    let borrowed = value.borrow();
    match (segment, &*borrowed) {
        (Segment::Field(field), CfdValue::Object { fields, .. }) => fields
            .get(field)
            .cloned()
            .ok_or_else(|| format!("missing field `{field}`")),
        (Segment::Field(field), CfdValue::Union { value, .. }) => {
            let inner = value.clone();
            drop(borrowed);
            select_segment(inner, &Segment::Field(field.clone()))
        }
        (Segment::Index(index), CfdValue::Array(items)) => items
            .get(*index)
            .cloned()
            .ok_or_else(|| format!("array index `{index}` is out of bounds")),
        (Segment::Index(index), CfdValue::Dict(entries)) => entries
            .get(*index)
            .map(|(_, v)| v.clone())
            .ok_or_else(|| format!("dict index `{index}` is out of bounds")),
        (Segment::Index(index), CfdValue::Union { value, .. }) => {
            let inner = value.clone();
            drop(borrowed);
            select_segment(inner, &Segment::Index(*index))
        }
        (Segment::Field(field), other) => {
            Err(format!("cannot select field `{field}` from {}", other.type_name()))
        }
        (Segment::Index(index), other) => {
            Err(format!("cannot index {} at `{index}`", other.type_name()))
        }
    }
}

fn print_definition(cft: &CftContainer, name: &str) -> Result<(), String> {
    if let Some(def) = cft.resolve_type(name) {
        println!("{}", format_type_def(&def));
        return Ok(());
    }
    if let Some(def) = cft.resolve_enum(name) {
        println!("{}", format_enum_def(&def));
        return Ok(());
    }
    Err(format!("unknown type or enum `{name}`"))
}

fn format_type_def(def: &CftSchemaType) -> String {
    if let Some(alias) = &def.alias {
        return format!("type {} = {};", def.name, alias);
    }
    let mut out = format!("type {} {{", def.name);
    for field in &def.fields {
        out.push_str(&format!(
            "\n  {}: {};{}",
            field.name,
            field.ty,
            if field.has_default { " // default" } else { "" }
        ));
    }
    out.push_str("\n}");
    out
}

fn format_enum_def(def: &CftSchemaEnum) -> String {
    let mut out = format!("enum {} {{", def.name);
    let mut next = 0;
    for variant in &def.variants {
        let value = variant.value.unwrap_or(next);
        next = value + 1;
        out.push_str(&format!("\n  {} = {},", variant.name, value));
    }
    out.push_str("\n}");
    out
}

#[derive(Debug, Default)]
struct GraphRenderer {
    seen: Vec<(CfdValueRef, usize)>,
    next_id: usize,
}

impl GraphRenderer {
    fn new() -> Self {
        Self {
            seen: Vec::new(),
            next_id: 1,
        }
    }

    fn render(mut self, value: &CfdValueRef) -> String {
        self.render_ref(value)
    }

    fn render_ref(&mut self, value: &CfdValueRef) -> String {
        match &*value.borrow() {
            CfdValue::Null => "null".to_string(),
            CfdValue::Int(v) => v.to_string(),
            CfdValue::Float(v) => v.to_string(),
            CfdValue::Bool(v) => v.to_string(),
            CfdValue::String(v) => quoted(v),
            CfdValue::Enum {
                enum_type,
                variant,
                value: v,
            } => format!(
                "{{\"$enum\":{},\"value\":{}}}",
                quoted(&format!("{}.{}.{}", enum_type.module, enum_type.name, variant)),
                v
            ),
            CfdValue::Object { type_name, fields } => {
                if let Some(id) = self.seen_id(value) {
                    return format!("{{\"$ref\":{}}}", quoted(&id.to_string()));
                }
                let id = self.mark_seen(value);
                let mut parts = vec![format!("\"$id\":{}", quoted(&id.to_string()))];
                if let Some(tn) = type_name {
                    parts.push(format!(
                        "\"$type\":{}",
                        quoted(&format!("{}.{}", tn.module, tn.name))
                    ));
                }
                for (name, field) in fields {
                    parts.push(format!("{}:{}", quoted(name), self.render_ref(field)));
                }
                format!("{{{}}}", parts.join(","))
            }
            CfdValue::Union { union_type, value: v } => format!(
                "{{\"$union\":{},\"value\":{}}}",
                quoted(&format!("{}.{}", union_type.module, union_type.name)),
                self.render_ref(v)
            ),
            CfdValue::Array(items) => {
                if let Some(id) = self.seen_id(value) {
                    return format!("{{\"$ref\":{}}}", quoted(&id.to_string()));
                }
                let id = self.mark_seen(value);
                let rendered = items
                    .iter()
                    .map(|item| self.render_ref(item))
                    .collect::<Vec<_>>()
                    .join(",");
                format!("{{\"$id\":{},\"$array\":[{}]}}", quoted(&id.to_string()), rendered)
            }
            CfdValue::Dict(entries) => {
                if let Some(id) = self.seen_id(value) {
                    return format!("{{\"$ref\":{}}}", quoted(&id.to_string()));
                }
                let id = self.mark_seen(value);
                let rendered = entries
                    .iter()
                    .map(|(k, v)| {
                        format!(
                            "{{\"key\":{},\"value\":{}}}",
                            self.render_ref(k),
                            self.render_ref(v)
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(",");
                format!("{{\"$id\":{},\"$dict\":[{}]}}", quoted(&id.to_string()), rendered)
            }
        }
    }

    fn seen_id(&self, value: &CfdValueRef) -> Option<usize> {
        self.seen
            .iter()
            .find(|(seen, _)| CfdValueRef::ptr_eq(seen, value))
            .map(|(_, id)| *id)
    }

    fn mark_seen(&mut self, value: &CfdValueRef) -> usize {
        let id = self.next_id;
        self.next_id += 1;
        self.seen.push((value.clone(), id));
        id
    }
}

fn quoted(value: &str) -> String {
    let mut out = String::from("\"");
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if ch.is_control() => out.push_str(&format!("\\u{:04x}", u32::from(ch))),
            ch => out.push(ch),
        }
    }
    out.push('"');
    out
}
