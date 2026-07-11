use super::*;

#[test]

fn data_model_schema_projection_uses_cft_compiler_context() {

    let compiler_context =

        std::fs::read_to_string("crates/coflow-data-model/src/compiler_context.rs")

            .expect("read data-model compiler context");



    assert!(

        compiler_context.contains("cft: &'a CompiledSchema")
            && compiler_context.contains("cft_view = schema.compiled_schema()"),

        "data-model schema projection should borrow the canonical coflow-cft CompiledSchema"

    );

    assert!(

        compiler_context.contains("CftTypeMeta"),

        "data-model schema view should reuse coflow-cft type metadata instead of local type metadata"

    );

    assert!(

        compiler_context.contains("CftFieldMeta") && compiler_context.contains("CftSchemaTypeRef"),

        "data-model schema view should reuse coflow-cft field metadata and type refs"

    );

    for forbidden in [

        "schema.all_types()",

        "schema.all_enums()",

        "pub(crate) struct TypeMeta",

        "pub(crate) struct FieldMeta",

        "pub(crate) enum CfdType",

        "pub(crate) struct EnumMeta",

        "pub(crate) types:",

        "pub(crate) enums:",

        "children:",

        ".types.get(",

        ".types.values()",

        ".types.keys()",

        "CftSchemaType,",

        "CftSchemaEnum,",

        "CftSchemaField,",

    ] {

        assert!(

            !compiler_context.contains(forbidden),

            "data-model schema view should not rebuild schema projection from `{forbidden}`"

        );

    }

}



#[test]

fn data_model_value_semantics_uses_cft_compiler_context() {

    let value_semantics =

        std::fs::read_to_string("crates/coflow-data-model/src/value_semantics.rs")

            .expect("read data-model value semantics");



    assert!(

        value_semantics.contains("schema: &CompiledSchema"),

        "data-model value semantics should accept coflow-cft CompiledSchema as its schema query"

    );

    for expected in [

        "pub fn validate_complete_value_for_schema",

        "pub fn validate_fragment_value_for_schema",

    ] {

        assert!(

            value_semantics.contains(expected),

            "data-model value semantics should expose explicit completeness interface `{expected}`"

        );

    }

    for forbidden in [

        "CompiledSchema::new(schema)",

        ".resolve_type(",

        ".has_enum(",

        ".all_fields",

        ".types.get(",

        ".enums.contains_key(",

    ] {

        assert!(

            !value_semantics.contains(forbidden),

            "data-model value semantics should not use raw schema query `{forbidden}`"

        );

    }

}



#[test]

