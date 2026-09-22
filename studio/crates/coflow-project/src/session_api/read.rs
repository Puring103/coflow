use crate::data_model::CfdDataModel;
use crate::session::{ProjectSchemaSession, ProjectSession};
use crate::{DiagnosticSet, ProjectQueries};
use coflow_core::schema::CftSchema;

/// Read capability for a built project.
///
/// The owning runtime session is intentionally sealed. Hosts can query this
/// capability but cannot unwrap it or reach mutation methods.
///
/// ```compile_fail
/// fn escape_session(session: coflow_project::ReadOnlyProjectSession) {
///     let _ = session.into_session();
/// }
/// ```
#[derive(Debug)]
pub struct ReadOnlyProjectSession {
    pub(crate) session: ProjectSession,
}

impl ReadOnlyProjectSession {
    pub fn diff_against_head(&self) -> Result<crate::ProjectDiff, DiagnosticSet> {
        crate::diff::diff_against_head(&self.session, 0)
    }

    pub(super) const fn new(session: ProjectSession) -> Self {
        Self { session }
    }

    #[must_use]
    pub fn queries(&self) -> ProjectQueries<'_> {
        ProjectQueries::new(&self.session, 0)
    }

    #[must_use]
    pub fn schema(&self) -> &CftSchema {
        self.session.schema()
    }

    #[must_use]
    pub const fn model(&self) -> &CfdDataModel {
        self.session.model()
    }

    #[must_use]
    pub fn into_diagnostics(self) -> DiagnosticSet {
        self.session.into_diagnostics()
    }

    #[must_use]
    pub fn into_schema_session(self) -> ProjectSchemaSession {
        self.session.into_schema_session()
    }
}

#[derive(Debug)]
pub struct BuildProjectSession {
    session: ProjectSession,
}

impl BuildProjectSession {
    pub(super) const fn new(session: ProjectSession) -> Self {
        Self { session }
    }

    #[must_use]
    pub fn queries(&self) -> ProjectQueries<'_> {
        ProjectQueries::new(&self.session, 0)
    }

    #[must_use]
    pub fn schema(&self) -> &CftSchema {
        self.session.schema()
    }

    #[must_use]
    pub const fn model(&self) -> &CfdDataModel {
        self.session.model()
    }

    #[must_use]
    pub fn into_diagnostics(self) -> DiagnosticSet {
        self.session.into_diagnostics()
    }
}
