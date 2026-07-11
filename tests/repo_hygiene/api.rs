use super::*;

#[test]
fn api_source_contexts_use_compiled_schema_not_full_container() {
    let provider =
        std::fs::read_to_string("crates/coflow-api/src/provider.rs").expect("read API provider");
    let resolve_context = struct_block(&provider, "pub struct SourceResolveContext")
        .expect("find SourceResolveContext");

    assert!(
        provider.contains("use coflow_cft::CompiledSchema;"),
        "source provider contexts should depend on the schema query facade"
    );
    assert!(
        !resolve_context.contains("schema"),
        "source provider resolve context should not expose schema; only load needs schema semantics"
    );
    for forbidden in [
        "use coflow_cft::CftContainer",
        "pub schema: &'a CftContainer",
        "pub schema: &'a mut CftContainer",
    ] {
        assert!(
            !provider.contains(forbidden),
            "source provider contexts should not expose full schema container `{forbidden}`"
        );
    }
}

#[test]
fn api_export_context_uses_compiled_schema_not_full_container() {
    let data_output = std::fs::read_to_string("crates/coflow-api/src/data_output.rs")
        .expect("read API data output");
    let export_context =
        struct_block(&data_output, "pub struct ExportContext").expect("find ExportContext");

    assert!(
        data_output.contains("use coflow_cft::CompiledSchema;"),
        "export context should depend on the schema query facade"
    );
    assert!(
        export_context.contains("pub schema: &'a CompiledSchema"),
        "export context should expose schema view"
    );
    assert!(
        !export_context.contains("CftContainer"),
        "export context should not expose full schema container"
    );
}

#[test]
fn api_codegen_context_uses_compiled_schema_not_full_container() {
    let codegen =
        std::fs::read_to_string("crates/coflow-api/src/codegen.rs").expect("read API codegen");
    let codegen_context =
        struct_block(&codegen, "pub struct CodegenContext").expect("find CodegenContext");

    assert!(
        codegen.contains("use coflow_cft::CompiledSchema;"),
        "codegen context should depend on the schema query facade"
    );
    assert!(
        codegen_context.contains("pub schema: &'a CompiledSchema"),
        "codegen context should expose schema view"
    );
    assert!(
        !codegen_context.contains("CftContainer"),
        "codegen context should not expose full schema container"
    );
}


#[test]
fn checker_does_not_depend_on_project_config() {
    let manifest = std::fs::read_to_string("crates/coflow-checker/Cargo.toml")
        .expect("read checker manifest");
    let checker = std::fs::read_to_string("crates/coflow-checker/src/lib.rs")
        .expect("read checker source");

    assert!(
        !manifest.contains("coflow-project"),
        "coflow-checker should accept a check plan, not depend on project config"
    );
    for forbidden in ["coflow_project", "DimensionConfig"] {
        assert!(
            !checker.contains(forbidden),
            "coflow-checker should not know project config item `{forbidden}`"
        );
    }
    assert!(
        checker.contains("pub struct DimensionCheckPlan")
            && checker.contains("pub struct DimensionCheckRound"),
        "coflow-checker should expose its own dimension check plan interface"
    );
}

