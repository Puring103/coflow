use coflow_cfc::{
    BuildErrors, CfcContainer, CfcError, CfcImport, CfcModuleResult, CfcResult, CfcSchemaEnum,
    CfcSchemaType, CfcValue, CfcValueRef, CheckError, ModuleId, ParseErrors, ResolveError,
};
use std::collections::HashMap;
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
            let (json, file) = parse_check_args(&args)?;
            if json {
                return check_json(file);
            }
            let loaded = load_file(file)?;
            report_checks(&loaded.container, &loaded.result)?;
            println!("ok");
            Ok(())
        }
        "get" => {
            let file = required_arg(&args, 2, "missing file")?;
            let path = required_arg(&args, 3, "missing path")?;
            let loaded = load_file(file)?;
            report_checks(&loaded.container, &loaded.result)?;
            let value =
                select_root_path(loaded.result.root().ok_or("missing root module")?, &path)?;
            println!("{}", GraphRenderer::new().render(&value));
            Ok(())
        }
        "type" => {
            let file = required_arg(&args, 2, "missing file")?;
            let name = required_arg(&args, 3, "missing type name")?;
            let loaded = load_file(file)?;
            print_definition(&loaded, &name)
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
  cfc check [--json] <file.cfc>
  cfc get <file.cfc> <path>
  cfc type <file.cfc> <type-name>"
        .to_string()
}

fn parse_check_args(args: &[String]) -> Result<(bool, String), String> {
    match (args.get(2).map(String::as_str), args.get(3)) {
        (Some("--json"), Some(file)) => Ok((true, file.clone())),
        (Some("--json"), None) => Err("missing file".to_string()),
        (Some(file), None) => Ok((false, file.to_string())),
        (Some(file), Some(_)) => Err(format!("unexpected extra argument after `{file}`")),
        (None, _) => Err("missing file".to_string()),
    }
}

fn check_json(file: String) -> Result<(), String> {
    let root_path = canonicalize_path(Path::new(&file))?;
    let root = ModuleId::from(path_to_module_id(&root_path));
    let root_source = fs::read_to_string(&root_path)
        .map_err(|err| format!("failed to read `{}`: {err}", root_path.display()))?;
    let mut aliases = HashMap::new();
    let mut container = CfcContainer::new();
    let result = match container.load_graph(root.clone(), root_source, |from, import| {
        resolve_import(from, import, &mut aliases)
    }) {
        Ok(result) => result,
        Err(error) => {
            println!("{}", cfc_error_json(&path_to_module_id(&root_path), error));
            return Err("check failed".to_string());
        }
    };
    let errors = container.check(&result);
    println!(
        "{}",
        check_errors_json(&path_to_module_id(&root_path), &errors)
    );
    if errors.is_empty() {
        Ok(())
    } else {
        Err("check failed".to_string())
    }
}

#[derive(Debug)]
struct LoadedGraph {
    container: CfcContainer,
    result: CfcResult,
    root: ModuleId,
    aliases: HashMap<(ModuleId, String), ModuleId>,
}

fn load_file(file: String) -> Result<LoadedGraph, String> {
    let root_path = canonicalize_path(Path::new(&file))?;
    let root = ModuleId::from(path_to_module_id(&root_path));
    let root_source = fs::read_to_string(&root_path)
        .map_err(|err| format!("failed to read `{}`: {err}", root_path.display()))?;
    let mut aliases = HashMap::new();
    let mut container = CfcContainer::new();
    let result = container
        .load_graph(root.clone(), root_source, |from, import| {
            resolve_import(from, import, &mut aliases)
        })
        .map_err(format_cfc_error)?;
    Ok(LoadedGraph {
        container,
        result,
        root,
        aliases,
    })
}

fn resolve_import(
    from: &ModuleId,
    import: &CfcImport,
    aliases: &mut HashMap<(ModuleId, String), ModuleId>,
) -> Result<(ModuleId, String), ResolveError> {
    let from_path = Path::new(from.as_str());
    let base = from_path.parent().unwrap_or_else(|| Path::new(""));
    let import_path = Path::new(&import.path);
    let path = if import_path.is_absolute() {
        import_path.to_path_buf()
    } else {
        base.join(import_path)
    };
    let path = canonicalize_path(&path).map_err(ResolveError::new)?;
    let module = ModuleId::from(path_to_module_id(&path));
    let source = fs::read_to_string(&path)
        .map_err(|err| ResolveError::new(format!("failed to read `{}`: {err}", path.display())))?;
    aliases.insert((from.clone(), import.alias.clone()), module.clone());
    Ok((module, source))
}

