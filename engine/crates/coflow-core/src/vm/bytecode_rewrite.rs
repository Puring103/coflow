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
        for site in &mut self.for_sites {
            site.target = relocate(site.target)?;
        }
        self.instructions = instructions;
        self.spans = spans;
        // 调用者必须根据改写阶段重建活跃性或分配寄存器，再使用分析结果。
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{schema::CftValueType, source::Span, vm::bytecode::ForSite};

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
