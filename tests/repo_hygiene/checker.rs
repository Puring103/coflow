use super::*;

#[test]

fn checker_diagnostic_rendering_does_not_live_in_evaluator_rs() {

    let evaluator = std::fs::read_to_string("crates/coflow-checker/src/check/evaluator.rs")

        .expect("read checker evaluator");

    let diagnostics = std::fs::read_to_string("crates/coflow-checker/src/check/diagnostics.rs")

        .expect("read checker diagnostics");



    for expected in [

        "pub(super) struct CheckExplanation",

        "pub(super) fn render_stmt",

        "pub(super) fn render_expr",

        "pub(super) fn format_value_for_message",

        "pub(super) fn dimension_lookup_error_message",

    ] {

        assert!(

            diagnostics.contains(expected),

            "checker diagnostic helper `{expected}` should live in check/diagnostics.rs"

        );

        assert!(

            !evaluator.contains(expected),

            "checker diagnostic helper `{expected}` should not live in evaluator.rs"

        );

    }

}



#[test]

fn checker_numeric_ops_do_not_live_in_evaluator_rs() {

    let evaluator = std::fs::read_to_string("crates/coflow-checker/src/check/evaluator.rs")

        .expect("read checker evaluator");

    let ops = std::fs::read_to_string("crates/coflow-checker/src/check/ops.rs")

        .expect("read checker ops");



    for expected in [

        "pub(super) fn checked_int",

        "pub(super) fn checked_shift",

        "pub(super) fn unary_op",

        "pub(super) fn compare",

        "pub(super) fn compare_order",

        "pub(super) fn eager_bin_op",

        "pub(super) fn expect_bool_operand",

    ] {

        assert!(

            ops.contains(expected),

            "checker operation helper `{expected}` should live in check/ops.rs"

        );

        assert!(

            !evaluator.contains(expected),

            "checker operation helper `{expected}` should not live in evaluator.rs"

        );

    }

    for forbidden in [

        "整数取负溢出",

        "不支持的一元运算",

        "整数加法溢出",

        "不支持的二元运算",

        "操作数不是 bool",

    ] {

        assert!(

            !evaluator.contains(forbidden),

            "checker evaluator should not own eager binary op branch `{forbidden}`"

        );

    }

}



#[test]

fn checker_builtin_value_helpers_do_not_live_in_evaluator_rs() {

    let evaluator = std::fs::read_to_string("crates/coflow-checker/src/check/evaluator.rs")

        .expect("read checker evaluator");

    let builtin_values =

        std::fs::read_to_string("crates/coflow-checker/src/check/builtin_values.rs")

            .expect("read checker builtin values");



    for expected in [

        "pub(super) fn len_value",

        "pub(super) fn contains_value",

        "pub(super) fn unique_value",

        "pub(super) fn keys_value",

        "pub(super) fn values_value",

        "pub(super) fn matches_value",

        "pub(super) fn min_max_value",

        "pub(super) fn sum_value",

    ] {

        assert!(

            builtin_values.contains(expected),

            "checker builtin value helper `{expected}` should live in check/builtin_values.rs"

        );

        assert!(

            !evaluator.contains(expected),

            "checker builtin value helper `{expected}` should not live in evaluator.rs"

        );

    }

}



#[test]

fn checker_builtin_call_helpers_do_not_live_in_evaluator_rs() {

    let evaluator = std::fs::read_to_string("crates/coflow-checker/src/check/evaluator.rs")

        .expect("read checker evaluator");

    let builtin_calls = std::fs::read_to_string("crates/coflow-checker/src/check/builtin_calls.rs")

        .expect("read checker builtin call helpers");



    for expected in [

        "pub(super) struct CallSignature",

        "pub(super) enum CallTarget",

        "pub(super) enum CallSignatureError",

        "pub(super) fn matches_pattern_arg",

        "fn require_arity",

    ] {

        assert!(

            builtin_calls.contains(expected),

            "checker builtin call helper `{expected}` should live in check/builtin_calls.rs"

        );

        assert!(

            !evaluator.contains(expected),

            "checker builtin call helper `{expected}` should not live in evaluator.rs"

        );

    }

    for forbidden in [

        "枚举构造函数需要 1 个参数",

        "matches 的 pattern 必须是字符串字面量",

    ] {

        assert!(

            !evaluator.contains(forbidden),

            "checker evaluator should not own builtin call validation branch `{forbidden}`"

        );

    }

}



#[test]