fn data_model_runtime_model_is_split_by_responsibility() {

    let model =

        std::fs::read_to_string("crates/coflow-data-model/src/model.rs").expect("read model");

    let ids =

        std::fs::read_to_string("crates/coflow-data-model/src/model/ids.rs").expect("read ids");

    let domain = std::fs::read_to_string("crates/coflow-data-model/src/model/domain.rs")

        .expect("read domain");

    let dimensions = std::fs::read_to_string("crates/coflow-data-model/src/model/dimensions.rs")

        .expect("read dimensions");

    let edges =

        std::fs::read_to_string("crates/coflow-data-model/src/model/edges.rs").expect("read edges");

    let tables = std::fs::read_to_string("crates/coflow-data-model/src/model/tables.rs")

        .expect("read tables");

    let value =

        std::fs::read_to_string("crates/coflow-data-model/src/model/value.rs").expect("read value");

    let input =

        std::fs::read_to_string("crates/coflow-data-model/src/model/input.rs").expect("read input");



    for expected in [

        "pub struct CfdTypeId",

        "pub struct CfdDomainId",

        "pub struct CfdRecordId",

    ] {

        assert!(

            ids.contains(expected),

            "data-model id type `{expected}` should live in model/ids.rs"

        );

        assert!(

            !model.contains(expected),

            "data-model id type `{expected}` should not live in model.rs"

        );

    }

    for expected in ["pub struct CfdDomainIndex"] {

        assert!(

            domain.contains(expected),

            "data-model domain type `{expected}` should live in model/domain.rs"

        );

        assert!(

            !model.contains(expected),

            "data-model domain type `{expected}` should not live in model.rs"

        );

    }

    for expected in [

        "pub enum DimensionFieldLookupError",

        "pub struct DimensionFieldValue",

        "pub fn dimension_field_value",

    ] {

        assert!(

            dimensions.contains(expected),

            "data-model dimension helper `{expected}` should live in model/dimensions.rs"

        );

        assert!(

            !model.contains(expected),

            "data-model dimension helper `{expected}` should not live in model.rs"

        );

    }

    for expected in [

        "pub struct RefSite",

        "pub struct RefEdge",

        "pub struct SpreadSite",

        "pub struct SpreadEdge",

    ] {

        assert!(

            edges.contains(expected),

            "data-model edge type `{expected}` should live in model/edges.rs"

        );

        assert!(

            !model.contains(expected),

            "data-model edge type `{expected}` should not live in model.rs"

        );

    }

    for expected in ["pub struct CfdTable", "pub struct CfdPolymorphicIndex"] {

        assert!(

            tables.contains(expected),

            "data-model table type `{expected}` should live in model/tables.rs"

        );

        assert!(

            !model.contains(expected),

            "data-model table type `{expected}` should not live in model.rs"

        );

    }

    for expected in [

        "pub struct CfdRecord",

        "pub struct CfdObject",

        "pub enum CfdValue",

        "pub enum CfdDictKey",

        "pub struct CfdEnumValue",

    ] {

        assert!(

            value.contains(expected),

            "data-model value type `{expected}` should live in model/value.rs"

        );

        assert!(

            !model.contains(expected),

            "data-model value type `{expected}` should not live in model.rs"

        );

    }

    for expected in [

        "pub struct CfdInputRecord",

        "pub enum CfdInputValue",

        "pub enum CfdInputDictKey",

    ] {

        assert!(

            input.contains(expected),

            "data-model input type `{expected}` should live in model/input.rs"

        );

        assert!(

            !model.contains(expected),

            "data-model input type `{expected}` should not live in model.rs"

        );

    }

    assert!(

        model.lines().count() < 800,

        "coflow-data-model model.rs should stay below the 800-line large-module threshold"

    );

}



#[test]

