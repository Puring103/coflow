//! 构建期固定数据链接、快照优化和程序映像的一次性发布。
use super::{fixed, BuildDiagnostic, Contract, OptimizationProfile, ValueId, ScalarKey};
use crate::{schema::CftValueType, vm::{bytecode::{Constant, FormatPart, FunctionId, Program},
    compiler::{self, CompileContext}, executor::{self, FunctionBinding, Slot}, image::ValidatedProgram}};
use std::{collections::{BTreeMap, BTreeSet, HashMap}, sync::Arc};
use super::fixed::View as Stored;
/// 程序区只保存相对固定区的绑定，不包含实例堆或 Host 地址。
#[derive(Debug)]
pub(super) struct ProgramImage<P = ValidatedProgram> {
    pub(super) functions: BTreeMap<ValueId, FunctionId>,
    pub(super) direct: Vec<FunctionBinding<P>>,
    pub(super) programs: crate::vm::contract_programs::ContractPrograms<P>,
}
impl<P> Default for ProgramImage<P> {
    fn default() -> Self { Self { functions: BTreeMap::new(), direct: Vec::new(), programs: Default::default() } }
}
/// 链接器只读取构建期数据，不依赖执行实例、Host 绑定或动态堆。
pub(super) struct LinkContext<'a> {
    pub profile: OptimizationProfile,
    pub contract: &'a Contract,
    pub values: &'a fixed::FixedValues,
    pub constants: &'a BTreeMap<String, ValueId>,
    pub record_lookup: &'a BTreeMap<String, BTreeMap<String, ValueId>>,
    pub contract_values: &'a BTreeSet<ValueId>,
    pub function_imports: &'a BTreeMap<ValueId, BTreeMap<String, String>>,
    pub function_locations: &'a BTreeMap<ValueId, crate::CallableLocation>,
}
impl LinkContext<'_> {
    fn fixed_field(&self, id: ValueId, slot: u16) -> Option<ValueId> { self.values.field(id, usize::from(slot)) }
    fn record(&self, ty: &str, key: &str) -> Result<ValueId, String> {
        self.record_lookup.get(ty).and_then(|values| values.get(key)).copied()
            .ok_or_else(|| format!("record {ty}::{key} not found"))
    }
}
impl ProgramImage {
    pub(super) fn build(runtime: &LinkContext<'_>) -> Result<Self, BuildDiagnostic> {
        let mut state = ProgramImage::<Program>::default();
        let mut bindings = BTreeMap::new();
        let function_ids = runtime.values.callables().enumerate().map(|(index, (value, ..))| {
            u32::try_from(index).map(|index| (value, FunctionId(index))).map_err(|_| "程序区过大")
        }).collect::<Result<BTreeMap<_, _>, _>>()?;
        let mut unlinked = runtime.contract.ir().lower(runtime.profile == OptimizationProfile::Release)
        .map_err(|error| BuildDiagnostic {
            code: "FUNCTION".into(),
            source: error.path.unwrap_or_else(|| error.module.to_string()),
            message: error.message,
            span: Some((error.span.start, error.span.end)),
        })?;
        // Contract 程序只包含符号引用；发布 Runtime 前统一绑定到当前不可变快照。
        for program in unlinked.functions.values_mut() {
            link_program(runtime, Arc::make_mut(program), &function_ids)?;
        }
        for check in &mut unlinked.checks {
            link_program(runtime, Arc::make_mut(&mut check.program), &function_ids)?;
        }
        state.programs = unlinked;
        let mut programs: BTreeMap<_, Arc<Program>> = BTreeMap::new();
        for (id, source, owner, template) in runtime.values.callables() {
            let type_name = owner
                .and_then(|id| runtime.values.object_type(id))
                .map(str::to_owned);
            if runtime.contract_values.contains(&id) {
                let location = runtime
                    .function_locations
                    .get(&id)
                    .ok_or("missing contract source location")?;
                let constant_module = location.module.clone();
                let mut owner_type = type_name.clone();
                let program = loop {
                    let module = constant_module
                        .clone()
                        .or_else(|| {
                            owner_type
                                .as_ref()
                                .and_then(|name| runtime.contract.schema().resolve_type(name))
                                .map(|meta| meta.module.clone())
                        })
                        .ok_or("missing compiled program module")?;
                    let key = crate::vm::contract_programs::ProgramKey {
                        module,
                        owner: owner_type.clone(),
                        offset: location.span.start,
                    };
                    if let Some(program) = state.programs.functions.get(&key) {
                        break program.clone();
                    }
                    let previous = owner_type.clone();
                    owner_type = owner_type.and_then(|name| {
                        runtime
                            .contract
                            .schema()
                            .resolve_type(&name)
                            .and_then(|meta| meta.parent.as_ref().map(ToString::to_string))
                    });
                    if owner_type == previous || (owner_type.is_none() && constant_module.is_none())
                    {
                        return Err("missing compiled contract program".into());
                    }
                };
                bindings.insert(
                    id,
                    FunctionBinding {
                        program,
                        owner: owner.map_or(Slot::None, Slot::handle),
                        captures: Box::default(),
                    },
                );
                continue;
            }
            let imports = runtime
                .function_imports
                .get(&id)
                .cloned()
                .unwrap_or_default();
            let location = runtime.function_locations.get(&id);
            let source = location.map_or(source.as_ref(), |location| location.source.as_str());
            // 编译缓存直接借用源码与位置，不构造包含重复源码的调试字符串键。
            let key = (
                source,
                location.map(|location| (&location.module, location.span.start, location.span.end, &location.path)),
                type_name.clone(),
                template,
                imports.clone(),
            );
            let program = if let Some(program) = programs.get(&key) {
                program.clone()
            } else {
                let owner_type = type_name
                    .as_ref()
                    .and_then(|name| runtime.contract.schema().resolve_type(name))
                    .map(|meta| {
                        if meta.kind == coflow_language::cft::syntax::ast::TypeKind::Data {
                            CftValueType::Object(meta.name.clone())
                        } else {
                            CftValueType::RecordRef(meta.name.clone())
                        }
                    });
                let context = CompileContext {
                    owner: owner_type,
                    imports,
                    ..CompileContext::default()
                };
                let mut program = if template {
                    compiler::analyze_template(
                        runtime.contract.schema(),
                        source,
                        &format!("value#{id}"),
                        context,
                    )
                } else {
                    compiler::analyze(
                        runtime.contract.schema(),
                        source,
                        &format!("value#{id}"),
                        context,
                    )
                }
                .and_then(|function| {
                    function
                        .lower_optimized(runtime.profile == OptimizationProfile::Release)
                        .map_err(|message| compiler::CompileError {
                            span: function
                                .body
                                .first()
                                .map_or(crate::source::Span::default(), |node| node.span),
                            message,
                        })
                })
                .map_err(|error| BuildDiagnostic {
                    code: "FUNCTION".into(),
                    source: location
                        .and_then(|location| location.path.clone())
                        .unwrap_or_default(),
                    message: error.message,
                    span: Some((
                        error.span.start + location.map_or(0, |location| location.span.start),
                        error.span.end + location.map_or(0, |location| location.span.start),
                    )),
                })?;
                if let Some(location) = location {
                    program.locate(None, location.path.clone(), location.span.start);
                }
                link_program(runtime, &mut program, &function_ids)?;
                let program =
                    Arc::new(program);
                programs.insert(key, program.clone());
                program
            };
            bindings.insert(
                id,
                FunctionBinding {
                    program,
                    owner: owner.map_or(Slot::None, Slot::handle),
                    captures: Box::default(),
                },
            );
        }
        // 固定 owner 的专化只属于当前映像；代码预算耗尽后保留通用程序。
        // 不改写嵌套闭包，它们可能显式绑定新构造对象的 self。
        if runtime.profile == OptimizationProfile::Release {
            let mut remaining = 65_536usize;
            for binding in bindings.values_mut() {
                use crate::vm::bytecode::Opcode;
                let Slot::Handle(owner) = binding.owner else { continue; };
        let owner = owner.get();
                let count = binding.program.instructions.len();
                if count > remaining || !binding.program.instructions.iter().any(|i| matches!(i.opcode(), Some(Opcode::SelfValue | Opcode::SelfField))) { continue; }
                let mut specialized = (*binding.program).clone();
                let mut changed = false;
                for instruction in &mut specialized.instructions {
                    let id = match instruction.opcode() {
                        Some(Opcode::SelfValue) => Some(owner),
                        Some(Opcode::SelfField) => runtime.fixed_field(owner, instruction.c()),
                        _ => None,
                    };
                    if let Some(id) = id {
                        *instruction = fixed_instruction(id, instruction.a())?;
                        changed = true;
                    }
                }
                if changed {
                    fold_fixed_reads(runtime, &mut specialized)?;
                    fold_scalar_control_flow(&mut specialized)?;
                    fold_format_plans(runtime, &mut specialized)?;
                    link_direct_calls(&mut specialized, &function_ids)?;
                    binding.program = Arc::new(specialized);
                    remaining -= count;
                }
            }
        }
        // 编号在链接前统一分配，实际绑定在全部程序完成后一次发布；递归不形成 Arc 环。
        if !function_ids.keys().eq(bindings.keys()) { return Err("程序编号没有对应绑定".into()); }
        state.direct = bindings.into_values().collect();
        if runtime.profile == OptimizationProfile::Release {
            let effects = crate::vm::optimization::call_effects(&state.direct)?;
            let callees = state.direct.iter().map(|binding| binding.program.clone()).collect::<Vec<_>>();
            let mut inline_budget = 65_536;
            for binding in &mut state.direct {
                use crate::vm::bytecode::Opcode;
                if !binding.program.instructions.iter().any(|i| i.opcode() == Some(Opcode::CallDirect)) { continue; }
                let mut program = (*binding.program).clone();
                let inlined = crate::vm::optimization::inline_scalar_calls(&mut program, &callees, &mut inline_budget)?;
                if inlined { fold_scalar_control_flow(&mut program)?; fold_format_plans(runtime, &mut program)?; }
                program.build_local_liveness()?;
                let remove = program.instructions.iter().enumerate().map(|(pc, instruction)| {
                    instruction.opcode() == Some(Opcode::CallDirect)
                        && effects[program.direct_calls[instruction.index() as usize].function.0 as usize].discardable()
                        && program.live.get(pc + 1).is_some_and(|live| !live.contains(&instruction.a()))
                }).collect::<Vec<_>>();
                if inlined || remove.iter().any(|removed| *removed) {
                    compact_instructions(&mut program, &remove)?;
                    binding.program = Arc::new(program);
                }
            }
        }
        state.functions = function_ids;
        for binding in &state.direct {
            state.validate_direct_calls(runtime.contract.schema(), &binding.program)?;
        }
        for program in state.programs.functions.values() {
            state.validate_direct_calls(runtime.contract.schema(), program)?;
        }
        for check in &state.programs.checks {
            state.validate_direct_calls(runtime.contract.schema(), &check.program)?;
        }
        state.publish().map_err(BuildDiagnostic::from)
    }
}
impl ProgramImage<Program> {
    fn validate_direct_calls(
        &self,
        schema: &crate::schema::CftSchema,
        program: &Program,
    ) -> Result<(), String> {
        use crate::vm::bytecode::Opcode;
        for instruction in &program.instructions {
            if instruction.opcode() != Some(Opcode::CallDirect) {
                continue;
            }
            let site = program
                .direct_calls
                .get(instruction.index() as usize)
                .ok_or("直接调用附表越界")?;
            let target = &self
                .direct
                .get(site.function.0 as usize)
                .ok_or("直接调用程序编号越界")?
                .program;
            let arguments = program
                .operands(site.arguments_start, site.arguments_len)
                .ok_or("直接调用参数越界")?;
            if arguments.len() != target.parameters.len()
                || program.registers[instruction.a() as usize] != target.result
            {
                return Err("直接调用签名不匹配".into());
            }
            for (argument, expected) in arguments.iter().zip(&target.parameters) {
                if !schema.value_type_assignable(&program.registers[*argument as usize], expected) {
                    return Err("直接调用参数类型不匹配".into());
                }
            }
        }
        for closure in &program.closures {
            self.validate_direct_calls(schema, &closure.program)?;
        }
        Ok(())
    }

