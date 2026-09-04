use coflow_codegen::{
    CodeArtifactFile, CodeArtifactSet, CodeGenerator, CodegenDescriptor, CodegenError, CodegenInput,
    CodegenRegistry,
};

#[derive(Debug)]
struct TestGenerator;

static TEST_DESCRIPTOR: CodegenDescriptor = CodegenDescriptor {
    id: "test",
    language: "Test",
    file_extensions: &["test"],
    runtime_package: "",
    runtime_version: "",
    needs_model: false,
};

impl CodeGenerator for TestGenerator {
    fn descriptor(&self) -> &'static CodegenDescriptor {
        &TEST_DESCRIPTOR
    }

    fn generate(&self, _input: CodegenInput<'_>) -> Result<CodeArtifactSet, CodegenError> {
        CodeArtifactSet::new(Vec::new())
    }
}

#[test]
fn artifacts_reject_duplicate_or_non_portable_paths() {
    let duplicate = CodeArtifactSet::new(vec![
        CodeArtifactFile {
            relative_path: "Item.cs".into(),
            contents: String::new(),
        },
        CodeArtifactFile {
            relative_path: "Item.cs".into(),
            contents: String::new(),
        },
    ]);
    assert!(matches!(duplicate, Err(CodegenError::DuplicateArtifactPath(_))));

    for paths in [
        vec!["../Item.cs"],
        vec!["Data/Item.cs", "data/item.cs"],
        vec!["generated", "generated/Item.cs"],
        vec!["AUX.cs"],
        vec!["generated/name?.cs"],
        vec!["generated/name. "],
    ] {
        let files = paths
            .into_iter()
            .map(|path| CodeArtifactFile {
                relative_path: path.into(),
                contents: String::new(),
            })
            .collect();
        assert!(CodeArtifactSet::new(files).is_err());
    }
}

#[test]
fn registry_rejects_duplicate_generator_ids() {
    let mut registry = CodegenRegistry::default();
    assert!(registry.register(TestGenerator).is_ok());
    assert!(registry.register(TestGenerator).is_err());
}
