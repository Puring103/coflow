use crate::project_schema::open_project_schema_attempt;
use crate::{DiagnosticSet, Project, ProjectSchemaSession, SchemaTextOverride};
use std::hash::{Hash, Hasher};

/// Owns the published schema generation for one project.
///
/// Hosts call [`Self::refresh`] after filesystem-backed project changes;
/// the cache determines whether schema inputs changed.
#[derive(Debug)]
pub struct SchemaCache {
    project: Project,
    published: Option<SchemaGeneration>,
    attempted: Option<SchemaGeneration>,
}

/// Private cache record for one immutable schema generation.
///
/// Keeping parsed modules and the semantic schema behind the same fingerprint
/// ensures language hosts never reparse text that the compiler already read.
#[derive(Debug)]
struct SchemaGeneration {
    fingerprint: u64,
    session: ProjectSchemaSession,
}

impl SchemaCache {
    #[must_use]
    pub const fn new(project: Project) -> Self {
        Self {
            project,
            published: None,
            attempted: None,
        }
    }

    #[must_use]
    pub fn schema(&self) -> Option<&ProjectSchemaSession> {
        self.published
            .as_ref()
            .map(|generation| &generation.session)
    }

    /// Returns the latest build attempt, including an invalid CFT module set.
    /// Language tooling uses this for diagnostics while [`Self::schema`] keeps
    /// pointing at the last successfully published schema.
    #[must_use]
    pub fn latest_attempt(&self) -> Option<&ProjectSchemaSession> {
        self.attempted
            .as_ref()
            .or(self.published.as_ref())
            .map(|generation| &generation.session)
    }

    #[must_use]
    pub fn into_latest_attempt(self) -> Option<ProjectSchemaSession> {
        self.attempted
            .or(self.published)
            .map(|generation| generation.session)
    }

    /// Refreshes the published schema only when CFT text or dimension variants change.
    ///
    /// A failed rebuild leaves the last successful generation available for
    /// language tooling, while the returned diagnostics still prevent callers
    /// from treating the failed refresh as a valid project build.
    ///
    /// # Errors
    ///
    /// Returns project, schema, or source diagnostics when the candidate cannot be built.
    pub fn refresh(&mut self) -> Result<bool, DiagnosticSet> {
        self.refresh_with_overrides(&[])
    }

    /// Rebuilds from a host's current in-memory CFT document snapshots.
    ///
    /// A failed candidate is retained only as `latest_attempt` for diagnostics;
    /// it never replaces the published generation used by semantic consumers.
    ///
    /// # Errors
    ///
    /// Returns project, schema, or source diagnostics when the candidate cannot be built.
    pub fn refresh_with_overrides(
        &mut self,
        overrides: &[SchemaTextOverride],
    ) -> Result<bool, DiagnosticSet> {
        let fingerprint = schema_input_fingerprint(&self.project, overrides)?;
        if self
            .attempted
            .as_ref()
            .is_some_and(|generation| generation.fingerprint == fingerprint)
        {
            return self.attempt_result();
        }
        if self
            .published
            .as_ref()
            .is_some_and(|generation| generation.fingerprint == fingerprint)
        {
            self.attempted = None;
            return Ok(false);
        }

        let diagnostics = self.project.schema_diagnostic_set();
        let session = open_project_schema_attempt(self.project.clone(), diagnostics, overrides)?;
        let generation = SchemaGeneration {
            fingerprint,
            session,
        };
        let diagnostics = generation.session.diagnostics().clone().into_set();
        let changed = self
            .published
            .as_ref()
            .is_none_or(|published| published.fingerprint != fingerprint);

        // Publish only a fully valid schema; failed editor text must not
        // invalidate semantic queries that still rely on the last good one.
        self.attempted = Some(generation);
        if diagnostics.is_empty() {
            self.published = self.attempted.take();
            return Ok(changed);
        }
        Err(diagnostics)
    }

    fn attempt_result(&self) -> Result<bool, DiagnosticSet> {
        let Some(attempt) = self.attempted.as_ref() else {
            return Ok(false);
        };
        let diagnostics = attempt.session.diagnostics().clone().into_set();
        if diagnostics.is_empty() {
            Ok(false)
        } else {
            Err(diagnostics)
        }
    }
}

#[cfg(test)]
mod schema_cache_tests {
    #![allow(clippy::expect_used)]

    use std::fs;

    use super::SchemaCache;
    use crate::{project::normalize_path, Project, ProjectSessionFactory, SchemaTextOverride};

