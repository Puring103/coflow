#![allow(clippy::expect_used, clippy::panic)]

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use coflow_api::{
    DecodedSourceOptions, Diagnostic, DiagnosticSet, LoadedSource, ProbeResult, ProjectSourceRef,
    ProviderRegistry, ResolvedSource, SourceLoadContext, SourceLocationSpec, SourceProvider,
    SourceProviderDescriptor, SourceTransaction, SourceTransactionCompensation, SourceWriter,
    WriteBatchFailure, WriteCellRequest, WriteContext, WriteOutcome, WriterCapabilities,
    WriterDescriptor,
};
use coflow_data_model::{
    CfdInputRecord, CfdInputValue, CfdPathSegment, CfdValue, RecordOrigin, SourceDocument,
};
use coflow_project::Project;
use coflow_runtime::{MutationOp, MutationRequest, MutationValue, RecordCoordinate, Runtime};

const PROVIDER_ID: &str = "transaction-test";

static SOURCE_DESCRIPTOR: SourceProviderDescriptor = SourceProviderDescriptor {
    id: PROVIDER_ID,
    display_name: "Transaction test source",
    extensions: &["txn"],
    uri_schemes: &["txn"],
    option_keys: &[],
};

static WRITER_DESCRIPTOR: WriterDescriptor = WriterDescriptor {
    id: PROVIDER_ID,
    display_name: "Transaction test writer",
    capabilities: WriterCapabilities {
        provider_id: String::new(),
        can_edit_field: true,
        can_edit_key: false,
        can_insert_record: false,
        can_delete_record: false,
        requires_full_refresh_after_write: true,
        is_remote: true,
    },
};

static NEXT_TEST_ID: AtomicUsize = AtomicUsize::new(0);

#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Default)]
struct Faults {
    begin_failure_source: Option<String>,
    preflight_failure_call: Option<usize>,
    stage_failure_call: Option<usize>,
    fail_load_after_write: bool,
    abort_failure: bool,
    compensate_failure: bool,
    prepare_commit_failure_source: Option<String>,
    unsupported_remote: bool,
    emit_provider_diagnostic: bool,
}

#[derive(Debug, Default)]
struct Counts {
    loads: usize,
    batches: usize,
    begins: usize,
    preflights: usize,
    writes: usize,
    aborts: usize,
    compensates: usize,
    prepare_commits: usize,
    commits: usize,
}

#[derive(Debug, Default)]
struct TestState {
    remote_values: BTreeMap<String, i64>,
    faults: Faults,
    counts: Counts,
}

type SharedState = Arc<Mutex<TestState>>;

#[derive(Debug, Clone)]
struct TestProvider {
    state: SharedState,
}

impl SourceProvider for TestProvider {
    fn descriptor(&self) -> &'static SourceProviderDescriptor {
        &SOURCE_DESCRIPTOR
    }

    fn probe(&self, source: &ProjectSourceRef<'_>) -> ProbeResult {
        if source.source_type == Some(PROVIDER_ID) {
            ProbeResult::certain()
        } else {
            ProbeResult::none()
        }
    }

    fn decode_options(
        &self,
        _options: &serde_json::Value,
    ) -> Result<DecodedSourceOptions, DiagnosticSet> {
        Ok(DecodedSourceOptions::new(PROVIDER_ID, ()))
    }

    fn load(
        &self,
        ctx: SourceLoadContext<'_>,
        source: &ResolvedSource,
    ) -> Result<LoadedSource, DiagnosticSet> {
        self.state
            .lock()
            .expect("lock test provider state")
            .counts
            .loads += 1;
        let (value, origin) = match &source.location {
            SourceLocationSpec::Path(path) => {
                let value = std::fs::read_to_string(path)
                    .map_err(|error| test_error("TEST-LOAD", error.to_string()))?
                    .trim()
                    .parse::<i64>()
                    .map_err(|error| test_error("TEST-LOAD", error.to_string()))?;
                (
                    value,
                    RecordOrigin::File {
                        path: path.clone(),
                        span: None,
                    },
                )
            }
            SourceLocationSpec::Uri(uri) => {
                let value = {
                    let state = self.state.lock().expect("lock test provider state");
                    if state.faults.fail_load_after_write && state.counts.writes > 0 {
                        return Err(test_error(
                            "TEST-LOAD",
                            "injected post-write rebuild failure",
                        ));
                    }
                    state.remote_values.get(uri).copied().ok_or_else(|| {
                        test_error("TEST-LOAD", format!("missing remote value for `{uri}`"))
                    })?
                };
                (
                    value,
                    RecordOrigin::Table {
                        document: SourceDocument::Remote(uri.clone()),
                        sheet: "items".to_string(),
                        row: 2,
                        id_column: 1,
                        field_columns: BTreeMap::from([(vec!["value".to_string()], 2)]),
                    },
                )
            }
        };
        let key = source_record_key(source);
        let mut fields = BTreeMap::from([("value", CfdInputValue::Int(value))]);
        if ctx
            .schema
            .type_meta("Item")
            .is_some_and(|item| item.own_fields.iter().any(|field| field.name == "target"))
        {
            fields.insert(
                "target",
                if key == "two" {
                    CfdInputValue::record_ref("one")
                } else {
                    CfdInputValue::Null
                },
            );
        }
        Ok(LoadedSource {
            records: vec![CfdInputRecord::new(key, "Item", fields).with_origin(origin)],
        })
    }
}