    fn publish(self) -> Result<ProgramImage, String> {
        let mut cache = HashMap::<*const Program, Arc<ValidatedProgram>>::new();
        let mut publish = |program: Arc<Program>| -> Result<Arc<ValidatedProgram>, String> {
            let key = Arc::as_ptr(&program);
            if let Some(published) = cache.get(&key) { return Ok(published.clone()); }
            let published = Arc::new(ValidatedProgram::new(Arc::unwrap_or_clone(program))?);
            cache.insert(key, published.clone());
            Ok(published)
        };
        let programs = crate::vm::contract_programs::ContractPrograms {
            functions: self.programs.functions.into_iter().map(|(key, program)| Ok((key, publish(program)?))).collect::<Result<_, String>>()?,
            checks: self.programs.checks.into_iter().map(|check| Ok(crate::vm::contract_programs::CheckProgram {
                owner: check.owner, name: check.name, module: check.module, program: publish(check.program)?,
            })).collect::<Result<_, String>>()?,
        };
        let direct = self.direct.into_iter().map(|binding| Ok(FunctionBinding {
            program: publish(binding.program)?, owner: binding.owner, captures: binding.captures,
        })).collect::<Result<_, String>>()?;
        Ok(ProgramImage { functions: self.functions, direct, programs })
    }
}
impl ProgramImage {
    pub(super) fn checks(&self) -> &[crate::vm::contract_programs::CheckProgram<ValidatedProgram>] { &self.programs.checks }
}
fn fixed_instruction(
    id: ValueId,
    destination: u16,
) -> Result<crate::vm::bytecode::Instruction, String> {
    use crate::vm::bytecode::{Instruction, Opcode};
    Ok(match Slot::from_scalar_id(id) {
        Some(Slot::None) => Instruction::new(Opcode::Constant, destination, 1, 0, 3),
        Some(Slot::Bool(value)) => Instruction::new(
            Opcode::Constant,
            destination,
            if value { 3 } else { 2 },
            0,
            3,
        ),
        Some(Slot::Int(value)) => {
            Instruction::indexed(Opcode::Constant, destination, value as u32).with_flags(1)
        }
        Some(Slot::Float(value)) => {
            Instruction::indexed(Opcode::Constant, destination, value.to_bits()).with_flags(2)
        }
        _ => Instruction::indexed(
            Opcode::LoadFixed,
            destination,
            u32::try_from(id).map_err(|_| "固定值槽超限")?,
        ),
    })
}

