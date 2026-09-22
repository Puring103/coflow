//! 宿主输入构建为不可变映像；Arena 只在构建期间负责静态值身份与引用连接。
use super::*;
use crate::{loading::{self, SourceAnalysis, SourceInput}, CfdDataModel, CfdDictKey, CfdValue};
use std::sync::atomic::Ordering;

#[derive(Debug)]
pub struct RuntimeBuild {
    pub analyses: Vec<SourceAnalysis>,
    pub diagnostics: Vec<BuildDiagnostic>,
    pub runtime: Result<Arc<Runtime>, String>,
}

#[derive(Debug, Clone)]
pub struct BuildDiagnostic {
    pub code: String,
    pub source: String,
    pub message: String,
    pub span: Option<(usize, usize)>,
}

impl From<String> for BuildDiagnostic {
    fn from(message: String) -> Self {
        Self {
            code: "BUILD".into(),
            source: String::new(),
            message,
            span: None,
        }
    }
}
impl From<&str> for BuildDiagnostic {
    fn from(message: &str) -> Self {
        message.to_string().into()
    }
}
#[derive(Debug, Clone)]
pub struct RuntimeBuilder {
    contract: Arc<Contract>,
    sources: Vec<SourceInput>,
    bindings: HostBindings,
    profile: OptimizationProfile,
}
impl RuntimeBuilder {
    pub fn new(contract: Arc<Contract>) -> Self {
        Self {
            contract,
            sources: Vec::new(),
            bindings: BTreeMap::new(),
            profile: OptimizationProfile::default(),
        }
    }
    pub fn optimization_profile(&mut self, profile: OptimizationProfile) {
        self.profile = profile;
    }
    pub fn add_source(&mut self, input: SourceInput) {
        self.sources.push(input);
    }
    /// 来源名称只用于诊断，同名输入仍逐次追加；匿名来源由核心稳定编号。
    pub fn add_text(&mut self, source: &str, name: Option<&str>) {
        let name = name
            .map(str::to_string)
            .unwrap_or_else(|| format!("source-{}", self.sources.len() + 1));
        self.add_source(SourceInput::new(name, source));
    }
    pub fn bind(&mut self, name: String, service: Arc<dyn HostService>) -> Result<(), String> {
        if name == "Coflow::Check" {
            if self.bindings.contains_key(&name) {
                return Err("duplicate Host binding Coflow::Check".into());
            }
            self.bindings.insert(name, service);
            return Ok(());
        }
        let ty = self
            .contract
            .schema()
            .resolve_type(&name)
            .ok_or_else(|| format!("unknown Host type {name}"))?;
        if !ty.is_host {
            return Err(format!("{name} is not a Host service"));
        }
        if self.bindings.contains_key(&name) {
            return Err(format!("duplicate Host binding {name}"));
        }
        for field in ty.all_fields() {
            let present = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                service.has_member(
                    field.name.as_str(),
                    &field.value_type,
                    self.contract.schema(),
                )
            }))
            .map_err(|_| format!("Host binding validation panicked: {name}.{}", field.name))?;
            if !present {
                return Err(format!(
                    "Host binding does not implement {name}.{}",
                    field.name
                ));
            }
        }
        self.bindings.insert(name, service);
        Ok(())
    }
    pub fn build(self) -> RuntimeBuild {
        let (analyses, model) = loading::load(self.contract.schema(), self.sources);
        let mut diagnostics: Vec<_> = analyses
            .iter()
            .flat_map(|source| {
                source.diagnostics.iter().map(|error| BuildDiagnostic {
                    code: format!("{:?}", error.code),
                    source: source.input.path.to_string_lossy().into_owned(),
                    message: error.message.clone(),
                    span: Some((error.span.start, error.span.end)),
                })
            })
            .collect();
        if let Err(loading::CfdTextLoadError::DataModel {
            diagnostics: errors,
            origins,
        }) = &model
        {
            for error in &errors.diagnostics {
                let origin = error.primary.as_ref().and_then(|label| {
                    label
                        .origin
                        .as_ref()
                        .or_else(|| label.record.and_then(|id| origins.get(id.index())))
                });
                let source = match origin {
                    Some(crate::RecordOrigin::File { path, .. }) => {
                        path.to_string_lossy().into_owned()
                    }
                    _ => String::new(),
                };
                diagnostics.push(BuildDiagnostic {
                    code: error.code.to_string(),
                    source,
                    message: error.message.clone(),
                    span: None,
                });
            }
        }
        let runtime = model
            .map_err(|e| e.to_string())
            .and_then(|model| {
                Runtime::from_model_with_profile(
                    self.contract,
                    model,
                    self.bindings,
                    self.profile,
                )
                .map_err(|diagnostic| {
                    let message = diagnostic.message.clone();
                    diagnostics.push(diagnostic);
                    message
                })
            })
            .map(Arc::new);
        if let Err(message) = &runtime {
            if diagnostics.is_empty() {
                diagnostics.push(BuildDiagnostic {
                    code: "BUILD".into(),
                    source: String::new(),
                    message: message.clone(),
                    span: None,
                });
            }
        }
        RuntimeBuild {
            analyses,
            diagnostics,
            runtime,
        }
    }
}