#[derive(Debug, Clone)]
struct TestWriter {
    state: SharedState,
}

impl SourceWriter for TestWriter {
    fn descriptor(&self) -> &'static WriterDescriptor {
        &WRITER_DESCRIPTOR
    }

    fn begin_transaction(
        &self,
        _ctx: WriteContext<'_>,
        source: &ResolvedSource,
    ) -> Result<SourceTransaction, DiagnosticSet> {
        let source_name = source_name(source);
        let mut state = self.state.lock().expect("lock test writer state");
        state.counts.begins += 1;
        if state.faults.begin_failure_source.as_deref() == Some(&source_name) {
            return Err(test_error(
                "TEST-BEGIN",
                format!("injected begin failure for `{source_name}`"),
            ));
        }
        let transaction = match &source.location {
            SourceLocationSpec::Path(_) => Ok(SourceTransaction::RuntimeSnapshot),
            SourceLocationSpec::Uri(uri) if state.faults.unsupported_remote => {
                Ok(SourceTransaction::Unsupported)
            }
            SourceLocationSpec::Uri(uri) => {
                let snapshot = state.remote_values.get(uri).copied().ok_or_else(|| {
                    test_error("TEST-BEGIN", format!("missing remote value for `{uri}`"))
                })?;
                Ok(SourceTransaction::Compensation(Box::new(
                    TestCompensation {
                        state: Arc::clone(&self.state),
                        source: uri.clone(),
                        snapshot,
                    },
                )))
            }
        };
        drop(state);
        transaction
    }

    fn preflight(&self, _ctx: WriteContext<'_>, _request: &WriteCellRequest<'_>) -> DiagnosticSet {
        let mut state = self.state.lock().expect("lock test writer state");
        state.counts.preflights += 1;
        let call = state.counts.preflights;
        if state.faults.preflight_failure_call == Some(call) {
            test_error(
                "TEST-PREFLIGHT",
                format!("injected preflight failure on call {call}"),
            )
        } else {
            DiagnosticSet::empty()
        }
    }

    fn write_field(
        &self,
        _ctx: WriteContext<'_>,
        request: &WriteCellRequest<'_>,
    ) -> Result<WriteOutcome, DiagnosticSet> {
        if !matches!(request.field_path, [CfdPathSegment::Field(field)] if field == "value") {
            return Err(test_error("TEST-WRITE", "unexpected field path"));
        }
        let CfdValue::Int(value) = request.new_value else {
            return Err(test_error("TEST-WRITE", "expected an integer value"));
        };
        let (call, emit_provider_diagnostic) = {
            let mut state = self.state.lock().expect("lock test writer state");
            state.counts.writes += 1;
            let call = state.counts.writes;
            match &request.source.location {
                SourceLocationSpec::Path(path) => std::fs::write(path, value.to_string())
                    .map_err(|error| test_error("TEST-WRITE", error.to_string()))?,
                SourceLocationSpec::Uri(uri) => {
                    state.remote_values.insert(uri.clone(), *value);
                }
            }
            (call, state.faults.emit_provider_diagnostic)
        };
        let stage_failed = self
            .state
            .lock()
            .expect("lock test writer state")
            .faults
            .stage_failure_call
            == Some(call);
        if stage_failed {
            return Err(test_error(
                "TEST-STAGE",
                format!("injected stage failure on call {call}"),
            ));
        }
        let diagnostics = if emit_provider_diagnostic {
            DiagnosticSet::one(Diagnostic {
                code: "TEST-PROVIDER-INFO".to_string(),
                stage: "WRITE".to_string(),
                severity: coflow_api::Severity::Warning,
                message: format!("provider write {call} completed"),
                primary: None,
                related: Vec::new(),
            })
        } else {
            DiagnosticSet::empty()
        };
        Ok(WriteOutcome { diagnostics })
    }

    fn write_field_batch(
        &self,
        ctx: WriteContext<'_>,
        requests: &[WriteCellRequest<'_>],
    ) -> Result<Vec<WriteOutcome>, WriteBatchFailure> {
        self.state.lock().expect("lock writer state").counts.batches += 1;
        requests
            .iter()
            .enumerate()
            .map(|(index, request)| {
                self.write_field(ctx, request)
                    .map_err(|diagnostics| WriteBatchFailure { index, diagnostics })
            })
            .collect()
    }
}