fn link_program(
    runtime: &LinkContext<'_>,
    program: &mut Program,
    functions: &BTreeMap<ValueId, FunctionId>,
) -> Result<(), String> {
    use crate::vm::bytecode::{Instruction, Opcode};
    program.tail_calls = runtime.profile == OptimizationProfile::Release;

    for instruction in &mut program.instructions {
        if instruction.opcode() != Some(Opcode::Reference) {
            continue;
        }
        let name = program
            .names
            .get(instruction.index() as usize)
            .ok_or_else(|| format!("{}: 引用索引越界", program.name))?;
        if name.starts_with("$host::") {
            *instruction =
                Instruction::indexed(Opcode::LoadHost, instruction.a(), instruction.index());
            continue;
        }
        let id = if let Some(name) = name.strip_prefix("$const::") {
            *runtime
                .constants
                .get(name)
                .ok_or_else(|| format!("{}: 未链接常量 {name}", program.name))?
        } else {
            let (ty, key) = name
                .rsplit_once("::")
                .ok_or_else(|| format!("{}: 无效记录引用 {name}", program.name))?;
            runtime
                .record(ty, key)
                .map_err(|error| format!("{}: {name}: {error}", program.name))?
        };
        *instruction = fixed_instruction(id, instruction.a())?;
    }
    for closure in &mut program.closures {
        link_program(runtime, Arc::make_mut(&mut closure.program), functions)?;
    }
    if runtime.profile == OptimizationProfile::Release {
        fold_fixed_reads(runtime, program)?;
        fold_scalar_control_flow(program)?;
        fold_format_plans(runtime, program)?;
        fuse_int_immediates(program)?;
    }
    link_direct_calls(program, functions)?;
    if program
        .instructions
        .iter()
        .any(|instruction| instruction.opcode() == Some(Opcode::Reference))
    {
        return Err(format!("{}: Runtime 映像仍包含未链接引用", program.name));
    }
    program.validate_local()
}