#[test]
fn api_registry_is_split_by_responsibility() {
    let registry =
        std::fs::read_to_string("crates/coflow-api/src/registry.rs").expect("read API registry");
    let selection = std::fs::read_to_string("crates/coflow-api/src/registry/selection.rs")
        .expect("read API registry selection");
    let errors = std::fs::read_to_string("crates/coflow-api/src/registry/errors.rs")
        .expect("read API registry errors");

    for expected in [
        "pub fn select_source_provider",
        "SourceProviderSelectionError::UnknownSourceProvider",
        "SourceProviderSelectionError::AmbiguousSourceProviders",
    ] {
        assert!(
            selection.contains(expected),
            "API registry selection item `{expected}` should live in registry/selection.rs"
        );
        assert!(
            !registry.contains(expected),
            "API registry selection item `{expected}` should not live in registry.rs"
        );
    }
    for expected in [
        "pub enum SourceProviderSelectionError",
        "pub struct ProviderRegistrationError",
        "impl std::error::Error for ProviderRegistrationError",
    ] {
        assert!(
            errors.contains(expected),
            "API registry error item `{expected}` should live in registry/errors.rs"
        );
        assert!(
            !registry.contains(expected),
            "API registry error item `{expected}` should not live in registry.rs"
        );
    }
    assert!(
        registry.lines().count() < 260,
        "coflow-api registry.rs should stay focused on provider storage and lookup"
    );
    for expected in [
        "source_providers:",
        "source_writers:",
        "pub fn register_source_provider",
        "pub fn register_source_writer",
        "pub fn source_provider(",
        "pub fn source_writer(",
        "pub fn source_provider_descriptors",
        "pub fn source_writer_descriptors",
    ] {
        assert!(
            registry.contains(expected),
            "API registry should expose source provider/writer item `{expected}`"
        );
    }
    for forbidden in [
        "DataLoader",
        "DataWriter",
        "register_loader",
        "register_writer",
        "pub fn loader(",
        "pub fn writer(",
        "pub fn loader_descriptors",
        "pub fn writer_descriptors",
        "    loaders: BTreeMap",
        "    writers: BTreeMap",
    ] {
        assert!(
            !registry.contains(forbidden),
            "API registry should use source provider/writer naming instead of `{forbidden}`"
        );
    }
}

#[test]
fn api_registry_supports_shared_role_registration() {
    let registry =
        std::fs::read_to_string("crates/coflow-api/src/registry.rs").expect("read API registry");
    let registration = std::fs::read_to_string("crates/coflow-api/src/registry/registration.rs")
        .expect("read API registry registration");
    let bundle = std::fs::read_to_string("crates/coflow-api/src/registry/bundle.rs")
        .expect("read API provider bundle");

    for expected in [
        "pub fn register_source_writer_arc",
        "pub fn register_table_manager_arc",
        "pub fn register_dimension_source_manager_arc",
        "fn insert_provider<T: ?Sized>",
    ] {
        assert!(
            registration.contains(expected),
            "API registry shared role registration item `{expected}` should live in registry/registration.rs"
        );
        assert!(
            !registry.contains(expected),
            "API registry shared role registration item `{expected}` should not live in registry.rs"
        );
    }

    for forbidden in [
        "self.source_writers.insert(id, Arc::new(writer))",
        "self.table_managers.insert(id, Arc::new(manager))",
        "self.dimension_source_managers.insert(id, Arc::new(manager))",
    ] {
        assert!(
            !registry.contains(forbidden),
            "API registry should route role storage through shared Arc registration, not `{forbidden}`"
        );
    }

    for expected in [
        "pub struct ProviderBundle",
        "pub fn register_bundle(",
        "ensure_available(",
        "self.source_providers.extend(bundle.source_providers)",
    ] {
        assert!(
            bundle.contains(expected),
            "provider bundle should validate all roles before atomic registration via `{expected}`"
        );
    }
}

#[test]
fn builtins_share_provider_role_instances() {
    let builtins = std::fs::read_to_string("crates/coflow-builtins/src/lib.rs")
        .expect("read builtins registry");

    for expected in [
        "let excel_writer = Arc::new(coflow_loader_excel::ExcelWriter::new())",
        "let csv_writer = Arc::new(coflow_loader_csv::CsvWriter::new())",
        "let lark_writer = Arc::new(coflow_loader_lark::LarkSheetWriter::default())",
        "let cfd_writer = Arc::new(coflow_loader_cfd::CfdWriter::new())",
        "bundle.add_source_writer_arc(Arc::clone(&excel_writer))",
        "bundle.add_table_manager_arc(Arc::clone(&excel_writer))",
        "bundle.add_dimension_source_manager_arc(Arc::clone(&csv_writer))",
        "bundle.add_dimension_source_manager_arc(Arc::clone(&cfd_writer))",
        "registry.register_bundle(bundle)",
    ] {
        assert!(
            builtins.contains(expected),
            "builtins should share provider role instance via `{expected}`"
        );
    }

    for (constructor, count) in [
        ("ExcelWriter::new()", 1),
        ("CsvWriter::new()", 1),
        ("LarkSheetWriter::default()", 1),
        ("CfdWriter::new()", 1),
    ] {
        assert_eq!(
            builtins.matches(constructor).count(),
            count,
            "builtins should construct `{constructor}` exactly once"
        );
    }
}