#[derive(Debug)]
struct TestCompensation {
    state: SharedState,
    source: String,
    snapshot: i64,
}

impl SourceTransactionCompensation for TestCompensation {
    fn abort(&mut self) -> Result<(), DiagnosticSet> {
        let mut state = self.state.lock().expect("lock compensation state");
        state.counts.aborts += 1;
        let failed = state.faults.abort_failure;
        drop(state);
        if failed {
            Err(test_error("TEST-ABORT", "injected abort failure"))
        } else {
            Ok(())
        }
    }

    fn compensate(&mut self) -> Result<(), DiagnosticSet> {
        let mut state = self.state.lock().expect("lock compensation state");
        state.counts.compensates += 1;
        if state.faults.compensate_failure {
            drop(state);
            return Err(test_error(
                "TEST-COMPENSATE",
                "injected compensation failure",
            ));
        }
        state
            .remote_values
            .insert(self.source.clone(), self.snapshot);
        drop(state);
        Ok(())
    }

    fn prepare_commit(&mut self) -> Result<(), DiagnosticSet> {
        let mut state = self.state.lock().expect("lock compensation state");
        state.counts.prepare_commits += 1;
        let failed =
            state.faults.prepare_commit_failure_source.as_deref() == Some(self.source.as_str());
        drop(state);
        if failed {
            Err(test_error("TEST-COMMIT", "injected prepare commit failure"))
        } else {
            Ok(())
        }
    }

    fn commit(&mut self) {
        self.state
            .lock()
            .expect("lock compensation state")
            .counts
            .commits += 1;
    }
}

#[test]
fn remote_batch_publishes_one_generation_and_commits_once() {
    let fixture = Fixture::remote(&[("txn://one", 1)]);
    let mut session = fixture.open();

    let report = session.apply_mutation(mutation_request(vec![
        set_value("one", 2),
        set_value("one", 3),
    ]));

    assert!(report.write_ok);
    assert_eq!(report.applied.len(), 2);
    assert_eq!(session.revision(), 1);
    assert_eq!(session_value(&session, "one"), 3);
    let state = fixture.state.lock().expect("lock fixture state");
    assert_eq!(state.remote_values["txn://one"], 3);
    assert_eq!(state.counts.begins, 1);
    assert_eq!(state.counts.batches, 1);
    assert_eq!(state.counts.writes, 2);
    assert_eq!(state.counts.commits, 1);
    assert_eq!(state.counts.prepare_commits, 1);
    assert_eq!(state.counts.compensates, 0);
    drop(state);
}

