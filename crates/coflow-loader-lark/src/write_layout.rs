use coflow_api::{DiagnosticSet, ResolvedSource, WriteContext};
use coflow_data_model::{RecordOrigin, SourceDocument};
use coflow_loader_table_core::{resolve_table_write_layout, TableWriteLayout};

use crate::diagnostics::table_diagnostics_to_api;
use crate::http::LarkHttpClient;
use crate::remote::LarkAuth;
use crate::source::{
    lark_document_spreadsheet_token, lark_source_options, sheet_config_from_options,
};
use crate::LarkSheetWriter;

pub(crate) struct LarkInsertLayoutRequest<'a, C> {
    pub(crate) ctx: WriteContext<'a>,
    pub(crate) writer: &'a LarkSheetWriter<C>,
    pub(crate) source: &'a ResolvedSource,
    pub(crate) spreadsheet_token: &'a str,
    pub(crate) sheet_id: &'a str,
    pub(crate) sheet: &'a str,
    pub(crate) actual_type: &'a str,
    pub(crate) auth: &'a LarkAuth,
}

pub(crate) fn lark_insert_layout<C>(
    request: &LarkInsertLayoutRequest<'_, C>,
) -> Result<TableWriteLayout, DiagnosticSet>
where
    C: LarkHttpClient + Send + Sync,
{
    if let Some(model) = request.ctx.model {
        if let Some(layout) = model.records().find_map(|(_, record)| {
            let RecordOrigin::Table {
                document,
                sheet: record_sheet,
                id_column,
                field_columns,
                ..
            } = &record.origin
            else {
                return None;
            };
            let SourceDocument::Remote(doc) = document else {
                return None;
            };
            (lark_document_spreadsheet_token(doc).as_deref() == Some(request.spreadsheet_token)
                && record_sheet == request.sheet
                && record.actual_type() == request.actual_type)
                .then_some(TableWriteLayout {
                    id_column: *id_column,
                    field_columns: field_columns.clone(),
                })
        }) {
            return Ok(layout);
        }
    }
    let header = request.writer.read_lark_header(
        request.spreadsheet_token,
        request.sheet_id,
        request.auth,
    )?;
    let config = sheet_config_from_options(
        lark_source_options(request.source)?,
        request.sheet,
        request.actual_type,
    )?;
    resolve_table_write_layout(
        request.ctx.schema,
        std::path::Path::new(request.spreadsheet_token),
        &config,
        &header,
    )
    .map_err(table_diagnostics_to_api)
}