fn data_model_compiler_indexes_are_split_out() {

    let compiler = std::fs::read_to_string("crates/coflow-data-model/src/compiler.rs")

        .expect("read data-model compiler");

    let indexes = std::fs::read_to_string("crates/coflow-data-model/src/compiler/indexes.rs")

        .expect("read data-model compiler indexes");

    let resolve = std::fs::read_to_string("crates/coflow-data-model/src/compiler/resolve.rs")

        .expect("read data-model compiler resolve");

    let defaults = std::fs::read_to_string("crates/coflow-data-model/src/compiler/defaults.rs")

        .expect("read data-model compiler defaults");

    let validate = std::fs::read_to_string("crates/coflow-data-model/src/compiler/validate.rs")

        .expect("read data-model compiler validation");

    let dicts = std::fs::read_to_string("crates/coflow-data-model/src/compiler/validate/dicts.rs")

        .expect("read data-model compiler dict validation");



    for expected in [

        "pub(super) struct ModelIndexes",

        "pub(super) fn build_indexes",

        "fn add_polymorphic_ids",

        "pub(super) fn validate_singletons",

    ] {

        assert!(

            indexes.contains(expected),

            "data-model compiler index helper `{expected}` should live in compiler/indexes.rs"

        );

        assert!(

            !compiler.contains(expected),

            "data-model compiler index helper `{expected}` should not live in compiler.rs"

        );

    }

    for expected in [

        "pub(super) struct ValueResolver",

        "pub(super) fn resolve_record_fields",

        "fn resolve_node",

        "fn resolve_value",

        "fn resolve_ref_target",

        "fn resolve_dict_spread",

        "fn resolve_spread_field",

    ] {

        assert!(

            resolve.contains(expected),

            "data-model compiler resolve helper `{expected}` should live in compiler/resolve.rs"

        );

        assert!(

            !compiler.contains(expected),

            "data-model compiler resolve helper `{expected}` should not live in compiler.rs"

        );

    }

    for forbidden in ["fn resolve_fields", "fn resolve_ref_target", "fn resolve_spread_field"] {

        assert!(

            !validate.contains(forbidden),

            "data-model validation should not own value resolution helper `{forbidden}`"

        );

    }

    for expected in [

        "pub(super) fn default_field_value",

        "fn default_value",

        "fn default_object_value",

        "fn push_default_type_mismatch",

        "fn non_nullable_type",

        "CftSchemaDefaultValue",

    ] {

        assert!(

            defaults.contains(expected),

            "data-model compiler default helper `{expected}` should live in compiler/defaults.rs"

        );

        assert!(

            !compiler.contains(expected),

            "data-model compiler default helper `{expected}` should not live in compiler.rs"

        );

    }

    for expected in [

        "pub(super) struct Validator",

        "pub(super) fn validate_record",

        "pub(super) fn validate_value",

        "fn top_level_spread_source",

    ] {

        assert!(

            validate.contains(expected),

            "data-model compiler validation helper `{expected}` should live in compiler/validate.rs"

        );

        assert!(

            !compiler.contains(expected),

            "data-model compiler validation helper `{expected}` should not live in compiler.rs"

        );

    }

    for expected in [

        "pub(super) fn validate_dict_entries",

        "fn validate_dict_key",

    ] {

        assert!(

            dicts.contains(expected),

            "data-model compiler dict validation helper `{expected}` should live in compiler/validate/dicts.rs"

        );

        assert!(

            !validate.contains(expected),

            "data-model compiler dict validation helper `{expected}` should not live in compiler/validate.rs"

        );

        assert!(

            !compiler.contains(expected),

            "data-model compiler dict validation helper `{expected}` should not live in compiler.rs"

        );

    }

    assert!(

        compiler.lines().count() < 800,

        "coflow-data-model compiler.rs should stay below the 800-line large-module threshold"

    );

}



#[test]

