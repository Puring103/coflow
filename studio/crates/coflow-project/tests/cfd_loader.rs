use coflow_core::loading::{load, SourceInput};
use coflow_core::schema::{build_schema, parse_modules, CftFile, CftSchema, ModuleId};
use coflow_core::CfdValue;
use std::fs;
use std::path::Path;

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn compile_schema(source: &str) -> CftSchema {
    let modules = parse_modules([CftFile::from_source(ModuleId::from("main"), source)]);
    build_schema(&modules).expect("schema should compile")
}

#[test]
fn showcase_files_load_together() -> TestResult {
    let examples_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../examples/showcase");
    let schema_source = [
        "schema/01-records.cft",
        "schema/02-defaults.cft",
        "schema/03-enums.cft",
        "schema/04-flags.cft",
        "schema/05-arrays.cft",
        "schema/06-dictionaries.cft",
        "schema/07-inheritance.cft",
        "schema/08-references.cft",
        "schema/09-options.cft",
        "schema/10-checks.cft",
        "schema/11-conditional-checks.cft",
        "schema/12-quantifiers.cft",
        "schema/13-functions.cft",
    ]
    .into_iter()
    .map(|path| fs::read_to_string(examples_dir.join(path)))
    .collect::<Result<Vec<_>, _>>()?
    .join("\n");
    let schema = compile_schema(&schema_source);
    let source = [
        "data/01-records.cfd",
        "data/02-defaults.cfd",
        "data/03-enums.cfd",
        "data/04-flags.cfd",
        "data/05-arrays.cfd",
        "data/06-dictionaries.cfd",
        "data/07-inheritance.cfd",
        "data/08-references.cfd",
        "data/09-options.cfd",
        "data/10-checks.cfd",
        "data/11-conditional-checks.cfd",
        "data/12-quantifiers.cfd",
        "data/13-functions.cfd",
    ]
    .into_iter()
    .map(|path| fs::read_to_string(examples_dir.join(path)))
    .collect::<Result<Vec<_>, _>>()?
    .join("\n");

    let model = load(&schema, [SourceInput::new("showcase.cfd", source)]).1?;

    let product_id = model
        .lookup_assignable(&schema, "Product", "notebook")
        .expect("notebook product");
    let product = model.record(product_id).expect("notebook product record");
    assert_eq!(
        product.field("name"),
        Some(&CfdValue::String("Notebook".to_string()))
    );

    let bundle_id = model
        .lookup_assignable(&schema, "EffectBundle", "starter_effects")
        .expect("effect bundle");
    let bundle = model.record(bundle_id).expect("effect bundle record");
    let Some(CfdValue::Object(primary)) = bundle.field("primary") else {
        panic!("expected polymorphic primary effect");
    };
    assert_eq!(primary.actual_type.as_str(), "HealEffect");
    assert_eq!(primary.field("amount"), Some(&CfdValue::Int(0)));
    Ok(())
}
