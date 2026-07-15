#![allow(clippy::expect_used)]

use coflow_cft::{
    BucketName, ConstName, DimensionName, EnumName, EnumVariantName, FieldName, RecordKey,
    TypeName, VariantName,
};
use std::collections::BTreeMap;

#[test]
fn semantic_names_validate_and_support_string_lookup() {
    let type_name = TypeName::new("Item").expect("valid type name");
    let mut types = BTreeMap::new();
    types.insert(type_name, 1);

    assert_eq!(types.get("Item"), Some(&1));
    assert!(TypeName::new("bad-name").is_err());
    assert!(FieldName::new("id").is_err());
    assert!(EnumName::new("enum").is_err());
    assert!(EnumVariantName::new("123").is_err());
    assert!(ConstName::new("").is_err());
    assert!(DimensionName::new("zh-CN").is_err());
    assert!(BucketName::new("ui_text").is_ok());
    assert!(RecordKey::new("sword_fire").is_ok());
}

#[test]
fn dimension_variant_rejects_reserved_default() {
    assert!(VariantName::new("zh").is_ok());
    assert!(VariantName::new("default").is_err());
    assert!(VariantName::new("zh-CN").is_err());
}