    #[test]
    fn reverting_to_published_schema_discards_failed_attempt() {
        let root = tempfile::tempdir().expect("temp project");
        let schema_path = root.path().join("schema.cft");
        fs::write(&schema_path, "table Item { value: int; }\n").expect("write schema");
        fs::write(
            root.path().join("coflow.yaml"),
            "schema: schema.cft\ndata: data/\ncodegen:\n  - language: csharp\n    dir: generated/\n",
        )
        .expect("write config");
        let project = Project::open_schema_only(Some(root.path())).expect("open project");
        let mut runtime = SchemaCache::new(project);

        assert_eq!(runtime.refresh(), Ok(true));
        assert!(runtime
            .refresh_with_overrides(&[SchemaTextOverride {
                requested_module: None,
                normalized_path: normalize_path(&schema_path),
                source: "table Item { value: Missing; }\n".to_string(),
            }])
            .is_err());
        assert!(runtime
            .latest_attempt()
            .is_some_and(crate::session::ProjectSchemaSession::has_diagnostics));

        assert_eq!(runtime.refresh(), Ok(false));
        assert!(runtime
            .latest_attempt()
            .is_some_and(|attempt| !attempt.has_diagnostics()));
    }

    #[test]
    fn data_errors_preserve_unrelated_records_and_complete_diagnostics() {
        let root = tempfile::tempdir().expect("temp project");
        fs::create_dir_all(root.path().join("data")).expect("create data");
        fs::write(
            root.path().join("schema.cft"),
            "table Item { name: string; value: int; }\n",
        )
        .expect("write schema");
        fs::write(
            root.path().join("coflow.yaml"),
            concat!(
                "schema: schema.cft\n",
                "data: data/\n",
                "codegen:\n",
                "  - language: csharp\n",
                "    dir: generated/\n",
            ),
        )
        .expect("write config");
        fs::write(
            root.path().join("data/items.cfd"),
            concat!(
                "broken: Item { name: 42, value: 1, extra: true, }\n",
                "valid: Item { name: \"Valid\", value: 2, }\n",
            ),
        )
        .expect("write data");

        let project = Project::open_schema_only(Some(root.path())).expect("open project");
        let session = ProjectSessionFactory::new()
            .open_read_only_session(project)
            .expect("data diagnostics must not prevent a session");
        let queries = session.queries();

        assert_eq!(queries.record_count(), 1);
        assert!(queries
            .record_views_in_file("data/items.cfd")
            .any(|record| record.coordinate.key.as_str() == "valid"));
        let codes = queries
            .diagnostics()
            .as_set()
            .iter()
            .map(|diagnostic| diagnostic.code.as_str())
            .collect::<Vec<_>>();
        assert!(codes.iter().any(|code| code.contains("TypeMismatch")));
        assert!(codes.iter().any(|code| code.contains("UnknownField")));
    }

    #[test]
    fn duplicate_records_are_all_rejected_without_hiding_other_records() {
        let root = tempfile::tempdir().expect("temp project");
        fs::create_dir_all(root.path().join("data")).expect("create data");
        fs::write(
            root.path().join("schema.cft"),
            "table Item { value: int; }\n",
        )
        .expect("write schema");
        fs::write(
            root.path().join("coflow.yaml"),
            concat!(
                "schema: schema.cft\n",
                "data: data/\n",
                "codegen:\n",
                "  - language: csharp\n",
                "    dir: generated/\n",
            ),
        )
        .expect("write config");
        fs::write(
            root.path().join("data/items.cfd"),
            concat!(
                "duplicate: Item { value: 1, }\n",
                "duplicate: Item { value: 2, }\n",
                "survivor: Item { value: 3, }\n",
            ),
        )
        .expect("write data");

        let project = Project::open_schema_only(Some(root.path())).expect("open project");
        let session = ProjectSessionFactory::new()
            .open_read_only_session(project)
            .expect("duplicates must produce a partial session");
        let queries = session.queries();

        assert_eq!(queries.record_count(), 1);
        assert_eq!(queries.rejected_records().len(), 2);
        assert!(queries
            .record_views_in_file("data/items.cfd")
            .any(|record| record.coordinate.key.as_str() == "survivor"));
        assert!(queries
            .diagnostics()
            .as_set()
            .iter()
            .any(|diagnostic| diagnostic.code == "DATA-011"));
    }
}

fn schema_input_fingerprint(
    project: &Project,
    overrides: &[SchemaTextOverride],
) -> Result<u64, DiagnosticSet> {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for module in project.schema_sources()? {
        module.module_id.hash(&mut hasher);
        module.canonical_path.hash(&mut hasher);
        let source = overrides
            .iter()
            .enumerate()
            .rev()
            .find(|(_, source_override)| {
                source_override
                    .requested_module
                    .as_deref()
                    .is_some_and(|requested| requested == module.module_id)
                    || crate::project::normalize_path(&module.canonical_path)
                        == source_override.normalized_path
            })
            .map_or(&module.source, |(_, source_override)| {
                &source_override.source
            });
        source.hash(&mut hasher);
    }
    for source_override in overrides {
        source_override.requested_module.hash(&mut hasher);
        source_override.normalized_path.hash(&mut hasher);
        source_override.source.hash(&mut hasher);
    }
    Ok(hasher.finish())
}
