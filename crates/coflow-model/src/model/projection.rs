use crate::{CfdDataModel, CfdDiagnostic, CfdDiagnostics, CfdErrorCode, CfdModelBuilder};
use coflow_language::cft::CftSchema;
use std::collections::BTreeMap;
use std::sync::Arc;

use super::DimensionProjection;

impl CfdDataModel {
    /// 合并所选维度并重新求值受影响的引用及嵌套模板，所有记录使用同一视图。
    /// 连续调用保留其他维度选择，并从原始基础来源重新求值本次选择。
    /// # Errors
    ///
    /// 维度不存在、投影后的值无效或引用无法解析时返回模型诊断。
    pub fn project_dimensions(
        &self,
        schema: &CftSchema,
        dimensions: &BTreeMap<String, String>,
    ) -> Result<Self, CfdDiagnostics> {
        if dimensions.is_empty() {
            return Ok(self.clone());
        }
        for dimension in dimensions.keys() {
            if schema.resolve_dimension(dimension).is_none() {
                return Err(CfdDiagnostic::error(
                    CfdErrorCode::TypeMismatch,
                    format!("unknown dimension `{dimension}`"),
                )
                .into());
            }
        }
        let (source, mut selections) = self.projection.as_ref().map_or_else(
            || (Arc::new(self.clone()), BTreeMap::new()),
            |projection| {
                (
                    Arc::clone(&projection.source),
                    projection.selections.clone(),
                )
            },
        );
        selections.extend(dimensions.clone());
        // schema 身份也属于缓存前提；相同选择不能沿用另一个 schema 的校验结论。
        if self.schema_compilation == schema.compilation_id()
            && self
                .projection
                .as_ref()
                .is_some_and(|view| view.selections == selections)
        {
            return Ok(self.clone());
        }
        let mut projected = match crate::build::try_project(&source, schema, &selections) {
            Some(projected) => projected,
            None => revalidate_projection(&source, schema, &selections)?,
        };
        projected.projection = Some(DimensionProjection { source, selections });
        Ok(projected)
    }
}

