#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use super::*;
use coflow_language::{build_schema, parse_modules, CftFile, CftSchema, ModuleId};
use std::collections::BTreeMap;

fn schema(source: &str) -> CftSchema {
    let modules = parse_modules([CftFile::from_source(ModuleId::from("main"), source)]);
    build_schema(&modules, &Default::default()).expect("schema compiles")
}

fn all(files: &[GeneratedFile]) -> String {
    files
        .iter()
        .map(|file| file.contents.as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn emits_declarations_and_runtime_metadata() {
    let files = generate_csharp_cfd(
        &schema("type Item { name: string; }"),
        &CsharpCodegenOptions::new("Game.Config"),
        &["data/items.cfd".to_string()],
        BTreeMap::new(),
        None,
    )
    .expect("generate");
    let output = all(&files);
    assert!(!output.contains("SourceFiles"));
    assert!(output.contains("ICoflowTypeMetadata"));
    assert!(output.contains("[ModuleInitializer]"));
    assert!(output.contains("public static class CoflowData"));
    assert!(!output.contains("CoflowGeneratedRegistry"));
    assert!(output.contains("CoflowGeneratedContract : ICoflowGeneratedContract"));
    assert!(output.contains("CoflowGeneratedTypeMetadata, ICoflowRecordMetadata"));
    assert!(output.contains("CoflowFieldBinding.Create<global::Game.Config.Item, string>"));
    assert!(output.contains("PopulateCft_4974656D((global::Game.Config.Item)target, record, context)"));
    assert!(output.contains("target._coflowName = CfdValueReader.String"));
    assert!(!output.contains("var loaded = ReadCft_4974656D"));
    assert!(!output.contains("public Type GetFieldType(string fieldName) => fieldName switch"));
    assert!(!output.contains("public Delegate GetFieldReader(string fieldName) => fieldName switch"));
    assert!(output.contains("DeclaredType => \"Item\""));
    assert!(output.contains("CfdValueReader.String"));
    assert!(output.contains("ReadCft_4974656DFields"));
    assert!(!output.contains("CfdMaterializer"));
    assert!(!output.contains("Load(ICfdTextLoader loader"));
    assert!(!output.contains("public sealed partial class CoflowTables"));
    assert!(output.contains("using CoflowRuntime;"));
    assert!(output.contains("using CoflowRuntime.Generated;"));
    assert!(!output.contains("Newtonsoft.Json"));
    assert!(!output.contains("MessagePack"));
    assert!(!output.contains(".json"));
    assert!(!output.contains(".msgpack"));
}

#[test]
fn emits_cft_structs_as_reference_types_with_value_equality() {
    let files = generate_csharp_cfd(
        &schema("@struct sealed type Point { x: int; y: int; }"),
        &CsharpCodegenOptions::new("Game.Config"),
        &[],
        BTreeMap::new(),
        None,
    )
    .expect("generate struct");
    let output = all(&files);
    assert!(output.contains("public sealed partial class Point : IEquatable<Point>"));
    assert!(output.contains("public bool Equals(Point? other)"));
    assert!(output.contains("EqualityComparer<long>.Default.Equals(X, other.X)"));
    assert!(!output.contains("EqualityComparer<object>.Default"));
    assert!(!output.contains("partial struct Point"));
    assert!(!output.contains("CoflowStringTableToken<Point>"));
    assert!(output.contains(
        "Cft_506F696E74CoflowMetadata : CoflowGeneratedTypeMetadata, ICoflowTypeMetadata"
    ));
    assert!(!output.contains("Cft_506F696E74CoflowMetadata : ICoflowRecordMetadata"));
    assert!(!output.contains("public Type KeyType"));
    assert!(!output.contains("public object ParseKey"));
    assert!(!output.contains("public Delegate GetKeyReader"));
}

#[test]
fn expands_type_aliases_without_emitting_alias_declarations() {
    let files = generate_csharp_cfd(
        &schema(
            r#"
type Predicate = fn(input: int) -> bool;
type OptionalName = Option<string>;
type Rule {
    predicate: Predicate;
    name: OptionalName = None;
}
"#,
        ),
        &CsharpCodegenOptions::new("App.Config"),
        &[],
        BTreeMap::new(),
        None,
    )
    .expect("generate aliases");
    let output = all(&files);

    assert!(files
        .iter()
        .any(|file| file.relative_path.to_string_lossy() == "Rule.cs"));
    assert!(!files.iter().any(|file| {
        matches!(
            file.relative_path.to_string_lossy().as_ref(),
            "Predicate.cs" | "OptionalName.cs"
        )
    }));
    assert!(output.contains("public bool Predicate(long input)"));
    assert!(!output.contains("BindPredicate"));
    assert!(output.contains("Option<string> Name"));
    assert!(!output.contains("class Predicate"));
    assert!(!output.contains("class OptionalName"));
}

#[test]
fn emits_scalar_constants_in_internal_runtime_metadata() {
    let files = generate_csharp_cfd(
        &schema(
            "namespace common; const LEVEL: int = 42; const RATIO: float = 0.5; const ENABLED: bool = true; const LABEL: string = \"line\\ntext\"; type Item { value: int; }",
        ),
        &CsharpCodegenOptions::new("Game.Config"),
        &[],
        BTreeMap::new(),
        None,
    )
    .expect("generate");
    let output = all(&files);

    assert!(output.contains("public IReadOnlyList<CoflowConstant> Constants"));
    assert!(output.contains(
        "new CoflowConstant(\"common::LEVEL\", typeof(long), 42L)"
    ));
    assert!(output.contains(
        "new CoflowConstant(\"common::RATIO\", typeof(double), 0.5D)"
    ));
    assert!(output.contains(
        "new CoflowConstant(\"common::ENABLED\", typeof(bool), true)"
    ));
    assert!(output.contains(
        "new CoflowConstant(\"common::LABEL\", typeof(string), \"line\\ntext\")"
    ));
    assert!(!output.contains("public class Constants"));
}

#[test]
fn emits_strongly_typed_compound_constants_and_deferred_record_references() {
    let files = generate_csharp_cfd(
        &schema(
            r#"
enum Mode { Primary = 1 }
sealed type Stats { hp: int; mode: Mode = Mode::Primary; }
type Item { name: string; }
const VALUES: [int] = [1, 2];
const WEIGHTS: {string: int} = { "fire": 10 };
const STATS: Stats = { hp: 100 };
const ITEM: Option<&Item> = Some(&Item::sword);
"#,
        ),
        &CsharpCodegenOptions::new("Game.Config"),
        &[],
        BTreeMap::new(),
        None,
    )
    .expect("generate compound constants");
    let output = all(&files);

    assert!(output.contains("typeof(IReadOnlyList<long>)"));
    assert!(output.contains("CoflowConstantValues.List<long>(1L, 2L)"));
    assert!(output.contains("CoflowConstantValues.Dictionary<string, long>"));
    assert!(output.contains("new global::Game.Config.Stats(null, string.Empty, 100L"));
    assert!(output.contains("typeof(Option<global::Game.Config.Item>)"));
    assert!(output.contains("static context => Option<global::Game.Config.Item>.Some("));
    assert!(output.contains(
        "context.Resolve<global::Game.Config.Item>(\"Item\", \"sword\")"
    ));
}

#[test]
fn preserves_display_metadata_as_xml_docs() {
    let files = generate_csharp(
        &schema(
            r#"@label("Item") @description("Description") type Item { @label("Name") name: string; }"#,
        ),
        &CsharpCodegenOptions::new("Game.Config"),
    )
    .expect("generate");
    let item = files
        .iter()
        .find(|file| file.relative_path.as_os_str() == "Item.cs")
        .expect("item file");
    assert!(item
        .contents
        .contains("<summary>Item: Description</summary>"));
    assert!(item.contents.contains("<summary>Name</summary>"));
}

#[test]
fn preserves_all_annotations_in_runtime_metadata() {
    let files = generate_csharp_cfd(
        &schema(
            r#"
@flag @label("Modes") enum Mode { @description("Primary") Primary = 1 }
enum ItemId {}
sealed type Stats { hp: int; }
@idAsEnum(ItemId) @label("Items") type Item { @expand stats: Stats; mode: Mode; }
"#,
        ),
        &CsharpCodegenOptions::new("Game.Config"),
        &[],
        BTreeMap::new(),
        None,
    )
    .expect("generate");
    let output = all(&files);
    assert!(output.contains("new CoflowAnnotation(\"flag\""));
    assert!(output.contains("new CoflowAnnotation(\"label\""));
    assert!(output.contains("new CoflowAnnotation(\"description\""));
    assert!(output.contains("new CoflowAnnotation(\"expand\""));
    assert!(output.contains("new CoflowAnnotation(\"idAsEnum\""));
    assert!(output.contains(
        "CoflowAnnotationArgumentKind.Name, \"ItemId\""
    ));
    assert!(output.contains("FieldAnnotations(string fieldName)"));
    assert!(output.contains("VariantAnnotations(string variantName)"));
}

#[test]
fn preserves_custom_annotations_in_runtime_metadata() {
    let files = generate_csharp_cfd(
        &schema(
            r#"
namespace game;
@CustomTag(game::Marker, "text", 1, 1.5, true)
type Item {
  @EditorHint("compact") value: int;
}
"#,
        ),
        &CsharpCodegenOptions::new("Game.Config"),
        &[],
        BTreeMap::new(),
        None,
    )
    .expect("generate");
    let output = all(&files);

    assert!(output.contains("new CoflowAnnotation(\"CustomTag\""));
    assert!(output.contains(
        "CoflowAnnotationArgumentKind.Name, \"game::Marker\""
    ));
    assert!(output.contains("CoflowAnnotationArgumentKind.String, \"text\""));
    assert!(output.contains("CoflowAnnotationArgumentKind.Int, 1L"));
    assert!(output.contains("CoflowAnnotationArgumentKind.Float, 1.5D"));
    assert!(output.contains("CoflowAnnotationArgumentKind.Bool, true"));
    assert!(output.contains("new CoflowAnnotation(\"EditorHint\""));
    assert!(output.contains("CoflowAnnotationArgumentKind.String, \"compact\""));
}

#[test]
fn descriptor_declares_cfd_runtime_contract() {
    assert_eq!(CSHARP_CFD_CODEGEN_DESCRIPTOR.id, "csharp");
    assert_eq!(
        CSHARP_CFD_CODEGEN_DESCRIPTOR.runtime_package,
        "Coflow.Cfd.Runtime"
    );
    assert!(CSHARP_CFD_CODEGEN_DESCRIPTOR.needs_model);
}

#[test]
fn emits_empty_type_reader_without_invalid_argument_list() {
    let files = generate_csharp_cfd(
        &schema("type Empty { }"),
        &CsharpCodegenOptions::new("Game.Config"),
        &["data/empty.cfd".to_string()],
        BTreeMap::new(),
        None,
    )
    .expect("generate");
    let output = all(&files);
    assert!(output.contains("CfdValueReader.ValidateFields(fields);"));
    assert!(!output.contains("ValidateFields(fields, );"));
}

#[test]
fn emits_singleton_metadata_without_a_generated_database() {
    let files = generate_csharp_cfd(
        &schema("@singleton type Settings { value: int; }"),
        &CsharpCodegenOptions::new("Game.Config"),
        &["data/settings.cfd".to_string()],
        BTreeMap::new(),
        None,
    )
    .expect("generate");
    let output = all(&files);
    assert!(output.contains("public bool IsSingleton => true;"));
    assert!(output.contains("public Type RuntimeType => typeof(global::Game.Config.Settings);"));
    assert!(!output.contains("public sealed partial class CoflowTables"));
}

#[test]
fn preserves_host_singleton_in_generated_metadata() {
    let files = generate_csharp_cfd(
        &schema("@Host @singleton type Api { environment: string; log: fn(string) -> (); }"),
        &CsharpCodegenOptions::new("CoflowConfig"),
        &[],
        BTreeMap::new(),
        None,
    )
    .expect("generate");
    let output = all(&files);
    assert!(output.contains("public bool IsSingleton => true;"));
    assert!(output.contains(
        "Cft_417069CoflowMetadata : CoflowGeneratedTypeMetadata, ICoflowHostMetadata"
    ));
    assert!(output.contains("public void Configure("));
    assert!(output.contains("string environment,\n        Action<string> log"));
    assert!(output.contains("CoflowHostFunctionBinding.Create(_coflowLog.RuntimeEntry, log)"));
    assert!(output.contains("CreateHostCft_417069(context)"));
    assert!(!output.contains("BindLog(Action<string> implementation)"));
}

#[test]
fn host_binding_includes_inherited_fields_and_functions() {
    let files = generate_csharp_cfd(
        &schema(
            "abstract type ServicesBase { region: string; report: fn(string) -> (); } @Host @singleton type Services : ServicesBase { environment: string; log: fn(string) -> (); }",
        ),
        &CsharpCodegenOptions::new("CoflowConfig"),
        &[],
        BTreeMap::new(),
        None,
    )
    .expect("generate");
    let output = all(&files);
    assert!(output.contains("string region,"));
    assert!(output.contains("Action<string> report,"));
    assert!(output.contains("CoflowHostFunctionBinding.Create(_coflowReport.RuntimeEntry, report)"));
    assert!(output.contains("CoflowHostFunctionBinding.Create(_coflowLog.RuntimeEntry, log)"));
    assert!(output.contains(": base(hostSlot, default!, report)"));
}

#[test]
fn maps_option_result_unit_and_function_types() {
    let files = generate_csharp(
        &schema(
            "type Failure { code: int; } type Api { optional: Option<int>; run: fn(input: string) -> Result<(), Failure>; }",
        ),
        &CsharpCodegenOptions::new("CoflowConfig"),
    )
    .expect("generate");
    let output = all(&files);
    assert!(output.contains("Option<long> Optional"));
    assert!(output.contains("internal CoflowFunctionEntry<Func<string, Result<Unit, global::CoflowConfig.Failure>>> _coflowRun"));
    assert!(output.contains("public Result<Unit, global::CoflowConfig.Failure> Run(string input)"));
    assert!(!output.contains("BindRun"));
    assert!(output.contains("_coflowRun.Function(input)"));
    assert!(output.contains("internal Api("));
    assert!(!output.contains("Func<string, Result<Unit, Failure>> Run { get;"));
}

#[test]
fn loads_function_values_nested_in_collections_and_option() {
    let files = generate_csharp_cfd(
        &schema(
            "type Pipeline { handlers: [fn(int) -> int]; named: {string: fn(string) -> bool}; optional: Option<fn(int) -> int> = None; }",
        ),
        &CsharpCodegenOptions::new("CoflowConfig"),
        &[],
        BTreeMap::new(),
        None,
    )
    .expect("generate nested function values");
    let output = all(&files);

    assert!(output.contains("IReadOnlyList<Func<long, long>> Handlers"));
    assert!(output.contains("IReadOnlyDictionary<string, Func<string, bool>> Named"));
    assert!(output.contains("Option<Func<long, long>> Optional"));
    assert!(output.contains("context.FunctionValueAot<Func<long, long>>(item, typeof(long)"));
    assert!(output.contains("context.FunctionValueAot<Func<string, bool>>(item, typeof(bool)"));
    assert!(output.contains("CoflowDelegateAdapter.Register<Func<long, long>>"));
}

#[test]
fn ordinary_function_loader_marks_the_cfd_body_as_required() {
    let files = generate_csharp_cfd(
        &schema("type Rule { evaluate: fn(value: int) -> int; notify: fn(message: string) -> (); }"),
        &CsharpCodegenOptions::new("CoflowConfig"),
        &[],
        BTreeMap::new(),
        None,
    )
    .expect("generate");
    let output = all(&files);
    assert!(output.contains(
        "context.RequiredFunction(CfdValueReader.FindField(fields, \"evaluate\"), \"evaluate\", typeof(long), typeof(long))"
    ));
    assert!(output.contains("public long Evaluate(long value)"));
    assert!(output.contains("public void Notify(string message)"));
    assert!(!output.contains("public void Configure("));
    assert!(!output.contains("BindEvaluate"));
    assert!(!output.contains("BindNotify"));
}

#[test]
fn generated_metadata_does_not_embed_physical_source_paths() {
    let schema = schema("type Item { value: int; }");
    let files = generate_csharp_cfd_with_manifest(
        &schema,
        &CsharpCodegenOptions::new("Game.Config"),
        &[SourceManifestEntry {
            logical_path: "data/dimensions/language/Item_value.cfd".to_string(),
            origin: SourceOrigin::Dimension {
                dimension: "language".to_string(),
                source_type: "Item".to_string(),
                field: "value".to_string(),
            },
        }],
        BTreeMap::new(),
        None,
    )
    .expect("generate");
    let output = all(&files);
    assert!(!output.contains("data/dimensions/language/Item_value.cfd"));
    assert!(output.contains("ICoflowTypeMetadata"));
}

#[test]
fn emits_source_enum_mappings_and_flag_masks() {
    let files = generate_csharp_cfd(
        &schema(
            "enum item_rarity { common_value, rare_value }\n@flag enum item_flags { fire = 1, ice = 2 }\ntype Item { rarity: item_rarity; flags: item_flags; }",
        ),
        &CsharpCodegenOptions::new("Game.Config"),
        &["data/items.cfd".to_string()],
        BTreeMap::new(),
        None,
    )
    .expect("generate");
    let output = all(&files);
    assert!(output.contains("ReadEnumCft_6974656D5F726172697479"));
    assert!(output.contains("\"common_value\" or \"item_rarity::common_value\""));
    assert!(output.contains("ReadEnumCft_6974656D5F666C616773"));
    assert!(output.contains(" 3L"));
    assert!(output.contains(
        "CoflowFieldBinding.CreateEnum<global::Game.Config.Item, global::Game.Config.ItemRarity>"
    ));
    assert!(output.contains("static value => (long)value"));
    assert!(!output.contains("public object GetKey(object record)"));
    assert!(output.contains("CfdValueReader.Object(node, context, \"Item\""));
}

#[test]
fn emits_schema_defaults_in_direct_cfd_readers() {
    let files = generate_csharp_cfd(
        &schema(
            r#"
enum item_rarity { common_value, rare_value }
type Stats { hp: int = 10; }
type Item {
    rarity: item_rarity = item_rarity::common_value;
    enabled: bool = false;
    label: string = "line\ntext";
    tags: [string] = [];
    weights: {string: int} = {};
    stats: Stats = {};
    target: Option<&Item> = None;
    fallback: Option<int> = Some(4);
    outcome: Result<string, int> = Ok("ready");
    failure: Result<string, int> = Err(7);
}
"#,
        ),
        &CsharpCodegenOptions::new("Game.Config"),
        &["data/items.cfd".to_string()],
        BTreeMap::new(),
        None,
    )
    .expect("generate");
    let output = all(&files);
    assert!(output.contains("CfdValueReader.FindField(fields, \"enabled\")"));
    assert!(output.contains("valueEnabled ? CfdValueReader.Boolean(valueEnabled) : false"));
    assert!(output.contains("ItemRarity.CommonValue"));
    assert!(output.contains("\"line\\ntext\""));
    assert!(output.contains("CoflowConstantValues.List<string>()"));
    assert!(output.contains("CoflowConstantValues.Dictionary<string, long>()"));
    assert!(output.contains("new global::Game.Config.Stats(null, string.Empty, 10L)"));
    assert!(output.contains("valueTarget")
        && output.contains("Option<global::Game.Config.Item>.None"));
    assert!(output.contains("Option<long>.Some(4L)"));
    assert!(output.contains("Result<string, long>.Ok(\"ready\")"));
    assert!(output.contains("Result<string, long>.Err(7L)"));
}

#[test]
fn rejects_recursive_object_defaults_during_codegen() {
    let error = generate_csharp_cfd(
        &schema("type Node { child: Node = {}; }"),
        &CsharpCodegenOptions::new("Game.Config"),
        &[],
        BTreeMap::new(),
        None,
    )
    .expect_err("recursive object defaults must not generate recursive constructors");

    assert!(error.to_string().contains("schema default dependency cycle"));
}

#[test]
fn emits_source_names_for_polymorphic_bindings_and_assignability() {
    let files = generate_csharp_cfd(
        &schema(
            r#"
abstract type reward_base {}
sealed type fixed_reward : reward_base { amount: int; }
type concrete_base { name: string; }
sealed type concrete_child : concrete_base { amount: int; }
type Holder { reward: reward_base; target: &reward_base; concrete: concrete_base; }
"#,
        ),
        &CsharpCodegenOptions::new("Game.Config"),
        &["data/rewards.cfd".to_string()],
        BTreeMap::new(),
        None,
    )
    .expect("generate");
    let output = all(&files);
    assert!(output.contains("DeclaredType => \"fixed_reward\""));
    assert!(output.contains(
        "AssignableTypes { get; } = new string[] { \"fixed_reward\", \"reward_base\" }"
    ));
    assert!(output.contains("\"fixed_reward\" => ReadCft_66697865645F726577617264(node, context)"));
    assert!(!output.contains("Readfixed_reward"));
    assert!(output.contains(
        "CfdValueReader.Reference<global::Game.Config.RewardBase>(CfdValueReader.Field(fields, \"target\"), context, \"reward_base\")"
    ));
    assert!(!output.contains("expected a polymorphic object or reference"));
    assert!(output.contains("\"reward\" => \"reward_base\""));
    assert!(output.contains("\"target\" => \"reward_base\""));
    assert!(output.contains("ObjectFieldType(string fieldName)"));
    assert!(output.contains("ReferenceFieldType(string fieldName)"));
    assert!(output.contains("\"concrete_child\" => ReadCft_636F6E63726574655F6368696C64(node, context)"));
    assert!(output.contains("null or \"concrete_base\" =>"));
    assert!(output.contains(
        "CfdValueReader.Object(node, context, \"concrete_base\", ReadCft_636F6E63726574655F62617365Fields)"
    ));
}

#[test]
fn maps_cft_namespaces_to_csharp_namespaces_and_stable_metadata() {
    let files = generate_csharp_cfd(
        &schema(
            r#"
namespace game::items;
enum Rarity { Common }
type Item { rarity: Rarity = Rarity::Common; }
"#,
        ),
        &CsharpCodegenOptions::new("Project.Config"),
        &[],
        BTreeMap::new(),
        None,
    )
    .expect("generate");

    let item = files
        .iter()
        .find(|file| file.relative_path.as_os_str() == "game/items/Item.cs")
        .expect("namespaced Item file");
    assert!(item.contents.contains("namespace Project.Config.game.items"));

    let output = all(&files);
    assert!(output.contains("DeclaredType => \"game::items::Item\""));
    assert!(output.contains("typeof(global::Project.Config.game.items.Item)"));
    assert!(output.contains("global::Project.Config.game.items.Rarity.Common"));
    assert!(output.contains("ReadEnumCft_67616D653A3A6974656D733A3A526172697479"));
}

#[test]
fn registry_generator_returns_safe_code_artifacts() {
    let mut registry = coflow_runtime::codegen::CodegenRegistry::default();
    registry
        .register(CsharpCfdCodeGenerator)
        .expect("register C#");
    assert!(registry.get("csharp").is_some());
    assert!(registry.register(CsharpCfdCodeGenerator).is_err());
}