/// 仅沿同一基本块传播已链接函数身份，分支入口与所有写入均杀死旧事实。
fn link_direct_calls(
    program: &mut Program,
    functions: &BTreeMap<ValueId, FunctionId>,
) -> Result<(), String> {
    use crate::vm::bytecode::{DirectCallSite, Instruction, Opcode};
    let leaders = program.block_leaders()?;
    let writes = program
        .instructions
        .iter()
        .copied()
        .map(|instruction| program.written_registers(instruction))
        .collect::<Result<Vec<_>, _>>()?;
    let mut known = HashMap::new();
    for (pc, instruction) in program.instructions.iter_mut().enumerate() {
        if leaders.contains(&pc) {
            known.clear();
        }
        let original = *instruction;
        let linked = match original.opcode() {
            Some(Opcode::LoadFixed) => functions.get(&u64::from(original.index())).copied(),
            Some(Opcode::Move) => known.get(&original.b()).copied(),
            _ => None,
        };
        if original.opcode() == Some(Opcode::Call) {
            let site = program.calls.get(original.index() as usize).ok_or("调用附表越界")?;
            if let Some(function) = known.get(&site.target).copied() {
                let index = u32::try_from(program.direct_calls.len()).map_err(|_| "直接调用附表过大")?;
                program.direct_calls.push(DirectCallSite { function, arguments_start: site.arguments_start, arguments_len: site.arguments_len });
                *instruction = Instruction::indexed(Opcode::CallDirect, original.a(), index);
            }
        }
        for register in &writes[pc] { known.remove(register); }
        if let Some(function) = linked { known.insert(original.a(), function); }
    }
    program.build_local_liveness()?;
    // 直接调用不再需要函数值载入；只删除已证明无 Host/求值行为且结果不活跃的节点。
    let remove = program.instructions.iter().enumerate().map(|(pc, instruction)| {
        let removable = instruction.opcode() == Some(Opcode::Move)
            || (instruction.opcode() == Some(Opcode::LoadFixed)
                && functions.contains_key(&u64::from(instruction.index())));
        removable && program.live.get(pc + 1).is_none_or(|live| !live.contains(&instruction.a()))
    }).collect::<Vec<_>>();
    compact_instructions(program, &remove)?;
    let old_calls = std::mem::take(&mut program.calls);
    for instruction in &mut program.instructions {
        if instruction.opcode() == Some(Opcode::Call) {
            let site = old_calls.get(instruction.index() as usize).ok_or("调用附表越界")?.clone();
            let index = u32::try_from(program.calls.len()).map_err(|_| "调用附表过大")?;
            program.calls.push(site);
            *instruction = Instruction::indexed(Opcode::Call, instruction.a(), index);
        }
    }
    Ok(())
}