impl Runtime {
    pub(crate) fn from_model(
        contract: Arc<Contract>,
        model: CfdDataModel,
        bindings: HostBindings,
    ) -> Result<Self, BuildDiagnostic> {
        Self::from_model_with_profile(contract, model, bindings, OptimizationProfile::default())
    }

    pub(crate) fn from_model_with_profile(
        contract: Arc<Contract>,
        model: CfdDataModel,
        bindings: HostBindings,
        profile: OptimizationProfile,
    ) -> Result<Self, BuildDiagnostic> {
        let mut arena = Arena {
            contract: &contract,
            values: Vec::new(),
            records: BTreeMap::new(),
            constant_callables: BTreeMap::new(),
            contract_values: BTreeSet::new(),
            function_imports: BTreeMap::new(),
            function_locations: BTreeMap::new(),
        };
        // 先分配所有记录身份，再连接字段，允许自引用及跨记录循环。
        for (_, record) in model.records() {
            arena.record(record.actual_type(), record.key());
        }
        let mut dimension_variants = BTreeMap::<String, BTreeSet<String>>::new();
        for (_, record) in model.records() {
            for values in record.dimension_fields.values() {
                dimension_variants
                    .entry(values.dimension.to_string())
                    .or_default()
                    .extend(values.variants.keys().map(ToString::to_string));
            }
        }
        for ty in contract.schema().singleton_types().filter(|ty| ty.is_host) {
            arena.record(
                ty.name.as_str(),
                ty.name.rsplit("::").next().unwrap_or(ty.name.as_str()),
            );
        }
        // 常量先建立独立存储，后续字段复用其中的函数与模板身份。
        let check_record_ids: BTreeMap<ValueId, crate::CfdRecordId> = model
            .records()
            .map(|(id, record)| {
                (
                    arena.records[&(record.actual_type().to_string(), record.key().to_string())],
                    id,
                )
            })
            .collect();
        let mut constants = BTreeMap::new();
        for constant in contract.schema().all_consts() {
            constants.insert(
                constant.name.to_string(),
                arena.constant(&constant.value, None)?,
            );
        }
        for (_, record) in model.records() {
            let id = arena.records[&(record.actual_type().to_string(), record.key().to_string())];
            let ty = contract
                .schema()
                .resolve_type(record.actual_type())
                .ok_or("missing record schema")?;
            if ty.is_host {
                continue;
            }
            let mut fields = Vec::new();
            let key = arena.push(Value::String(record.key().to_string()));
            fields.push(("id".into(), key));
            for field in ty.all_fields() {
                let value = record
                    .field(field.name.as_str())
                    .ok_or_else(|| format!("missing field {}", field.name))?;
                let base = arena.value(value, &field.value_type, Some(id))?;
                let value_id = if let Some(binding) = &field.dimension {
                    let mut variants = BTreeMap::new();
                    if let Some(stored) = record.dimension_field(field.name.as_str()) {
                        debug_assert_eq!(stored.dimension, binding.dimension);
                        for (variant, value) in &stored.variants {
                            variants.insert(
                                variant.to_string(),
                                arena.value(&value.value, &field.value_type, Some(id))?,
                            );
                        }
                    }
                    // 全局枚举视图与显式覆盖分别保存；None 也是有效覆盖。
                    let explicit = variants.keys().cloned().collect();
                    if let Some(all) = dimension_variants.get(binding.dimension.as_str()) {
                        for variant in all {
                            variants.entry(variant.clone()).or_insert(base);
                        }
                    }
                    arena.push(Value::Dimension {
                        default: base,
                        variants,
                        explicit,
                    })
                } else {
                    base
                };
                fields.push((field.name.to_string(), value_id));
            }
            arena.values[id as usize] = Value::Object {
                type_name: record.actual_type().into(),
                key: Some(record.key().into()),
                fields,
                bases: Vec::new(),
            };
        }
        for ty in contract.schema().singleton_types().filter(|ty| ty.is_host) {
            let key = ty.name.rsplit("::").next().unwrap_or(ty.name.as_str());
            let id = arena.records[&(ty.name.to_string(), key.into())];
            let mut fields = Vec::new();
            let key_id = arena.push(Value::String(key.into()));
            fields.push(("id".into(), key_id));
            for field in ty.all_fields() {
                let value = if matches!(field.value_type, CftValueType::Function(..)) {
                    Value::Function {
                        source: "".into(),
                        owner: Some(id),
                        host: Some((ty.name.to_string(), field.name.to_string())),
                    }
                } else {
                    Value::HostData {
                        service: ty.name.to_string(),
                        field: field.name.to_string(),
                        value_type: field.value_type.clone(),
                    }
                };
                let value_id = arena.push(value);
                fields.push((field.name.to_string(), value_id));
            }
            arena.values[id as usize] = Value::Object {
                type_name: ty.name.to_string(),
                key: Some(key.into()),
                fields,
                bases: Vec::new(),
            };
        }
        let Arena {
            values,
            records,
            contract_values,
            function_imports,
            function_locations,
            ..
        } = arena;
        let (values, remap) = fixed::compact(values)?;
        let map = |id: ValueId| remap[id as usize];
        let records = records
            .into_iter()
            .map(|(key, id)| (key, map(id)))
            .collect::<BTreeMap<_, _>>();
        let contract_values = contract_values.into_iter().map(map).collect();
        let function_imports = function_imports
            .into_iter()
            .map(|(id, imports)| (map(id), imports))
            .collect();
        let function_locations = function_locations
            .into_iter()
            .map(|(id, location)| (map(id), location))
            .collect();
        let check_record_ids = check_record_ids
            .into_iter()
            .map(|(id, record)| (map(id), record))
            .collect();
        for id in constants.values_mut() {
            *id = map(*id);
        }
        let identity = NEXT_RUNTIME
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |v| v.checked_add(1))
            .map_err(|_| "Runtime identity exhausted")?;
        // 按静态查询类型建立一次索引，宿主枚举不重复扫描或复制整张表。
        let mut table_values = BTreeMap::new();
        let mut record_lookup: BTreeMap<String, BTreeMap<String, ValueId>> = BTreeMap::new();
        for ty in contract
            .schema()
            .all_types()
            .filter(|ty| ty.kind != coflow_language::cft::syntax::ast::TypeKind::Data)
        {
            let mut ids = Vec::new();
            let mut lookup = BTreeMap::new();
            for ((actual, key), id) in &records {
                if contract.schema().is_assignable(actual, &ty.name) {
                    ids.push(*id);
                    lookup.insert(key.clone(), *id);
                }
            }
            record_lookup.insert(ty.name.to_string(), lookup);
            table_values.insert(ty.name.to_string(), ids);
        }
        let check_reporter = Arc::new(checking::CheckReporter::default());
        let mut bindings = bindings;
        bindings
            .entry("Coflow::Check".into())
            .or_insert_with(|| check_reporter.clone());
        let fixed_count = values.len() as ValueId;
        let values = fixed::FixedValues::new(values)?;
        let programs = image::ProgramImage::build(&image::LinkContext {
            profile, contract: &contract, values: &values, constants: &constants,
            record_lookup: &record_lookup, contract_values: &contract_values,
            function_imports: &function_imports, function_locations: &function_locations,
        })?;
        let runtime = Self {
            identity,
            image: Arc::new(RuntimeImage {
                profile,
                contract,
                values,
                records,
                table_values,
                check_record_ids,
                record_lookup,
                constants,
                programs,
            }),
            bindings,
            released: std::cell::Cell::new(false),
            creator_thread: std::thread::current().id(),
            execution: std::cell::Cell::new(0),
            vm: state::VmState::new(fixed_count),
            check_reporter,
        };
        Ok(runtime)
    }
}