#[test]
fn same_key_rename_does_not_open_a_provider_transaction() {
    let fixture = Fixture::remote(&[("txn://one", 1)]);
    let mut session = fixture.open();
    let initial_revision = session.revision();
    let coordinate = RecordCoordinate::new("Item", "one");

    let report = session.apply_mutation(mutation_request(vec![MutationOp::RenameRecord {
        record: coordinate.clone(),
        file: None,
        new_key: "one".to_string(),
    }]));

    assert!(report.write_ok);
    assert!(!report.generation_changed);
    assert_eq!(session.revision(), initial_revision);
    assert_eq!(report.applied.len(), 1);
    assert_eq!(report.applied[0].outcome.touched, vec![coordinate]);
    let state = fixture.state.lock().expect("lock fixture state");
    assert_eq!(state.counts.begins, 0);
    assert_eq!(state.counts.writes, 0);
    assert_eq!(state.remote_values["txn://one"], 1);
    drop(state);
}

#[test]
fn same_field_value_does_not_open_a_provider_transaction() {
    let fixture = Fixture::remote(&[("txn://one", 1)]);
    let mut session = fixture.open();
    let initial_revision = session.revision();

    let report = session.apply_mutation(mutation_request(vec![set_value("one", 1)]));

    assert!(report.write_ok);
    assert!(!report.generation_changed);
    assert_eq!(session.revision(), initial_revision);
    assert_eq!(report.applied.len(), 1);
    assert!(report.affected_files.is_empty());
    let state = fixture.state.lock().expect("lock fixture state");
    assert_eq!(state.counts.begins, 0);
    assert_eq!(state.counts.preflights, 0);
    assert_eq!(state.counts.writes, 0);
    assert_eq!(state.remote_values["txn://one"], 1);
    drop(state);
}

#[test]
fn later_batch_write_is_not_folded_against_the_original_generation() {
    let fixture = Fixture::remote(&[("txn://one", 1)]);
    let mut session = fixture.open();

    let report = session.apply_mutation(mutation_request(vec![
        set_value("one", 2),
        set_value("one", 1),
    ]));

    assert!(report.write_ok, "diagnostics: {:?}", report.diagnostics);
    assert!(report.generation_changed);
    assert_eq!(session_value(&session, "one"), 1);
    let state = fixture.state.lock().expect("lock fixture state");
    assert_eq!(state.remote_values["txn://one"], 1);
    assert_eq!(state.counts.writes, 2);
    drop(state);
}

#[test]
fn mutation_rebuild_reuses_the_open_generation_schema() {
    let fixture = Fixture::remote(&[("txn://one", 1)]);
    let mut session = fixture.open();
    std::fs::write(
        fixture.root.join("schema.cft"),
        "this is no longer valid CFT",
    )
    .expect("replace schema after generation opens");

    let report = session.apply_mutation(mutation_request(vec![set_value("one", 2)]));

    assert!(report.write_ok, "diagnostics: {:?}", report.diagnostics);
    assert!(report.generation_changed);
    assert_eq!(session_value(&session, "one"), 2);
    let state = fixture.state.lock().expect("lock fixture state");
    assert_eq!(state.counts.writes, 1);
    assert_eq!(state.counts.commits, 1);
    assert_eq!(state.counts.compensates, 0);
    drop(state);
}

#[test]
fn mutation_rebuild_reloads_only_affected_sources() {
    let fixture = Fixture::remote(&[("txn://one", 1), ("txn://two", 2)]);
    let mut session = fixture.open();
    assert_eq!(
        fixture
            .state
            .lock()
            .expect("lock fixture state")
            .counts
            .loads,
        2
    );

    let report = session.apply_mutation(mutation_request(vec![set_value("one", 3)]));

    assert!(report.write_ok, "diagnostics: {:?}", report.diagnostics);
    assert_eq!(session_value(&session, "one"), 3);
    assert_eq!(session_value(&session, "two"), 2);
    let state = fixture.state.lock().expect("lock fixture state");
    assert_eq!(state.counts.loads, 3);
    drop(state);
}