pub(super) fn fuse_int_immediates(program: &mut Program) -> Result<(), String> {
    use crate::vm::bytecode::{Instruction, Opcode};

    // 其他前驱可能绕过常量写入；只在同一基本块中融合。
    program.build_local_liveness()?;
    let leaders = program.block_leaders()?;
    let mut remove = vec![false; program.instructions.len()];
    for pc in 0..program.instructions.len().saturating_sub(1) {
        if leaders.contains(&(pc + 1)) {
            continue;
        }
        let constant = program.instructions[pc];
        let binary = program.instructions[pc + 1];
        if constant.opcode() != Some(Opcode::Constant)
            || constant.flags() != 1
            || binary.opcode() != Some(Opcode::IntBinary)
        {
            continue;
        }
        let temporary = constant.a();
        let (current, immediate_left) = if binary.b() == temporary && binary.a() == binary.c() {
            (binary.c(), true)
        } else if binary.c() == temporary && binary.a() == binary.b() {
            (binary.b(), false)
        } else {
            continue;
        };
        if current == temporary {
            continue;
        }
        if program
            .live
            .get(pc + 2)
            .is_some_and(|live| live.contains(&temporary))
        {
            continue;
        }
        let flags = binary.flags() | if immediate_left { 0x80 } else { 0 };
        program.instructions[pc + 1] = Instruction::indexed(
            Opcode::IntBinaryImmediate,
            current,
            constant.index(),
        )
        .with_flags(flags);
        remove[pc] = true;
    }
    compact_instructions(program, &remove)
}

pub(super) fn compact_instructions(program: &mut Program, remove: &[bool]) -> Result<(), String> {
    if remove.len() != program.instructions.len() {
        return Err("指令删除掩码长度不匹配".into());
    }
    if !remove.iter().any(|remove| *remove) {
        return Ok(());
    }
    program.rewrite_instructions(|_, pc, instruction, output| {
        if !remove[pc] {
            output.push(instruction);
        }
        Ok(())
    })?;
    program.build_local_liveness()
}

