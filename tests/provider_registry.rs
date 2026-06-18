#[test]
fn builtin_registry_contains_all_default_providers() {
    let registry = coflow::builtin_registry();

    assert!(registry.loader("excel").is_some());
    assert!(registry.loader("lark-sheet").is_some());
    assert!(registry.loader("cfd").is_some());
    assert!(registry.exporter("json").is_some());
    assert!(registry.exporter("messagepack").is_some());
    assert!(registry.codegen("csharp").is_some());
}
