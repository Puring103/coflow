use super::output::file_error_report;
use coflow_api::ProviderRegistry;
use coflow_runtime::{
    create_data_file, sync_data_header, DataCreateFileOptions, DataFileReport,
    DataSyncHeaderOptions, ProjectSchemaSession,
};

pub(super) fn create_file_report(
    session: &ProjectSchemaSession,
    registry: &ProviderRegistry,
    file: String,
    actual_type: Option<String>,
    provider: Option<String>,
    sheet: Option<String>,
) -> DataFileReport {
    create_data_file(
        session,
        registry,
        DataCreateFileOptions {
            file,
            actual_type,
            provider,
            sheet,
        },
    )
    .unwrap_or_else(|diagnostics| file_error_report(&diagnostics))
}

pub(super) fn create_table_report(
    session: &ProjectSchemaSession,
    registry: &ProviderRegistry,
    source: String,
    actual_type: Option<String>,
    provider: Option<&str>,
    sheet: Option<String>,
) -> DataFileReport {
    let result = create_data_file(
        session,
        registry,
        DataCreateFileOptions {
            file: source,
            actual_type,
            provider: provider.map(str::to_string),
            sheet,
        },
    );
    result.unwrap_or_else(|diagnostics| file_error_report(&diagnostics))
}

pub(super) fn sync_header_report(
    session: &ProjectSchemaSession,
    registry: &ProviderRegistry,
    file: String,
    actual_type: String,
    provider: Option<String>,
    sheet: Option<String>,
) -> DataFileReport {
    let result = sync_data_header(
        session,
        registry,
        DataSyncHeaderOptions {
            file,
            actual_type,
            provider,
            sheet,
        },
    );
    result.unwrap_or_else(|diagnostics| file_error_report(&diagnostics))
}
