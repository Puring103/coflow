use crate::artifacts::{artifact_error, enum_lockfile_path, read_id_as_enum_values};
use crate::codegen::IdAsEnumValues;
use crate::{
    DiagnosticSet, IdAsEnumInfo, MutationAppliedOp, Project, ProjectFileUpdate, ProjectQueries,
};
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn prepare_values(
    project: &Project,
    info: Vec<IdAsEnumInfo>,
) -> Result<IdAsEnumValues, DiagnosticSet> {
    let current = collect_ids(info);
    let mut values = read_id_as_enum_values(project)?;
    values.retain(|enum_name, _| current.contains_key(enum_name));

    for (enum_name, enum_ids) in current {
        let entries = values.entry(enum_name).or_default();
        let current_ids = enum_ids.ids.iter().cloned().collect::<BTreeSet<_>>();
        entries.retain(|key, _| current_ids.contains(key));
        validate_values(project, entries, enum_ids.is_flags)?;
        for key in enum_ids.ids {
            if entries.contains_key(&key) {
                continue;
            }
            let used = entries.values().copied().collect::<BTreeSet<_>>();
            let value = allocate_value(project, &used, enum_ids.is_flags)?;
            entries.insert(key, value);
        }
    }
    Ok(values)
}

pub(super) fn prepare_rename_update(
    project: &Project,
    queries: ProjectQueries<'_>,
    applied: &[MutationAppliedOp],
) -> Result<Option<ProjectFileUpdate>, DiagnosticSet> {
    let path = enum_lockfile_path(project);
    let expected = match std::fs::read(&path) {
        Ok(contents) => Some(contents),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => {
            return Err(artifact_error(
                &path,
                format!("failed to read enum lock: {error}"),
            ));
        }
    };
    let mut values = read_id_as_enum_values(project)?;
    if values.is_empty() {
        return Ok(None);
    }

    let mut changed = false;
    for item in applied {
        let Some((old, new)) = &item.outcome.renamed else {
            continue;
        };
        let Some(enum_name) = queries.id_as_enum_name_for_type(old.actual_type.as_ref()) else {
            continue;
        };
        let Some(entries) = values.get_mut(&enum_name) else {
            continue;
        };
        let Some(value) = entries.remove(old.key.as_ref()) else {
            continue;
        };
        entries.insert(new.key.to_string(), value);
        changed = true;
    }

    if !changed {
        return Ok(None);
    }
    let contents = serde_json::to_vec_pretty(&values).map_err(|error| {
        artifact_error(&path, format!("failed to serialize enum lock: {error}"))
    })?;
    Ok(Some(ProjectFileUpdate::new(path, expected, contents)))
}

#[derive(Debug)]
struct EnumIds {
    ids: Vec<String>,
    is_flags: bool,
}

fn collect_ids(info: Vec<IdAsEnumInfo>) -> BTreeMap<String, EnumIds> {
    let mut result = BTreeMap::new();
    for item in info {
        let entry = result.entry(item.enum_name).or_insert_with(|| EnumIds {
            ids: Vec::new(),
            is_flags: item.is_flags,
        });
        let mut seen = entry.ids.iter().cloned().collect::<BTreeSet<_>>();
        entry
            .ids
            .extend(item.ids.into_iter().filter(|key| seen.insert(key.clone())));
    }
    result
}

fn allocate_value(
    project: &Project,
    used: &BTreeSet<i64>,
    is_flags: bool,
) -> Result<i64, DiagnosticSet> {
    if is_flags {
        let mut candidate = 1_i64;
        loop {
            if !used.contains(&candidate) {
                return Ok(candidate);
            }
            candidate = candidate.checked_mul(2).ok_or_else(|| {
                artifact_error(
                    project.config_path(),
                    "@idAsEnum exhausted all positive i64 flag values",
                )
            })?;
        }
    }
    let mut candidate = 0_i64;
    while used.contains(&candidate) {
        candidate = candidate.checked_add(1).ok_or_else(|| {
            artifact_error(
                project.config_path(),
                "@idAsEnum exhausted all non-negative i64 values",
            )
        })?;
    }
    Ok(candidate)
}

fn validate_values(
    project: &Project,
    values: &BTreeMap<String, i64>,
    is_flags: bool,
) -> Result<(), DiagnosticSet> {
    if !is_flags {
        return Ok(());
    }
    if let Some((key, value)) = values
        .iter()
        .find(|(_, value)| **value <= 0 || (**value & (**value - 1)) != 0)
    {
        return Err(artifact_error(
            project.config_path(),
            format!("@idAsEnum flag key `{key}` has invalid stable value `{value}`"),
        ));
    }
    Ok(())
}
