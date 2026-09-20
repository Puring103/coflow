//! 程序发布边界：执行器只接受不可变的已验证程序，不能接受可编辑编译产物。
use super::bytecode::{Constant, Opcode, Program};
use std::sync::Arc;

/// 发布后的源码位置按连续相同区间压缩；压缩不省空间时保留稠密表示。
#[derive(Debug, Clone)]
pub(crate) enum SourceMap {
    Dense(Vec<coflow_language::source::Span>),
    Runs { entries: Vec<(usize, coflow_language::source::Span)>, len: usize },
}
impl SourceMap {
    fn new(spans: Vec<coflow_language::source::Span>) -> Self {
        let mut entries = Vec::new();
        for (pc, span) in spans.iter().copied().enumerate() {
            if entries.last().is_none_or(|(_, previous)| *previous != span) { entries.push((pc, span)); }
        }
        if entries.len() * size_of::<(usize, coflow_language::source::Span)>() < spans.len() * size_of::<coflow_language::source::Span>() {
            entries.shrink_to_fit(); Self::Runs { entries, len: spans.len() }
        } else { Self::Dense(spans) }
    }
    pub(crate) fn get(&self, pc: usize) -> Option<&coflow_language::source::Span> {
        match self {
            Self::Dense(spans) => spans.get(pc),
            Self::Runs { entries, len } if pc < *len => entries.get(entries.partition_point(|(start, _)| *start <= pc).checked_sub(1)?).map(|(_, span)| span),
            Self::Runs { .. } => None,
        }
    }
    pub(crate) fn first(&self) -> Option<&coflow_language::source::Span> { self.get(0) }
    #[cfg(test)]
    pub(crate) fn storage_bytes(&self) -> usize {
        match self { Self::Dense(spans) => spans.capacity() * size_of::<coflow_language::source::Span>(), Self::Runs { entries, .. } => entries.capacity() * size_of::<(usize, coflow_language::source::Span)>() }
    }
    fn expand(&self) -> Vec<coflow_language::source::Span> {
        match self { Self::Dense(spans) => spans.clone(), Self::Runs { len, .. } => (0..*len).map(|pc| *self.get(pc).unwrap()).collect() }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ValidatedProgram {
    pub(crate) spans: SourceMap,
    program: Arc<Program>,
    closures: Vec<Arc<ValidatedProgram>>,
    static_text: Option<Constant>,
    tail_returns: Vec<bool>,
}

impl ValidatedProgram {
    pub(crate) fn new(mut program: Program) -> Result<Self, String> {
        program.prepare_call_windows()?;
        // 外部传入的活跃集合不可信，必须从控制流和真实读写集合重新建立。
        program.build_liveness()?;
        program.validate()?;
        Self::publish(Arc::new(program))
    }

    fn publish(program: Arc<Program>) -> Result<Self, String> {
        let mut program = Arc::unwrap_or_clone(program);
        let closures: Vec<Arc<Self>> = program
            .closures
            .iter()
            .map(|site| Self::publish(site.program.clone()).map(Arc::new))
            .collect::<Result<_, _>>()?;
        let static_text = if program.tail_calls { Self::find_static_text(&program) } else { None };
        let tail_returns = program.instructions.iter().enumerate().map(|(pc, instruction)| {
            if !program.tail_calls || !matches!(instruction.opcode(), Some(Opcode::Call | Opcode::CallDirect)) { return false; }
            let mut aliases = std::collections::BTreeSet::from([instruction.a()]);
            let mut next = pc + 1;
            // 发布期证明调用结果仅经复制和无条件跳转返回；环或复杂出口保留普通调用。
            for _ in 0..1024 {
                let Some(instruction) = program.instructions.get(next) else { return false; };
                match instruction.opcode() {
                    Some(Opcode::Return) => return aliases.contains(&instruction.a()),
                    Some(Opcode::Move) => {
                        let same = aliases.contains(&instruction.b());
                        aliases.remove(&instruction.a());
                        if same { aliases.insert(instruction.a()); }
                        next += 1;
                    }
                    Some(Opcode::Jump) => next = instruction.index() as usize,
                    _ => return false,
                }
            }
            false
        }).collect();
        // 子程序也仅保存已发布版本，防止父附表继续持有未压缩的映射副本。
        for (site, closure) in program.closures.iter_mut().zip(&closures) { site.program = closure.program.clone(); }
        let spans = SourceMap::new(std::mem::take(&mut program.spans));
        Ok(Self { program: Arc::new(program), spans, closures, static_text, tail_returns })
    }

    /// 只接受无分支、无调用、无读取的常量复制链，不运行用户程序来探测纯度。
    fn find_static_text(program: &Program) -> Option<Constant> {
        if !program.parameters.is_empty() || !program.captures.is_empty() { return None; }
        let mut texts = vec![None; program.registers.len()];
        for instruction in &program.instructions {
            match instruction.opcode()? {
                Opcode::Constant => {
                    texts[instruction.a() as usize] = if instruction.flags() == 0 {
                        match program.constants.get(instruction.index() as usize)? {
                            Constant::String(text) => Some(text.clone()), _ => None,
                        }
                    } else { None };
                }
                Opcode::Move => texts[instruction.a() as usize] = texts[instruction.b() as usize].clone(),
                Opcode::Return => return texts[instruction.a() as usize].clone().map(Constant::String),
                _ => return None,
            }
        }
        None
    }

    pub(crate) fn to_editable(&self) -> Program {
        let mut program = (*self.program).clone();
        program.spans = self.spans.expand();
        for (site, closure) in program.closures.iter_mut().zip(&self.closures) { site.program = Arc::new(closure.to_editable()); }
        program
    }

    pub(crate) fn is_tail_call(&self, pc: usize) -> bool { self.tail_returns.get(pc).copied().unwrap_or(false) }

    pub(crate) fn static_text(&self) -> Option<&Constant> { self.static_text.as_ref() }

    pub(crate) fn closure(&self, index: usize) -> Option<Arc<Self>> {
        self.closures.get(index).cloned()
    }
}

impl std::ops::Deref for ValidatedProgram {
    type Target = Program;
    fn deref(&self) -> &Self::Target {
        &self.program
    }
}

#[cfg(test)]
mod tests {
    use super::SourceMap;
    use coflow_language::source::Span;
    #[test]
    fn source_map_round_trips_runs_and_dense_fault_locations() {
        for spans in [vec![Span { start: 3, end: 9 }; 100], (0..100).map(|start| Span { start, end: start + 1 }).collect()] {
            let map = SourceMap::new(spans.clone());
            for (pc, expected) in spans.iter().enumerate() { assert_eq!(map.get(pc), Some(expected)); }
            assert_eq!(map.get(spans.len()), None);
            assert_eq!(map.expand(), spans);
            assert!(map.storage_bytes() <= spans.capacity() * size_of::<Span>());
        }
        assert!(SourceMap::new(vec![]).first().is_none());
    }
}
