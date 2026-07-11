#![allow(clippy::expect_used)]

use coflow_data_model::{CfdPathSegment, CfdValue};
use coflow_project::Project;
use coflow_runtime::Runtime;

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
        std::fs::write(
            root.join("schema.cft"),
            "type Item { name: string; }\n",
        )
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

#[test]
fn read_and_build_sessions_expose_generation_queries() {
    let fixture = TempProject::new("queries");

    let read_session = runtime()
        .open_read_only_session(fixture.open())
        .expect("open read session");
    assert_eq!(read_session.queries().revision(), 0);
    assert!(read_session.queries().record_view("Item", "sword").is_some());

    let build_session = runtime()
        .build_project_session(fixture.open())
        .expect("open build session");
    assert_eq!(build_session.queries().revision(), 0);
    assert!(build_session.queries().record_view("Item", "sword").is_some());
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
