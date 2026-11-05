use super::*;

type Dense = BTreeMap<FieldCoordinate, DiffFieldState>;

fn compact(dense: &Dense, contexts: &[(String, String)]) -> SemanticSnapshot {
    let mut snapshot = SemanticSnapshot {
        available: true,
        contexts: contexts
            .iter()
            .cloned()
            .map(|key| (key, BTreeMap::new()))
            .collect(),
        ..Default::default()
    };
    for (key, state) in dense.iter().filter(|(key, _)| key.3.is_none()) {
        snapshot
            .records
            .insert((key.0.clone(), key.1.clone()), Some("data.cfd".into()));
        snapshot.fields.insert(key.clone(), state.clone());
    }
    for (key, state) in dense.iter().filter(|(key, _)| key.3.is_some()) {
        let base = (key.0.clone(), key.1.clone(), key.2.clone(), None, None);
        if snapshot.fields.get(&base) != Some(state) {
            snapshot
                .contexts
                .get_mut(&(key.3.clone().unwrap(), key.4.clone().unwrap()))
                .unwrap()
                .insert(base, state.clone());
        }
    }
    snapshot
}

/// 保留优化前密集字段表的独立遍历作为 oracle，比较完整状态与输出顺序。
fn dense_differences(before: &Dense, after: &Dense) -> Vec<SemanticFieldDiff> {
    let mut differences = Vec::new();
    for key in BTreeSet::from_iter(before.keys().chain(after.keys())) {
        let old = before.get(key);
        let new = after.get(key);
        let assigned = assignment_changed(old, new);
        let effective = old.and_then(|state| state.effective.as_ref())
            != new.and_then(|state| state.effective.as_ref());
        let rendered = old.and_then(|state| state.rendered.as_ref())
            != new.and_then(|state| state.rendered.as_ref());
        if assigned || effective || rendered {
            differences.push(SemanticFieldDiff {
                actual_type: key.0.clone(),
                id: key.1.clone(),
                field: key.2.clone(),
                dimension: key.3.clone(),
                variant: key.4.clone(),
                assignment_changed: assigned,
                effective_changed: effective,
                rendered_changed: rendered,
                before: old.cloned(),
                after: new.cloned(),
            });
        }
    }
    differences
}

fn fixture(round: usize, after: bool, contexts: &[(String, String)]) -> Dense {
    let mut fields = BTreeMap::new();
    for record in 0..18 {
        if after && record == round % 18 || !after && record == (round + 1) % 18 {
            continue;
        }
        for field in 0..5 {
            let key = (
                format!("T{}", record % 3),
                format!("r{record}"),
                format!("f{field}"),
                None,
                None,
            );
            let mut base = DiffFieldState {
                assigned: field != 0,
                origin: if field == 0 {
                    "schema_default"
                } else {
                    "explicit_base"
                }
                .into(),
                expression: (field != 0).then(|| format!("[{},{}]", record, field)),
                effective: Some(super::super::ModelValue::Int {
                    value: (record * 5 + field).to_string(),
                }),
                rendered: (field == 4).then(|| json!(format!("r{record}"))),
            };
            if after && (record * 7 + field + round) % 11 == 0 {
                base.assigned = !base.assigned;
                base.origin = "schema_default".into();
                base.expression = None;
            }
            if after && (record + field + round) % 7 == 0 {
                base.effective = Some(super::super::ModelValue::Int {
                    value: "999".into(),
                });
            }
            if after && (record + field + round) % 9 == 0 {
                base.expression = base.expression.map(|text| format!("# trivia\n{text}"));
            }
            if after && field == 4 && (record + round) % 3 == 0 {
                base.rendered = Some(json!("indirect template update"));
            }
            fields.insert(key.clone(), base.clone());
            for (index, (dimension, variant)) in contexts.iter().enumerate() {
                let mut state = base.clone();
                if (record + field + index + usize::from(after) * round) % 13 == 0 {
                    state.origin = "dimension_override".into();
                    state.assigned = true;
                    state.expression = Some("None".into());
                    state.effective = Some(super::super::ModelValue::None {});
                    state.rendered = None;
                }
                fields.insert(
                    (
                        key.0.clone(),
                        key.1.clone(),
                        key.2.clone(),
                        Some(dimension.clone()),
                        Some(variant.clone()),
                    ),
                    state,
                );
            }
        }
    }
    fields
}

#[test]
fn sparse_contexts_match_dense_report_for_members_variants_sources_and_templates() {
    for round in 0..64 {
        let before_contexts = vec![
            ("language".into(), "en".into()),
            ("language".into(), "zh".into()),
            ("platform".into(), "pc".into()),
        ];
        let mut after_contexts = before_contexts.clone();
        if round % 2 == 0 {
            after_contexts.remove(0);
        }
        if round % 3 == 0 {
            after_contexts.push(("platform".into(), "mobile".into()));
        }
        let before = fixture(round, false, &before_contexts);
        let after = fixture(round, true, &after_contexts);
        let old = compact(&before, &before_contexts);
        let new = compact(&after, &after_contexts);
        assert!(
            old.fields.len() + old.contexts.values().map(BTreeMap::len).sum::<usize>()
                < before.len()
        );
        let actual = compare(&old, &new);
        let expected = dense_differences(&before, &after);
        let base = expected
            .iter()
            .filter(|change| change.dimension.is_none())
            .collect::<Vec<_>>();
        let dimensions = expected
            .iter()
            .filter(|change| change.dimension.is_some())
            .collect::<Vec<_>>();
        assert_eq!(json!(actual.fields), json!(base), "base round {round}");
        assert_eq!(
            json!(actual.dimensions),
            json!(dimensions),
            "dimensions round {round}"
        );
        assert_eq!(actual.changed_dimension_fields, dimensions.len());
        assert_eq!(
            actual.changed_fields,
            expected
                .iter()
                .map(|change| (&change.actual_type, &change.id, &change.field))
                .collect::<BTreeSet<_>>()
                .len()
        );
        assert_eq!(
            actual.changed_records,
            expected
                .iter()
                .map(|change| (&change.actual_type, &change.id))
                .collect::<BTreeSet<_>>()
                .len()
        );
    }
}