impl Runtime {
    pub fn from_image(image: Arc<RuntimeImage>, bindings: HostBindings) -> Result<Self, String> {
        let mut checked = RuntimeBuilder::new(image.contract.clone());
        for (name, service) in bindings {
            checked.bind(name, service)?;
        }
        let check_reporter = Arc::new(checking::CheckReporter::default());
        checked
            .bindings
            .entry("Coflow::Check".into())
            .or_insert_with(|| check_reporter.clone());
        let identity = NEXT_RUNTIME
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |v| v.checked_add(1))
            .map_err(|_| "Runtime identity exhausted")?;
        Ok(Self {
            identity,
            vm: state::VmState::new(image.values.len()),
            image,
            bindings: checked.bindings,
            released: std::cell::Cell::new(false),
            creator_thread: std::thread::current().id(),
            execution: std::cell::Cell::new(0),
            check_reporter,
        })
    }
}

struct Arena<'a> {
    contract: &'a Contract,
    values: Vec<Value>,
    records: BTreeMap<(String, String), ValueId>,
    constant_callables: BTreeMap<String, ValueId>,
    contract_values: BTreeSet<ValueId>,
    function_imports: BTreeMap<ValueId, BTreeMap<String, String>>,
    function_locations: BTreeMap<ValueId, crate::CallableLocation>,
}
impl Arena<'_> {
    fn constant(
        &mut self,
        value: &crate::schema::CftStaticValue,
        owner: Option<ValueId>,
    ) -> Result<ValueId, String> {
        use crate::schema::CftStaticValue as C;
        let next = match value {
            C::Int(value) => {
                Value::Int(i32::try_from(*value).map_err(|_| "constant int outside i32")?)
            }
            C::Float(value) => Value::Float(*value as f32),
            C::Bool(value) => Value::Bool(*value),
            C::String(value) => Value::String(value.clone()),
            C::Enum {
                enum_name, value, ..
            } => Value::Enum {
                type_name: enum_name.to_string(),
                value: u32::try_from(*value).map_err(|_| "invalid constant enum")?,
            },
            C::OptionNone => Value::None,
            C::OptionSome(value) => return self.constant(value, owner),
            C::Function(source) | C::FormattedString(source) => {
                let origin = source
                    .constant_origin
                    .as_ref()
                    .ok_or("constant callable has no origin")?;
                if let Some(id) = self.constant_callables.get(origin) {
                    return Ok(*id);
                }
                let value = if matches!(value, C::Function(_)) {
                    Value::Function {
                        source: source.source.as_str().into(),
                        owner,
                        host: None,
                    }
                } else {
                    Value::Template {
                        source: source.source.as_str().into(),
                        owner,
                    }
                };
                let id = self.push(value);
                self.constant_callables.insert(origin.clone(), id);
                self.function_locations.insert(id, source.into());
                self.contract_values.insert(id);
                return Ok(id);
            }
            C::Array(values) => Value::Array(
                values
                    .iter()
                    .map(|value| self.constant(value, owner))
                    .collect::<Result<_, _>>()?,
            ),
            C::Dictionary(values) => {
                let mut entries = DictionaryValue::with_capacity(values.len());
                for (key, value) in values {
                    let scalar = const_scalar_key(key)?;
                    let key = self.constant(key, owner)?;
                    let value = self.constant(value, owner)?;
                    if entries.insert(scalar, (key, value)).is_some() {
                        return Err("duplicate dictionary key".into());
                    }
                }
                Value::Dict(entries)
            }
            C::Object { type_name, fields } => {
                let id = self.push(Value::None);
                let mut values = Vec::with_capacity(fields.len());
                for (name, value) in fields {
                    values.push((name.to_string(), self.constant(value, Some(id))?));
                }
                self.values[id as usize] = Value::Object {
                    type_name: type_name.to_string(),
                    key: None,
                    fields: values,
                    bases: Vec::new(),
                    };
                return Ok(id);
            }
            C::RecordReference { type_name, key } => {
                return self
                    .records
                    .iter()
                    .find(|((actual, record_key), _)| {
                        record_key == key && self.contract.schema().is_assignable(actual, type_name)
                    })
                    .map(|(_, id)| *id)
                    .ok_or_else(|| format!("missing constant reference {type_name}::{key}"))
            }
        };
        Ok(self.push(next))
    }

    fn push(&mut self, value: Value) -> ValueId {
        let id = self.values.len() as ValueId;
        self.values.push(value);
        id
    }
    fn record(&mut self, ty: &str, key: &str) -> ValueId {
        let coordinate = (ty.into(), key.into());
        if let Some(id) = self.records.get(&coordinate) {
            return *id;
        }
        let id = self.push(Value::None);
        self.records.insert(coordinate, id);
        id
    }
    fn value(
        &mut self,
        value: &CfdValue,
        ty: &CftValueType,
        owner: Option<ValueId>,
    ) -> Result<ValueId, String> {
        let origin = match value {
            CfdValue::Function(value) => value.constant_origin.as_ref(),
            CfdValue::FormattedString(value) => value.constant_origin.as_ref(),
            _ => None,
        };
        if let Some(origin) = origin {
            return self
                .constant_callables
                .get(origin)
                .copied()
                .ok_or_else(|| format!("missing constant callable {origin}"));
        }
        let ty = if let CftValueType::Option(inner) = ty {
            inner.as_ref()
        } else {
            ty
        };
        let next = match value {
            CfdValue::OptionNone => Value::None,
            CfdValue::OptionSome(value) => return self.value(value, ty, owner),
            CfdValue::Bool(v) => Value::Bool(*v),
            CfdValue::Int(v) => Value::Int(i32::try_from(*v).map_err(|_| "int outside i32 range")?),
            CfdValue::Float(v) => Value::Float(*v as f32),
            CfdValue::String(v) => Value::String(v.clone()),
            CfdValue::FormattedString(v) => Value::Template {
                source: v.source.as_str().into(),
                owner,
            },
            CfdValue::Function(v) => Value::Function {
                source: v.source.as_str().into(),
                owner,
                host: None,
            },
            CfdValue::Enum(v) => Value::Enum {
                type_name: v.enum_name.to_string(),
                value: u32::try_from(v.value).map_err(|_| "enum outside u32 range")?,
            },
            CfdValue::Ref(key) => {
                let CftValueType::RecordRef(ty) = ty else {
                    return Err("reference type mismatch".into());
                };
                return self
                    .records
                    .iter()
                    .find(|((actual, k), _)| {
                        k == key.as_str() && self.contract.schema().is_assignable(actual, ty)
                    })
                    .map(|(_, id)| *id)
                    .ok_or_else(|| format!("missing reference {ty}::{key}"));
            }
            CfdValue::Object(object) => {
                let id = self.push(Value::None);
                let meta = self
                    .contract
                    .schema()
                    .resolve_type(object.actual_type())
                    .ok_or("unknown data type")?;
                let field_types: Vec<_> = meta
                    .all_fields()
                    .map(|f| (f.name.to_string(), f.value_type.clone()))
                    .collect();
                let mut fields = Vec::new();
                for (name, ty) in field_types {
                    let v = object.field(&name).ok_or("missing data field")?;
                    fields.push((name, self.value(v, &ty, Some(id))?));
                }
                self.values[id as usize] = Value::Object {
                    type_name: object.actual_type().into(),
                    key: None,
                    fields,
                    bases: Vec::new(),
                    };
                return Ok(id);
            }
            CfdValue::Array(items) => {
                let CftValueType::Array(inner) = ty else {
                    return Err("array type mismatch".into());
                };
                Value::Array(
                    items
                        .iter()
                        .map(|v| self.value(v, inner, owner))
                        .collect::<Result<_, _>>()?,
                )
            }
            CfdValue::Dict(items) => {
                let CftValueType::Dict(_, inner) = ty else {
                    return Err("dictionary type mismatch".into());
                };
                let mut entries = DictionaryValue::with_capacity(items.len());
                for (k, v) in items {
                    let scalar = match k {
                        CfdDictKey::Bool(v) => ScalarKey::Bool(*v),
                        CfdDictKey::String(v) => ScalarKey::String(v.clone()),
                        CfdDictKey::Int(v) => ScalarKey::Int(
                            i32::try_from(*v).map_err(|_| "dictionary key outside i32 range")?,
                        ),
                        CfdDictKey::Enum(v) => ScalarKey::Enum {
                            type_name: v.enum_name.to_string(),
                            value: u32::try_from(v.value).map_err(|_| "invalid enum key")?,
                        },
                    };
                    let key = match k {
                        CfdDictKey::Bool(v) => Value::Bool(*v),
                        CfdDictKey::String(v) => Value::String(v.clone()),
                        CfdDictKey::Int(v) => Value::Int(
                            i32::try_from(*v).map_err(|_| "dictionary key outside i32 range")?,
                        ),
                        CfdDictKey::Enum(v) => Value::Enum {
                            type_name: v.enum_name.to_string(),
                            value: u32::try_from(v.value).map_err(|_| "invalid enum key")?,
                        },
                    };
                    let key = self.push(key);
                    let value = self.value(v, inner, owner)?;
                    if entries.insert(scalar, (key, value)).is_some() {
                        return Err("duplicate dictionary key".into());
                    }
                }
                Value::Dict(entries)
            }
        };
        let id = self.push(next);
        match value {
            CfdValue::Function(function) => {
                self.function_imports.insert(id, function.imports.clone());
                if let Some(location) = &function.location {
                    self.function_locations.insert(id, location.clone());
                }
            }
            CfdValue::FormattedString(template) => {
                self.function_imports.insert(id, template.imports.clone());
                if let Some(location) = &template.location {
                    self.function_locations.insert(id, location.clone());
                }
            }
            _ => {}
        }
        if matches!(value,CfdValue::Function(function) if function.from_default)
            || matches!(value,CfdValue::FormattedString(template) if template.from_default)
        {
            self.contract_values.insert(id);
        }
        Ok(id)
    }
}

/// 从编译期常量提取字典键的归一化表示；其余类型不是合法键。
fn const_scalar_key(value: &crate::schema::CftStaticValue) -> Result<ScalarKey, String> {
    use crate::schema::CftStaticValue as C;
    Ok(match value {
        C::Bool(value) => ScalarKey::Bool(*value),
        C::Int(value) => {
            ScalarKey::Int(i32::try_from(*value).map_err(|_| "constant int outside i32")?)
        }
        C::String(value) => ScalarKey::String(value.clone()),
        C::Enum {
            enum_name, value, ..
        } => ScalarKey::Enum {
            type_name: enum_name.to_string(),
            value: u32::try_from(*value).map_err(|_| "invalid constant enum")?,
        },
        _ => return Err("invalid dictionary key".into()),
    })
}