#[test]
fn api_writer_contract_is_split_by_responsibility() {
    let writer =
        std::fs::read_to_string("crates/coflow-api/src/writer.rs").expect("read API writer");
    let capabilities = std::fs::read_to_string("crates/coflow-api/src/writer/capabilities.rs")
        .expect("read API writer capabilities");
    let requests = std::fs::read_to_string("crates/coflow-api/src/writer/requests.rs")
        .expect("read API writer requests");

    for expected in [
        "pub struct WriterDescriptor",
        "pub struct WriterCapabilities",
        "impl WriterCapabilities",
    ] {
        assert!(
            capabilities.contains(expected),
            "API writer capability item `{expected}` should live in writer/capabilities.rs"
        );
        assert!(
            !writer.contains(expected),
            "API writer capability item `{expected}` should not live in writer.rs"
        );
    }
    for expected in [
        "pub type WriteFieldPathSegment = CfdPathSegment",
        "pub struct WriteCellRequest",
        "pub struct InsertRecordRequest",
        "pub struct DeleteRecordRequest",
        "pub struct RenameRecordRequest",
        "pub struct RewriteRecordReferencesRequest",
        "pub struct SpreadRewriteTarget",
        "pub struct WriteOutcome",
        "pub struct WriteContext",
    ] {
        assert!(
            requests.contains(expected),
            "API writer request item `{expected}` should live in writer/requests.rs"
        );
        assert!(
            !writer.contains(expected),
            "API writer request item `{expected}` should not live in writer.rs"
        );
    }
    assert!(
        requests.contains("use coflow_cft::CompiledSchema;"),
        "API writer requests should depend on the schema query facade"
    );
    for forbidden in [
        "CftContainer",
        "pub schema: &'a CftContainer",
        "pub schema: &'a mut CftContainer",
    ] {
        assert!(
            !requests.contains(forbidden),
            "API writer requests should not expose full schema container `{forbidden}`"
        );
    }
    assert!(
        requests.contains("pub schema: &'a CompiledSchema"),
        "writer schema-bearing requests should expose schema view"
    );
    assert!(
        writer.contains("pub trait SourceWriter"),
        "API writer trait should remain in writer.rs"
    );
    assert!(
        !writer.contains("pub trait DataWriter"),
        "API writer contract should be named SourceWriter"
    );
    for forbidden in ["CreateTableRequest", "fn create_table"] {
        assert!(
            !writer.contains(forbidden),
            "table creation should live on TableManager, not SourceWriter: `{forbidden}`"
        );
    }
    assert!(
        writer.lines().count() < 170,
        "coflow-api writer.rs should stay focused on the SourceWriter trait"
    );
}

#[test]
fn api_table_operations_do_not_expose_schema_owner() {
    let operations =
        std::fs::read_to_string("crates/coflow-api/src/operations.rs").expect("read operations");
    let table_context =
        struct_block(&operations, "pub struct TableContext").expect("find TableContext");
    let create_request =
        struct_block(&operations, "pub struct CreateTableRequest").expect("find CreateTableRequest");
    let sync_request =
        struct_block(&operations, "pub struct SyncHeaderRequest").expect("find SyncHeaderRequest");

    for expected in [
        "pub struct TableContext",
        "pub struct TableHeaderOptions",
        "pub struct CreateTableRequest",
        "pub struct SyncHeaderRequest",
        "fn type_for_sheet(",
        "fn sheet_for_type(",
        "fn header_options(",
    ] {
        assert!(
            operations.contains(expected),
            "table operation item `{expected}` should remain in operations.rs"
        );
    }
    assert!(
        operations.contains("use coflow_cft::CompiledSchema;"),
        "sync header should depend on the schema query facade"
    );
    assert!(
        sync_request.contains("pub schema: Option<&'a CompiledSchema>"),
        "sync header should expose only optional schema view metadata"
    );
    assert!(
        !table_context.contains("schema") && !create_request.contains("schema"),
        "table context and create table request should receive planned headers, not schema"
    );
    assert!(
        !operations.contains("CftContainer"),
        "table operations should not expose full schema container"
    );
}


