#![allow(clippy::expect_used)]

use coflow_data_model::{CfdPathSegment, CfdValue};
use coflow_project::Project;
use coflow_runtime::{ProjectQueries, ProjectRuntime, Runtime};

struct TempProject {
    root: std::path::PathBuf,
}

impl TempProject {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "coflow-runtime-capabilities-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("data")).expect("create data directory");
        std::fs::write(root.join("schema.cft"), "type Item { name: string; }\n")
            .expect("write schema");
        std::fs::write(
            root.join("data/items.cfd"),
            "sword: Item { name: \"Sword\" }\n",
        )
        .expect("write data");
        std::fs::write(
            root.join("coflow.yaml"),
            "schema: schema.cft\nsources:\n  - path: data\n",
        )
        .expect("write project configuration");
        Self { root }
    }

    fn open(&self) -> Project {
        Project::open_schema_only(Some(&self.root.join("coflow.yaml"))).expect("open project")
    }
}

impl Drop for TempProject {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn runtime() -> Runtime {
    Runtime::new(
        coflow_builtins::default_provider_registry().expect("create default provider registry"),
    )
}

fn assert_same_generation_corpus(left: ProjectQueries<'_>, right: ProjectQueries<'_>) {
    assert_eq!(left.revision(), right.revision());
    assert!(std::ptr::eq(left.diagnostics(), right.diagnostics()));
    assert_eq!(left.record_count(), right.record_count());
    assert_eq!(left.file_tree(), right.file_tree());
    assert!(std::ptr::eq(
        left.loader_extensions(),
        right.loader_extensions()
    ));
}

#[test]
fn read_and_build_sessions_expose_generation_queries() {
    let fixture = TempProject::new("queries");

    let read_session = runtime()
        .open_read_only_session(fixture.open())
        .expect("open read session");
    assert_eq!(read_session.queries().revision(), 0);
    assert!(read_session
        .queries()
        .record_view("Item", "sword")
        .is_some());
    assert_same_generation_corpus(read_session.queries(), read_session.queries());

    let build_session = runtime()
        .build_project_session(fixture.open())
        .expect("open build session");
    assert_eq!(build_session.queries().revision(), 0);
    assert!(build_session
        .queries()
        .record_view("Item", "sword")
        .is_some());
    assert_same_generation_corpus(build_session.queries(), build_session.queries());
}

#[test]
fn write_session_owns_registry_and_publishes_successful_generation() {
    let fixture = TempProject::new("write-success");
    let mut session = {
        let runtime = runtime();
        runtime
            .open_write_session(fixture.open())
            .expect("open write session")
    };

    assert_eq!(session.revision(), 0);
    assert_eq!(
        session
            .queries()
            .field_value("Item", "sword", &[CfdPathSegment::Field("name".into())]),
        Some(&CfdValue::String("Sword".into()))
    );

    session
        .write_field(
            "Item",
            "sword",
            &[CfdPathSegment::Field("name".into())],
            &CfdValue::String("Blade".into()),
        )
        .expect("write field");

    assert_eq!(session.revision(), 1);
    assert_eq!(session.queries().revision(), 1);
    assert_same_generation_corpus(session.queries(), session.queries());
    assert_eq!(
        session
            .queries()
            .field_value("Item", "sword", &[CfdPathSegment::Field("name".into())]),
        Some(&CfdValue::String("Blade".into()))
    );
}

#[test]
fn failed_write_preserves_revision_and_generation() {
    let fixture = TempProject::new("write-failure");
    let mut session = runtime()
        .open_write_session(fixture.open())
        .expect("open write session");

    let result = session.write_field(
        "Item",
        "missing",
        &[CfdPathSegment::Field("name".into())],
        &CfdValue::String("Missing".into()),
    );

    assert!(result.is_err());
    assert_eq!(session.revision(), 0);
    assert_eq!(session.queries().revision(), 0);
    assert_eq!(
        session
            .queries()
            .field_value("Item", "sword", &[CfdPathSegment::Field("name".into())]),
        Some(&CfdValue::String("Sword".into()))
    );
}

#[test]
fn project_runtime_reuses_schema_until_schema_inputs_change() {
    let fixture = TempProject::new("schema-refresh");
    let mut runtime = ProjectRuntime::new(fixture.open());

    assert!(runtime.refresh().expect("initial schema refresh"));
    let first = runtime
        .schema()
        .expect("published schema session")
        .schema()
        .expect("published schema") as *const _;
    assert!(!runtime.refresh().expect("unchanged schema refresh"));
    assert!(std::ptr::eq(
        first,
        runtime
            .schema()
            .expect("reused schema session")
            .schema()
            .expect("reused schema")
    ));

    std::fs::write(
        fixture.root.join("schema.cft"),
        "type Item { name: string; value: int; }\n",
    )
    .expect("change schema");
    assert!(runtime.refresh().expect("changed schema refresh"));
    assert!(!std::ptr::eq(
        first,
        runtime
            .schema()
            .expect("rebuilt schema session")
            .schema()
            .expect("rebuilt schema")
    ));
}
