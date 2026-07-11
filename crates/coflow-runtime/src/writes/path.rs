use coflow_data_model::{format_cfd_dict_key, CfdPath, CfdPathSegment, CfdRecord, CfdValue};

pub(super) fn value_at_path<'a>(record: &'a CfdRecord, path: &CfdPath) -> Option<&'a CfdValue> {
    let mut segments = path.segments.iter();
    let CfdPathSegment::Field(field) = segments.next()? else {
        return None;
    };
    let mut current = record.fields().get(field)?;
    for segment in segments {
        current = match (segment, current) {
            (CfdPathSegment::Field(field), CfdValue::Object(record)) => {
                record.fields().get(field)?
            }
            (CfdPathSegment::Index(index), CfdValue::Array(items)) => items.get(*index)?,
            (CfdPathSegment::DictKey(key), CfdValue::Dict(entries)) => entries
                .iter()
                .find(|(entry_key, _)| format_cfd_dict_key(entry_key) == *key)
                .map(|(_, value)| value)?,
            _ => return None,
        };
    }
    Some(current)
}