/// 只折叠实际成功的标量计算，随后按真实跳转边删除不可达节点。
/// 调用者须先链接所有符号，未执行分支里的非法引用仍然阻止发布。
pub(super) fn fold_scalar_control_flow(program: &mut Program) -> Result<(), String> {
    use crate::vm::bytecode::{Instruction, Opcode};
    let leaders = program.block_leaders()?;
    let mut known = HashMap::new();
    let mut remove = vec![false; program.instructions.len()];
    for pc in 0..program.instructions.len() {
        if leaders.contains(&pc) {
            known.clear();
        }
        let instruction = program.instructions[pc];
        let a = instruction.a();
        let input = |register| known.get(&register).copied();
        let result = match instruction.opcode().ok_or("未知操作码")? {
            Opcode::Constant => match instruction.flags() {
                1 => Some(Slot::Int(instruction.index() as i32)),
                2 => Some(Slot::Float(f32::from_bits(instruction.index()))),
                3 => Some(match instruction.b() { 0 => Slot::Unit, 1 => Slot::None, 2 => Slot::Bool(false), _ => Slot::Bool(true) }),
                0 => match program.constants.get(instruction.index() as usize) {
                    Some(Constant::Int(value)) => Some(Slot::Int(*value)), Some(Constant::Float(value)) => Some(Slot::Float(*value)),
                    Some(Constant::Bool(value)) => Some(Slot::Bool(*value)), Some(Constant::None) => Some(Slot::None), Some(Constant::Unit) => Some(Slot::Unit), _ => None,
                }, _ => None,
            },
            Opcode::Move => input(instruction.b()),
            Opcode::Binary | Opcode::IntBinary | Opcode::FloatBinary => input(instruction.b()).zip(input(instruction.c()))
                .and_then(|(left, right)| executor::scalar_binary(instruction.flags(), left, right)).and_then(Result::ok),
            Opcode::IntBinaryImmediate => input(a).and_then(|value| {
                let immediate = Slot::Int(instruction.index() as i32);
                let (left, right) = if instruction.flags() & 0x80 != 0 { (immediate, value) } else { (value, immediate) };
                executor::scalar_binary(instruction.flags() & 0x7f, left, right)?.ok()
            }),
            Opcode::ConvertFloat => input(instruction.b()).and_then(|value| if let Slot::Int(value) = value { Some(Slot::Float(value as f32)) } else { None }),
            Opcode::IsSome => input(instruction.b()).map(|value| Slot::Bool(value != Slot::None)),
            Opcode::Unary => input(instruction.b()).and_then(|value| match (instruction.flags(), value) {
                (0, Slot::Int(value)) => value.checked_neg().map(Slot::Int), (0, Slot::Float(value)) => Some(Slot::Float(-value)),
                (1, Slot::Bool(value)) => Some(Slot::Bool(!value)), (2, Slot::Int(value)) => Some(Slot::Int(!value)), _ => None,
            }),
            Opcode::JumpFalse => {
                if let Some(Slot::Bool(condition)) = input(a) {
                    if condition { remove[pc] = true; }
                    else { program.instructions[pc] = Instruction::indexed(Opcode::Jump, 0, instruction.index()); }
                }
                None
            }
            _ => None,
        };
        for register in program.written_registers(instruction)? { known.remove(&register); }
        if let Some(value) = result {
            let replacement = match value {
                Slot::Int(value) => Instruction::indexed(Opcode::Constant, a, value as u32).with_flags(1),
                Slot::Float(value) => Instruction::indexed(Opcode::Constant, a, value.to_bits()).with_flags(2),
                Slot::Unit => Instruction::new(Opcode::Constant, a, 0, 0, 3),
                Slot::None => Instruction::new(Opcode::Constant, a, 1, 0, 3),
                Slot::Bool(value) => Instruction::new(Opcode::Constant, a, if value { 3 } else { 2 }, 0, 3),
                _ => continue,
            };
            program.instructions[pc] = replacement; known.insert(a, value);
        }
    }
    compact_instructions(program, &remove)?;
    let mut reachable = vec![false; program.instructions.len()];
    let mut pending = vec![0];
    while let Some(pc) = pending.pop() {
        if reachable[pc] { continue; } reachable[pc] = true;
        let instruction = program.instructions[pc];
        if let Some(target) = program.branch_target(instruction)? { pending.push(target); }
        if !matches!(instruction.opcode(), Some(Opcode::Jump | Opcode::Return)) && pc + 1 < reachable.len() { pending.push(pc + 1); }
    }
    let remove = reachable.iter().map(|reachable| !reachable).collect::<Vec<_>>();
    compact_instructions(program, &remove)
}

