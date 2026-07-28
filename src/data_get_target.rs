use coflow_runtime::RecordCoordinate;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub(crate) struct DataGetTarget {
    pub(crate) config_or_dir: Option<PathBuf>,
    pub(crate) selector: Option<RecordCoordinate>,
}

pub(crate) fn parse_data_get_target(values: &[String]) -> Result<DataGetTarget, String> {
    match values {
        [] => Ok(DataGetTarget {
            config_or_dir: None,
            selector: None,
        }),
        [only] if looks_like_config_path(only) => Ok(DataGetTarget {
            config_or_dir: Some(PathBuf::from(only)),
            selector: None,
        }),
        [only] if looks_like_record_selector(only) => Ok(DataGetTarget {
            config_or_dir: None,
            selector: Some(parse_record_selector(only)?),
        }),
        [only] => Ok(DataGetTarget {
            config_or_dir: Some(PathBuf::from(only)),
            selector: None,
        }),
        [config_or_dir, selector] => Ok(DataGetTarget {
            config_or_dir: Some(PathBuf::from(config_or_dir)),
            selector: Some(parse_record_selector(selector)?),
        }),
        _ => Err("data get accepts at most CONFIG_OR_DIR and TYPE.KEY".to_string()),
    }
}

fn looks_like_record_selector(value: &str) -> bool {
    value.split_once('.').is_some_and(|(actual_type, key)| {
        !actual_type.is_empty() && !key.is_empty() && !value.contains('/') && !value.contains('\\')
    })
}

fn looks_like_config_path(value: &str) -> bool {
    let path = Path::new(value);
    if path.exists() || value.contains('/') || value.contains('\\') {
        return true;
    }
    path.extension().is_some_and(|extension| {
        extension.eq_ignore_ascii_case("yaml") || extension.eq_ignore_ascii_case("yml")
    })
}

fn parse_record_selector(value: &str) -> Result<RecordCoordinate, String> {
    let Some((actual_type, key)) = value.split_once('.') else {
        return Err(format!(
            "record selector `{value}` must be written as TYPE.KEY"
        ));
    };
    if actual_type.is_empty() || key.is_empty() {
        return Err(format!(
            "record selector `{value}` must be written as TYPE.KEY"
        ));
    }
    RecordCoordinate::try_new(actual_type, key).map_err(|error| error.to_string())
}
