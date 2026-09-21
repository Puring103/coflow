//! 解释器指令分发微基准：手工构造 Program，不经过编译器与 Runtime 堆。
//! 用于隔离"指令解码 + 分发 + 寄存器读写"自身的开销。
use crate::schema::CftValueType;
use crate::vm::bytecode::{Constant, Instruction, Opcode, Program};
use crate::vm::executor::{execute, Binding, ExecutionHost, Slot};
use crate::vm::ExecutionError;
use std::cmp::Ordering;
use std::sync::Arc;
use std::time::Instant;

type VmResult<T> = Result<T, ExecutionError>;
fn err() -> ExecutionError {
    ExecutionError::InvalidAccess("unused".into())
}

/// 最小 Host：常量直接映射，其余操作报错（基准只走标量路径）。
struct NullHost;

impl ExecutionHost for NullHost {
    fn constant(&self, value: &Constant) -> VmResult<Slot> {
        Ok(match value {
            Constant::Int(v) => Slot::Int(*v),
            Constant::Float(v) => Slot::Float(*v),
            _ => Slot::None,
        })
    }
    fn field(&self, _: Slot, _: u16) -> VmResult<Slot> {
        Err(err())
    }
    fn index(&self, _: Slot, _: Slot) -> VmResult<Slot> {
        Err(err())
    }
    fn index_array(&self, _: Slot, _: Slot) -> VmResult<Slot> { Err(err()) }
    fn index_dict(&self, _: Slot, _: Slot) -> VmResult<Slot> { Err(err()) }
    fn index_string(&self, _: Slot, _: Slot) -> VmResult<Slot> { Err(err()) }
    fn reference(&self, _: &str) -> VmResult<Slot> {
        Err(err())
    }
    fn equals(&self, _: Slot, _: Slot) -> VmResult<bool> {
        Ok(false)
    }
    fn compare(&self, _: Slot, _: Slot) -> VmResult<Option<Ordering>> {
        Ok(None)
    }
    fn concatenate(&self, _: Slot, _: Slot) -> VmResult<Slot> {
        Err(err())
    }
    fn accumulate_text(&self, _: Slot, _: Slot) -> VmResult<Slot> {
        Err(err())
    }
    fn enum_unary(&self, _: Slot) -> VmResult<Slot> {
        Err(err())
    }
    fn enum_binary(&self, _: u8, _: Slot, _: Slot) -> VmResult<Slot> {
        Err(err())
    }
    fn is_type(&self, _: Slot, _: &str) -> VmResult<bool> {
        Ok(false)
    }
    fn callable(&self, _: Slot) -> VmResult<crate::vm::executor::Callable<'_>> {
        Err(err())
    }
    fn call_host(&self, _: Slot, _: &[Slot]) -> VmResult<Slot> {
        Err(err())
    }
    fn closure(&self, _: Binding, _: bool) -> VmResult<Slot> {
        Err(err())
    }
    fn array(&self, _: Vec<Slot>) -> VmResult<Slot> {
        Err(err())
    }
    fn dictionary(&self, _: Vec<(Slot, Slot)>) -> VmResult<Slot> {
        Err(err())
    }
    fn reserve_object(&self, _: &str) -> VmResult<Slot> {
        Err(err())
    }
    fn initialize_object(&self, _: Slot, _: &str, _: Vec<(&str, Slot)>) -> VmResult<Slot> {
        Err(err())
    }
    fn object(&self, _: &str, _: Vec<(&str, Slot)>) -> VmResult<Slot> {
        Err(err())
    }
    fn template(&self, _: Slot) -> VmResult<Option<crate::vm::executor::CallBinding<'_>>> {
        Ok(None)
    }
    fn format(&self, _: &[crate::vm::bytecode::FormatPart], _: &[Slot]) -> VmResult<Slot> {
        Err(err())
    }
    fn length(&self, _: Slot) -> VmResult<usize> {
        Err(err())
    }
    fn iterator(&self, _: Slot, _: usize) -> VmResult<Slot> {
        Err(err())
    }
    fn iter_next(&self, _: Slot, _: usize) -> VmResult<(Slot, Slot)> {
        Err(err())
    }
    fn builtin(&self, _: &str, _: Slot, _: &[Slot], _: &CftValueType) -> VmResult<Slot> {
        Err(err())
    }
    fn roots(&self, _: &[Slot]) -> VmResult<()> {
        Ok(())
    }
}