fn fold_format_plans(runtime: &LinkContext<'_>, program: &mut Program) -> Result<(), String> {
    use crate::vm::bytecode::{Instruction, Opcode};
    let leaders = program.block_leaders()?;
    let mut known = HashMap::new();
    for pc in 0..program.instructions.len() {
        if leaders.contains(&pc) {
            known.clear();
        }
        let instruction = program.instructions[pc];
        let opcode = instruction.opcode().ok_or("未知操作码")?;
        let constant = match opcode {
            Opcode::Constant => match instruction.flags() {
                0 => program.constants.get(instruction.index() as usize).cloned(),
                1 => Some(Constant::Int(instruction.index() as i32)),
                2 => Some(Constant::Float(f32::from_bits(instruction.index()))),
                3 => match instruction.b() {
                    0 => Some(Constant::Unit),
                    1 => Some(Constant::None),
                    2 => Some(Constant::Bool(false)),
                    _ => Some(Constant::Bool(true)),
                },
                _ => None,
            },
            Opcode::Move => known.get(&instruction.b()).cloned(),
            Opcode::LoadFixed => fixed_constant(runtime.values, u64::from(instruction.index())),
            _ => None,
        };
        if opcode == Opcode::Format {
            let plan = program
                .formats
                .get_mut(instruction.index() as usize)
                .ok_or("格式计划越界")?;
            let mut merged = Vec::new();
            for part in std::mem::take(plan) {
                let part = match part {
                    FormatPart::Value(register) => {
                        let text = match known.get(&register) {
                            Some(Constant::String(text)) => Some(text.clone()),
                            Some(Constant::Int(value)) => Some(value.to_string()),
                            Some(Constant::Float(value)) => Some(value.to_string()),
                            Some(Constant::Bool(value)) => Some(value.to_string()),
                            Some(Constant::Enum { name, value }) => Some(runtime.contract.schema().resolve_enum(name).and_then(|meta| meta.variant_by_value.get(&i64::from(*value)).and_then(|index| meta.variants.get(*index))).map_or_else(|| value.to_string(), |variant| variant.name.to_string())),
                            _ => None,
                        };
                        text.map_or(FormatPart::Value(register), FormatPart::Text)
                    }
                    part => part,
                };
                if let (Some(FormatPart::Text(previous)), FormatPart::Text(text)) = (merged.last_mut(), &part) { previous.push_str(text); }
                else { merged.push(part); }
            }
            *plan = merged;
            let text = match plan.as_slice() { [] => Some(String::new()), [FormatPart::Text(text)] => Some(text.clone()), _ => None };
            if let Some(text) = text {
                let index = u32::try_from(program.constants.len()).map_err(|_| "常量区超限")?;
                program.constants.push(Constant::String(text));
                program.instructions[pc] = Instruction::indexed(Opcode::Constant, instruction.a(), index);
            }
        }
        for register in program.written_registers(instruction)? { known.remove(&register); }
        if let Some(constant) = constant { known.insert(instruction.a(), constant); }
    }
    // 静态片段不再占用寄存器；清除失去使用者的常量装载并统一重定位分支。
    program.build_local_liveness()?;
    let remove = program.instructions.iter().enumerate().map(|(pc, instruction)| {
        (matches!(instruction.opcode(), Some(Opcode::Constant | Opcode::Move))
            || (instruction.opcode() == Some(Opcode::LoadFixed)
                && !runtime.values.is_host(u64::from(instruction.index()))))
            && program.live.get(pc + 1).is_some_and(|live| !live.contains(&instruction.a()))
    }).collect::<Vec<_>>();
    compact_instructions(program, &remove)
}