fn checker_dependency_collection_does_not_live_in_evaluator_or_runner_rs() {

    let evaluator = std::fs::read_to_string("crates/coflow-checker/src/check/evaluator.rs")

        .expect("read checker evaluator");

    let runner =

        std::fs::read_to_string("crates/coflow-checker/src/check/runner.rs").expect("read runner");

    let deps =

        std::fs::read_to_string("crates/coflow-checker/src/check/deps.rs").expect("read deps");



    for expected in [

        "pub(super) struct DependencyCollector",

        "pub(super) struct DependencyGraphBuilder",

    ] {

        assert!(

            deps.contains(expected),

            "checker dependency helper `{expected}` should live in check/deps.rs"

        );

        assert!(

            !evaluator.contains(expected) && !runner.contains(expected),

            "checker dependency helper `{expected}` should not live in evaluator.rs or runner.rs"

        );

    }

}



#[test]

fn checker_quantifier_item_expansion_does_not_live_in_evaluator_rs() {

    let evaluator = std::fs::read_to_string("crates/coflow-checker/src/check/evaluator.rs")

        .expect("read checker evaluator");

    let quantifiers = std::fs::read_to_string("crates/coflow-checker/src/check/quantifiers.rs")

        .expect("read checker quantifier helpers");



    assert!(

        quantifiers.contains("pub(super) fn quantifier_items"),

        "checker quantifier item expansion should live in check/quantifiers.rs"

    );

    assert!(

        !evaluator.contains("fn quantifier_items"),

        "checker quantifier item expansion should not live in evaluator.rs"

    );

    for forbidden in ["量词目标不是集合", "format_check_key_for_path"] {

        assert!(

            !evaluator.contains(forbidden),

            "checker evaluator should not own quantifier item expansion branch `{forbidden}`"

        );

    }

}



#[test]

fn checker_statement_execution_does_not_live_in_evaluator_rs() {

    let evaluator = std::fs::read_to_string("crates/coflow-checker/src/check/evaluator.rs")

        .expect("read checker evaluator");

    let statements = std::fs::read_to_string("crates/coflow-checker/src/check/statements.rs")

        .expect("read checker statements");



    for expected in [

        "pub(super) fn eval_check_block",

        "fn eval_stmts",

        "fn eval_stmt",

        "fn eval_quantifier_stmt",

        "fn rewrite_all_failures",

        "fn emit_any_failure",

        "fn emit_none_failures",

    ] {

        assert!(

            statements.contains(expected),

            "checker statement helper `{expected}` should live in check/statements.rs"

        );

        assert!(

            !evaluator.contains(expected),

            "checker statement helper `{expected}` should not live in evaluator.rs"

        );

    }

    for forbidden in [

        "check 表达式没有求值为 bool",

        "when 条件没有求值为 bool",

        "CheckAllQuantifierFailed",

        "CheckAnyQuantifierFailed",

        "CheckNoneQuantifierFailed",

    ] {

        assert!(

            !evaluator.contains(forbidden),

            "checker evaluator should not own statement execution branch `{forbidden}`"

        );

    }

}



#[test]

fn checker_expression_dispatch_does_not_live_in_evaluator_rs() {

    let evaluator = std::fs::read_to_string("crates/coflow-checker/src/check/evaluator.rs")

        .expect("read checker evaluator");

    let expressions = std::fs::read_to_string("crates/coflow-checker/src/check/expressions.rs")

        .expect("read checker expressions");



    for expected in [

        "pub(super) fn eval_expr",

        "fn eval_field_expr",

        "fn eval_index_expr",

        "fn eval_is_expr",

        "fn eval_cmp_chain_expr",

    ] {

        assert!(

            expressions.contains(expected),

            "checker expression helper `{expected}` should live in check/expressions.rs"

        );

    }

    for forbidden in [

        "CftSchemaCheckExprKind::Int",

        "CftSchemaCheckExprKind::Field",

        "CftSchemaCheckExprKind::Index",

        "CftSchemaCheckExprKind::CmpChain",

    ] {

        assert!(

            !evaluator.contains(forbidden),

            "checker evaluator should not own expression dispatch branch `{forbidden}`"

        );

    }

}



#[test]

fn checker_field_read_helpers_do_not_live_in_evaluator_rs() {

    let evaluator = std::fs::read_to_string("crates/coflow-checker/src/check/evaluator.rs")

        .expect("read checker evaluator");

    let fields = std::fs::read_to_string("crates/coflow-checker/src/check/fields.rs")

        .expect("read checker field helpers");



    for expected in [

        "pub(super) fn field_type_for_record",

        "pub(super) fn current_field",

        "pub(super) fn field_value",

        "pub(super) fn virtual_id",

    ] {

        assert!(

            fields.contains(expected),

            "checker field helper `{expected}` should live in check/fields.rs"

        );

        assert!(

            !evaluator.contains(expected),

            "checker field helper `{expected}` should not live in evaluator.rs"

        );

    }

    for forbidden in [

        "不能访问 null 的字段",

        "记录没有字段",

        "字段访问目标不是对象",

        "dict entry 没有字段",

    ] {

        assert!(

            !evaluator.contains(forbidden),

            "checker evaluator should not own field read branch `{forbidden}`"

        );

    }

}



