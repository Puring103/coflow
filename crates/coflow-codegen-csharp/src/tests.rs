#![allow(
    clippy::expect_used,
    clippy::needless_raw_string_hashes,
    clippy::panic,
    clippy::unwrap_used
)]

use super::*;
use coflow_language::cft::{
    build_schema, parse_modules, CftDimensionInputs, CftFile, CftSchema, ModuleId,
};
use std::collections::BTreeMap;

fn schema(source: &str) -> CftSchema {
    let modules = parse_modules([CftFile::from_source(ModuleId::from("main"), source)]);
    build_schema(&modules, &CftDimensionInputs::default()).expect("schema compiles")
}

fn all(files: &[GeneratedFile]) -> String {
    files
        .iter()
        .map(|file| file.contents.replace("\r\n", "\n"))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn preserves_source_identifier_spelling() {
    let files = generate_csharp_cfd(&schema(
        "enum item_kind { rare_item } type item_data { hit_points: int; displayName: string; Kind: item_kind; apply_bonus: fn(BonusValue: int) -> int; }"
    ), BTreeMap::new(), None).expect("generate original names");
    let output = all(&files);
    assert!(files
        .iter()
        .any(|file| file.relative_path.as_os_str() == "item_data.cs"));
    assert!(output.contains("long hit_points"));
    assert!(output.contains("string displayName"));
    assert!(output.contains("global::item_kind Kind"));
    assert!(output.contains("rare_item ="));
    assert!(output.contains("apply_bonus(global::Coflow.Runtime.Coflow coflow, long BonusValue)"));
    assert!(!output.contains("HitPoints"));
}

#[test]
fn id_as_enum_preserves_record_key_spelling() {
    let schema = schema("enum ItemId {} @idAsEnum(ItemId) type Item {}");
    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "rare_sword",
        "Item",
        [] as [(&str, coflow_model::LoadedValueDraft); 0],
    );
    let model = builder.build().expect("model");
    let values = BTreeMap::from([(
        "ItemId".to_string(),
        BTreeMap::from([("rare_sword".to_string(), 1)]),
    )]);
    let variants = id_as_enum_variants(&schema, &model, &values).expect("stable enum variants");
    let files = generate_csharp_cfd(&schema, variants, None).expect("generate enum");
    let output = all(&files);
    assert!(output.contains("rare_sword = 1"));
    assert!(!output.contains("RareSword"));
}

#[test]
fn empty_abstract_type_has_one_parameterless_constructor() {
    let files = generate_csharp(&schema("abstract type Empty {} type Child : Empty {}"))
        .expect("generate empty inheritance");
    let empty = &files
        .iter()
        .find(|file| file.relative_path.as_os_str() == "Empty.cs")
        .unwrap()
        .contents;
    assert_eq!(empty.matches("Empty(").count(), 1, "{empty}");
    assert!(empty.contains("protected internal Empty("));
}