fn canonicalize_path(path: &Path) -> Result<PathBuf, String> {
    fs::canonicalize(path).map_err(|err| format!("failed to resolve `{}`: {err}", path.display()))
}

fn path_to_module_id(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn report_checks(container: &CfcContainer, result: &CfcResult) -> Result<(), String> {
    let errors = container.check(result);
    if errors.is_empty() {
        Ok(())
    } else {
        Err(format_check_errors(&errors))
    }
}

fn cfc_error_json(file: &str, error: CfcError) -> String {
    match error {
        CfcError::Parse(errors) => parse_errors_json(file, &errors),
        CfcError::Build(errors) => build_errors_json(file, &errors),
        CfcError::Module(error) => single_diagnostic_json(file, "module", "Module", &error.message),
        CfcError::Import(error) => single_diagnostic_json(file, "import", "Import", &error.message),
        CfcError::Resolve(error) => {
            single_diagnostic_json(file, "resolve", "Resolve", &error.message)
        }
    }
}

fn parse_errors_json(file: &str, errors: &ParseErrors) -> String {
    let items = errors
        .errors
        .iter()
        .map(|error| {
            diagnostic_json(
                file,
                "parse",
                &format!("{:?}", error.kind),
                &error.message,
                Some(error.span),
            )
        })
        .collect::<Vec<_>>();
    format!("[{}]", items.join(","))
}

fn build_errors_json(file: &str, errors: &BuildErrors) -> String {
    let items = errors
        .errors
        .iter()
        .filter(|error| error.span.is_some())
        .map(|error| {
            diagnostic_json(
                file,
                "build",
                &format!("{:?}", error.kind),
                &error.message,
                error.span,
            )
        })
        .collect::<Vec<_>>();
    format!("[{}]", items.join(","))
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
    format!("[{}]", diagnostic_json(file, stage, kind, message, None))
}

fn diagnostic_json(
    file: &str,
    stage: &str,
    kind: &str,
    message: &str,
    span: Option<coflow_cfc::Span>,
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

fn span_json(span: coflow_cfc::Span) -> String {
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

fn format_cfc_error(error: CfcError) -> String {
    match error {
        CfcError::Parse(errors) => format_parse_errors(&errors),
        CfcError::Module(error) => error.message,
        CfcError::Import(error) => error.message,
        CfcError::Resolve(error) => error.message,
        CfcError::Build(errors) => format_build_errors(&errors),
    }
}

fn format_parse_errors(errors: &ParseErrors) -> String {
    errors
        .errors
        .iter()
        .map(|error| format!("{:?}: {}", error.kind, error.message))
        .collect::<Vec<_>>()
        .join("\n")
}

fn format_build_errors(errors: &BuildErrors) -> String {
    errors
        .errors
        .iter()
        .map(|error| format!("{:?}: {}", error.kind, error.message))
        .collect::<Vec<_>>()
        .join("\n")
}

fn format_check_errors(errors: &[CheckError]) -> String {
    errors
        .iter()
        .map(|error| format!("{:?}: {}", error.kind, error.message))
        .collect::<Vec<_>>()
        .join("\n")
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Segment {
    Field(String),
    Index(usize),
}

fn select_root_path(module: &CfcModuleResult, path: &str) -> Result<CfcValueRef, String> {
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
                    .map_err(|_| format!("index must be a nonnegative integer in `{path}`"))?;
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

fn select_segment(value: CfcValueRef, segment: &Segment) -> Result<CfcValueRef, String> {
    let borrowed = value.borrow();
    match (segment, &*borrowed) {
        (Segment::Field(field), CfcValue::Object { fields, .. }) => fields
            .get(field)
            .cloned()
            .ok_or_else(|| format!("missing field `{field}`")),
        (Segment::Field(field), CfcValue::Union { value, .. }) => {
            let inner = value.clone();
            drop(borrowed);
            select_segment(inner, &Segment::Field(field.clone()))
        }
        (Segment::Index(index), CfcValue::Array(items)) => items
            .get(*index)
            .cloned()
            .ok_or_else(|| format!("array index `{index}` is out of bounds")),
        (Segment::Index(index), CfcValue::Dict(entries)) => entries
            .get(*index)
            .map(|(_, value)| value.clone())
            .ok_or_else(|| format!("dict index `{index}` is out of bounds")),
        (Segment::Index(index), CfcValue::Union { value, .. }) => {
            let inner = value.clone();
            drop(borrowed);
            select_segment(inner, &Segment::Index(*index))
        }
        (Segment::Field(field), other) => Err(format!(
            "cannot select field `{field}` from {}",
            other.type_name()
        )),
        (Segment::Index(index), other) => {
            Err(format!("cannot index {} at `{index}`", other.type_name()))
        }
    }
}

fn print_definition(loaded: &LoadedGraph, name: &str) -> Result<(), String> {
    let (module, local_name) = resolve_definition_name(loaded, name)?;
    if let Some(def) = loaded.container.type_def(&module, &local_name) {
        println!("{}", format_type_def(&def));
        return Ok(());
    }
    if let Some(def) = loaded.container.enum_def(&module, &local_name) {
        println!("{}", format_enum_def(&def));
        return Ok(());
    }
    Err(format!("unknown type or enum `{name}`"))
}

fn resolve_definition_name(loaded: &LoadedGraph, name: &str) -> Result<(ModuleId, String), String> {
    if let Some((alias, local)) = name.split_once('.') {
        let module = loaded
            .aliases
            .get(&(loaded.root.clone(), alias.to_string()))
            .cloned()
            .ok_or_else(|| format!("unknown import alias `{alias}`"))?;
        Ok((module, local.to_string()))
    } else {
        Ok((loaded.root.clone(), name.to_string()))
    }
}

fn format_type_def(def: &CfcSchemaType) -> String {
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

fn format_enum_def(def: &CfcSchemaEnum) -> String {
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
    seen: Vec<(CfcValueRef, usize)>,
    next_id: usize,
}

impl GraphRenderer {
    fn new() -> Self {
        Self {
            seen: Vec::new(),
            next_id: 1,
        }
    }

    fn render(mut self, value: &CfcValueRef) -> String {
        self.render_ref(value)
    }

    fn render_ref(&mut self, value: &CfcValueRef) -> String {
        match &*value.borrow() {
            CfcValue::Null => "null".to_string(),
            CfcValue::Int(value) => value.to_string(),
            CfcValue::Float(value) => value.to_string(),
            CfcValue::Bool(value) => value.to_string(),
            CfcValue::String(value) => quoted(value),
            CfcValue::Enum {
                enum_type,
                variant,
                value,
            } => format!(
                "{{\"$enum\":{},\"value\":{}}}",
                quoted(&format!(
                    "{}.{}.{}",
                    enum_type.module, enum_type.name, variant
                )),
                value
            ),
            CfcValue::Object { type_name, fields } => {
                if let Some(id) = self.seen_id(value) {
                    return format!("{{\"$ref\":{}}}", quoted(&id.to_string()));
                }
                let id = self.mark_seen(value);
                let mut parts = vec![format!("\"$id\":{}", quoted(&id.to_string()))];
                if let Some(type_name) = type_name {
                    parts.push(format!(
                        "\"$type\":{}",
                        quoted(&format!("{}.{}", type_name.module, type_name.name))
                    ));
                }
                for (name, field) in fields {
                    parts.push(format!("{}:{}", quoted(name), self.render_ref(field)));
                }
                format!("{{{}}}", parts.join(","))
            }
            CfcValue::Union { union_type, value } => {
                format!(
                    "{{\"$union\":{},\"value\":{}}}",
                    quoted(&format!("{}.{}", union_type.module, union_type.name)),
                    self.render_ref(value)
                )
            }
            CfcValue::Array(items) => {
                if let Some(id) = self.seen_id(value) {
                    return format!("{{\"$ref\":{}}}", quoted(&id.to_string()));
                }
                let id = self.mark_seen(value);
                let items = items
                    .iter()
                    .map(|item| self.render_ref(item))
                    .collect::<Vec<_>>()
                    .join(",");
                format!(
                    "{{\"$id\":{},\"$array\":[{}]}}",
                    quoted(&id.to_string()),
                    items
                )
            }
            CfcValue::Dict(entries) => {
                if let Some(id) = self.seen_id(value) {
                    return format!("{{\"$ref\":{}}}", quoted(&id.to_string()));
                }
                let id = self.mark_seen(value);
                let entries = entries
                    .iter()
                    .map(|(key, value)| {
                        format!(
                            "{{\"key\":{},\"value\":{}}}",
                            self.render_ref(key),
                            self.render_ref(value)
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(",");
                format!(
                    "{{\"$id\":{},\"$dict\":[{}]}}",
                    quoted(&id.to_string()),
                    entries
                )
            }
        }
    }

    fn seen_id(&self, value: &CfcValueRef) -> Option<usize> {
        self.seen
            .iter()
            .find(|(seen, _)| CfcValueRef::ptr_eq(seen, value))
            .map(|(_, id)| *id)
    }

    fn mark_seen(&mut self, value: &CfcValueRef) -> usize {
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
