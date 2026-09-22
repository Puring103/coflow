//! 离线布局实验使用的候选编码，生产映像仅使用定长指令。
use coflow_core::vm::bytecode::{Instruction, Opcode};
/// 紧凑候选编码保留相同逻辑指令；高操作数通过标记和完整字扩展。
/// 两种编码共用指令语义，便于按实际程序测量总体积与解码成本。
pub fn encode_compact(instructions: &[Instruction]) -> Vec<u8> {
    let mut bytes = Vec::new();
    for instruction in instructions {
        if instruction.a() <= 255
            && instruction.b() <= 255
            && instruction.c() <= 255
            && instruction.flags() == 0
        {
            bytes.extend_from_slice(&[
                instruction.to_le_bytes()[0],
                instruction.a() as u8,
                instruction.b() as u8,
                instruction.c() as u8,
            ]);
        } else {
            bytes.extend_from_slice(&[255, 0, 0, 0]);
            bytes.extend_from_slice(&instruction.to_le_bytes());
        }
    }
    bytes
}
pub fn decode_compact(bytes: &[u8]) -> Result<Vec<Instruction>, String> {
    let mut instructions = Vec::new();
    let mut position = 0;
    while position < bytes.len() {
        let word = bytes
            .get(position..position + 4)
            .ok_or("截断的字节码指令")?;
        position += 4;
        let instruction = if word[0] == 255 {
            if word[1..] != [0, 0, 0] {
                return Err("无效的扩展标记".into());
            }
            let extended = bytes.get(position..position + 8).ok_or("截断的扩展指令")?;
            position += 8;
            Instruction::from_le_bytes(extended.try_into().map_err(|_| "无效的扩展指令")?)
        } else {
            Instruction::new(
                Opcode::from_byte(word[0]).ok_or("未知操作码")?,
                u16::from(word[1]),
                u16::from(word[2]),
                u16::from(word[3]),
                0,
            )
        };
        instruction.opcode().ok_or("未知操作码")?;
        instructions.push(instruction);
    }
    Ok(instructions)
}

