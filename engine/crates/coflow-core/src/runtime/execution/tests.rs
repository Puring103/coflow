use super::*;
    use crate::source::Span;
    use crate::vm::bytecode::{ForSite, Instruction as I, Opcode as O};

    #[test]
    fn failed_heap_allocation_does_not_publish_identity_or_accounting() {
        let mut heap = Heap::default();
        assert!(VmState::allocate_in_heap(&mut heap, Value::String("budget".into()), None, 0).is_err());
        assert_eq!(heap.bytes, 0);
        assert_eq!(heap.live_values, 0);
        assert!(heap.values.is_empty());
        assert!(heap.generations.is_empty());
    }

    #[test]
    fn generational_handles_reject_stale_ids_and_retire_exhausted_slots() {
        let mut heap = Heap::default();
        let allocate = |heap: &mut Heap| {
            let Slot::Handle(id) = VmState::allocate_in_heap(heap, Value::Int(1), None, usize::MAX).unwrap() else { panic!("handle") };
            id.get()
        };
        let first = allocate(&mut heap);
        let index = heap.index(first).unwrap();
        heap.remove_slot(index).unwrap();
        let second = allocate(&mut heap);
        assert_eq!(heap.index(second), Some(index));
        assert_ne!(first, second);
        assert!(heap.get(first).is_none());
        assert!(heap.get(0).is_none());
        assert!(heap.get(Slot::Int(1).scalar_id().unwrap()).is_none());
        heap.generations[index] = Heap::MAX_GENERATION;
        heap.remove_slot(index).unwrap();
        let third = allocate(&mut heap);
        assert_ne!(heap.index(third), Some(index));
        assert!(heap.get(second).is_none());
    }

    #[cfg(feature = "cft-compiler")]
    #[test]
    fn lowering_emits_typed_index_opcodes_and_preserves_results() {
        use crate::schema::{build_schema, parse_modules, CftFile, ModuleId};
        let schema = build_schema(&parse_modules([CftFile::from_source(
            ModuleId::from("typed-index"),
            r#"table Rule {
                run: fn(array: [int], dict: {int: int}, text: string) -> int => {
                    array[1] + dict[2] + text[1].len()
                };
            }"#,
        )]))
        .unwrap();
        let mut builder = RuntimeBuilder::new(Arc::new(Contract::new(schema).unwrap()));
        builder.add_text("rule: Rule {}", None);
        let runtime = builder.build().runtime.unwrap();
        let opcodes = runtime
            .code()
            .direct
            .iter()
            .flat_map(|binding| binding.program.instructions.iter())
            .filter_map(|instruction| instruction.opcode())
            .collect::<Vec<_>>();
        assert!(opcodes.contains(&O::IndexArray));
        assert!(opcodes.contains(&O::IndexDict));
        assert!(opcodes.contains(&O::IndexString));

        let rule = runtime.record("Rule", "rule").unwrap();
        let run = runtime.field(rule, "run").unwrap();
        assert!(matches!(
            runtime
                .invoke(
                    run,
                    &[
                        HostValue::Array(vec![HostValue::Int(4), HostValue::Int(5)]),
                        HostValue::Dictionary(vec![(HostValue::Int(2), HostValue::Int(7))]),
                        HostValue::String("a界".into()),
                    ],
                    ExecutionLimits::default(),
                )
                .unwrap(),
            HostValue::Int(13)
        ));
    }

    #[cfg(feature = "cft-compiler")]
    #[test]
    fn regex_cache_obeys_shared_budget_and_releases_after_failure() {
        use crate::schema::{build_schema, parse_modules, CftFile, ModuleId};
        let schema = build_schema(&parse_modules([CftFile::from_source(
            ModuleId::from("regex"),
            "table Rule { value: int = 1; }",
        )]))
        .unwrap();
        let mut builder = RuntimeBuilder::new(Arc::new(Contract::new(schema).unwrap()));
        builder.add_text("rule: Rule {}", None);
        let runtime = builder.build().runtime.unwrap();
        {
            let host = runtime
                .execution_host(ExecutionLimits {
                    max_heap_bytes: 1024,
                    ..ExecutionLimits::default()
                })
                .unwrap();
            assert!(host.regex_match("a+", "aaa").is_err());
            assert!(runtime.vm.regexes.borrow().is_empty());
        }
        {
            let host = runtime.execution_host(ExecutionLimits::default()).unwrap();
            assert!(host.regex_match(r"^\p{Greek}+$", "αβγ").unwrap());
            assert!(!host.regex_match(r"^\p{Greek}+$", "abc").unwrap());
            assert!(host.regex_match("a+", "aaa").unwrap());
            assert!(host.regex_match("[", "a").is_err());
            assert!(host.regex_match("b+", "bbb").unwrap());
            assert_eq!(runtime.vm.regexes.borrow().len(), 3);
        }
        assert_eq!(runtime.vm.regexes.borrow().capacity(), 0);
    }

    #[cfg(feature = "cft-compiler")]
    #[test]
    #[ignore = "原生分配与 GC 测量，release 单测试线程运行"]
    fn runtime_memory_probe() {
        use crate::allocation_probe;
        use crate::schema::{build_schema, parse_modules, CftFile, ModuleId};
        use std::time::Instant;
        let workloads = [
            ("transient_strings", "int", "var total: int = 0; for i in 0..20000 { total += i.string().len(); } total"),
            ("numeric_array", "[int]", "build [int] as b { for i in 0..20000 { b.append(i); } }"),
            ("optional_array", "[int?]", "build [int?] as b { for i in 0..20000 { if i % 2 == 0 { b.append(None); } else { b.append(i); } } }"),
            ("data_chain", "Node", "var node: Node = Node { value: 0 }; for i in 0..2000 { node = Node { value: i, next: node }; } node"),
            ("closure_escape", "fn() -> int", "var values: [int] = build [int] as b { for i in 0..2000 { b.append(i); } }; fn() -> int { values.sum() }"),
            ("dictionary", "{int: int}", "build {int: int} as b { for i in 0..2000 { b[i] = i; } }"),
            ("map_filter", "int", "var values: [int] = [0,1,2,3,4,5,6,7,8,9]; var total: int = 0; for i in 0..2000 { total += values.map(fn(x: int) -> int { x % 7 }).filter(fn(x: int) -> bool { x > 2 }).sum(); } total"),
            ("template", "int", "var total: int = 0; for i in 0..2000 { var value: fstring = f\"item-{i}\"; total += value.len(); } total"),
        ];
        for (workload, result_type, body) in workloads {
          for profile in [OptimizationProfile::Debug, OptimizationProfile::Release] {
            let name = format!("{workload}/{profile:?}");
            let started = Instant::now();
            let ((contract, contract_bytes), compiled) = allocation_probe::measure(|| {
                let source = format!("data Node {{ value: int; next: Node?; }} table Rule {{ run: fn() -> {result_type} => {{ {body} }}; }}");
                let modules = parse_modules([CftFile::from_source(ModuleId::from("memory"), source)]);
                let contract = Arc::new(Contract::new(build_schema(&modules).unwrap()).unwrap());
                let contract_bytes = contract.to_bytes().unwrap().len();
                (contract, contract_bytes)
            });
            println!("compile workload={name},ns={},allocations={compiled:?},contract_bytes={contract_bytes}", started.elapsed().as_nanos());
            let started = Instant::now();
            let (runtime, linked) = allocation_probe::measure(|| { let mut builder = RuntimeBuilder::new(contract); builder.optimization_profile(profile); builder.add_text("rule: Rule {}", None); builder.build().runtime.unwrap() });
            println!("link workload={name},ns={},allocations={linked:?},fixed_regions={:?}", started.elapsed().as_nanos(), runtime.values.storage_sizes());
            let programs = &runtime.code().direct;
            println!("image workload={name},programs={},instruction_bytes={},source_map_bytes={},gc_map_bytes={},operand_bytes={}", programs.len(), programs.iter().map(|binding| binding.program.instructions.capacity()*size_of::<crate::vm::bytecode::Instruction>()).sum::<usize>(), programs.iter().map(|binding| binding.program.spans.storage_bytes()).sum::<usize>(), programs.iter().map(|binding| binding.program.live.capacity()*size_of::<Vec<u16>>() + binding.program.live.iter().map(|live| live.capacity()*size_of::<u16>()).sum::<usize>()).sum::<usize>(), programs.iter().map(|binding| binding.program.operands.capacity()*size_of::<u16>()).sum::<usize>());
            let function = runtime.field(runtime.record("Rule", "rule").unwrap(), "run").unwrap();
            let (result, executed) = allocation_probe::measure(|| runtime.invoke(function, &[], ExecutionLimits::default()).unwrap());
            println!("execute workload={name},allocations={executed:?},heap={:?}", runtime.vm.heap.borrow().metrics);
            if let HostValue::Existing { value, .. } = result { runtime.release_value(value).unwrap(); }
            runtime.collect().unwrap();
            println!("released workload={name},live_values={},retained_heap_budget_bytes={}", runtime.dynamic_value_count().unwrap(), runtime.vm.heap.borrow().total_bytes());
            assert_eq!(runtime.dynamic_value_count().unwrap(), 0);
          }
        }
    }

    #[cfg(feature = "cft-compiler")]
    #[test]
    #[ignore = "与 C# 记录读取探针使用相同 1000 条记录，测量原生存活和峰值"]
    fn record_read_native_memory_probe() {
        use crate::allocation_probe;
        let source = (0..1000).map(|i| format!("h{i}: Hero {{ name: \"Hero {i}\", stats: Stats {{ health: {}, weights: [1, 2, 3] }} }}\n", i + 1)).collect::<String>() + "RuntimeSettings: RuntimeSettings {}";
        let (contract, loaded) = allocation_probe::measure(|| {
            Arc::new(
                Contract::from_bytes(include_bytes!(
                    "../../../../../runtimes/csharp/tests/integration/generated/coflow.contract"
                ))
                .unwrap(),
            )
        });
        let (runtime, linked) = allocation_probe::measure(|| {
            let mut builder = RuntimeBuilder::new(contract.clone());
            builder.add_text(&source, None);
            builder.build().runtime.unwrap()
        });
        assert_eq!(runtime.records("Character").unwrap().len(), 1000);
        println!("snapshot_native records=1000,contract={loaded:?},image_and_instance={linked:?},fixed_regions={:?}", runtime.values.storage_sizes());
        let (_, released) = allocation_probe::measure(|| {
            drop(runtime);
            drop(contract);
        });
        println!("snapshot_native released={released:?}");
        assert_eq!(
            loaded.bytes_current + linked.bytes_current + released.bytes_current,
            0
        );
    }

    #[cfg(feature = "cft-compiler")]
    #[test]
    fn direct_calls_preserve_recursion_and_branch_reassigned_function_values() {
        use crate::schema::{build_schema, parse_modules, CftFile, ModuleId};
        let schema = build_schema(&parse_modules([CftFile::from_source(ModuleId::from("calls"), r#"
            const inc: fn(int) -> int = fn(x: int) -> int { x + 1 };
            const recur: fn(int) -> int = fn(n: int) -> int { if n == 0 { 0 } else { recur(n - 1) + 1 } };
            table Rule {
                run: fn(n: int) -> int => {
                    var selected: fn(int) -> int = inc;
                    if n > 0 { selected = fn(x: int) -> int { x + 10 }; }
                    selected(n) + inc(n) + recur(n)
                };
            }
        "#)])).unwrap();
        let contract = Arc::new(Contract::new(schema).unwrap());
        for profile in [OptimizationProfile::Debug, OptimizationProfile::Release] {
            let mut builder = RuntimeBuilder::new(contract.clone());
            builder.optimization_profile(profile);
            builder.add_text("a: Rule {}", None);
            let runtime = builder.build().runtime.unwrap();
            assert!(runtime.code().direct.iter().any(|binding| binding
                .program
                .instructions
                .iter()
                .any(|instruction| instruction.opcode() == Some(O::CallDirect))));
            let function = runtime
                .field(runtime.record("Rule", "a").unwrap(), "run")
                .unwrap();
            for (input, expected) in [(0, 2), (2, 17), (8, 35)] {
                assert!(
                    matches!(runtime.invoke(function, &[HostValue::Int(input)], ExecutionLimits::default()).unwrap(),
                    HostValue::Int(value) if value == expected)
                );
            }
        }
    }

    #[cfg(feature = "cft-compiler")]
    #[test]
    fn call_windows_preserve_permuted_repeated_arguments_and_loop_targets() {
        use crate::schema::{build_schema, parse_modules, CftFile, ModuleId};
        let schema = build_schema(&parse_modules([CftFile::from_source(ModuleId::from("windows"), r#"
            const encode: fn(int, int, int) -> int = fn(a: int, b: int, c: int) -> int { a * 100 + b * 10 + c };
            table Rule {
                run: fn(a: int, b: int, c: int) -> int => {
                    var total: int = 0;
                    for index in 0..3 {
                        if index == 1 { continue; }
                        total += encode(c, b, a) + encode(a, a, c);
                    }
                    total
                };
            }
        "#)])).unwrap();
        let contract = Arc::new(Contract::new(schema).unwrap());
        for profile in [OptimizationProfile::Debug, OptimizationProfile::Release] {
            let mut builder = RuntimeBuilder::new(contract.clone());
            builder.optimization_profile(profile);
            builder.add_text("a: Rule {}", None);
            let runtime = builder.build().runtime.unwrap();
            let function = runtime
                .field(runtime.record("Rule", "a").unwrap(), "run")
                .unwrap();
            assert!(matches!(
                runtime
                    .invoke(
                        function,
                        &[HostValue::Int(1), HostValue::Int(2), HostValue::Int(3)],
                        ExecutionLimits::default()
                    )
                    .unwrap(),
                HostValue::Int(868)
            ));
        }
    }

    #[cfg(feature = "cft-compiler")]
    #[test]
    fn fixed_field_and_collection_reads_do_not_borrow_dynamic_heap() {
        use crate::schema::{build_schema, parse_modules, CftFile, ModuleId};
        let schema = build_schema(&parse_modules([CftFile::from_source(
            ModuleId::from("fixed"),
            "table Rule { values: [int] = [4, 5]; mapping: {int: int} = {1: 7}; number: int = 9; }",
        )]))
        .unwrap();
        let mut builder = RuntimeBuilder::new(Arc::new(Contract::new(schema).unwrap()));
        builder.add_text("a: Rule {}", None);
        let runtime = builder.build().runtime.unwrap();
        let record = runtime.record("Rule", "a").unwrap();
        let array = Slot::handle(runtime.field(record, "values").unwrap());
        let dictionary = Slot::handle(runtime.field(record, "mapping").unwrap());
        let host = runtime.execution_host(ExecutionLimits::default()).unwrap();
        // 故意占用动态堆写借用；任何固定读误入堆都会触发 RefCell 冲突。
        let _heap = runtime.vm.heap.borrow_mut();
        assert_eq!(host.field(Slot::handle(record), 3).unwrap(), Slot::Int(9));
        assert_eq!(host.length(array).unwrap(), 2);
        assert_eq!(host.index(array, Slot::Int(1)).unwrap(), Slot::Int(5));
        assert_eq!(host.iterator(array, 0).unwrap(), Slot::Int(4));
        assert_eq!(host.iter_next(array, 1).unwrap(), (Slot::Int(1), Slot::Int(5)));
        assert_eq!(host.length(dictionary).unwrap(), 1);
        assert_eq!(host.index(dictionary, Slot::Int(1)).unwrap(), Slot::Int(7));
        assert_eq!(host.iterator(dictionary, 0).unwrap(), Slot::Int(1));
        assert_eq!(host.iter_next(dictionary, 0).unwrap(), (Slot::Int(1), Slot::Int(7)));
    }

    #[cfg(feature = "cft-compiler")]
    #[test]
    fn pinned_early_value_does_not_accumulate_dead_heap_slots() {
        use crate::schema::{build_schema, parse_modules, CftFile, ModuleId};
        let schema = build_schema(&parse_modules([CftFile::from_source(ModuleId::from("gc"), "table Item {}")])).unwrap();
        let runtime = RuntimeBuilder::new(Arc::new(Contract::new(schema).unwrap())).build().runtime.unwrap();
        let allocate = || {
            let Slot::Handle(id) = VmState::allocate_in_heap(&mut runtime.vm.heap.borrow_mut(), Value::String("live".into()), None, 1024 * 1024).unwrap() else { panic!("heap handle"); };
        let id = id.get();
            id
        };
        let pinned = allocate();
        runtime.retain_value(pinned).unwrap();
        let stale = allocate();
        runtime.collect().unwrap();
        for _ in 0..5000 {
            let current = allocate();
            assert_ne!(current, stale);
            assert!(runtime.vm.value(stale).is_err());
            runtime.collect().unwrap();
        }
        assert!(runtime.vm.heap.borrow().values.len() <= 2);
        assert_eq!(runtime.dynamic_value_count().unwrap(), 1);
        runtime.release_value(pinned).unwrap();
        runtime.collect().unwrap();
        assert_eq!(runtime.dynamic_value_count().unwrap(), 0);
    }

    fn program(instructions: Vec<I>) -> Program {
        let mut p = Program::new(
            "fusion".into(),
            String::new(),
            vec![CftValueType::Int; 2],
            CftValueType::Int,
        );
        p.spans = vec![Span { start: 0, end: 0 }; instructions.len()];
        p.instructions = instructions;
        p.build_liveness().unwrap();
        p
    }

    #[test]
    fn scalar_folding_uses_inline_integer_payload_and_keeps_overflow() {
        let mut p = program(vec![
            I::indexed(O::Constant, 0, 10).with_flags(1),
            I::indexed(O::IntBinaryImmediate, 0, (-3i32) as u32),
            I::new(O::Return, 0, 0, 0, 0),
        ]);
        fold_scalar_control_flow(&mut p).unwrap();
        assert_eq!(p.instructions[1].opcode(), Some(O::Constant));
        assert_eq!(p.instructions[1].index(), 7);
        let mut p = program(vec![
            I::indexed(O::Constant, 0, i32::MAX as u32).with_flags(1),
            I::indexed(O::IntBinaryImmediate, 0, 1),
            I::new(O::Return, 0, 0, 0, 0),
        ]);
        fold_scalar_control_flow(&mut p).unwrap();
        assert_eq!(p.instructions[1].opcode(), Some(O::IntBinaryImmediate));
    }

    #[test]
    fn fusion_does_not_replace_a_constant_bypassed_by_another_predecessor() {
        let mut p = program(vec![
            I::indexed(O::Jump, 0, 2),
            I::indexed(O::Constant, 1, 7).with_flags(1),
            I::new(O::IntBinary, 0, 0, 1, 0),
            I::new(O::Return, 0, 0, 0, 0),
        ]);
        let original = p.instructions.clone();
        fuse_int_immediates(&mut p).unwrap();
        assert_eq!(p.instructions, original);
    }

    #[test]
    fn fusion_does_not_remove_the_current_value_assignment() {
        let mut p = program(vec![
            I::indexed(O::Constant, 0, 7).with_flags(1),
            I::new(O::IntBinary, 0, 0, 0, 0),
            I::new(O::Return, 0, 0, 0, 0),
        ]);
        let original = p.instructions.clone();
        fuse_int_immediates(&mut p).unwrap();
        assert_eq!(p.instructions, original);
    }

    #[test]
    fn compaction_relocates_loop_targets_without_rewriting_descriptor_ids() {
        let mut p = program(vec![
            I::indexed(O::Constant, 1, 7).with_flags(1),
            I::new(O::IntBinary, 0, 0, 1, 0),
            I::new(O::Return, 0, 0, 0, 0),
        ]);
        p.instructions.insert(0, I::indexed(O::ForPrep, 0, 0));
        p.spans.insert(0, Span { start: 0, end: 0 });
        p.for_sites.push(ForSite {
            limit: 1,
            target: 3,
            exclusive: true,
        });
        fuse_int_immediates(&mut p).unwrap();
        p.validate().unwrap();
        assert_eq!(p.instructions.len(), 3);
        assert_eq!(p.instructions[0].index(), 0);
        assert_eq!(p.for_sites[0].target, 2);
        assert_eq!(p.instructions[1].opcode(), Some(O::IntBinaryImmediate));
    }

#[cfg(feature = "cft-compiler")]
#[test]
fn fixed_collections_and_idle_dynamic_graphs_do_not_allocate_or_rescan() {
    use crate::schema::{build_schema, parse_modules, CftFile, ModuleId};
    let values = std::iter::repeat_n("1", 4096).collect::<Vec<_>>().join(",");
    let source = format!("table Rule {{ values: [int] = [{values}]; sum: fn() -> int => {{ self.values.sum() }}; make: fn() -> [int] => {{ [1, 2, 3] }}; }}");
    let schema = build_schema(&parse_modules([CftFile::from_source(ModuleId::from("buffers"), source)])).unwrap();
    let mut builder = RuntimeBuilder::new(Arc::new(Contract::new(schema).unwrap()));
    builder.add_text("rule: Rule {}", None);
    let runtime = builder.build().runtime.unwrap();
    let rule = runtime.record("Rule", "rule").unwrap();
    let sum = runtime.field(rule, "sum").unwrap();
    let make = runtime.field(rule, "make").unwrap();
    let HostValue::Existing { value: pinned, .. } = runtime.invoke(make, &[], ExecutionLimits::default()).unwrap() else { panic!("array") };
    let (collections, allocations) = {
        let heap = runtime.vm.heap.borrow();
        (heap.metrics.collections, heap.metrics.allocations)
    };
    for _ in 0..10 {
        // 固定数组载荷远大于动态预算，借用求和只需要执行窗口。
        let result = runtime.invoke(sum, &[], ExecutionLimits { max_heap_bytes: 8192, ..ExecutionLimits::default() }).unwrap();
        assert!(matches!(result, HostValue::Int(4096)));
    }
    {
        let heap = runtime.vm.heap.borrow();
        assert_eq!(heap.metrics.collections, collections);
        assert_eq!(heap.metrics.allocations, allocations);
        assert!(heap.buffer_bytes > 0);
        assert_eq!(heap.buffer_bytes, runtime.vm.buffers.borrow().storage_bytes());
    }
    // 极小预算失败后必须归还状态，后续正常调用仍可复用实例。
    assert!(runtime.invoke(sum, &[], ExecutionLimits { max_heap_bytes: 1, ..ExecutionLimits::default() }).is_err());
    assert!(matches!(runtime.invoke(sum, &[], ExecutionLimits::default()).unwrap(), HostValue::Int(4096)));
    let borrowed = runtime.value(pinned).unwrap();
    runtime.release_value(pinned).unwrap();
    runtime.invoke(sum, &[], ExecutionLimits::default()).unwrap();
    assert!(runtime.value(pinned).is_ok());
    drop(borrowed);
    runtime.invoke(sum, &[], ExecutionLimits::default()).unwrap();
    assert!(runtime.value(pinned).is_err());
    assert!(runtime.vm.heap.borrow().metrics.collections > collections);
}

#[cfg(feature = "cft-compiler")]
#[test]
fn retained_graphs_are_not_scanned_for_every_small_allocation() {
    use crate::schema::{build_schema, parse_modules, CftFile, ModuleId};
    let schema = build_schema(&parse_modules([CftFile::from_source(ModuleId::from("gc-pressure"),
        "table Rule { make: fn() -> [int] => { [1, 2, 3] }; scratch: fn(text: string) -> int => { (text + text).len() }; }")])).unwrap();
    let mut builder = RuntimeBuilder::new(Arc::new(Contract::new(schema).unwrap()));
    builder.add_text("rule: Rule {}", None);
    let runtime = builder.build().runtime.unwrap();
    let rule = runtime.record("Rule", "rule").unwrap();
    let HostValue::Existing { value, .. } = runtime.invoke(runtime.field(rule, "make").unwrap(), &[], ExecutionLimits::default()).unwrap() else { panic!("array") };
    let collections = runtime.vm.heap.borrow().metrics.collections;
    let scratch = runtime.field(rule, "scratch").unwrap();
    for _ in 0..10 { assert!(matches!(runtime.invoke(scratch, &[HostValue::String("text".into())], ExecutionLimits::default()).unwrap(), HostValue::Int(8))); }
    assert_eq!(runtime.vm.heap.borrow().metrics.collections, collections);
    assert!(runtime.collect().unwrap() > 0);
    assert!(runtime.value(value).is_ok());
    runtime.release_value(value).unwrap();
    runtime.collect().unwrap();
    assert_eq!(runtime.dynamic_value_count().unwrap(), 0);
}