#[test]
#[ignore = "requires the .NET 10 SDK"]
fn generated_identifiers_and_empty_inheritance_compile_in_csharp() {
    let output_dir =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/csharp-codegen-regressions");
    std::fs::create_dir_all(&output_dir).expect("create compilation directory");
    let files = generate_csharp_cfd(
        &schema(
            r#"
        abstract type empty_base {}
        type child_type : empty_base { hit_points: int; }
        enum item_kind { rare_item }
        @struct sealed type item_value { hit_points: int; Kind: item_kind; }
        @singleton type empty_config {}
        @Host @singleton type host_config { apply_bonus: fn(BonusValue: int) -> int; }
    "#,
        ),
        BTreeMap::new(),
        None,
    )
    .expect("generate compilation fixtures");
    for file in files {
        std::fs::write(output_dir.join(file.relative_path), file.contents)
            .expect("write generated source");
    }
    std::fs::write(output_dir.join("Regression.csproj"), r#"<Project Sdk="Microsoft.NET.Sdk">
      <PropertyGroup><TargetFramework>net10.0</TargetFramework><OutputType>Exe</OutputType><Nullable>enable</Nullable><LangVersion>9.0</LangVersion></PropertyGroup>
      <ItemGroup><ProjectReference Include="../../runtimes/csharp/Coflow.Runtime/Coflow.Runtime.csproj" /></ItemGroup>
    </Project>"#).expect("write compilation project");
    std::fs::write(output_dir.join("Program.cs"), r#"
        var item = new item_value(7, item_kind.rare_item);
        if (item.hit_points != 7 || item.Kind != item_kind.rare_item) throw new System.Exception("field initialization");
        var child = new child_type("row", 9);
        if (child.hit_points != 9) throw new System.Exception("inherited construction");
    "#).expect("write runtime assertions");
    let output = std::process::Command::new("dotnet")
        .args(["run", "--project"])
        .arg(output_dir.join("Regression.csproj"))
        .output()
        .expect("run .NET compilation");
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn emits_declarations_and_runtime_metadata() {
    let files = generate_csharp_cfd(
        &schema("type Item { name: string; }"),
        BTreeMap::new(),
        None,
    )
    .expect("generate");
    let output = all(&files);
    assert!(!output.contains("SourceFiles"));
    assert!(output.contains("ICoflowTypeMetadata"));
    assert!(output.contains("public CoflowSchemaRuntime Runtime { get; } = BuildRuntime();"));
    assert!(output.contains("var runtime = new CoflowSchemaRuntimeBuilder();"));
    assert!(!output.contains("[ModuleInitializer]"));
    assert!(output.contains("public static class Schema"));
    assert!(output.contains("public static global::Coflow.Runtime.Coflow Create(global::Coflow.Runtime.CoflowOptions? options = null)"));
    assert!(!output.contains("CoflowGeneratedRegistry"));
    assert!(output.contains("CoflowSchema : ICoflowSchema"));
    assert!(output.contains("CoflowMetadata : ICoflowRecordMetadata"));
    assert!(output
        .contains("CoflowTableFactory.String<global::Item>(values, static record => record.Id)"));
    assert!(!output.contains("new CoflowStringTable<"));
    assert!(output.contains("CoflowFieldBinding.Create<global::Item, string>"));
    assert!(
        output.contains("runtime.RegisterTypeCodec<global::Item>(\n            new CoflowTypeId(")
    );
    assert!(output.contains("static (ref CoflowValueWriter writer, global::Item value)"));
    assert!(output.contains("writer.Write(value.name);"));
    assert!(output.contains("writer.WriteValueId(value._coflowId);"));
    assert!(output.contains("PopulateCft_4974656D((global::Item)target, record, context)"));
    assert!(output.contains("target._coflowname = CfdValueReader.String"));
    assert!(!output.contains("var loaded = ReadCft_4974656D"));
    assert!(!output.contains("public Type GetFieldType(string fieldName) => fieldName switch"));
    assert!(
        !output.contains("public Delegate GetFieldReader(string fieldName) => fieldName switch")
    );
    assert!(output.contains("DeclaredType => \"Item\""));
    assert!(output.contains("CfdValueReader.String"));
    assert!(output.contains("ReadCft_4974656DFields"));
    assert!(!output.contains("CfdMaterializer"));
    assert!(!output.contains("Load(ICfdTextLoader loader"));
    assert!(!output.contains("public sealed partial class CoflowTables"));
    assert!(output.contains("using Coflow.Runtime;"));
    assert!(output.contains("using Coflow.Runtime.CompilerServices;"));
    assert!(!output.contains("Newtonsoft.Json"));
    assert!(!output.contains("MessagePack"));
    assert!(!output.contains(".json"));
    assert!(!output.contains(".msgpack"));
}

#[test]
fn registers_abstract_generated_types_for_value_id_vm_layouts() {
    let files = generate_csharp_cfd(
        &schema("abstract type Ability { value: int; } type Damage : Ability { extra: int; }"),
        BTreeMap::new(),
        None,
    )
    .expect("generate abstract layout");
    let output = all(&files);
    assert!(output.contains("runtime.RegisterType<global::Ability>(new CoflowTypeId("));
    assert!(output.contains("runtime.RegisterTypeCodec<global::Damage>("));
}

#[test]
fn emits_cft_structs_as_readonly_value_types() {
    let files = generate_csharp_cfd(
        &schema("@struct sealed type Point { x: int; y: int; }"),
        BTreeMap::new(),
        None,
    )
    .expect("generate struct");
    let output = all(&files);
    assert!(output.contains("public readonly partial struct Point : IEquatable<Point>"));
    assert!(output.contains("public bool Equals(Point other)"));
    assert!(output.contains("EqualityComparer<long>.Default.Equals(x, other.x)"));
    assert!(!output.contains("EqualityComparer<object>.Default"));
    assert!(output.contains("internal readonly CoflowValueId _coflowId"));
    assert!(output.contains("internal readonly bool _coflowInitialized"));
    assert!(output.contains("_coflowInitialized = true;"));
    assert!(output.contains("static value => value._coflowInitialized"));
    assert!(output.contains("runtime.RegisterStruct<global::Point>("));
    assert!(output.contains("3, 0, 0,"));
    assert!(output.contains("writer.Write(value.x);"));
    assert!(output.contains("reader.Read<long>()"));
    assert!(output.contains("true, 1, 0, 0"));
    assert!(!output.contains("CoflowStringTableToken<Point>"));
    assert!(output.contains("Cft_506F696E74CoflowMetadata : ICoflowTypeMetadata"));
    assert!(!output.contains("Cft_506F696E74CoflowMetadata : ICoflowRecordMetadata"));
    assert!(!output.contains("public Type KeyType"));
    assert!(!output.contains("public object ParseKey"));
    assert!(!output.contains("public object GetKey"));
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
    assert!(
        output.contains("public bool predicate(global::Coflow.Runtime.Coflow coflow, long input)")
    );
    assert!(!output.contains("BindPredicate"));
    assert!(output.contains("Option<string> name"));
    assert!(!output.contains("class Predicate"));
    assert!(!output.contains("class OptionalName"));
}

#[test]
fn emits_scalar_constants_in_internal_runtime_metadata() {
    let files = generate_csharp_cfd(
        &schema(
            "const LEVEL: int = 42; const RATIO: float = 0.5; const ENABLED: bool = true; const LABEL: string = \"line\\ntext\"; type Item { value: int; }",
        ),
        BTreeMap::new(),
        None,
    )
    .expect("generate");
    let output = all(&files);

    assert!(output.contains("public IReadOnlyList<CoflowConstant> Constants"));
    assert!(output.contains("new CoflowConstant(\"LEVEL\", typeof(long), 42L)"));
    assert!(output.contains("new CoflowConstant(\"RATIO\", typeof(double), 0.5D)"));
    assert!(output.contains("new CoflowConstant(\"ENABLED\", typeof(bool), true)"));
    assert!(output.contains("new CoflowConstant(\"LABEL\", typeof(string), \"line\\ntext\")"));
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
        BTreeMap::new(),
        None,
    )
    .expect("generate compound constants");
    let output = all(&files);

    assert!(output.contains("typeof(IReadOnlyList<long>)"));
    assert!(output.contains("CoflowConstantValues.List<long>(1L, 2L)"));
    assert!(output.contains("CoflowConstantValues.Dictionary<string, long>"));
    assert!(output.contains("new global::Stats(null, string.Empty, 100L"));
    assert!(output.contains("typeof(Option<global::Item>)"));
    assert!(output.contains("static context => Option<global::Item>.Some("));
    assert!(output.contains("context.Resolve<global::Item>(\"Item\", \"sword\")"));
}

#[test]
fn preserves_display_metadata_as_xml_docs() {
    let files = generate_csharp(&schema(
        r#"@label("Item") @description("Description") type Item { @label("Name") name: string; }"#,
    ))
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
    assert!(output.contains("CoflowAnnotationArgumentKind.Name, \"ItemId\""));
    assert!(output.contains("IReadOnlyList<CoflowFieldMetadata> Fields"));
    assert!(!output.contains("FieldAnnotations(string fieldName)"));
    assert!(output.contains("VariantAnnotations(string variantName)"));
}

#[test]
fn preserves_custom_annotations_in_runtime_metadata() {
    let files = generate_csharp_cfd(
        &schema(
            r#"
@CustomTag(Marker, "text", 1, 1.5, true)
type Item {
  @EditorHint("compact") value: int;
}
"#,
        ),
        BTreeMap::new(),
        None,
    )
    .expect("generate");
    let output = all(&files);

    assert!(output.contains("new CoflowAnnotation(\"CustomTag\""));
    assert!(output.contains("CoflowAnnotationArgumentKind.Name, \"Marker\""));
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
        "Coflow.Runtime"
    );
}

#[test]
fn emits_empty_type_reader_without_invalid_argument_list() {
    let files =
        generate_csharp_cfd(&schema("type Empty { }"), BTreeMap::new(), None).expect("generate");
    let output = all(&files);
    assert!(output.contains("CfdValueReader.ValidateFields(fields);"));
    assert!(!output.contains("ValidateFields(fields, );"));
}

#[test]
fn emits_typed_vm_factories_without_delegate_reflection_contract() {
    let files = generate_csharp_cfd(
        &schema("type Entry { name: string; enabled: bool = true; }"),
        BTreeMap::new(),
        None,
    )
    .expect("generate");
    let output = all(&files);
    assert!(output.contains("public CoflowVmFactory CreateVmObjectFactory"));
    assert!(output.contains("frame.Read<string>(0)"));
    assert!(output.contains("frame.Read<bool>(1)"));
    assert!(output.contains("new CoflowVmFactory(Type.EmptyTypes, typeof(bool)"));
    assert!(output.contains("frame.Write(true)"));
    assert!(!output.contains("private delegate global::Entry VmObjectFactory"));
    assert!(!output.contains("public Delegate CreateVmObjectFactory"));
}

#[test]
fn emits_singleton_metadata_without_a_generated_database() {
    let files = generate_csharp_cfd(
        &schema("@singleton type Settings { value: int; }"),
        BTreeMap::new(),
        None,
    )
    .expect("generate");
    let output = all(&files);
    assert!(output.contains("public bool IsSingleton => true;"));
    assert!(output.contains("public Type RuntimeType => typeof(global::Settings);"));
    assert!(output.contains("Cft_53657474696E6773CoflowMetadata : ICoflowRecordMetadata"));
    assert!(output.contains("public object GetKey(object value) => string.Empty;"));
    assert!(output.contains("new global::Settings()"));
    assert!(!output.contains("public sealed partial class CoflowTables"));
}

#[test]
fn preserves_host_singleton_in_generated_metadata() {
    let files = generate_csharp_cfd(
        &schema("@Host @singleton type Api { environment: string; log: fn(string) -> (); }"),
        BTreeMap::new(),
        None,
    )
    .expect("generate");
    let output = all(&files);
    assert!(output.contains("public bool IsSingleton => true;"));
    assert!(output.contains("Cft_417069CoflowMetadata : ICoflowHostMetadata"));
    assert!(!output.contains("public void Configure("));
    assert!(output.contains("public Api(\n        string environment,\n        Action<string> log"));
    assert!(output.contains("internal Action<string> _coflowlog { get; }"));
    assert!(output.contains("BindHostCft_417069(value, context)"));
    assert!(output.contains("context.BindHostFunction(CoflowHostFunctionBinding.Create("));
    assert!(output.contains("var snapshotValue = (Api)value.MemberwiseClone();"));
    assert!(output.contains("snapshotValue._coflowId = coflowId;"));
    assert!(!output.contains("value._coflowId = coflowId;"));
    assert!(!output.contains("BindLog(Action<string> implementation)"));
}

#[test]
fn host_binding_includes_inherited_fields_and_functions() {
    let files = generate_csharp_cfd(
        &schema(
            "abstract type ServicesBase { region: string; report: fn(string) -> (); } @Host @singleton type Services : ServicesBase { environment: string; log: fn(string) -> (); }",
        ),
        BTreeMap::new(),
        None,
    )
    .expect("generate");
    let output = all(&files);
    assert!(output.contains("string region,"));
    assert!(output.contains("Action<string> report,"));
    assert!(output.contains("internal Action<string> _coflowreport { get; }"));
    assert!(output.contains("internal Action<string> _coflowlog { get; }"));
    assert!(output.contains(": base(region)"));
}

#[test]
fn maps_option_result_unit_and_function_types() {
    let files = generate_csharp(
        &schema(
            "type Failure { code: int; } type Api { optional: Option<int>; run: fn(input: string) -> Result<(), Failure>; }",
        ),
    )
    .expect("generate");
    let output = all(&files);
    assert!(output.contains("Option<long> optional"));
    assert!(!output.contains("internal CoflowFunctionEntry"));
    assert!(output.contains("public Result<Unit, global::Failure> run(global::Coflow.Runtime.Coflow coflow, string input)"));
    assert!(!output.contains("BindRun"));
    assert!(output.contains("CoflowInvoker.Invoke<string, Result<Unit, global::Failure>>"));
    assert!(output.contains("public Api("));
    assert!(!output.contains("Func<string, Result<Unit, Failure>> Run { get;"));
}

#[test]
fn loads_function_values_nested_in_collections_and_option() {
    let files = generate_csharp_cfd(
        &schema(
            "type Pipeline { handlers: [fn(int) -> int]; named: {string: fn(string) -> bool}; optional: Option<fn(int) -> int> = None; }",
        ),
        BTreeMap::new(),
        None,
    )
    .expect("generate nested function values");
    let output = all(&files);

    assert!(output.contains("IReadOnlyList<CoflowFunction<long, long>> handlers"));
    assert!(output.contains("IReadOnlyDictionary<string, CoflowFunction<string, bool>> named"));
    assert!(output.contains("Option<CoflowFunction<long, long>> optional"));
    assert!(output.contains("context.FunctionValue<CoflowFunction<long, long>>(item, typeof(long)"));
    assert!(
        output.contains("context.FunctionValue<CoflowFunction<string, bool>>(item, typeof(bool)")
    );
    assert!(!output.contains("CoflowDelegateAdapter"));
}

#[test]
fn ordinary_function_loader_marks_the_cfd_body_as_required() {
    let files = generate_csharp_cfd(
        &schema(
            "type Rule { evaluate: fn(value: int) -> int; notify: fn(message: string) -> (); }",
        ),
        BTreeMap::new(),
        None,
    )
    .expect("generate");
    let output = all(&files);
    assert!(output.contains(
        "context.RequiredFunction(CfdValueReader.FindField(fields, \"evaluate\"), \"evaluate\", typeof(long), typeof(long))"
    ));
    assert!(
        output.contains("public long evaluate(global::Coflow.Runtime.Coflow coflow, long value)")
    );
    assert!(
        output.contains("public void notify(global::Coflow.Runtime.Coflow coflow, string message)")
    );
    assert!(!output.contains("public void Configure("));
    assert!(!output.contains("BindEvaluate"));
    assert!(!output.contains("BindNotify"));
}

#[test]
fn function_defaults_use_the_runtime_function_entry_path() {
    let files = generate_csharp_cfd(
        &schema(
            "type Rule { evaluate: fn(value: int) -> int = fn(value: int) -> int { value + 1 }; }",
        ),
        BTreeMap::new(),
        None,
    )
    .expect("generate function default");
    let output = all(&files);
    assert!(output.contains("context.DefaultFunction(\"fn(value: int) -> int { value + 1 }\", \"evaluate\", typeof(long), typeof(long))"));
    assert!(output.contains("CfdValueReader.FindField(fields, \"evaluate\") is { } valueevaluate"));
    assert!(output.contains("context.RequiredFunction(valueevaluate, \"evaluate\""));
}

#[test]
fn generated_metadata_has_no_physical_source_paths() {
    let schema = schema("type Item { value: int; }");
    let files = generate_csharp_cfd(&schema, BTreeMap::new(), None).expect("generate");
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
        BTreeMap::new(),
        None,
    )
    .expect("generate");
    let output = all(&files);
    assert!(output.contains("ReadEnumCft_6974656D5F726172697479"));
    assert!(output.contains("\"common_value\" or \"item_rarity::common_value\""));
    assert!(output.contains("ReadEnumCft_6974656D5F666C616773"));
    assert!(output.contains(" 3L"));
    assert!(output.contains("CoflowFieldBinding.CreateEnum<global::Item, global::item_rarity>"));
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
@struct sealed type Offset { value: int; }
type Item {
    offset: Offset = Offset { value: 5 };
    rarity: item_rarity = item_rarity::common_value;
    enabled: bool = false;
    label: string = "line\ntext";
    tags: [string] = [];
    weights: {string: int} = {};
    stats: Stats = {};
    target: Option<&Item> = None;
    fallback: Option<int> = Some(4);
}
"#,
        ),
        BTreeMap::new(),
        None,
    )
    .expect("generate");
    let output = all(&files);
    assert!(output.contains("CfdValueReader.FindField(fields, \"enabled\")"));
    assert!(output.contains("valueenabled ? CfdValueReader.Boolean(valueenabled) : false"));
    assert!(output.contains("item_rarity.common_value"));
    assert!(output.contains("\"line\\ntext\""));
    assert!(output.contains("CoflowConstantValues.List<string>()"));
    assert!(output.contains("CoflowConstantValues.Dictionary<string, long>()"));
    assert!(output.contains("new global::Stats(null, string.Empty, 10L)"));
    assert!(output.contains("new global::Offset(5L)"));
    assert!(output.contains("valuetarget") && output.contains("Option<global::Item>.None"));
    assert!(output.contains("Option<long>.Some(4L)"));
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
        BTreeMap::new(),
        None,
    )
    .expect("generate");
    let output = all(&files);
    assert!(output.contains("DeclaredType => \"fixed_reward\""));
    assert!(output
        .contains("AssignableTypes { get; } = new string[] { \"fixed_reward\", \"reward_base\" }"));
    assert!(output.contains("\"fixed_reward\" => ReadCft_66697865645F726577617264(node, context)"));
    assert!(output.contains("var objectType = CfdValueReader.ObjectDeclaredType(node);"));
    assert!(!output.contains("CfdObjectValue"));
    assert!(!output.contains("CfdDictionaryValue"));
    assert!(!output.contains("CfdNoneValue"));
    assert!(!output.contains("Readfixed_reward"));
    assert!(output.contains(
        "CfdValueReader.Reference<global::reward_base>(CfdValueReader.Field(fields, \"target\"), context, \"reward_base\")"
    ));
    assert!(!output.contains("expected a polymorphic object or reference"));
    assert!(output.contains("false, \"reward_base\""));
    assert!(output.contains("false, null, \"reward_base\""));
    assert!(!output.contains("ObjectFieldType(string fieldName)"));
    assert!(!output.contains("ReferenceFieldType(string fieldName)"));
    assert!(output.contains("IReadOnlyList<CoflowFieldMetadata> Fields"));
    assert!(output
        .contains("\"concrete_child\" => ReadCft_636F6E63726574655F6368696C64(node, context)"));
    assert!(output.contains("null or \"concrete_base\" =>"));
    assert!(output.contains(
        "CfdValueReader.Object(node, context, \"concrete_base\", ReadCft_636F6E63726574655F62617365Fields)"
    ));
}