fn cft_schema_compiler_symbols_are_split_out() {

    let compiler = std::fs::read_to_string("crates/coflow-cft/src/schema/compiler.rs")

        .expect("read CFT schema compiler");

    let symbols = std::fs::read_to_string("crates/coflow-cft/src/schema/compiler/symbols.rs")

        .expect("read CFT schema compiler symbols");

    let annotations =

        std::fs::read_to_string("crates/coflow-cft/src/schema/compiler/annotations.rs")

            .expect("read CFT schema compiler annotations");

    let build = std::fs::read_to_string("crates/coflow-cft/src/schema/compiler/build.rs")

        .expect("read CFT schema compiler build projection");

    let defaults = std::fs::read_to_string("crates/coflow-cft/src/schema/compiler/defaults.rs")

        .expect("read CFT schema compiler defaults");

    let types = std::fs::read_to_string("crates/coflow-cft/src/schema/compiler/types.rs")

        .expect("read CFT schema compiler type validation");



    for expected in [

        "pub(super) fn report_dangling_annotations",

        "pub(super) fn collect_symbols",

        "pub(super) fn validate_identifier",

        "fn insert_symbol",

        "pub(super) fn validate_enums",

    ] {

        assert!(

            symbols.contains(expected),

            "CFT schema compiler symbol helper `{expected}` should live in schema/compiler/symbols.rs"

        );

        assert!(

            !compiler.contains(expected),

            "CFT schema compiler symbol helper `{expected}` should not live in schema/compiler.rs"

        );

    }

    for expected in [

        "pub(super) fn validate_annotations",

        "fn register_id_as_enum_name",

        "fn validate_annotation_list",

        "fn validate_field_annotations",

        "fn validate_id_as_enum_name",

    ] {

        assert!(

            annotations.contains(expected),

            "CFT schema compiler annotation helper `{expected}` should live in schema/compiler/annotations.rs"

        );

        assert!(

            !compiler.contains(expected),

            "CFT schema compiler annotation helper `{expected}` should not live in schema/compiler.rs"

        );

    }

    for expected in [

        "pub(super) fn build_schema",

        "fn build_schema_field",

        "fn collect_all_schema_fields",

        "fn schema_default_value",

        "fn enum_variant_value",

        "fn localized_bucket",

    ] {

        assert!(

            build.contains(expected),

            "CFT schema compiler build helper `{expected}` should live in schema/compiler/build.rs"

        );

        assert!(

            !compiler.contains(expected),

            "CFT schema compiler build helper `{expected}` should not live in schema/compiler.rs"

        );

    }

    for expected in [

        "pub(super) fn validate_defaults",

        "fn default_expr_type",

        "fn default_enum_variant_type",

    ] {

        assert!(

            defaults.contains(expected),

            "CFT schema compiler default helper `{expected}` should live in schema/compiler/defaults.rs"

        );

        assert!(

            !compiler.contains(expected),

            "CFT schema compiler default helper `{expected}` should not live in schema/compiler.rs"

        );

    }

    for expected in [

        "pub(super) fn validate_type_headers",

        "pub(super) fn validate_field_shapes",

        "pub(super) fn validate_inheritance",

        "fn report_inheritance_cycle",

        "pub(super) fn build_full_fields",

        "pub(super) fn ancestry_chain",

        "pub(super) fn resolve_field_type",

        "fn validate_field_type",

        "pub(super) fn collect_ancestor_fields",

    ] {

        assert!(

            types.contains(expected),

            "CFT schema compiler type helper `{expected}` should live in schema/compiler/types.rs"

        );

        assert!(

            !compiler.contains(expected),

            "CFT schema compiler type helper `{expected}` should not live in schema/compiler.rs"

        );

    }

    assert!(

        compiler.lines().count() < 260,

        "coflow-cft schema/compiler.rs should stay focused on phase orchestration and shared utilities"

    );

}



#[test]

fn cft_type_checker_operator_rules_are_split_out() {

    let type_checker = std::fs::read_to_string("crates/coflow-cft/src/schema/type_checker.rs")

        .expect("read CFT type checker");

    let ops = std::fs::read_to_string("crates/coflow-cft/src/schema/type_checker/ops.rs")

        .expect("read CFT type checker operators");



    for expected in [

        "pub(super) fn check_unary",

        "pub(super) fn check_binop",

        "pub(super) fn check_comparison",

        "fn operator_mismatch",

        "fn is_flag_enum",

    ] {

        assert!(

            ops.contains(expected),

            "CFT type checker operator helper `{expected}` should live in schema/type_checker/ops.rs"

        );

        assert!(

            !type_checker.contains(expected),

            "CFT type checker operator helper `{expected}` should not live in schema/type_checker.rs"

        );

    }

    assert!(

        type_checker.lines().count() < 800,

        "coflow-cft schema/type_checker.rs should stay below the 800-line large-module threshold"

    );

}



#[test]

fn cft_type_checker_function_rules_are_split_out() {

    let type_checker = std::fs::read_to_string("crates/coflow-cft/src/schema/type_checker.rs")

        .expect("read CFT type checker");

    let functions =

        std::fs::read_to_string("crates/coflow-cft/src/schema/type_checker/functions.rs")

            .expect("read CFT type checker function rules");



    for expected in [

        "pub(super) fn check_call",

        "pub(super) fn check_method_call",

        "fn check_contains_method",

        "fn check_matches_method",

        "fn expect_arity",

    ] {

        assert!(

            functions.contains(expected),

            "CFT type checker function helper `{expected}` should live in schema/type_checker/functions.rs"

        );

        assert!(

            !type_checker.contains(expected),

            "CFT type checker function helper `{expected}` should not live in schema/type_checker.rs"

        );

    }

    assert!(

        type_checker.lines().count() < 500,

        "coflow-cft schema/type_checker.rs should stay focused on check expression dispatch"

    );

}



