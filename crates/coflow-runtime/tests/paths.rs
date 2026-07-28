use coflow_data_model::{CfdDictKey, CfdPath, CfdPathSegment, LoadedDictKeyDraft};
use coflow_runtime::{dict_key_path_text, format_field_path};

#[test]
fn canonical_path_preserves_field_index_and_escaped_dict_key_identity() {
    let key = CfdDictKey::String("quote\"slash\\line\nnext\tcell".to_string());
    let key_text = dict_key_path_text(&key);
    assert_eq!(key_text, r#""quote\"slash\\line\nnext\tcell""#);

    let path = CfdPath::root()
        .field("items")
        .index(3)
        .dict_key_value(&key)
        .field("value");
    assert_eq!(
        path.segments,
        vec![
            CfdPathSegment::Field("items".to_string()),
            CfdPathSegment::Index(3),
            CfdPathSegment::DictKey(key_text),
            CfdPathSegment::Field("value".to_string()),
        ]
    );
    assert_eq!(
        format_field_path(&path),
        r#"items[3]["quote\"slash\\line\nnext\tcell"].value"#
    );
}

#[test]
fn unvalidated_string_dict_keys_use_the_same_canonical_escape_rules() {
    let path = CfdPath::root().dict_key_input(&LoadedDictKeyDraft::String(
        "quote\"slash\\line\nnext\tcell".to_string(),
    ));
    assert_eq!(
        path.segments,
        vec![CfdPathSegment::DictKey(
            r#""quote\"slash\\line\nnext\tcell""#.to_string()
        )]
    );
}