fn fold_fixed_reads(runtime: &LinkContext<'_>, program: &mut Program) -> Result<(), String> {
    use crate::vm::bytecode::Opcode;

    let leaders = program.block_leaders()?;
    let writes = program
        .instructions
        .iter()
        .copied()
        .map(|instruction| program.written_registers(instruction))
        .collect::<Result<Vec<_>, _>>()?;

    let mut fixed = HashMap::<crate::vm::bytecode::Register, ValueId>::new();
    let mut keys = HashMap::<crate::vm::bytecode::Register, ScalarKey>::new();
    for (pc, instruction) in program.instructions.iter_mut().enumerate() {
        if leaders.contains(&pc) {
            fixed.clear();
            keys.clear();
        }
        let original = *instruction;
        let opcode = original.opcode().ok_or("未知操作码")?;
        let target = original.a();
        let key = match opcode {
            Opcode::Constant => match original.flags() {
                1 => Some(ScalarKey::Int(original.index() as i32)),
                3 if original.b() >= 2 => Some(ScalarKey::Bool(original.b() == 3)),
                0 => match program.constants.get(original.index() as usize) {
                    Some(Constant::Int(value)) => Some(ScalarKey::Int(*value)),
                    Some(Constant::Bool(value)) => Some(ScalarKey::Bool(*value)),
                    Some(Constant::String(value)) => Some(ScalarKey::String(value.clone())),
                    Some(Constant::Enum { name, value }) => Some(ScalarKey::Enum { type_name: name.clone(), value: *value }),
                    _ => None,
                }, _ => None,
            },
            Opcode::Move => keys.get(&original.b()).cloned(),
            _ => None,
        };
        let linked = match opcode {
            Opcode::LoadFixed => Some(u64::from(original.index())),
            Opcode::Move => fixed.get(&original.b()).copied(),
            Opcode::Field => fixed
                .get(&original.b())
                .copied()
                .and_then(|owner| runtime.fixed_field(owner, original.c())),
            Opcode::Index
            | Opcode::IndexArray
            | Opcode::IndexDict
            | Opcode::IndexString => {
                let (receiver, key) = if original.flags() == 1 {
                    let site = program.index_consts.get(original.index() as usize).ok_or("索引附表越界")?;
                    let key = match site.key { Constant::Int(value) => Some(ScalarKey::Int(value)), _ => None };
                    (site.receiver, key)
                } else { (original.b(), keys.get(&original.c()).cloned()) };
                fixed.get(&receiver).and_then(|id| runtime.values.view(*id)).and_then(|value| match (value, key.as_ref()) {
                    (Stored::Array(values), Some(ScalarKey::Int(index))) => usize::try_from(*index).ok().and_then(|index| values.get(index)),
                    (Stored::Dict(values), Some(key)) => values.get(key).map(|(_, value)| *value),
                    _ => None,
                })
            }
            _ => None,
        };
        for register in &writes[pc] {
            fixed.remove(register);
            keys.remove(register);
        }
        if let Some(key) = key {
            keys.insert(target, key);
        }
        if let Some(id) = linked {
            // Host 占位符的读取有可观察行为，不能传播为可重复使用的静态事实。
            if runtime.values.is_host(id) {
                continue;
            }
            fixed.insert(target, id);
            let key = match fixed_constant(runtime.values, id) {
                Some(Constant::Int(value)) => Some(ScalarKey::Int(value)),
                Some(Constant::Bool(value)) => Some(ScalarKey::Bool(value)),
                Some(Constant::String(value)) => Some(ScalarKey::String(value)),
                Some(Constant::Enum { name, value }) => Some(ScalarKey::Enum { type_name: name, value }),
                _ => None,
            };
            if let Some(key) = key {
                keys.insert(target, key);
            }
            *instruction = fixed_instruction(id, target)?;
        }
    }
    program.build_local_liveness()
}



/// 折叠只读取标量和文本，不为识别值种类重建对象或集合。
fn fixed_constant(values: &fixed::FixedValues, id: ValueId) -> Option<Constant> {
    if let Some(value) = values.scalar(id) {
        return Some(match value { Slot::None => Constant::None, Slot::Bool(v) => Constant::Bool(v), Slot::Int(v) => Constant::Int(v), Slot::Float(v) => Constant::Float(v), _ => return None });
    }
    if let Some(Stored::String(text)) = values.view(id) { return Some(Constant::String(text.into())); }
    values.enum_value(id).map(|(name, value)| Constant::Enum { name: name.into(), value })
}
