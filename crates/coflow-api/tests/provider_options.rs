#![allow(clippy::expect_used)]

use coflow_api::DecodedSourceOptions;

#[derive(Debug, PartialEq, Eq)]
struct SecretOptions {
    token: String,
}

#[derive(Debug)]
struct OtherOptions;

#[test]
fn decoded_options_preserve_provider_identity_and_concrete_type() {
    let options = DecodedSourceOptions::new(
        "test-provider",
        SecretOptions {
            token: "private-token".to_string(),
        },
    );

    let decoded = options
        .require::<SecretOptions>("test-provider")
        .expect("matching provider options");
    assert_eq!(decoded.token, "private-token");
    assert!(options.require::<SecretOptions>("other-provider").is_err());
    assert!(options.require::<OtherOptions>("test-provider").is_err());
}

#[test]
fn decoded_options_debug_output_does_not_render_values() {
    let options = DecodedSourceOptions::new(
        "test-provider",
        SecretOptions {
            token: "private-token".to_string(),
        },
    );

    let debug = format!("{options:?}");
    assert!(debug.contains("test-provider"));
    assert!(!debug.contains("private-token"));
}
