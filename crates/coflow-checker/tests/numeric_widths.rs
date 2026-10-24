#![allow(clippy::expect_used, clippy::unwrap_used)]
#[path = "checker_common/mod.rs"]
mod common;
use common::*;

#[test]
fn numeric_promotion_and_precision_survive_options_collections_and_constants() {
    let schema = compile_schema(
        r#"
        const ONE: long = 1;
        const PRECISE: double = 0.1;
        const RATE = 0.1;
        table T {
            i: int = 16777217;
            l: long = 2147483648;
            f: float = 0.0;
            d: double = 0.0;
            precise: double = 0.1;
            rounded: float = 0.1;
            maybe: Option<double> = Some(0.1);
            nums: [long] = [2147483647, 1];
            ds: [double] = [0.1, 0.2];
            keys: {int: long} = {1: 2147483648};
            check Numeric {
                precise == 0.1;
                precise == RATE;
                rounded != 0.1;
                1e100 > precise;
                i + f == 16777216;
                i + d == 16777217;
                l + ONE == 2147483649;
                nums.sum() == l;
                ds.sum().approxEqual(PRECISE + 0.2, 0.0000001);
                when maybe is Some(value) { value == PRECISE; value.abs() == PRECISE; }
                all key, value in keys { key + value == 2147483649; }
            }
        }
    "#,
    );
    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record("one", "T", std::iter::empty::<(&str, LoadedValueDraft)>());
    let model = builder.build().unwrap();
    run_model_checks(&model, &schema).unwrap();
}

#[test]
fn int_operations_report_overflow_instead_of_implicitly_widening() {
    for expression in ["i + 1 > 0", "values.sum() > 0", "i << 32 > 0"] {
        let schema = compile_schema(&format!("table T {{ i: int = 2147483647; values: [int] = [2147483647, 1]; check Overflow {{ {expression}; }} }}"));
        let mut builder = CfdDataModel::builder(&schema);
        builder.add_record("one", "T", std::iter::empty::<(&str, LoadedValueDraft)>());
        let model = builder.build().unwrap();
        assert!(run_model_checks(&model, &schema).is_err(), "{expression}");
    }
}
