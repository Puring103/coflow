use crate::codegen::IdAsEnumValues;
use crate::publication::artifacts::{artifact_error, enum_lockfile_path, read_id_as_enum_values};
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
        let entries = values.entry(enum_name.clone()).or_default();
        let max_value = if enum_ids.is_int64 {
            i64::MAX
        } else {
            i64::from(i32::MAX)
        };
        entries.retain(|key, _| enum_ids.seen.contains(key));
        let used = validate_values(project, &enum_name, entries, enum_ids.is_flags, max_value)?;
        let mut allocator = ValueAllocator::new(used, enum_ids.is_flags, max_value);
        for key in enum_ids.ids {
            if enum_ids.is_flags && matches!(key.as_str(), "None" | "Everything") {
                return Err(artifact_error(
                    project.config_path(),
                    "record ID conflicts with built-in flag member",
                ));
            }
            if entries.contains_key(&key) {
                continue;
            }
            let value = allocator
                .allocate()
                .map_err(|message| artifact_error(project.config_path(), message))?;
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
        let Some(value) = entries.remove(old.id.as_ref()) else {
            continue;
        };
        entries.insert(new.id.to_string(), value);
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
    seen: BTreeSet<String>,
    is_flags: bool,
    is_int64: bool,
}

fn collect_ids(info: Vec<IdAsEnumInfo>) -> BTreeMap<String, EnumIds> {
    let mut result = BTreeMap::new();
    for item in info {
        let entry = result.entry(item.enum_name).or_insert_with(|| EnumIds {
            ids: Vec::new(),
            seen: BTreeSet::new(),
            is_flags: item.is_flags,
            is_int64: item.is_int64,
        });
        for id in item.ids {
            if entry.seen.insert(id.clone()) {
                entry.ids.push(id);
            }
        }
    }
    result
}

/// 已占用编号只建立一次；游标单调前进，每个旧编号最多检查一次。
#[derive(Debug)]
struct ValueAllocator {
    used: std::iter::Peekable<std::vec::IntoIter<i64>>,
    next: Option<i64>,
    is_flags: bool,
    max_value: i64,
    #[cfg(test)]
    probes: usize,
}

impl ValueAllocator {
    fn new(used: Vec<i64>, is_flags: bool, max_value: i64) -> Self {
        Self {
            used: used.into_iter().peekable(),
            next: Some(i64::from(is_flags)),
            is_flags,
            max_value,
            #[cfg(test)]
            probes: 0,
        }
    }

    fn allocate(&mut self) -> Result<i64, &'static str> {
        while let Some(candidate) = self.next {
            if candidate > self.max_value {
                return Err("@idAsEnum exceeds enum width; add @int64 for 64-bit values");
            }
            self.next = if self.is_flags {
                candidate.checked_mul(2)
            } else {
                candidate.checked_add(1)
            };
            #[cfg(test)]
            {
                self.probes += 1;
            }
            if self.used.peek() == Some(&candidate) {
                self.used.next();
            } else {
                // 两个游标同向前进，新值无需插入，也无需重复树查找。
                return Ok(candidate);
            }
        }
        Err(if self.is_flags {
            "@idAsEnum exhausted all positive i64 flag values"
        } else {
            "@idAsEnum exhausted all non-negative i64 values"
        })
    }
}

fn validate_values(
    project: &Project,
    enum_name: &str,
    values: &BTreeMap<String, i64>,
    is_flags: bool,
    max_value: i64,
) -> Result<Vec<i64>, DiagnosticSet> {
    let mut owners = BTreeMap::new();
    let lock_path = enum_lockfile_path(project);
    for (key, value) in values {
        if *value < 0 || *value > max_value {
            return Err(artifact_error(
                &lock_path,
                format!("stable enum value for `{enum_name}::{key}` is outside the declared width"),
            ));
        }
        if is_flags && (*value <= 0 || (*value & (*value - 1)) != 0) {
            return Err(artifact_error(
                &lock_path,
                format!(
                    "@idAsEnum flag key `{enum_name}::{key}` has invalid stable value `{value}`"
                ),
            ));
        }
        // 每个记录必须拥有唯一编号；重复值会让目标语言的枚举查到另一个记录。
        if let Some(previous) = owners.insert(*value, key) {
            return Err(artifact_error(&lock_path, format!(
                "@idAsEnum `{enum_name}` assigns duplicate stable value `{value}` to `{previous}` and `{key}`"
            )));
        }
    }
    Ok(owners.into_keys().collect())
}

#[cfg(test)]
mod tests {
    use super::ValueAllocator;

    #[test]
    fn allocating_dense_ids_has_linear_scan_work() {
        let mut allocator = ValueAllocator::new(Vec::new(), false, i64::MAX);
        for expected in 0..100_000 {
            assert_eq!(allocator.allocate(), Ok(expected));
        }
        assert_eq!(allocator.probes, 100_000);

        let mut existing = ValueAllocator::new((0..50_000).collect(), false, i64::MAX);
        for expected in 50_000..100_000 {
            assert_eq!(existing.allocate(), Ok(expected));
        }
        assert_eq!(existing.probes, 100_000);
    }

    #[test]
    fn allocator_reuses_gaps_without_renumbering_existing_values() {
        let mut allocator = ValueAllocator::new(vec![0, 2, 4, i64::MAX], false, i64::MAX);
        assert_eq!(allocator.allocate(), Ok(1));
        assert_eq!(allocator.allocate(), Ok(3));
        assert_eq!(allocator.allocate(), Ok(5));
        assert_eq!(allocator.probes, 6);
    }

    #[test]
    fn flag_allocator_preserves_single_bits_and_width_limits() {
        let mut allocator = ValueAllocator::new(vec![1, 4], true, 8);
        assert_eq!(allocator.allocate(), Ok(2));
        assert_eq!(allocator.allocate(), Ok(8));
        assert!(allocator.allocate().is_err());
        let mut wide =
            ValueAllocator::new((0..62).map(|bit| 1_i64 << bit).collect(), true, i64::MAX);
        assert_eq!(wide.allocate(), Ok(1_i64 << 62));
        assert!(wide.allocate().is_err());
    }
}
