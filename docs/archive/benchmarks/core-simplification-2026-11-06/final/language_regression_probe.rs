use coflow_language::cft::syntax::parser::{parse_module_with_options, CftParseOptions};
use coflow_language::cft::{
    build_schema, build_schema_with_limits, parse_modules, CftFile, CftValueType, ModuleId,
};
use coflow_language::limits::StructuralLimits;
use coflow_model::cell_value::parse_schema_cell;

fn emit(id: &str, source: &str, value: String) {
    println!("{}", serde_json::json!({"id": id, "source": source, "result": value}));
}

fn parse(id: &str, source: &str, limits: StructuralLimits, full_ast: bool) {
    let result = std::panic::catch_unwind(|| {
        match parse_module_with_options(
            &ModuleId::from("probe.cft"),
            source,
            CftParseOptions { structural_limits: limits },
        ) {
            Ok(ast) if full_ast => format!("OK {ast:?}"),
            Ok(_) => "OK".into(),
            Err(error) => format!("ERR {error:?}"),
        }
    }).unwrap_or_else(|_| "PANIC".into());
    emit(id, source, result);
}

fn schema(id: &str, source: &str, limits: StructuralLimits) {
    let result = std::panic::catch_unwind(|| {
        let modules = parse_modules([CftFile::from_source(ModuleId::from("probe.cft"), source)]);
        match build_schema_with_limits(&modules, limits) {
            Ok(schema) => format!("OK {:?}", schema.resolve_type("Item")),
            Err(error) => format!("ERR {error:?}"),
        }
    }).unwrap_or_else(|_| "PANIC".into());
    emit(id, source, result);
}

fn main() {
    std::panic::set_hook(Box::new(|_| {}));
    let normal = StructuralLimits::default();
    let samples = [
        r#"@label("配\n置") table Item { name: string = "值"; check 校验: "消息" { name != "拒绝" : f"{{name}} {name} / {name}"; } } const Tail = "末尾";"#,
        "table Item { value: Option<Option<int>>=Some(Some(1)); }",
        r#"table Broken { name: string = "已消费"; check Invalid { name == "值" } table Fake { nested: ; } } @label("后续") table Following { missing: ; }"#,
        "abstract check Bad { ; } table Next { value: ; }",
        "sealed type Alias = int; table Next { value: ; }",
    ];
    for (sample_index, sample) in samples.iter().enumerate() {
        parse(&format!("full-{sample_index}"), sample, normal, true);
        for offset in sample.char_indices().map(|(offset, _)| offset) {
            parse(&format!("prefix-{sample_index}-{offset}"), &sample[..offset], normal, true);
        }
    }

    let types = [
        "int", "long", "bool", "string", "D", "T", "S", "E", "C", "Missing", "Alias",
        "&T", "&D", "&S", "&Missing", "&Alias", "[Missing]", "Option<Missing>",
        "{Missing: int}", "{int: Missing}", "{[int]: int}", "Option<[&Missing]>",
        "{string: Option<[long]>}",
    ];
    for (index, ty) in types.iter().enumerate() {
        let source = format!(
            "enum E {{ A }} data D {{}} table T {{}} singleton S {{}} type Alias = {{string: Option<[long]>}}; const C = 1; table Item {{ value: {ty} = 1; check Valid {{ value == 1; }} }}"
        );
        schema(&format!("types-{index}"), &source, normal);
    }
    for (index, source) in [
        "type A = B; type B = Option<A>; table Item { value: A = 1; check Valid { value == 1; } }",
        "table Base { value: Missing; } table Item: Base { own: int = 1; check Valid { value == 1; } }",
        "table Item { value: int; value: bool = 1; check Valid { value == 1; } }",
        "abstract sealed table Item: Missing { type: Missing = 1; check Valid { type == 1; } }",
        "table Same {} data Same {} table Item { value: Same; }",
    ].iter().enumerate() {
        schema(&format!("semantic-{index}"), source, normal);
    }

    let budget_samples = [
        "table Item { value: Option<Option<int>>=Some(Some(1)); }",
        "data Child { x: int; } table Item { value: [Child] = [Child { x: 1 }]; }",
        "table Item { values: [int]; check Valid { all x in values { x > 0; } } }",
        r#"table Item { name: string; value: string = f"前缀 {self.name} 尾部"; }"#,
    ];
    for (sample_index, source) in budget_samples.iter().enumerate() {
        for nodes in 0..18 {
            for depth in 0..6 {
                let limits = StructuralLimits::new(depth, nodes, 1000);
                parse(&format!("budget-parse-{sample_index}-{nodes}-{depth}"), source, limits, false);
                schema(&format!("budget-schema-{sample_index}-{nodes}-{depth}"), source, limits);
            }
        }
    }
    for (index, expression) in [
        "self.name()", "self.name + 1", "self.name:000", "self", "&Item::x.name()", "self.未知",
    ].iter().enumerate() {
        let source = format!("table Item {{ name: string; value: string = f\"前缀 {{{expression}}} 尾部\"; }}");
        parse(&format!("template-{index}"), &source, normal, false);
    }

    let schema = build_schema(&parse_modules([CftFile::from_source(
        ModuleId::from("cell.cft"), "table Item { name: string; }",
    )])).unwrap();
    for (index, source) in [
        "f\"hello {self.name}\"", "f\"x\" # trailing", "f\"x\"#trailing",
        "f\"x\" # ignored second f\"y\"", "f\"x\" # comment\n f\"y\"",
        "f\"x\" # comment\n other: 2", "f\"x\", other: 2", "f\"x\" other: 2",
        "f\"x\" } second: Item { name: \"y\"", "f\"x\" # comment\n", "f\"#inside\"",
        "f\"{self.name()}\"", "f\"{self.name + 1}\"", "f\"{{literal}} {self.name}\"",
    ].iter().enumerate() {
        emit(&format!("cell-{index}"), source, format!("{:?}", parse_schema_cell(&schema, &CftValueType::String, source)));
    }
}