#[test]

fn cft_compiled_schema_dimension_check_analysis_is_split_out() {

    let compiled_schema =

        std::fs::read_to_string("crates/coflow-cft/src/compiled_schema.rs").expect("read schema view");

    let dimension_checks =

        std::fs::read_to_string("crates/coflow-cft/src/compiled_schema/dimension_checks.rs")

            .expect("read schema view dimension check analysis");

    let queries = std::fs::read_to_string("crates/coflow-cft/src/compiled_schema/queries.rs")

        .expect("read schema view query helpers");



    for expected in [

        "pub(super) fn dimension_checks_for_type",

        "struct DimensionCheckAnalyzer",

        "fn stmt_dimensions",

        "fn expr_dimensions",

        "fn name_dimensions",

    ] {

        assert!(

            dimension_checks.contains(expected),

            "CFT schema view dimension check helper `{expected}` should live in compiled_schema/dimension_checks.rs"

        );

        assert!(

            !compiled_schema.contains(expected),

            "CFT schema view dimension check helper `{expected}` should not live in compiled_schema.rs"

        );

    }

    for forbidden in ["pub consts:", "pub types:", "pub enums:"] {

        assert!(

            !compiled_schema.contains(forbidden),

            "CFT schema view should expose query methods instead of public map field `{forbidden}`"

        );

    }

    assert!(
        !compiled_schema.contains("pub fn new("),
        "CompiledSchema must only be published by CftContainer compilation"
    );

    for forbidden in [

        "pub fields: BTreeMap",

        "pub dimension_fields: BTreeMap",

        "pub variants: BTreeMap",

    ] {

        assert!(

            !compiled_schema.contains(forbidden),

            "CFT schema metadata should not expose lookup index field `{forbidden}`"

        );

    }

    for expected in [

        "pub fn type_is_struct",

        "pub fn type_id_as_enum",

        "pub fn inherited_id_as_enum",

        "pub fn is_id_as_enum",

        "pub fn id_as_enum_names",

        "pub fn ref_target_names",

        "fn collect_ref_targets_for_type",

        "fn annotation_name_arg",

    ] {

        assert!(

            queries.contains(expected),

            "CFT schema query helper `{expected}` should live in compiled_schema/queries.rs"

        );

        assert!(

            !compiled_schema.contains(expected),

            "CFT schema query helper `{expected}` should not bloat compiled_schema.rs"

        );

    }

}



#[test]

