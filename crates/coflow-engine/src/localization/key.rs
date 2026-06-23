/// Translation key components.
///
/// Keys are formatted as `{Bucket}/{record_key}/{field_path}` per
/// `docs/spec/13-localization.md` §3. All segments must be valid CFT
/// identifiers; this is enforced upstream (data-model
/// `LocalizedRecordKeyInvalid`, schema `LocalizedBucketNotIdentifier`).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LocalizationKey {
    pub bucket: String,
    pub record_key: String,
    pub field_path: Vec<String>,
}

impl LocalizationKey {
    #[must_use]
    pub fn format(&self) -> String {
        format_key(&self.bucket, &self.record_key, &self.field_path)
    }
}

#[must_use]
pub fn format_key(bucket: &str, record_key: &str, field_path: &[String]) -> String {
    let mut out = String::with_capacity(
        bucket.len() + record_key.len() + field_path.iter().map(|s| s.len() + 1).sum::<usize>() + 2,
    );
    out.push_str(bucket);
    out.push('/');
    out.push_str(record_key);
    for segment in field_path {
        out.push('/');
        out.push_str(segment);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_simple_key() {
        assert_eq!(
            format_key("Item", "potion", &["name".to_string()]),
            "Item/potion/name"
        );
    }

    #[test]
    fn formats_nested_key() {
        assert_eq!(
            format_key("Item", "potion", &["stats".to_string(), "label".to_string()]),
            "Item/potion/stats/label"
        );
    }
}
