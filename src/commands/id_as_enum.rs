use crate::artifacts::{enum_lockfile_path, read_active_enum_lock};
use crate::commands::artifact_safety::artifact_diagnostic_set;
use coflow_api::DiagnosticSet;
use coflow_cft::{CftAnnotation, CftAnnotationValue, CompiledSchema};
use coflow_data_model::CfdDataModel;
use coflow_project::Project;
use serde::Serialize;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

type IdAsEnumLockfile = BTreeMap<String, BTreeMap<String, i64>>;

#[derive(Debug)]
pub(super) struct IdAsEnumArtifacts {
    pub(super) variants: Value,
    pub(super) lock_state: Value,
}

#[derive(Debug, Clone)]
struct IdAsEnumIds {
    ids: Vec<String>,
    is_flags: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct IdAsEnumVariant {
    name: String,
    value: i64,
}

pub(super) fn id_as_enum_variants_for_schema_only(
    project: &Project,
) -> Result<Value, DiagnosticSet> {
    let lockfile = enum_lockfile_path(project);
    let existing_locked = read_id_as_enum_lock(project, &lockfile)?;
    let variants = lockfile_to_variants(&existing_locked);
    variants_json(&lockfile, variants)
}

pub(super) fn prepare_id_as_enum_artifacts_for_build(
    project: &Project,
    schema: &CompiledSchema,
    model: &CfdDataModel,
) -> Result<IdAsEnumArtifacts, DiagnosticSet> {
    let lockfile = enum_lockfile_path(project);
    let id_as_enum_ids = collect_id_as_enum_ids(schema, model);
    let (locked, variants) = merge_id_as_enum_lockfile(project, &lockfile, id_as_enum_ids)?;
    let variants = variants_json(&lockfile, variants)?;
    let lock_state = serde_json::to_value(locked).map_err(|err| {
        artifact_diagnostic_set(
            &lockfile,
            format!("failed to serialize @idAsEnum lock state: {err}"),
        )
    })?;
    Ok(IdAsEnumArtifacts {
        variants,
        lock_state,
    })
}

fn collect_declared_id_as_enum_ids(
    schema: &CompiledSchema,
) -> BTreeMap<String, IdAsEnumIds> {
    let mut out = BTreeMap::new();
    for schema_type in schema.type_metas() {
        if let Some(enum_name) = annotation_name_arg(&schema_type.annotations, "idAsEnum") {
            let is_flags = schema
                .enum_meta(&enum_name)
                .is_some_and(|schema_enum| has_annotation(&schema_enum.annotations, "flag"));
            out.entry(enum_name).or_insert_with(|| IdAsEnumIds {
                ids: Vec::new(),
                is_flags,
            });
        }
    }
    out
}

fn collect_id_as_enum_ids(
    schema: &CompiledSchema,
    model: &CfdDataModel,
) -> BTreeMap<String, IdAsEnumIds> {
    let mut out = collect_declared_id_as_enum_ids(schema);
    for schema_type in schema.type_metas() {
        let Some(enum_name) = annotation_name_arg(&schema_type.annotations, "idAsEnum") else {
            continue;
        };

        let mut seen = BTreeSet::new();
        let mut variants = Vec::new();
        if let Some(index) = model.polymorphic_index(&schema_type.name) {
            for key in index.records.keys() {
                if seen.insert(key.clone()) {
                    variants.push(key.clone());
                }
            }
        } else {
            for (_record_id, record) in model.records_of_type(&schema_type.name) {
                let key = record.key();
                if seen.insert(key.to_string()) {
                    variants.push(key.to_string());
                }
            }
        }
        if let Some(entry) = out.get_mut(&enum_name) {
            entry.ids = variants;
        }
    }
    out
}

fn merge_id_as_enum_lockfile(
    project: &Project,
    lockfile: &Path,
    current_ids: BTreeMap<String, IdAsEnumIds>,
) -> Result<(IdAsEnumLockfile, BTreeMap<String, Vec<IdAsEnumVariant>>), DiagnosticSet> {
    if current_ids.is_empty() {
        return Ok((BTreeMap::new(), BTreeMap::new()));
    }

    let mut locked = read_id_as_enum_lock(project, lockfile)?;
    locked.retain(|enum_name, _| current_ids.contains_key(enum_name));

    for (enum_name, key_enum) in current_ids {
        let entries = locked.entry(enum_name).or_default();
        let current_set: BTreeSet<String> = key_enum.ids.iter().cloned().collect();
        entries.retain(|name, _| current_set.contains(name));
        validate_existing_id_as_enum_values(lockfile, entries, key_enum.is_flags)?;
        for id in key_enum.ids {
            if entries.contains_key(&id) {
                continue;
            }
            let used: BTreeSet<i64> = entries.values().copied().collect();
            let value = allocate_id_as_enum_value(lockfile, &used, key_enum.is_flags)?;
            entries.insert(id, value);
        }
    }

    let variants = locked
        .into_iter()
        .map(|(enum_name, entries)| {
            let mut variants = entries
                .into_iter()
                .map(|(name, value)| IdAsEnumVariant { name, value })
                .collect::<Vec<_>>();
            variants.sort_by(|left, right| {
                left.value
                    .cmp(&right.value)
                    .then_with(|| left.name.cmp(&right.name))
            });
            (enum_name, variants)
        })
        .collect();

    let locked = variants_to_lockfile(&variants);
    Ok((locked, variants))
}

fn allocate_id_as_enum_value(
    lockfile: &Path,
    used: &BTreeSet<i64>,
    is_flags: bool,
) -> Result<i64, DiagnosticSet> {
    if is_flags {
        let mut candidate: i64 = 1;
        loop {
            if !used.contains(&candidate) {
                return Ok(candidate);
            }
            candidate = candidate.checked_mul(2).ok_or_else(|| {
                artifact_diagnostic_set(
                    lockfile,
                    "@idAsEnum lockfile exhausted i64 flag enum values",
                )
            })?;
        }
    }
    let mut candidate: i64 = 0;
    while used.contains(&candidate) {
        candidate = candidate.checked_add(1).ok_or_else(|| {
            artifact_diagnostic_set(lockfile, "@idAsEnum lockfile exhausted i64 enum values")
        })?;
    }
    Ok(candidate)
}

fn validate_existing_id_as_enum_values(
    lockfile: &Path,
    entries: &BTreeMap<String, i64>,
    is_flags: bool,
) -> Result<(), DiagnosticSet> {
    if !is_flags {
        return Ok(());
    }
    if let Some((name, value)) = entries
        .iter()
        .find(|(_, value)| **value <= 0 || (**value & (**value - 1)) != 0)
    {
        return Err(artifact_diagnostic_set(
            lockfile,
            format!("@idAsEnum flag enum variant `{name}` has non-flag lockfile value `{value}`"),
        ));
    }
    Ok(())
}

fn read_id_as_enum_lock(
    project: &Project,
    diagnostic_path: &Path,
) -> Result<IdAsEnumLockfile, DiagnosticSet> {
    read_active_enum_lock(project)?
        .map(serde_json::from_value)
        .transpose()
        .map(Option::unwrap_or_default)
        .map_err(|err| {
            artifact_diagnostic_set(
                diagnostic_path,
                format!("failed to parse @idAsEnum lock state: {err}"),
            )
        })
}

fn lockfile_to_variants(locked: &IdAsEnumLockfile) -> BTreeMap<String, Vec<IdAsEnumVariant>> {
    locked
        .iter()
        .map(|(enum_name, entries)| {
            let mut variants = entries
                .iter()
                .map(|(name, value)| IdAsEnumVariant {
                    name: name.clone(),
                    value: *value,
                })
                .collect::<Vec<_>>();
            variants.sort_by(|left, right| {
                left.value
                    .cmp(&right.value)
                    .then_with(|| left.name.cmp(&right.name))
            });
            (enum_name.clone(), variants)
        })
        .collect()
}

fn variants_to_lockfile(variants: &BTreeMap<String, Vec<IdAsEnumVariant>>) -> IdAsEnumLockfile {
    variants
        .iter()
        .map(|(enum_name, entries)| {
            (
                enum_name.clone(),
                entries
                    .iter()
                    .map(|entry| (entry.name.clone(), entry.value))
                    .collect(),
            )
        })
        .collect()
}

fn variants_json(
    lockfile: &Path,
    variants: BTreeMap<String, Vec<IdAsEnumVariant>>,
) -> Result<Value, DiagnosticSet> {
    serde_json::to_value(variants).map_err(|err| {
        artifact_diagnostic_set(
            lockfile,
            format!("failed to serialize @idAsEnum variants: {err}"),
        )
    })
}

fn annotation_name_arg(annotations: &[CftAnnotation], name: &str) -> Option<String> {
    annotations
        .iter()
        .find(|annotation| annotation.name == name)
        .and_then(|annotation| annotation.args.first())
        .and_then(|arg| match arg {
            CftAnnotationValue::Name(value) => Some(value.clone()),
            _ => None,
        })
}

fn has_annotation(annotations: &[CftAnnotation], name: &str) -> bool {
    annotations.iter().any(|annotation| annotation.name == name)
}