fn cft_parser_check_expression_parser_is_split_out() {

    let parser =

        std::fs::read_to_string("crates/coflow-cft/src/parser.rs").expect("read CFT parser");

    let annotations = std::fs::read_to_string("crates/coflow-cft/src/parser/annotations.rs")

        .expect("read annotation parser");

    let check = std::fs::read_to_string("crates/coflow-cft/src/parser/check.rs")

        .expect("read check parser");

    let check_primary = std::fs::read_to_string("crates/coflow-cft/src/parser/check_primary.rs")

        .expect("read check primary parser");

    let defaults = std::fs::read_to_string("crates/coflow-cft/src/parser/defaults.rs")

        .expect("read default parser");

    let definitions = std::fs::read_to_string("crates/coflow-cft/src/parser/definitions.rs")

        .expect("read definition parser");

    let literals = std::fs::read_to_string("crates/coflow-cft/src/parser/literals.rs")

        .expect("read literal parser");

    let tokens = std::fs::read_to_string("crates/coflow-cft/src/parser/tokens.rs")

        .expect("read parser tokens");



    for expected in [

        "pub(super) fn parse_check_block",

        "fn parse_check_stmts",

        "fn parse_or_expr",

        "fn validate_cmp_chain",

        "enum CmpChainGroup",

    ] {

        assert!(

            check.contains(expected),

            "CFT parser check helper `{expected}` should live in parser/check.rs"

        );

        assert!(

            !parser.contains(expected),

            "CFT parser check helper `{expected}` should not live in parser.rs"

        );

    }

    for expected in [

        "pub(super) fn parse_postfix_expr",

        "fn parse_primary_expr",

        "pub(super) fn parse_type_predicate",

    ] {

        assert!(

            check_primary.contains(expected),

            "CFT parser primary check helper `{expected}` should live in parser/check_primary.rs"

        );

        assert!(

            !check.contains(expected),

            "CFT parser primary check helper `{expected}` should not live in parser/check.rs"

        );

        assert!(

            !parser.contains(expected),

            "CFT parser primary check helper `{expected}` should not live in parser.rs"

        );

    }

    for expected in [

        "pub(super) fn parse_annotation",

        "fn parse_annotation_arg",

        "fn parse_annotation_arg_for",

    ] {

        assert!(

            annotations.contains(expected),

            "CFT parser annotation helper `{expected}` should live in parser/annotations.rs"

        );

        assert!(

            !parser.contains(expected),

            "CFT parser annotation helper `{expected}` should not live in parser.rs"

        );

    }

    for expected in [

        "pub(super) fn parse_default_expr",

        "fn parse_negative_default",

        "fn parse_name_or_enum_default",

        "fn parse_array_default",

        "fn parse_object_default",

    ] {

        assert!(

            defaults.contains(expected),

            "CFT parser default helper `{expected}` should live in parser/defaults.rs"

        );

        assert!(

            !parser.contains(expected),

            "CFT parser default helper `{expected}` should not live in parser.rs"

        );

    }

    for expected in [

        "pub(super) fn parse_const",

        "pub(super) fn parse_enum",

        "pub(super) fn parse_type",

        "fn parse_field",

        "pub(super) fn parse_type_ref",

        "fn parse_type_ref_primary",

    ] {

        assert!(

            definitions.contains(expected),

            "CFT parser definition helper `{expected}` should live in parser/definitions.rs"

        );

        assert!(

            !parser.contains(expected),

            "CFT parser definition helper `{expected}` should not live in parser.rs"

        );

    }

    for expected in [

        "pub(super) fn parse_const_literal",

        "fn parse_negative_const_literal",

        "pub(super) fn parse_signed_int",

    ] {

        assert!(

            literals.contains(expected),

            "CFT parser literal helper `{expected}` should live in parser/literals.rs"

        );

        assert!(

            !parser.contains(expected),

            "CFT parser literal helper `{expected}` should not live in parser.rs"

        );

    }

    for expected in [

        "pub(super) fn token_name",

        "pub(super) fn reserved_keyword_name",

    ] {

        assert!(

            tokens.contains(expected),

            "CFT parser token helper `{expected}` should live in parser/tokens.rs"

        );

        assert!(

            !parser.contains(expected),

            "CFT parser token helper `{expected}` should not live in parser.rs"

        );

    }

    assert!(

        parser.lines().count() < 220,

        "coflow-cft parser.rs should stay focused on entrypoint and token cursor utilities"

    );

}



#[test]