#[test]
fn incremental_checks_match_full_checks_for_dependent_records() {
    let fixture = Fixture::remote(&[("txn://one", -1), ("txn://two", 1)]);
    std::fs::write(
        fixture.root.join("schema.cft"),
        r"
            type Item {
                value: int;
                target: &Item? = null;
                check {
                    value > 0;
                    target == null || target.value > 0;
                }
            }
        ",
    )
    .expect("write dependent check schema");
    let mut session = fixture.open();
    assert_eq!(session.queries().diagnostics().by_stage("CHECK").len(), 2);

    let report = session.apply_mutation(mutation_request(vec![set_value("one", 2)]));

    assert!(report.write_ok, "diagnostics: {:?}", report.diagnostics);
    assert!(report.check_ok, "diagnostics: {:?}", report.diagnostics);
    assert!(session.queries().diagnostics().by_stage("CHECK").is_empty());
    let full = fixture.open();
    assert_eq!(
        session.queries().diagnostics().flat_diagnostics(),
        full.queries().diagnostics().flat_diagnostics()
    );
}

#[test]
fn provider_diagnostics_and_affected_files_survive_successful_rebuild() {
    let fixture = Fixture::remote(&[("txn://one", 1)]);
    fixture
        .state
        .lock()
        .expect("lock fixture state")
        .faults
        .emit_provider_diagnostic = true;
    let mut session = fixture.open();

    let report = session.apply_mutation(mutation_request(vec![
        set_value("one", 2),
        set_value("one", 3),
    ]));

    assert!(report.write_ok);
    assert_eq!(report.affected_files, vec!["txn://one".to_string()]);
    assert_eq!(
        report
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == "TEST-PROVIDER-INFO")
            .count(),
        2,
        "each provider diagnostic should be reported once"
    );
    for applied in &report.applied {
        assert_eq!(applied.outcome.diagnostics.diagnostics.len(), 1);
        assert_eq!(
            applied.outcome.diagnostics.diagnostics[0].code,
            "TEST-PROVIDER-INFO"
        );
    }
}

#[test]
fn local_stage_failure_restores_source_and_keeps_old_generation() {
    let fixture = Fixture::local(1);
    fixture
        .state
        .lock()
        .expect("lock fixture state")
        .faults
        .stage_failure_call = Some(2);
    let mut session = fixture.open();

    let report = session.apply_mutation(mutation_request(vec![
        set_value("local", 2),
        set_value("local", 3),
    ]));

    assert!(!report.write_ok);
    assert!(report.applied.is_empty());
    assert_eq!(report.failed[0].index, 1);
    assert_eq!(session.revision(), 0);
    assert_eq!(session_value(&session, "local"), 1);
    assert_eq!(
        std::fs::read_to_string(fixture.root.join("local.txn"))
            .expect("read restored local source"),
        "1"
    );
}

#[test]
fn later_preflight_failure_happens_before_transaction_or_write() {
    let fixture = Fixture::remote(&[("txn://one", 1)]);
    fixture
        .state
        .lock()
        .expect("lock fixture state")
        .faults
        .preflight_failure_call = Some(2);
    let mut session = fixture.open();

    let report = session.apply_mutation(mutation_request(vec![
        set_value("one", 2),
        set_value("one", 3),
    ]));

    assert_failed_without_publish(&report, &session, "TEST-PREFLIGHT");
    let state = fixture.state.lock().expect("lock fixture state");
    assert_eq!(state.counts.preflights, 2);
    assert_eq!(state.counts.begins, 0);
    assert_eq!(state.counts.writes, 0);
    assert_eq!(state.remote_values["txn://one"], 1);
    drop(state);
}

#[test]
fn rebuild_failure_compensates_remote_source_and_keeps_old_generation() {
    let fixture = Fixture::remote(&[("txn://one", 1)]);
    fixture
        .state
        .lock()
        .expect("lock fixture state")
        .faults
        .fail_load_after_write = true;
    let mut session = fixture.open();

    let report = session.apply_mutation(mutation_request(vec![set_value("one", 2)]));

    assert_failed_without_publish(&report, &session, "TEST-LOAD");
    assert_eq!(session_value(&session, "one"), 1);
    let state = fixture.state.lock().expect("lock fixture state");
    assert_eq!(state.remote_values["txn://one"], 1);
    assert_eq!(state.counts.compensates, 1);
    assert_eq!(state.counts.commits, 0);
    drop(state);
}