/// 需要重新验证身份、预算或诊断的输入仍执行完整构建，并统一保留原有诊断顺序。
fn revalidate_projection(
    source: &CfdDataModel,
    schema: &CftSchema,
    selections: &BTreeMap<String, String>,
) -> Result<CfdDataModel, CfdDiagnostics> {
    let mut builder = CfdModelBuilder::new(schema);
    for (_, record) in source.records() {
        builder.add_loaded_record(crate::build::projection_input(record, selections)?);
    }
    let mut projected = builder.build()?;
    // 引用索引只记录当前投影基础值；单维查询继续使用原始覆盖元数据。
    for record in Arc::make_mut(&mut projected.records).iter_mut() {
        let record = Arc::make_mut(record);
        if let Some(original) = source
            .record_by_type_id(record.actual_type(), record.id())
            .and_then(|id| source.record(id))
        {
            record.dimension_fields = original.dimension_fields.clone();
        }
    }
    projected.dimension_variants = source.dimension_variants.clone();
    Ok(projected)
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::unwrap_used,
        clippy::panic,
        reason = "投影回归用完整构建作为语义和诊断对照。"
    )]
    use super::*;
    use crate::{
        cell_value, CfdPath, CfdValue, DimensionValueDraft, LoadedRecordDraft, LoadedValueDraft,
        RecordOrigin, RefSite,
    };
    use coflow_language::cft::{
        build_schema, parse_modules, CftFile, DimensionName, FieldName, ModuleId, RecordId,
        TypeName, VariantName,
    };
    use coflow_language::limits::StructuralLimits;

    fn schema(source: &str) -> CftSchema {
        build_schema(&parse_modules([CftFile::new(
            ModuleId::from("main"),
            "main.cft".into(),
            source,
        )]))
        .unwrap()
    }

    fn value(schema: &CftSchema, ty: &str, source: &str) -> LoadedValueDraft {
        match cell_value::parse_cell(schema, ty, source).unwrap() {
            cell_value::ParsedCell::Value(value) => value,
            cell_value::ParsedCell::Omitted => panic!("value expected"),
        }
    }

    fn overlay(
        id: &str,
        field: &str,
        dimension: &str,
        variant: &str,
        value: LoadedValueDraft,
    ) -> DimensionValueDraft {
        DimensionValueDraft {
            source_type: TypeName::new("Item").unwrap(),
            source_id: RecordId::new(id).unwrap(),
            field: FieldName::new(field).unwrap(),
            dimension: DimensionName::new(dimension).unwrap(),
            variant: VariantName::new(variant).unwrap(),
            value,
            origin: RecordOrigin::File {
                path: format!("{dimension}/{variant}.cfd").into(),
                span: None,
            },
        }
    }

    fn selections(items: &[(&str, &str)]) -> BTreeMap<String, String> {
        items
            .iter()
            .map(|(dimension, variant)| ((*dimension).into(), (*variant).into()))
            .collect()
    }

    #[expect(
        clippy::panic_in_result_fn,
        reason = "测试在行为不等价时必须断言失败，同时返回投影结果供调用方验证成功或拒绝。"
    )]
    fn assert_oracle(
        model: &CfdDataModel,
        schema: &CftSchema,
        selected: &BTreeMap<String, String>,
    ) -> Result<CfdDataModel, CfdDiagnostics> {
        let actual = model.project_dimensions(schema, selected);
        let (source, mut combined) = model.projection.as_ref().map_or_else(
            || (model, BTreeMap::new()),
            |view| (view.source.as_ref(), view.selections.clone()),
        );
        combined.extend(selected.clone());
        let expected = revalidate_projection(source, schema, &combined);
        match (&actual, &expected) {
            (Ok(actual), Ok(expected)) => {
                let mut without_view = actual.clone();
                without_view.projection = None;
                assert_eq!(&without_view, expected);
                assert!(actual.ref_edges().all(|edge| edge.site.dimension.is_none()));
            }
            (Err(actual), Err(expected)) => assert_eq!(actual, expected),
            _ => panic!("projection/oracle mismatch: {actual:?} / {expected:?}"),
        }
        actual
    }

    fn fixture() -> (CftSchema, CfdDataModel) {
        let schema = schema(
            r#"
            data Detail { target: &Item; text: string = f"{self.target.name}/{self.target.platform}"; }
            table Item {
                @localized name: string;
                @dimension("platform") platform: string;
                @dimension("mode") parent: &Item;
                label: string = f"{self.parent.name}/{self.platform}";
                bundle: Detail;
                entries: [Detail];
                metadata: {string: Detail};
                optional: Option<Detail> = None;
                score: double = 0.0;
            }
        "#,
        );
        let mut builder = CfdModelBuilder::new(&schema);
        for (id, parent) in [
            ("a", "b"),
            ("b", "a"),
            ("c", "c"),
            ("untouched", "untouched"),
        ] {
            builder.add_loaded_record(
                LoadedRecordDraft::new(
                    id,
                    "Item",
                    [
                        ("name", id.into()),
                        ("platform", "Desktop".into()),
                        ("parent", LoadedValueDraft::record_ref(parent)),
                        (
                            "bundle",
                            value(&schema, "Detail", &format!("{{target: &{parent}}}")),
                        ),
                        (
                            "entries",
                            value(&schema, "[Detail]", &format!("[{{target: &{parent}}}]")),
                        ),
                        (
                            "metadata",
                            value(
                                &schema,
                                "{string: Detail}",
                                &format!("{{\"key\": {{target: &{parent}}}}}"),
                            ),
                        ),
                        (
                            "optional",
                            value(
                                &schema,
                                "Option<Detail>",
                                &format!("Some({{target: &{parent}}})"),
                            ),
                        ),
                        ("score", LoadedValueDraft::Float(-0.0)),
                    ],
                )
                .with_origin(RecordOrigin::File {
                    path: format!("{id}.cfd").into(),
                    span: None,
                }),
            );
        }
        builder.add_dimension_value_draft(overlay("a", "name", "language", "ja", "甲😀".into()));
        builder.add_dimension_value_draft(overlay("b", "name", "language", "ja", "乙".into()));
        builder.add_dimension_value_draft(overlay(
            "a",
            "platform",
            "platform",
            "mobile",
            "Mobile".into(),
        ));
        builder.add_dimension_value_draft(overlay(
            "a",
            "parent",
            "mode",
            "alternate",
            LoadedValueDraft::record_ref("c"),
        ));
        builder.add_dimension_variant(
            DimensionName::new("language").unwrap(),
            VariantName::new("empty").unwrap(),
        );
        let model = builder.build().unwrap();
        (schema, model)
    }

    #[test]
    fn complete_projection_matches_full_build_for_combinations_and_nested_templates() {
        let (schema, model) = fixture();
        for language in ["ja", "empty", "unknown"] {
            for platform in ["mobile", "unknown"] {
                for mode in ["alternate", "unknown"] {
                    let selected = selections(&[
                        ("language", language),
                        ("platform", platform),
                        ("mode", mode),
                    ]);
                    let view = assert_oracle(&model, &schema, &selected).unwrap();
                    assert!(Arc::ptr_eq(&view.tables, &model.tables));
                    assert!(Arc::ptr_eq(
                        &view.record_by_domain_id,
                        &model.record_by_domain_id
                    ));
                    let stable = model.record_by_type_id("Item", "untouched").unwrap();
                    assert!(Arc::ptr_eq(
                        &view.records[stable.index()],
                        &model.records[stable.index()]
                    ));
                    for (id, record) in view.records() {
                        assert!(Arc::ptr_eq(
                            &record.dimension_fields,
                            &model.record(id).unwrap().dimension_fields
                        ));
                        assert!(
                            matches!(record.field("score"), Some(CfdValue::Float(number)) if number.to_bits() == (-0.0_f64).to_bits())
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn chained_selection_replaces_only_its_dimension_and_preserves_old_views_and_refs() {
        let (schema, model) = fixture();
        let original = model.clone();
        let ja = assert_oracle(&model, &schema, &selections(&[("language", "ja")])).unwrap();
        let combined = assert_oracle(
            &ja,
            &schema,
            &selections(&[("platform", "mobile"), ("mode", "alternate")]),
        )
        .unwrap();
        let one_shot = assert_oracle(
            &model,
            &schema,
            &selections(&[
                ("language", "ja"),
                ("platform", "mobile"),
                ("mode", "alternate"),
            ]),
        )
        .unwrap();
        assert_eq!(combined, one_shot);
        let fallback =
            assert_oracle(&combined, &schema, &selections(&[("language", "unknown")])).unwrap();
        let a = model.record_by_type_id("Item", "a").unwrap();
        let b = model.record_by_type_id("Item", "b").unwrap();
        let c = model.record_by_type_id("Item", "c").unwrap();
        let parent = RefSite::new(a, CfdPath::root().field("parent"));
        assert_eq!(model.resolve_ref(&parent), Some(b));
        assert_eq!(combined.resolve_ref(&parent), Some(c));
        assert_eq!(fallback.resolve_ref(&parent), Some(c));
        assert_eq!(model, original);
        assert_eq!(
            ja.record(a).unwrap().field("name"),
            Some(&CfdValue::String("甲😀".into()))
        );
        assert_eq!(
            fallback.record(a).unwrap().field("name"),
            Some(&CfdValue::String("a".into()))
        );
        assert!(model.ref_edges().any(|edge| edge.site.dimension.is_some()));
        let repeated = combined
            .project_dimensions(&schema, &selections(&[("language", "ja")]))
            .unwrap();
        assert!(Arc::ptr_eq(&combined.records, &repeated.records));
    }

    #[test]
    fn different_schema_and_editable_errors_require_full_projection_validation() {
        let original_schema = schema("table Item { @localized name: string; }");
        let mut builder = CfdModelBuilder::new(&original_schema);
        builder.add_record("a", "Item", [("name", "base".into())]);
        let model = builder.build().unwrap();
        let selected = selections(&[("language", "ja")]);
        let view = assert_oracle(&model, &original_schema, &selected).unwrap();
        let changed_schema = schema("table Item { @localized name: int; }");
        assert!(crate::build::try_project(&model, &changed_schema, &selected).is_none());
        assert!(assert_oracle(&view, &changed_schema, &selected).is_err());

        let broken_schema = schema("table Item { @localized name: string; missing: int; }");
        let mut builder = CfdModelBuilder::new(&broken_schema);
        builder.add_record("a", "Item", [("name", "base".into())]);
        let broken = builder.build_partial().unwrap();
        assert!(!broken.diagnostics.is_empty());
        assert!(crate::build::try_project(&broken.model, &broken_schema, &selected).is_none());
        assert!(assert_oracle(&broken.model, &broken_schema, &selected).is_err());
    }

    #[test]
    fn custom_source_limits_preserve_projection_default_budget_and_full_validation() {
        let schema = schema("table Item { @localized values: [int]; }");
        let mut builder =
            CfdModelBuilder::new(&schema).with_structural_limits(StructuralLimits::new(10, 5, 100));
        builder.add_record(
            "a",
            "Item",
            [(
                "values",
                LoadedValueDraft::Array(vec![1_i64.into(), 2_i64.into(), 3_i64.into()]),
            )],
        );
        let model = builder.build().unwrap();
        let selected = selections(&[("language", "unknown")]);
        assert!(crate::build::try_project(&model, &schema, &selected).is_none());
        let projected = assert_oracle(&model, &schema, &selected).unwrap();
        assert_eq!(projected.structural_limits, StructuralLimits::default());
    }

    #[test]
    fn individually_valid_overrides_report_combined_template_cycles_like_full_build() {
        let schema = schema(
            r#"table Item {
            @dimension("left") a: string;
            @dimension("right") b: string;
        }"#,
        );
        let mut builder = CfdModelBuilder::new(&schema);
        builder.add_record("a", "Item", [("a", "A".into()), ("b", "B".into())]);
        builder.add_dimension_value_draft(overlay(
            "a",
            "a",
            "left",
            "selected",
            value(&schema, "string", "f\"{self.b}\""),
        ));
        builder.add_dimension_value_draft(overlay(
            "a",
            "b",
            "right",
            "selected",
            value(&schema, "string", "f\"{self.a}\""),
        ));
        let model = builder.build().unwrap();
        assert_oracle(&model, &schema, &selections(&[("left", "selected")])).unwrap();
        assert_oracle(&model, &schema, &selections(&[("right", "selected")])).unwrap();
        let combined = selections(&[("left", "selected"), ("right", "selected")]);
        assert!(crate::build::try_project(&model, &schema, &combined).is_none());
        let diagnostics = assert_oracle(&model, &schema, &combined).unwrap_err();
        assert!(diagnostics
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == CfdErrorCode::ValueDependencyCycle));
    }

    #[test]
    fn source_above_default_depth_is_rejected_by_projection_with_identical_diagnostics() {
        // 此夹具故意以更宽预算构造超出产品默认深度的旧模型；测试线程为调试构建预留栈。
        std::thread::Builder::new()
            .stack_size(16 * 1024 * 1024)
            .spawn(|| {
                let schema = schema(
            "data Node { child: Option<Node> = None; } table Item { @localized node: Node; }",
        );
                let mut node = LoadedValueDraft::Object {
                    actual_type: Some("Node".into()),
                    fields: BTreeMap::new(),
                };
                for _ in 0..260 {
                    node = LoadedValueDraft::Object {
                        actual_type: Some("Node".into()),
                        fields: BTreeMap::from([(
                            "child".into(),
                            LoadedValueDraft::OptionSome(Box::new(node)),
                        )]),
                    };
                }
                let mut builder = CfdModelBuilder::new(&schema)
                    .with_structural_limits(StructuralLimits::new(512, 1000, 10_000));
                builder.add_record("a", "Item", [("node", node)]);
                let model = builder.build().unwrap();
                let diagnostics =
                    assert_oracle(&model, &schema, &selections(&[("language", "unknown")]))
                        .unwrap_err();
                assert!(diagnostics
                    .diagnostics
                    .iter()
                    .any(|diagnostic| diagnostic.code == CfdErrorCode::DataStructureLimitExceeded));
            })
            .unwrap()
            .join()
            .unwrap();
    }
}