fn csharp_codegen_schema_projection_uses_cft_schema_context() {

    let schema_context =

        std::fs::read_to_string("crates/coflow-codegen-csharp/src/schema_context.rs")

            .expect("read C# codegen schema context");

    let ir = std::fs::read_to_string("crates/coflow-codegen-csharp/src/ir.rs").expect("read C# IR");

    let lib =

        std::fs::read_to_string("crates/coflow-codegen-csharp/src/lib.rs").expect("read C# lib");

    let emit =

        std::fs::read_to_string("crates/coflow-codegen-csharp/src/emit.rs").expect("read C# emit");

    let codegen = format!("{schema_context}\n{ir}\n{lib}\n{emit}");



    assert!(

        schema_context.contains("pub fn new(schema: &CompiledSchema)")

            && ir.contains("schema: &CompiledSchema")

            && lib.contains("schema: &CompiledSchema"),

        "C# codegen should receive coflow-cft CompiledSchema instead of full schema container"

    );

    for forbidden in [

        "CftContainer",

        "schema.all_types()",

        "schema.all_enums()",

        "pub fn all_types",

        "pub fn all_enums",

        "pub struct TypeMeta",

        "pub struct FieldMeta",

        "pub enum FieldType",

        "children:",

        "fill_concrete_descendants",

        "pub enums:",

        "view.enums",

        ".types.get(",

        ".types.values()",

        ".types.keys()",

        ".enums.get(",

        ".enums.values()",

        ".enums.keys()",

        ".enums.contains_key(",

        "CftSchemaType,",

        "CftSchemaEnum,",

        "CftSchemaField,",

    ] {

        assert!(

            !codegen.contains(forbidden),

            "C# codegen should not rebuild schema projection from `{forbidden}`"

        );

    }

    assert!(

        codegen.contains("CftTypeMeta") && codegen.contains("CftFieldMeta"),

        "C# codegen should consume coflow-cft type/field metadata directly instead of defining local copies"

    );

    for expected in [

        "self.cft.id_as_enum_names()",

        "self.cft.inherited_id_as_enum(type_name)",

        "self.cft.ref_target_names()",

        "self.cft.type_is_struct(&ty.name)",

    ] {

        assert!(

            schema_context.contains(expected),

            "C# codegen schema facade should delegate `{expected}` to coflow-cft"

        );

    }

    for forbidden in [

        "fn type_id_as_enum",

        "fn collect_ref_targets_for_type",

        "fn collect_ref_targets_in_field",

        "fn collect_ref_targets_in_type",

        "annotation_name_arg(&ty.annotations",

        "has_annotation(&ty.annotations",

    ] {

        assert!(

            !schema_context.contains(forbidden),

            "C# codegen schema facade should not keep local schema semantic helper `{forbidden}`"

        );

    }

    assert!(

        codegen.contains("CftSchemaTypeRef"),

        "C# codegen emit path should consume coflow-cft type refs directly"

    );

}



#[test]

fn cft_lexer_tokens_are_split_out() {

    let lexer = std::fs::read_to_string("crates/coflow-cft/src/lexer.rs").expect("read CFT lexer");

    let tokens = std::fs::read_to_string("crates/coflow-cft/src/lexer/tokens.rs")

        .expect("read CFT lexer tokens");



    for expected in ["pub enum TokenKind", "pub struct Token"] {

        assert!(

            tokens.contains(expected),

            "CFT lexer token item `{expected}` should live in lexer/tokens.rs"

        );

        assert!(

            !lexer.contains(expected),

            "CFT lexer token item `{expected}` should not live in lexer.rs"

        );

    }

    assert!(

        lexer.lines().count() < 450,

        "coflow-cft lexer.rs should stay below the 450-line focused-module threshold"

    );

}



#[test]

fn csharp_codegen_emit_type_helpers_do_not_live_in_emit_rs() {

    let emit =

        std::fs::read_to_string("crates/coflow-codegen-csharp/src/emit.rs").expect("read C# emit");

    let identifiers =

        std::fs::read_to_string("crates/coflow-codegen-csharp/src/emit/identifiers.rs")

            .expect("read C# emit identifiers");

    let types = std::fs::read_to_string("crates/coflow-codegen-csharp/src/emit/types.rs")

        .expect("read C# emit types");



    for expected in [

        "pub(super) fn field_local_name",

        "pub(super) fn loader_reserved_local_names",

        "pub(super) fn plural_records_var",

        "pub(super) fn context_index_field_name",

    ] {

        assert!(

            identifiers.contains(expected),

            "C# emit identifier helper `{expected}` should live in emit/identifiers.rs"

        );

        assert!(

            !emit.contains(expected),

            "C# emit identifier helper `{expected}` should not live in emit.rs"

        );

    }

    for expected in [

        "pub(super) fn csharp_type",

        "pub(super) fn csharp_field_property_type",

        "pub(super) fn csharp_property_type",

        "pub(super) fn default_value_expr",

        "pub(super) fn collection_default_expr",

    ] {

        assert!(

            types.contains(expected),

            "C# emit type helper `{expected}` should live in emit/types.rs"

        );

        assert!(

            !emit.contains(expected),

            "C# emit type helper `{expected}` should not live in emit.rs"

        );

    }

}