#[test]
fn later_begin_failure_aborts_prior_remote_transaction_and_reports_abort_failure() {
    let fixture = Fixture::remote(&[("txn://one", 1), ("txn://two", 2)]);
    {
        let mut state = fixture.state.lock().expect("lock fixture state");
        state.faults.begin_failure_source = Some("txn://two".to_string());
        state.faults.abort_failure = true;
    }
    let mut session = fixture.open();

    let report = session.apply_mutation(mutation_request(vec![
        set_value("one", 10),
        set_value("two", 20),
    ]));

    assert_failed_without_publish(&report, &session, "TEST-BEGIN");
    assert!(has_diagnostic(&report, "WRITE-TXN-ABORT"));
    assert!(has_diagnostic(&report, "TEST-ABORT"));
    let state = fixture.state.lock().expect("lock fixture state");
    assert_eq!(state.counts.begins, 2);
    assert_eq!(state.counts.aborts, 1);
    assert_eq!(state.counts.writes, 0);
    drop(state);
}

#[test]
fn compensation_failure_is_retained_in_transaction_diagnostics() {
    let fixture = Fixture::remote(&[("txn://one", 1)]);
    {
        let mut state = fixture.state.lock().expect("lock fixture state");
        state.faults.stage_failure_call = Some(1);
        state.faults.compensate_failure = true;
    }
    let mut session = fixture.open();

    let report = session.apply_mutation(mutation_request(vec![set_value("one", 2)]));

    assert_failed_without_publish(&report, &session, "TEST-STAGE");
    assert!(has_diagnostic(&report, "WRITE-TXN-COMPENSATE"));
    assert!(has_diagnostic(&report, "TEST-COMPENSATE"));
    assert_eq!(
        fixture
            .state
            .lock()
            .expect("lock fixture state")
            .counts
            .compensates,
        1
    );
}

#[test]
fn later_prepare_commit_failure_compensates_before_any_source_is_published() {
    let fixture = Fixture::remote(&[("txn://one", 1), ("txn://two", 2)]);
    fixture
        .state
        .lock()
        .expect("lock fixture state")
        .faults
        .prepare_commit_failure_source = Some("txn://two".to_string());
    let mut session = fixture.open();

    let report = session.apply_mutation(mutation_request(vec![
        set_value("one", 10),
        set_value("two", 20),
    ]));

    assert_failed_without_publish(&report, &session, "WRITE-TXN-COMMIT");
    assert!(has_diagnostic(&report, "TEST-COMMIT"));
    assert_eq!(session_value(&session, "one"), 1);
    let state = fixture.state.lock().expect("lock fixture state");
    assert_eq!(state.remote_values["txn://one"], 1);
    assert_eq!(state.remote_values["txn://two"], 2);
    assert_eq!(state.counts.prepare_commits, 2);
    assert_eq!(state.counts.commits, 0);
    assert_eq!(state.counts.compensates, 2);
    drop(state);
}

#[test]
fn unsupported_remote_source_fails_before_any_write() {
    let fixture = Fixture::remote(&[("txn://one", 1)]);
    fixture
        .state
        .lock()
        .expect("lock fixture state")
        .faults
        .unsupported_remote = true;
    let mut session = fixture.open();

    let report = session.apply_mutation(mutation_request(vec![set_value("one", 2)]));

    assert_failed_without_publish(&report, &session, "WRITE-TXN-UNSUPPORTED");
    let state = fixture.state.lock().expect("lock fixture state");
    assert_eq!(state.counts.writes, 0);
    assert_eq!(state.remote_values["txn://one"], 1);
    drop(state);
}

struct Fixture {
    root: std::path::PathBuf,
    state: SharedState,
    registry: ProviderRegistry,
}

impl Fixture {
    fn local(value: i64) -> Self {
        let root = test_root("local");
        write_schema(&root);
        std::fs::write(root.join("local.txn"), value.to_string()).expect("write local source");
        std::fs::write(
            root.join("coflow.yaml"),
            "schema: schema.cft\nsources:\n  - type: transaction-test\n    path: local.txn\n",
        )
        .expect("write project config");
        Self::new(root, TestState::default())
    }

