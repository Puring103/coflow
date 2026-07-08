use coflow_api::{DiagnosticSet, WriteFieldPathSegment};
use coflow_data_model::{CfdPath, CfdPathSegment, CfdRecord, CfdValue};

pub(super) fn write_path_from_cfd_path(
    path: &CfdPath,
) -> Result<Vec<WriteFieldPathSegment>, DiagnosticSet> {
    path.segments
        .iter()
        .map(|segment| match segment {
            CfdPathSegment::Field(name) => Ok(WriteFieldPathSegment::Field(name.clone())),
            CfdPathSegment::Index(index) => Ok(WriteFieldPathSegment::Index(*index)),
            CfdPathSegment::DictKey(key) => Ok(WriteFieldPathSegment::DictKey(key.clone())),
        })
        .collect()
}

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
                .find(|(entry_key, _)| format_dict_key_for_path(entry_key) == *key)
                .map(|(_, value)| value)?,
            _ => return None,
        };
    }
    Some(current)
}

fn format_dict_key_for_path(key: &coflow_data_model::CfdDictKey) -> String {
    match key {
        coflow_data_model::CfdDictKey::String(value) => format!("\"{value}\""),
        coflow_data_model::CfdDictKey::Int(value) => value.to_string(),
        coflow_data_model::CfdDictKey::Enum(value) => value.variant.as_deref().map_or_else(
            || format!("{}({})", value.enum_name, value.value),
            |variant| format!("{}.{}", value.enum_name, variant),
        ),
    }
}

pub(super) fn cfd_path_from_write_path(path: &[WriteFieldPathSegment]) -> CfdPath {
    path.iter()
        .fold(CfdPath::root(), |path, segment| match segment {
            WriteFieldPathSegment::Field(field) => path.field(field.clone()),
            WriteFieldPathSegment::Index(index) => path.index(*index),
            WriteFieldPathSegment::DictKey(key) => path.dict_key(key.clone()),
        })
}

pub(super) fn cfd_path_to_write_path(path: &CfdPath) -> Vec<WriteFieldPathSegment> {
    path.segments
        .iter()
        .map(|segment| match segment {
            CfdPathSegment::Field(field) => WriteFieldPathSegment::Field(field.clone()),
            CfdPathSegment::Index(index) => WriteFieldPathSegment::Index(*index),
            CfdPathSegment::DictKey(key) => WriteFieldPathSegment::DictKey(key.clone()),
        })
        .collect()
}