#[test]

fn csharp_codegen_emit_database_helpers_do_not_live_in_emit_rs() {

    let emit =

        std::fs::read_to_string("crates/coflow-codegen-csharp/src/emit.rs").expect("read C# emit");

    let database = std::fs::read_to_string("crates/coflow-codegen-csharp/src/emit/database.rs")

        .expect("read C# emit database");



    for expected in [

        "pub fn build_csharp_database",

        "fn build_context_lookups",

        "fn build_json_load_steps",

        "fn build_messagepack_load_steps",

        "fn build_table_model",

        "fn sort_tables_by_dependencies",

        "fn collect_table_dependencies",

    ] {

        assert!(

            database.contains(expected),

            "C# emit database helper `{expected}` should live in emit/database.rs"

        );

        assert!(

            !emit.contains(expected),

            "C# emit database helper `{expected}` should not live in emit.rs"

        );

    }

}



#[test]

fn csharp_codegen_emit_reader_helpers_do_not_live_in_emit_rs() {

    let emit =

        std::fs::read_to_string("crates/coflow-codegen-csharp/src/emit.rs").expect("read C# emit");

    let readers = std::fs::read_to_string("crates/coflow-codegen-csharp/src/emit/readers.rs")

        .expect("read C# emit readers");



    for expected in [

        "pub(super) fn read_field_expr",

        "pub(super) fn read_token_expr",

        "pub(super) fn read_messagepack_field_expr",

        "pub(super) fn read_messagepack_expr",

        "fn read_dict_key_expr",

        "fn read_messagepack_dict_key_expr",

    ] {

        assert!(

            readers.contains(expected),

            "C# emit reader helper `{expected}` should live in emit/readers.rs"

        );

        assert!(

            !emit.contains(expected),

            "C# emit reader helper `{expected}` should not live in emit.rs"

        );

    }

}



#[test]

fn csharp_codegen_emit_loader_helpers_do_not_live_in_emit_rs() {

    let emit =

        std::fs::read_to_string("crates/coflow-codegen-csharp/src/emit.rs").expect("read C# emit");

    let loaders = std::fs::read_to_string("crates/coflow-codegen-csharp/src/emit/loaders.rs")

        .expect("read C# emit loaders");



    for expected in [

        "pub(super) fn loader_method",

        "pub(super) fn polymorphic_loader",

        "fn polymorphic_cases",

        "fn load_field",

        "pub(super) fn field_type_requires_context",

        "fn field_type_requires_context_inner",

    ] {

        assert!(

            loaders.contains(expected),

            "C# emit loader helper `{expected}` should live in emit/loaders.rs"

        );

        assert!(

            !emit.contains(expected),

            "C# emit loader helper `{expected}` should not live in emit.rs"

        );

    }

    assert!(

        emit.lines().count() < 320,

        "coflow-codegen-csharp emit.rs should stay below the 320-line focused-module threshold"

    );

}



#[test]

fn exporter_core_schema_projection_uses_cft_compiler_context() {

    let exporter =

        std::fs::read_to_string("crates/coflow-exporter-core/src/lib.rs").expect("read exporter");



    assert!(

        exporter.contains("schema: &CompiledSchema"),

        "exporter core schema traversal should receive coflow-cft CompiledSchema"

    );

    for forbidden in [

        "CftContainer",

        "schema.all_types()",

        "schema.all_enums()",

        "schema.resolve_type(",

        "CftSchemaField",

        "struct FieldMeta",

        "struct DataModelCompilerContext",

    ] {

        assert!(

            !exporter.contains(forbidden),

            "exporter core should not rebuild schema traversal from `{forbidden}`"

        );

    }

}