#[test]
fn emits_cft_types_in_the_global_namespace() {
    let files = generate_csharp_cfd(
        &schema(
            r#"
enum Rarity { Common }
type Item { rarity: Rarity = Rarity::Common; }
"#,
        ),
        BTreeMap::new(),
        None,
    )
    .expect("generate");

    let item = files
        .iter()
        .find(|file| file.relative_path.as_os_str() == "Item.cs")
        .expect("Item file");
    assert!(!item.contents.contains("namespace "));

    let output = all(&files);
    assert!(output.contains("DeclaredType => \"Item\""));
    assert!(output.contains("typeof(global::Item)"));
    assert!(output.contains("global::Rarity.Common"));
    assert!(output.contains("ReadEnumCft_526172697479"));
}

#[test]
fn registry_generator_returns_safe_code_artifacts() {
    let mut registry = coflow_codegen::CodegenRegistry::default();
    registry
        .register(CsharpCfdCodeGenerator)
        .expect("register C#");
    assert!(registry.get("csharp").is_some());
    assert!(registry.register(CsharpCfdCodeGenerator).is_err());
}

#[test]
fn namespace_qualifies_references_without_changing_source_names() {
    let modules = parse_modules([CftFile::from_source(ModuleId::from("main"),
        "enum Rarity { Common } type Item { rarity: Rarity = Rarity::Common; @localized title: string; target: Option<&Item> = None; }")]);
    let dimensions =
        CftDimensionInputs::try_new([("language", vec!["en".into()])]).expect("dimensions");
    let schema = build_schema(&modules, &dimensions).expect("schema");
    let files = generate_csharp_cfd_with_variants(&schema, BTreeMap::new(), None, "Game.Config")
        .expect("generate namespaced code");
    assert!(files
        .iter()
        .all(|file| file.contents.replace("\r\n", "\n").contains("namespace Game.Config\n{")));
    let output = all(&files);
    assert!(output.contains("typeof(global::Game.Config.Item)"));
    assert!(output.contains("global::Game.Config.Rarity.Common"));
    assert!(output.contains("Option<global::Game.Config.Item>"));
    assert!(output.contains("DeclaredType => \"Item\""));
    assert!(!output.contains("global::Item"));
    assert!(files
        .iter()
        .any(|file| file.relative_path == std::path::Path::new("Dimensions.cs")));
}

#[test]
fn invalid_namespaces_are_rejected() {
    let schema = schema("type Item { value: int; }");
    for namespace in [
        "Game..Config",
        ".Game",
        "Game.",
        "Game.class",
        "Game Config",
        "1Game",
        "Game;class Injected {}",
    ] {
        let error = generate_csharp_cfd_with_variants(&schema, BTreeMap::new(), None, namespace)
            .expect_err("invalid namespace");
        assert!(error.to_string().contains("invalid C# namespace"));
    }
}
