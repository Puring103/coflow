#![allow(clippy::expect_used, clippy::panic_in_result_fn)]

use coflow_cft::{
    CftCompileOptions, CftContainer, CftDiagnostic, CftErrorCode, ModuleId, Span, StructuralLimits,
    ValueDependencyMode,
};

const fn options(max_depth: u64, max_nodes: u64, max_work: u64) -> CftCompileOptions {
    CftCompileOptions {
        structural_limits: StructuralLimits::new(max_depth, max_nodes, max_work),
    }
}

fn compile(
    source: &str,
    options: CftCompileOptions,
) -> Result<CftContainer, coflow_cft::CftDiagnostics> {
    let mut container = CftContainer::new();
    container.add_module(ModuleId::from("main"), source)?;
    container.compile_with_options(options)?;
    Ok(container)
}

fn structural_error(source: &str, options: CftCompileOptions) -> CftDiagnostic {
    compile(source, options)
        .expect_err("schema should exceed compiler budget")
        .diagnostics
        .into_iter()
        .find(|diagnostic| diagnostic.code == CftErrorCode::SchemaStructureLimitExceeded)
        .expect("schema structural limit diagnostic")
}

#[test]
fn default_entry_matches_explicit_default_options() {
    let source = "type Item { value: [int]; check { value.len() >= 0; } }";
    let mut implicit = CftContainer::new();
    implicit
        .add_module(ModuleId::from("main"), source)
        .expect("implicit module");
    implicit.compile().expect("implicit compile");

    let explicit = compile(source, CftCompileOptions::default()).expect("explicit compile");
    assert_eq!(implicit.resolve_type("Item"), explicit.resolve_type("Item"));
}

#[test]
fn flat_schema_nodes_and_work_have_exact_boundaries() {
    let source = "type Item { value: int; }";
    compile(source, options(10, 3, 8)).expect("complete schema pipeline fits");

    let node_error = structural_error(source, options(10, 2, 10));
    assert_eq!(
        node_error.message,
        "type ref exceeds structural nodes limit 2 (observed 3)"
    );
    let work_error = structural_error(source, options(10, 10, 7));
    assert_eq!(
        work_error.message,
        "schema dependency exceeds structural work limit 7 (observed 8)"
    );
}

#[test]
fn type_ref_default_and_check_depth_are_bounded_during_compile() {
    let type_source = "type Item { value: [[[int]]]; }";
    compile(type_source, options(4, 100, 100)).expect("type ref depth four fits");
    let type_error = structural_error(type_source, options(3, 100, 100));
    assert_eq!(
        type_error.message,
        "type ref exceeds structural depth limit 3 (observed 4)"
    );

    let default_source = "type Item { value: {string: [int]} = {}; }";
    compile(default_source, options(3, 100, 100)).expect("default depth one fits");
    let nested_default = "type Item { value: int = [[[1]]]; }";
    let default_error = structural_error(nested_default, options(2, 100, 100));
    assert_eq!(
        default_error.message,
        "default value exceeds structural depth limit 2 (observed 3)"
    );

    let check_source = "type Item { check { 1 + (2 * 3) > 0; } }";
    compile(check_source, options(6, 100, 100)).expect("check depth six fits");
    let check_error = structural_error(check_source, options(5, 100, 100));
    assert_eq!(
        check_error.message,
        "check AST exceeds structural depth limit 5 (observed 6)"
    );
}

#[test]
fn inheritance_cycles_win_over_limits_only_after_the_back_edge_is_known() {
    let self_cycle = compile("type A : A {}", options(1, 100, 100))
        .expect_err("self-cycle must remain semantic");
    assert!(self_cycle
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == CftErrorCode::InheritanceCycle));
    assert!(!self_cycle
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == CftErrorCode::SchemaStructureLimitExceeded));

    let two_cycle = compile("type A : B {} type B : A {}", options(2, 100, 100))
        .expect_err("two-node cycle fits depth two");
    assert!(two_cycle
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == CftErrorCode::InheritanceCycle));

    let depth_error = structural_error("type A : B {} type B : A {}", options(1, 100, 100));
    assert_eq!(
        depth_error.message,
        "schema dependency exceeds structural depth limit 1 (observed 2)"
    );
}

#[test]
fn cycle_primary_is_stable_across_module_insertion_order() {
    fn cycle_diagnostic(reverse: bool) -> CftDiagnostic {
        let mut container = CftContainer::new();
        let modules = if reverse {
            [("z", "type A : B {}"), ("a", "type B : A {}")]
        } else {
            [("a", "type B : A {}"), ("z", "type A : B {}")]
        };
        for (module, source) in modules {
            container
                .add_module(ModuleId::from(module), source)
                .expect("cycle module");
        }
        container
            .compile_with_options(options(2, 100, 100))
            .expect_err("cycle")
            .diagnostics
            .into_iter()
            .find(|diagnostic| diagnostic.code == CftErrorCode::InheritanceCycle)
            .expect("cycle diagnostic")
    }

    let forward = cycle_diagnostic(false);
    let reverse = cycle_diagnostic(true);
    assert_eq!(forward.primary, reverse.primary);
    assert_eq!(forward.related, reverse.related);
    assert_eq!(
        forward.primary.map(|label| (label.module, label.span)),
        Some((ModuleId::from("a"), Span::new(9, 10)))
    );
}

#[test]
fn failed_strict_compile_keeps_the_last_published_reflection() {
    let mut container = CftContainer::new();
    container
        .add_module(ModuleId::from("main"), "type Item { value: [[[int]]]; }")
        .expect("module");
    container.compile().expect("default compile");
    assert!(container.resolve_type("Item").is_some());

    container
        .compile_with_options(options(3, 100, 100))
        .expect_err("strict recompile fails");
    assert!(container.resolve_type("Item").is_some());
}

#[test]
fn value_dependency_plans_share_the_compile_depth_budget() {
    let chain = "type A { next: B = {}; } type B { next: C = {}; } type C {}";
    compile(chain, options(3, 100, 1_000)).expect("dependency depth three fits");
    let error = structural_error(chain, options(2, 100, 1_000));
    assert_eq!(
        error.message,
        "schema dependency exceeds structural depth limit 2 (observed 3)"
    );

    let self_cycle = compile("type A { next: A = {}; }", options(1, 100, 1_000))
        .expect("known back edge wins at depth one");
    let cycle = self_cycle
        .compiled_schema()
        .value_dependencies()
        .materialization_order("A", ValueDependencyMode::SchemaDefaults)
        .expect("known root")
        .expect_err("self cycle is cached as a semantic result");
    assert_eq!(cycle.steps().len(), 1);
}