/// r0 = 计数器（初值 0），r1 = 常量 1，r2 = 上限 N，r3 = 比较结果。
/// 2..6 为循环体：r0 += 1；r0 <= N ? 回边 : 出口 Return r0。
fn loop_program(iterations: i32) -> Arc<crate::vm::image::ValidatedProgram> {
    let mut program = Program::new("loop".into(), String::new(), vec![], CftValueType::Int);
    program.registers = vec![CftValueType::Int; 4];
    program.constants = vec![
        Constant::Int(1),
        Constant::Int(iterations),
        Constant::Int(0),
    ];
    let instructions = vec![
        Instruction::indexed(Opcode::Constant, 1, 0), // r1 = 1
        Instruction::indexed(Opcode::Constant, 2, 1), // r2 = N
        Instruction::indexed(Opcode::Constant, 0, 2), // r0 = 0
        // 循环起点 = 3
        Instruction::new(Opcode::Binary, 0, 0, 1, 0), // r0 = r0 + r1
        Instruction::new(Opcode::Binary, 3, 0, 2, 10), // r3 = r0 <= r2
        Instruction::indexed(Opcode::JumpFalse, 3, 8), // 假 → 出口
        Instruction::indexed(Opcode::Iteration, 0, 0),
        Instruction::indexed(Opcode::Jump, 0, 3), // 回边
        // 出口 = 8
        Instruction::new(Opcode::Return, 0, 0, 0, 0),
    ];
    program.instructions = instructions;
    program.registers[3] = CftValueType::Bool;
    program.spans = vec![Default::default(); program.instructions.len()];
    Arc::new(crate::vm::image::ValidatedProgram::new(program).expect("valid benchmark program"))
}

#[test]
fn dispatch_micro() {
    let host = NullHost;
    let binding = Binding {
        program: loop_program(1_000_000),
        owner: Slot::None,
        captures: Box::default(),
    };
    // 预热
    let result = execute(
        &host,
        &binding,
        &[],
        crate::vm::executor::Budget::new(Default::default()),
    )
    .expect("run");
    assert!(matches!(result, Slot::Int(n) if n == 1_000_001));
    let start = Instant::now();
    let _ = execute(
        &host,
        &binding,
        &[],
        crate::vm::executor::Budget::new(Default::default()),
    );
    println!(
        "dispatch micro (1M iterations, ~7 instr each): {:?}",
        start.elapsed()
    );
}

/// 连续标量执行没有安全点，溢出出口仍必须报告实际指令的源码位置。
#[test]
fn local_pc_reports_scalar_fault_after_unsynchronized_instructions() {
    let mut program = Program::new("fault".into(), String::new(), vec![CftValueType::Int], CftValueType::Int);
    program.registers.push(CftValueType::Int);
    program.instructions = vec![
        Instruction::indexed(Opcode::Constant, 1, 1).with_flags(1),
        Instruction::indexed(Opcode::Jump, 0, 2),
        Instruction::new(Opcode::IntBinary, 0, 0, 1, 0),
        Instruction::new(Opcode::Return, 0, 0, 0, 0),
    ];
    program.spans = vec![Default::default(); 4];
    program.spans[2] = crate::source::Span { start: 42, end: 47 };
    let binding = Binding {
        program: Arc::new(crate::vm::image::ValidatedProgram::new(program).unwrap()),
        owner: Slot::None,
        captures: Box::default(),
    };
    let fault = execute(&NullHost, &binding, &[Slot::Int(i32::MAX)],
        crate::vm::executor::Budget::new(Default::default())).unwrap_err();
    let ExecutionError::Fault { span, .. } = fault else { panic!("expected fault"); };
    assert_eq!(span, crate::source::Span { start: 42, end: 47 });
}
