//! 字节码控制流、活跃集合、寄存器分配和结构验证。
use super::bytecode::*;
impl Program {
    pub(crate) fn branch_target(&self, instruction: Instruction) -> Result<Option<usize>, String> {
        let target = match instruction.opcode().ok_or("未知操作码")? {
            Opcode::Jump | Opcode::JumpFalse => instruction.index() as usize,
            Opcode::ForPrep | Opcode::ForLoop => {
                self.for_sites
                    .get(instruction.index() as usize)
                    .ok_or("区间循环附表越界")?
                    .target as usize
            }
            _ => return Ok(None),
        };
        if target >= self.instructions.len() {
            return Err("跳转越界".into());
        }
        Ok(Some(target))
    }

    /// 多目的指令必须完整报告写集合，供寄存器分配和常量失效共用。
    pub(crate) fn written_registers(&self, instruction: Instruction) -> Result<Vec<Register>, String> {
        Ok(match instruction.opcode().ok_or("未知操作码")? {
            Opcode::Jump | Opcode::JumpFalse | Opcode::Return | Opcode::Iteration
                | Opcode::ForPrep => Vec::new(),
            Opcode::IterNext => {
                let site = self.iter_nexts.get(instruction.index() as usize).ok_or("迭代附表越界")?;
                vec![site.key, site.value]
            }
            _ => vec![instruction.a()],
        })
    }

    pub(crate) fn block_leaders(&self) -> Result<std::collections::BTreeSet<usize>, String> {
        let mut leaders = std::collections::BTreeSet::from([0]);
        for (pc, instruction) in self.instructions.iter().copied().enumerate() {
            let target = self.branch_target(instruction)?;
            if let Some(target) = target {
                leaders.insert(target);
            }
            if (target.is_some() || instruction.opcode() == Some(Opcode::Return))
                && pc + 1 < self.instructions.len()
            {
                leaders.insert(pc + 1);
            }
        }
        Ok(leaders)
    }

