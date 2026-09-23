//! 字节码扩展与删除共用位置映射，源码位置和控制流附表随指令一起更新。
use super::bytecode::{Instruction, Opcode, Program};

impl Program {
    /// 每次回调生成一条原指令的替换序列；序列内分支仍使用原程序坐标。
    /// 删除目标映射到其后第一条保留指令，扩展目标映射到替换序列入口。
    pub(crate) fn rewrite_instructions(
        &mut self,
        mut rewrite: impl FnMut(
            &mut Self,
            usize,
            Instruction,
            &mut Vec<Instruction>,
        ) -> Result<(), String>,
    ) -> Result<bool, String> {
        let original = self.instructions.clone();
        let mut instructions = Vec::with_capacity(original.len());
        let mut spans = Vec::with_capacity(original.len());
        let mut positions = Vec::with_capacity(original.len() + 1);
        let mut changed = false;
        for (pc, instruction) in original.into_iter().enumerate() {
            let start = instructions.len();
            positions.push(u32::try_from(start).map_err(|_| "改写程序过大")?);
            rewrite(self, pc, instruction, &mut instructions)?;
            changed |= instructions[start..] != [instruction];
            spans.resize(instructions.len(), self.spans[pc]);
        }
        if !changed {
            return Ok(false);
        }
        positions.push(u32::try_from(instructions.len()).map_err(|_| "改写程序过大")?);
        let relocate = |target: u32| positions.get(target as usize).copied().ok_or("改写跳转越界");
        for instruction in &mut instructions {
            if let Some(opcode @ (Opcode::Jump | Opcode::JumpFalse)) = instruction.opcode() {
                *instruction = Instruction::indexed(
                    opcode,
                    instruction.a(),
                    relocate(instruction.index())?,
                ).with_flags(instruction.flags());
            }
        }
        let targets = self.for_sites.iter().map(|site| relocate(site.target)).collect::<Result<Vec<_>, _>>()?;
        for (site, target) in self.for_sites.iter_mut().zip(targets) {
            site.target = target;
        }
        self.instructions = instructions;
        self.spans = spans;
        // 改写后的旧活跃集合已失效，任何读取方必须先重新分析。
        self.live.clear();
        Ok(true)
    }

    /// 保证指令改写后的局部活跃信息已重建；未改写时复用原分析结果。
    pub(crate) fn rewrite_with_liveness(
        &mut self,
        rewrite: impl FnMut(&mut Self, usize, Instruction, &mut Vec<Instruction>) -> Result<(), String>,
    ) -> Result<bool, String> {
        let changed = self.rewrite_instructions(rewrite)?;
        if changed { self.build_local_liveness()?; }
        Ok(changed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{schema::CftValueType, source::Span, vm::bytecode::ForSite};

    #[test]
    fn rewrite_invalidates_liveness_until_rebuilt() {
        let mut program = Program::new("live".into(), String::new(), vec![CftValueType::Int], CftValueType::Int);
        program.instructions = vec![Instruction::new(Opcode::Return, 0, 0, 0, 0)];
        program.spans = vec![Span::new(0, 1)];
        program.build_liveness().unwrap();
        assert!(program.rewrite_instructions(|_, _, instruction, output| {
            output.push(Instruction::new(Opcode::Move, 0, 0, 0, 0));
            output.push(instruction);
            Ok(())
        }).unwrap());
        assert!(program.live.is_empty());
        assert!(program.validate().is_err());
        program.build_liveness().unwrap();
        program.validate().unwrap();
    }

    #[test]
    fn rewriting_with_liveness_publishes_valid_analysis() {
        let mut program = Program::new("live".into(), String::new(), vec![CftValueType::Int], CftValueType::Int);
        program.instructions = vec![Instruction::new(Opcode::Return, 0, 0, 0, 0)];
        program.spans = vec![Span::new(0, 1)];
        program.build_liveness().unwrap();
        assert!(program.rewrite_with_liveness(|_, _, instruction, output| {
            output.push(Instruction::new(Opcode::Move, 0, 0, 0, 0));
            output.push(instruction);
            Ok(())
        }).unwrap());
        program.validate().unwrap();
    }

    #[test]
    fn invalid_branch_rewrite_preserves_original_program() {
        let mut program = Program::new("branch".into(), String::new(), vec![CftValueType::Int], CftValueType::Int);
        program.instructions = vec![Instruction::new(Opcode::Return, 0, 0, 0, 0)];
        program.spans = vec![Span::new(0, 1)];
        program.build_liveness().unwrap();
        let original = program.instructions.clone();
        let live = program.live.clone();
        assert!(program.rewrite_instructions(|_, _, _, output| {
            output.push(Instruction::indexed(Opcode::Jump, 0, 10));
            Ok(())
        }).is_err());
        assert_eq!(program.instructions, original);
        assert_eq!(program.live, live);
    }

    #[test]
    fn expansion_and_removal_relocate_branches_loops_and_source_spans() {
        let mut program = Program::new("rewrite".into(), String::new(), vec![], CftValueType::Int);
        program.instructions = vec![
            Instruction::indexed(Opcode::JumpFalse, 0, 3).with_flags(1),
            Instruction::new(Opcode::Move, 0, 1, 0, 0),
            Instruction::indexed(Opcode::Jump, 0, 1),
            Instruction::new(Opcode::Move, 0, 0, 0, 0),
            Instruction::new(Opcode::Return, 0, 0, 0, 0),
        ];
        program.spans = (0..5).map(|pc| Span::new(pc * 10, pc * 10 + 5)).collect();
        program.for_sites = vec![
            ForSite { limit: 1, target: 1, exclusive: true },
            ForSite { limit: 1, target: 3, exclusive: false },
        ];
        let source = program.spans.clone();
        assert!(program.rewrite_instructions(|_, pc, instruction, output| {
            match pc {
                1 => output.extend([instruction, Instruction::new(Opcode::ConvertFloat, 0, 0, 0, 0)]),
                3 => {},
                _ => output.push(instruction),
            }
            Ok(())
        }).unwrap());
        assert_eq!(program.instructions[0].index(), 4);
        assert_eq!(program.instructions[0].flags(), 1);
        assert_eq!(program.instructions[3].index(), 1);
        assert_eq!(program.for_sites[0].target, 1);
        assert_eq!(program.for_sites[1].target, 4);
        assert_eq!(program.spans, [source[0], source[1], source[1], source[2], source[4]]);
    }
}