#[test]

fn checker_false_explanation_helpers_do_not_live_in_evaluator_rs() {

    let evaluator = std::fs::read_to_string("crates/coflow-checker/src/check/evaluator.rs")

        .expect("read checker evaluator");

    let explanations = std::fs::read_to_string("crates/coflow-checker/src/check/explanations.rs")

        .expect("read checker explanation helpers");



    for expected in [

        "pub(super) fn explain_false_expr",

        "fn explain_failed_comparison",

    ] {

        assert!(

            explanations.contains(expected),

            "checker false explanation helper `{expected}` should live in check/explanations.rs"

        );

        assert!(

            !evaluator.contains(expected),

            "checker false explanation helper `{expected}` should not live in evaluator.rs"

        );

    }

    for forbidden in [

        "至少一个操作数为 false",

        "两侧都为 true",

        "至少一侧为 true",

        "实际类型 =",

    ] {

        assert!(

            !evaluator.contains(forbidden),

            "checker evaluator should not own false explanation branch `{forbidden}`"

        );

    }

}



#[test]

fn checker_type_predicate_helpers_do_not_live_in_evaluator_rs() {

    let evaluator = std::fs::read_to_string("crates/coflow-checker/src/check/evaluator.rs")

        .expect("read checker evaluator");

    let type_predicates =

        std::fs::read_to_string("crates/coflow-checker/src/check/type_predicates.rs")

            .expect("read checker type predicate helpers");



    assert!(

        type_predicates.contains("pub(super) fn value_matches_predicate"),

        "checker type predicate helper should live in check/type_predicates.rs"

    );

    assert!(

        !evaluator.contains("fn eval_is") && !evaluator.contains("fn value_matches_predicate"),

        "checker type predicate helper should not live in evaluator.rs"

    );

    assert!(

        !evaluator.contains("schema.is_assignable(actual, type_name)"),

        "checker evaluator should not own type predicate assignability checks"

    );

}



#[test]

fn checker_dimension_variant_helpers_do_not_live_in_evaluator_rs() {

    let evaluator = std::fs::read_to_string("crates/coflow-checker/src/check/evaluator.rs")

        .expect("read checker evaluator");

    let dimensions = std::fs::read_to_string("crates/coflow-checker/src/check/dimensions.rs")

        .expect("read checker dimension helpers");



    assert!(

        dimensions.contains("pub(super) fn apply_dimension_variant"),

        "checker dimension variant helper should live in check/dimensions.rs"

    );

    assert!(

        !evaluator.contains("dimension_field_value(")

            && !evaluator.contains("dimension_lookup_error_message"),

        "checker evaluator should not own dimension variant lookup details"

    );

    assert!(

        evaluator.lines().count() < 800,

        "checker evaluator.rs should stay below the 800-line large-module threshold"

    );

}



#[test]

fn checker_index_access_does_not_live_in_evaluator_rs() {

    let evaluator = std::fs::read_to_string("crates/coflow-checker/src/check/evaluator.rs")

        .expect("read checker evaluator");

    let access =

        std::fs::read_to_string("crates/coflow-checker/src/check/access.rs").expect("read access");



    assert!(

        access.contains("pub(super) fn index_value"),

        "checker index access should live in check/access.rs"

    );

    assert!(

        !evaluator.contains("pub(super) fn index_value"),

        "checker index access should not live in evaluator.rs"

    );

    for forbidden in ["CheckMissingDictKey", "数组索引越界", "索引目标不是集合"] {

        assert!(

            !evaluator.contains(forbidden),

            "checker evaluator should not own index access branch `{forbidden}`"

        );

    }

}



#[test]

fn checker_enum_value_helpers_do_not_live_in_evaluator_rs() {

    let evaluator = std::fs::read_to_string("crates/coflow-checker/src/check/evaluator.rs")

        .expect("read checker evaluator");

    let enum_values = std::fs::read_to_string("crates/coflow-checker/src/check/enum_values.rs")

        .expect("read checker enum values");



    for expected in [

        "pub(super) fn enum_with_value",

        "pub(super) fn anonymous_enum_value",

    ] {

        assert!(

            enum_values.contains(expected),

            "checker enum value helper `{expected}` should live in check/enum_values.rs"

        );

        assert!(

            !evaluator.contains(expected),

            "checker enum value helper `{expected}` should not live in evaluator.rs"

        );

    }

}