    /// 指令和附表的读集合只有一份定义；数据流算法不重新解释操作码。
    pub(crate) fn read_registers(&self, instruction: Instruction) -> Result<Vec<Register>, String> {
        let opcode = instruction.opcode().ok_or("未知操作码")?;
        let index = instruction.index() as usize;
        let mut next = std::collections::BTreeSet::new();
        match opcode {
            Opcode::Build => {
                next.extend(self.builders.get(index).ok_or("构造操作越界")?.inputs());
            }
            Opcode::Move
            | Opcode::Field
            | Opcode::Unary
            | Opcode::ConvertFloat
            | Opcode::IsType
            | Opcode::IsSome
            | Opcode::ReadTemplate
            | Opcode::Length => {
                next.insert(instruction.b());
            }
            Opcode::Index
            | Opcode::IndexArray
            | Opcode::IndexDict
            | Opcode::IndexString => {
                if instruction.flags() == 0 {
                    next.extend([instruction.b(), instruction.c()]);
                } else {
                    next.insert(
                        self.index_consts.get(index).ok_or("索引附表越界")?.receiver,
                    );
                }
            }
            Opcode::Binary
            | Opcode::IntBinary
            | Opcode::FloatBinary
            | Opcode::IteratorValue => {
                next.extend([instruction.b(), instruction.c()]);
            }
            Opcode::SelfField => {
                // 读 owner（非寄存器），写字段值到目标寄存器；字段槽不是寄存器。
            }
            Opcode::LoadFixed | Opcode::LoadHost => {}
            Opcode::IntBinaryImmediate => {
                next.insert(instruction.a());
            }
            Opcode::ForPrep | Opcode::ForLoop => {
                let site = self.for_sites.get(index).ok_or("区间循环附表越界")?;
                next.insert(instruction.a());
                next.insert(site.limit);
            }
            Opcode::IterNext => {
                let site = self.iter_nexts.get(index).ok_or("迭代附表越界")?;
                // 输入与输出可以重叠，读集合独立于写集合。
                next.extend([site.collection, site.counter]);
            }
            Opcode::JumpFalse | Opcode::Return => {
                next.insert(instruction.a());
            }
            Opcode::Call => {
                let site = self.calls.get(index).ok_or("调用附表越界")?;
                next.insert(site.target);
                next.extend(
                    self.operands(site.arguments_start, site.arguments_len)
                        .ok_or("调用操作数范围越界")?,
                );
            }
            Opcode::CallDirect => {
                let site = self.direct_calls.get(index).ok_or("直接调用附表越界")?;
                next.extend(
                    self.operands(site.arguments_start, site.arguments_len)
                        .ok_or("直接调用操作数范围越界")?,
                );
            }
            Opcode::Closure => {
                let site = self.closures.get(index).ok_or("闭包附表越界")?;
                next.extend(&site.captures);
                next.extend(site.owner);
            }
            Opcode::Format => {
                for part in self.formats.get(index).ok_or("格式计划越界")? {
                    if let FormatPart::Value(register) = part {
                        next.insert(*register);
                    }
                }
            }
            Opcode::Array | Opcode::Dictionary => {
                next.extend(self.collections.get(index).ok_or("集合附表越界")?);
            }
            Opcode::Object => {
                if instruction.flags() == 0 {
                    next.insert(instruction.a());
                    next.extend(
                        self.objects
                            .get(index)
                            .ok_or("对象附表越界")?
                            .fields
                            .iter()
                            .map(|(_, r)| *r),
                    );
                }
            }
            Opcode::Builtin => {
                let site = self.builtins.get(index).ok_or("内建附表越界")?;
                next.insert(site.receiver);
                next.extend(
                    self.operands(site.arguments_start, site.arguments_len)
                        .ok_or("内建操作数范围越界")?,
                );
            }
            _ => {}
        }
        Ok(next.into_iter().collect())
    }
    /// 在控制流图上反向求活跃值；暂停调用使用调用点的 live-in。
    pub fn build_liveness(&mut self) -> Result<(), String> {
        for closure in &mut self.closures { std::sync::Arc::make_mut(&mut closure.program).build_liveness()?; }
        self.build_local_liveness()
    }
    pub(crate) fn build_local_liveness(&mut self) -> Result<(), String> {
        use std::collections::{BTreeSet, VecDeque};
        let count = self.instructions.len();
        let mut reads = Vec::with_capacity(count);
        let mut writes = Vec::with_capacity(count);
        let mut successors = Vec::with_capacity(count);
        let mut predecessors = vec![Vec::new(); count];
        for (pc, instruction) in self.instructions.iter().copied().enumerate() {
            reads.push(self.read_registers(instruction)?);
            writes.push(self.written_registers(instruction)?);
            let mut next = Vec::new();
            if !matches!(instruction.opcode(), Some(Opcode::Jump | Opcode::Return)) && pc + 1 < count { next.push(pc + 1); }
            if let Some(target) = self.branch_target(instruction)? { if !next.contains(&target) { next.push(target); } }
            for target in &next { predecessors.get_mut(*target).ok_or("控制流目标越界")?.push(pc); }
            successors.push(next);
        }
        let mut live = vec![BTreeSet::new(); count];
        let mut pending = (0..count).rev().collect::<VecDeque<_>>();
        let mut queued = vec![true; count];
        let mut steps = 0u64;
        // 仅重新处理活跃集合发生变化的前驱，避免循环 CFG 的反复全表扫描。
        while let Some(pc) = pending.pop_front() {
            queued[pc] = false;
            steps += 1;
            if steps > 10_000_000 { return Err("寄存器存活分析工作量超限".into()); }
            let mut next = BTreeSet::new();
            for target in &successors[pc] { next.extend(live[*target].iter().copied()); }
            for register in &writes[pc] { next.remove(register); }
            next.extend(reads[pc].iter().copied());
            if live[pc] != next {
                live[pc] = next;
                for predecessor in &predecessors[pc] {
                    if !queued[*predecessor] { queued[*predecessor] = true; pending.push_back(*predecessor); }
                }
            }
        }
        self.live = live.into_iter().map(|registers| registers.into_iter().collect()).collect();
        Ok(())
    }
    /// 用 CFG 存活信息形成保守区间，复用类型相同且不重叠的槽；参数保持起始 ABI。
    pub fn allocate_registers(&mut self) -> Result<(), String> {
        self.allocate_with_liveness(true)
    }
    /// IR 降低已验证当前活跃集合，分配前无需重复计算同一控制流。
    pub(crate) fn allocate_analyzed_registers(&mut self) -> Result<(), String> {
        self.allocate_with_liveness(false)
    }
    fn allocate_with_liveness(&mut self, rebuild: bool) -> Result<(), String> {
        for closure in &mut self.closures {
            std::sync::Arc::make_mut(&mut closure.program).allocate_with_liveness(rebuild)?;
        }
        if rebuild { self.build_local_liveness()?; }
        let mut intervals = vec![(usize::MAX, 0); self.registers.len()];
        for (pc, live) in self.live.iter().enumerate() {
            for register in live {
                let range = &mut intervals[usize::from(*register)];
                range.0 = range.0.min(pc);
                range.1 = range.1.max(pc);
            }
            let instruction = self.instructions[pc];
            for register in self.written_registers(instruction)? {
                let range = intervals
                    .get_mut(usize::from(register))
                    .ok_or("寄存器越界")?;
                range.0 = range.0.min(pc);
                range.1 = range.1.max(pc);
            }
        }
        let mut mapping = vec![0u16; self.registers.len()];
        let mut types = self.parameters.clone();
        let mut ends = Vec::new();
        for (index, range) in intervals.iter_mut().enumerate().take(self.parameters.len()) {
            range.0 = 0;
            mapping[index] = index as u16;
            ends.push(range.1);
        }
        let mut order = (self.parameters.len()..self.registers.len()).collect::<Vec<_>>();
        order.sort_by_key(|index| intervals[*index].0);
        for index in order {
            let (start, end) = intervals[index];
            let slot = types.iter().enumerate().find_map(|(slot, ty)| {
                if *ty == self.registers[index] && ends[slot] < start {
                    Some(slot)
                } else {
                    None
                }
            });
            let slot = slot.unwrap_or_else(|| {
                types.push(self.registers[index].clone());
                ends.push(0);
                types.len() - 1
            });
            mapping[index] = slot as u16;
            ends[slot] = end;
        }
        let map = |register: Register| mapping[usize::from(register)];
        for instruction in &mut self.instructions {
            let op = instruction.opcode().ok_or("未知操作码")?;
            let (mut a, mut b, mut c) = (instruction.a(), instruction.b(), instruction.c());
            if !matches!(op, Opcode::Jump | Opcode::Iteration | Opcode::IterNext) {
                a = map(a);
            }
            match op {
                Opcode::Move
                | Opcode::Field
                | Opcode::Unary
                | Opcode::ConvertFloat
                | Opcode::IsType
                | Opcode::IsSome
                | Opcode::ReadTemplate
                | Opcode::Length => b = map(b),
                Opcode::Index
                | Opcode::IndexArray
                | Opcode::IndexDict
                | Opcode::IndexString
                    if instruction.flags() == 0 =>
                {
                    b = map(b);
                    c = map(c);
                }
                Opcode::Binary
                | Opcode::IntBinary
                | Opcode::FloatBinary
                | Opcode::IteratorValue => {
                    b = map(b);
                    c = map(c);
                }
                _ => {}
            }
            *instruction = Instruction::new(op, a, b, c, instruction.flags());
        }
        for site in &mut self.calls {
            site.target = map(site.target);
        }
        for operation in &mut self.builders {
            *operation = operation.map(|r| Ok(map(r)))?;
        }
        for site in &mut self.closures {
            for r in &mut site.captures {
                *r = map(*r);
            }
            site.owner = site.owner.map(map);
        }
        for site in &mut self.objects {
            for (_, r) in &mut site.fields {
                *r = map(*r);
            }
        }
        for site in &mut self.collections {
            for r in site {
                *r = map(*r);
            }
        }
        for plan in &mut self.formats {
            for part in plan {
                if let FormatPart::Value(register) = part {
                    *register = map(*register);
                }
            }
        }
        for site in &mut self.builtins {
            site.receiver = map(site.receiver);
        }
        for register in &mut self.operands {
            *register = map(*register);
        }
        for site in &mut self.for_sites {
            site.limit = map(site.limit);
        }
        for site in &mut self.iter_nexts {
            site.collection = map(site.collection);
            site.counter = map(site.counter);
            site.key = map(site.key);
            site.value = map(site.value);
        }
        for site in &mut self.index_consts {
            site.receiver = map(site.receiver);
        }
        self.registers = types;
        self.build_local_liveness()
    }
    /// 编译结束时检查附表和控制流边界；可信契约加载无需再运行源码类型检查。
    pub fn validate(&self) -> Result<(), String> {
        self.validate_local()?;
        for site in &self.closures { site.program.validate()?; }
        Ok(())
    }
    pub(crate) fn validate_local(&self) -> Result<(), String> {
        if self.instructions.is_empty() || self.instructions.len() != self.spans.len() {
            return Err("程序缺少指令或源码映射".into());
        }
        if self.registers.len() > usize::from(u16::MAX) + 1
            || self.parameters.len() > self.registers.len()
        {
            return Err("无效的寄存器窗口".into());
        }
        if self.live.len() != self.instructions.len() {
            return Err("程序缺少完整存活信息".into());
        }
        for live in &self.live {
            if live.iter().any(|r| usize::from(*r) >= self.registers.len())
                || live.windows(2).any(|r| r[0] >= r[1])
            {
                return Err("无效的存活寄存器集合".into());
            }
        }
        if self.live.first().is_some_and(|live| {
            live.iter()
                .any(|r| usize::from(*r) >= self.parameters.len())
        }) {
            return Err("控制流存在未初始化寄存器读取".into());
        }
        if !self.registers.starts_with(&self.parameters) {
            return Err("参数槽类型与签名不一致".into());
        }
        let register = |r: Register| {
            if usize::from(r) < self.registers.len() {
                Ok(())
            } else {
                Err("寄存器越界".to_string())
            }
        };
        for instruction in &self.instructions {
            let opcode = instruction.opcode().ok_or("未知操作码")?;
            let index = instruction.index() as usize;
            let valid_flags = match opcode {
                Opcode::Unary => instruction.flags() <= 2,
                Opcode::Binary => instruction.flags() <= 17 || instruction.flags() == 0x80,
                Opcode::IntBinary => instruction.flags() <= 17,
                Opcode::IntBinaryImmediate => instruction.flags() & 0x7f <= 17,
                Opcode::FloatBinary => matches!(instruction.flags(), 0..=3 | 6..=12),
                Opcode::Constant => instruction.flags() <= 3,
                Opcode::Index
                | Opcode::IndexArray
                | Opcode::IndexDict
                | Opcode::IndexString => instruction.flags() <= 1,
                Opcode::Object | Opcode::ForPrep | Opcode::ForLoop => instruction.flags() <= 1,
                _ => instruction.flags() == 0,
            };
            if !valid_flags {
                return Err("无效的指令操作标志".into());
            }
            if !matches!(opcode, Opcode::Jump | Opcode::Iteration | Opcode::IterNext) {
                register(instruction.a())?;
            }
            let valid = match opcode {
                Opcode::Constant => {
                    if instruction.flags() == 0 {
                        index < self.constants.len()
                    } else {
                        true
                    }
                }
                Opcode::Capture => index < self.captures.len(),
                Opcode::Jump | Opcode::JumpFalse => index < self.instructions.len(),
                Opcode::ForPrep | Opcode::ForLoop => {
                    let site = self.for_sites.get(index).ok_or("区间循环附表越界")?;
                    register(instruction.a())?;
                    register(site.limit)?;
                    (site.target as usize) < self.instructions.len()
                }
                Opcode::IterNext => {
                    let site = self.iter_nexts.get(index).ok_or("迭代附表越界")?;
                    register(site.collection)?;
                    register(site.counter)?;
                    register(site.key)?;
                    register(site.value)?;
                    true
                }
                Opcode::SelfField => {
                    // C 是字段槽（编译期已限制在 u16），不是寄存器。
                    true
                }
                Opcode::Call => index < self.calls.len(),
                Opcode::CallDirect => index < self.direct_calls.len(),
                Opcode::Build => {
                    let operation = self.builders.get(index).ok_or("构造操作越界")?;
                    for input in operation.inputs() { register(input)?; }
                    true
                }
                Opcode::Closure => index < self.closures.len(),
                Opcode::Object => index < self.objects.len(),
                Opcode::Format => {
                    for part in self.formats.get(index).ok_or("格式计划越界")? { if let FormatPart::Value(value) = part { register(*value)?; } }
                    true
                }
                Opcode::Array | Opcode::Dictionary => {
                    index < self.collections.len()
                }
                Opcode::Builtin => index < self.builtins.len(),
                Opcode::Reference => index < self.names.len(),
                // 固定槽属于 Runtime 映像，Program 自身只验证编码宽度。
                Opcode::LoadFixed => true,
                Opcode::LoadHost => index < self.names.len(),
                Opcode::IntBinaryImmediate => true,
                Opcode::IsType => {
                    register(instruction.b())?;
                    usize::from(instruction.c()) < self.names.len()
                }
                Opcode::Field => {
                    register(instruction.b())?;
                    true
                }
                Opcode::Index
                | Opcode::IndexArray
                | Opcode::IndexDict
                | Opcode::IndexString => {
                    if instruction.flags() == 0 {
                        register(instruction.b())?;
                        register(instruction.c())?;
                    } else {
                        let site = self.index_consts.get(index).ok_or("索引附表越界")?;
                        register(site.receiver)?;
                        if !matches!(site.key, Constant::Int(_)) {
                            return Err("内联索引键不是 int".into());
                        }
                    }
                    true
                }
                Opcode::Binary
                | Opcode::IntBinary
                | Opcode::FloatBinary
                | Opcode::IteratorValue => {
                    register(instruction.b())?;
                    register(instruction.c())?;
                    true
                }
                Opcode::Move
                | Opcode::Unary
                | Opcode::ConvertFloat
                | Opcode::IsSome
                | Opcode::ReadTemplate
                | Opcode::Length => {
                    register(instruction.b())?;
                    true
                }
                Opcode::SelfValue | Opcode::Return | Opcode::Iteration => true,
            };
            if !valid {
                return Err("程序附表或跳转目标越界".into());
            }
        }
        for site in &self.calls {
            register(site.target)?;
            for r in self
                .operands(site.arguments_start, site.arguments_len)
                .ok_or("调用操作数范围越界")?
            {
                register(*r)?;
            }
        }
        for site in &self.direct_calls {
            for r in self
                .operands(site.arguments_start, site.arguments_len)
                .ok_or("直接调用操作数范围越界")?
            {
                register(*r)?;
            }
        }
        for site in &self.closures {
            if site.captures.len() != site.program.captures.len() {
                return Err("闭包捕获布局不匹配".into());
            }
            for r in &site.captures {
                register(*r)?;
            }
            if let Some(owner) = site.owner {
                register(owner)?;
            }
        }
        for site in &self.objects {
            for (_, r) in &site.fields {
                register(*r)?;
            }
        }
        for site in &self.collections {
            for r in site {
                register(*r)?;
            }
        }
        for site in &self.builtins {
            register(site.receiver)?;
            for r in self
                .operands(site.arguments_start, site.arguments_len)
                .ok_or("内建操作数范围越界")?
            {
                register(*r)?;
            }
        }
        Ok(())
    }
}
