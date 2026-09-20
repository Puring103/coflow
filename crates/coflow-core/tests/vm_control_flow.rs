use coflow_core::{
    schema::CftValueType as Ty,
    source::Span,
    vm::bytecode::{Constant, ForSite, IndexSite, Instruction as I, IterNextSite, Opcode as O, Program},
};

fn program(parameters: Vec<Ty>, registers: Vec<Ty>, instructions: Vec<I>) -> Program {
    let mut program = Program::new("control-flow".into(), String::new(), parameters, Ty::Int);
    program.registers = registers;
    program.spans = vec![Span { start: 0, end: 0 }; instructions.len()];
    program.instructions = instructions;
    program
}

#[test]
fn range_entry_liveness_follows_exit_pc_not_descriptor_index() {
    let mut p = program(vec![Ty::Int; 3], vec![Ty::Int; 4], vec![
        I::indexed(O::ForPrep, 0, 0),
        I::indexed(O::Constant, 3, 1).with_flags(1),
        I::new(O::Return, 3, 0, 0, 0),
        I::new(O::Return, 2, 0, 0, 0),
    ]);
    p.for_sites.push(ForSite { limit: 1, target: 3, exclusive: true });
    p.build_liveness().unwrap();
    assert_eq!(p.live[0], vec![0, 1, 2]);
    p.allocate_registers().unwrap();
    p.validate().unwrap();
    assert_eq!(p.for_sites[0].target, 3);
}

#[test]
fn constant_index_tracks_receiver_and_preserves_descriptor_during_allocation() {
    let mut p = program(vec![Ty::Array(Box::new(Ty::Int))],
        vec![Ty::Array(Box::new(Ty::Int)), Ty::Int, Ty::Int], vec![
            I::indexed(O::Constant, 1, 9).with_flags(1),
            I::indexed(O::Index, 2, 7).with_flags(1),
            I::new(O::Return, 2, 0, 0, 0),
        ]);
    p.index_consts = vec![IndexSite { receiver: 0, key: Constant::Int(0) }; 8];
    p.build_liveness().unwrap();
    assert_eq!(p.live[1], vec![0]);
    p.allocate_registers().unwrap();
    p.validate().unwrap();
    assert_eq!(p.instructions[1].index(), 7);
    assert_eq!(p.index_consts[7].receiver, 0);
}

#[test]
fn iterator_inputs_remain_live_when_output_reuses_an_input_slot() {
    let mut p = program(vec![Ty::Array(Box::new(Ty::Int)), Ty::Int],
        vec![Ty::Array(Box::new(Ty::Int)), Ty::Int, Ty::Int], vec![
            I::indexed(O::IterNext, 0, 0),
            I::new(O::Return, 2, 0, 0, 0),
        ]);
    p.iter_nexts.push(IterNextSite { collection: 0, counter: 1, key: 1, value: 2 });
    p.build_liveness().unwrap();
    assert_eq!(p.live[0], vec![0, 1]);
    p.validate().unwrap();
}