    fn remote(values: &[(&str, i64)]) -> Self {
        let root = test_root("remote");
        write_schema(&root);
        let mut sources = String::new();
        for (uri, _) in values {
            let _ = writeln!(sources, "  - type: transaction-test\n    url: {uri}");
        }
        std::fs::write(
            root.join("coflow.yaml"),
            format!("schema: schema.cft\nsources:\n{sources}"),
        )
        .expect("write project config");
        Self::new(
            root,
            TestState {
                remote_values: values
                    .iter()
                    .map(|(uri, value)| ((*uri).to_string(), *value))
                    .collect(),
                ..TestState::default()
            },
        )
    }

    fn new(root: std::path::PathBuf, initial: TestState) -> Self {
        let state = Arc::new(Mutex::new(initial));
        let mut registry = ProviderRegistry::default();
        registry
            .register_source_provider(TestProvider {
                state: Arc::clone(&state),
            })
            .expect("register test provider");
        registry
            .register_source_writer(TestWriter {
                state: Arc::clone(&state),
            })
            .expect("register test writer");
        Self {
            root,
            state,
            registry,
        }
    }

    fn open(&self) -> coflow_runtime::WriteProjectSession {
        let project = Project::open_schema_only(Some(&self.root.join("coflow.yaml")))
            .expect("open test project");
        Runtime::new(self.registry.clone())
            .open_write_session(project)
            .expect("open write session")
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

const fn mutation_request(ops: Vec<MutationOp>) -> MutationRequest {
    MutationRequest {
        stop_on_write_error: true,
        ops,
    }
}

fn set_value(key: &str, value: i64) -> MutationOp {
    MutationOp::SetField {
        record: RecordCoordinate::new("Item", key),
        file: None,
        path: vec![CfdPathSegment::Field("value".to_string())],
        value: MutationValue::Cfd(CfdValue::Int(value)),
    }
}

fn session_value(session: &coflow_runtime::WriteProjectSession, key: &str) -> i64 {
    let view = session
        .queries()
        .record_view("Item", key)
        .expect("record should remain in the published generation");
    match view.record.fields().get("value") {
        Some(CfdValue::Int(value)) => *value,
        other => panic!("expected Item.value integer, got {other:?}"),
    }
}

fn assert_failed_without_publish(
    report: &coflow_runtime::MutationReport,
    session: &coflow_runtime::WriteProjectSession,
    code: &str,
) {
    assert!(!report.write_ok);
    assert!(report.applied.is_empty());
    assert_eq!(session.revision(), 0);
    assert!(
        has_diagnostic(report, code),
        "missing diagnostic `{code}`: {report:?}"
    );
}

fn has_diagnostic(report: &coflow_runtime::MutationReport, code: &str) -> bool {
    report
        .failed
        .iter()
        .flat_map(|failure| &failure.diagnostics)
        .any(|diagnostic| diagnostic.code == code)
}

fn source_record_key(source: &ResolvedSource) -> String {
    match &source.location {
        SourceLocationSpec::Path(_) => "local".to_string(),
        SourceLocationSpec::Uri(uri) => uri
            .rsplit("//")
            .next()
            .map_or_else(|| uri.clone(), ToOwned::to_owned),
    }
}

fn source_name(source: &ResolvedSource) -> String {
    match &source.location {
        SourceLocationSpec::Path(path) => path.display().to_string(),
        SourceLocationSpec::Uri(uri) => uri.clone(),
    }
}

fn test_error(code: &str, message: impl Into<String>) -> DiagnosticSet {
    DiagnosticSet::one(Diagnostic::error(code, "TEST", message))
}

fn write_schema(root: &Path) {
    std::fs::create_dir_all(root).expect("create test root");
    std::fs::write(root.join("schema.cft"), "type Item { value: int; }\n")
        .expect("write test schema");
}

fn test_root(label: &str) -> std::path::PathBuf {
    let id = NEXT_TEST_ID.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "coflow-mutation-transaction-{label}-{}-{id}",
        std::process::id()
    ))
}
