use std::collections::BTreeSet;

use coflow_api::{ProviderRegistry, WriterCapabilities};
use coflow_cft::{CftSchema, CftSchemaTypeRef};
use coflow_data_model::{
    CfdPath, CfdPathSegment, CfdRecordId, CfdValue, DimensionValueLookup, RefSite,
};

use crate::indexes::{FileIndex, RecordIndex, SourceIndex};
use crate::{
    DiagnosticsStore, DimensionInfo, DimensionValueOrigin, DimensionValueState, DimensionValueView,
    EffectiveFieldWrite, FieldShapeInfo, FileTreeNode, FileTreeOptions, IdAsEnumInfo,
    ProjectSession, RecordCoordinate, RecordReferenceInfo, RecordView, RefTargetInfo,
};

/// Read-only capability over one immutable project generation.
///
/// Hosts receive this view instead of the owning runtime session, so query
/// code cannot reach mutation methods or replace the active generation.
#[derive(Debug, Clone, Copy)]
pub struct ProjectQueries<'a> {
    session: &'a ProjectSession,
    revision: u64,
}

impl<'a> ProjectQueries<'a> {
    pub(crate) const fn new(session: &'a ProjectSession, revision: u64) -> Self {
        Self { session, revision }
    }

    #[must_use]
    pub const fn revision(self) -> u64 {
        self.revision
    }

    #[must_use]
    pub const fn diagnostics(self) -> &'a DiagnosticsStore {
        self.session.diagnostics()
    }

    #[must_use]
    pub(crate) const fn sources(self) -> &'a SourceIndex {
        self.session.sources()
    }

    #[must_use]
    pub(crate) const fn records(self) -> &'a RecordIndex {
        self.session.records()
    }

    #[must_use]
    pub(crate) const fn files(self) -> &'a FileIndex {
        self.session.files()
    }

    #[must_use]
    pub fn source_file_count(self) -> usize {
        self.session.files().source_files().len()
    }

    #[must_use]
    pub fn record_count(self) -> usize {
        self.session.records().by_id().len()
    }

    #[must_use]
    pub fn record_count_for_type(self, actual_type: &str) -> usize {
        self.session
            .model()
            .table(actual_type)
            .map_or(0, |table| table.records.len())
    }

    #[must_use]
    pub fn schema_has_type(self, type_name: &str) -> bool {
        self.session.schema().resolve_type(type_name).is_some()
    }

    #[must_use]
    pub fn schema_type_names(self) -> Vec<String> {
        self.session
            .schema()
            .all_types()
            .map(|schema_type| schema_type.name.to_string())
            .collect()
    }

    #[must_use]
    pub fn schema_type_fields(self, type_name: &str) -> Vec<(String, String)> {
        self.session
            .schema()
            .resolve_type(type_name)
            .map(|meta| {
                meta.all_fields()
                    .map(|field| (field.name.to_string(), field.ty_ref.display_label()))
                    .collect()
            })
            .unwrap_or_default()
    }

    #[must_use]
    pub fn rejected_records(self) -> &'a [crate::RejectedRecordRef] {
        self.session.records().rejected()
    }

    #[must_use]
    pub fn has_source_file(self, file: &str) -> bool {
        self.session.files().source_files().contains(file)
    }

    #[must_use]
    pub fn has_unique_source_for_file(self, file: &str) -> bool {
        self.session.files().source_for_display(file).is_some()
    }

    pub fn rejected_records_in_file(
        self,
        file: &str,
    ) -> impl Iterator<Item = &'a crate::RejectedRecordRef> + 'a {
        self.session.records().rejected_in_file(file)
    }

    pub fn rejected_records_by_coordinate(
        self,
        actual_type: &str,
        key: &str,
    ) -> impl Iterator<Item = &'a crate::RejectedRecordRef> + 'a {
        self.session
            .records()
            .rejected_by_coordinate(actual_type, key)
    }

    #[must_use]
    pub fn has_diagnostics(self) -> bool {
        self.session.has_diagnostics()
    }

    #[must_use]
    fn id_for_coordinate(self, actual_type: &str, key: &str) -> Option<CfdRecordId> {
        self.session.id_for_coordinate(actual_type, key)
    }

    #[must_use]
    fn coordinate_of(self, id: CfdRecordId) -> Option<RecordCoordinate> {
        self.session.coordinate_of(id)
    }

    #[must_use]
    pub fn file_for_record(self, actual_type: &str, key: &str) -> Option<&'a str> {
        self.session.file_for_record(actual_type, key)
    }

    pub fn coordinates_in_file(
        self,
        file: &str,
    ) -> impl Iterator<Item = &'a RecordCoordinate> + 'a {
        self.session.coordinates_in_file(file)
    }

    #[must_use]
    pub fn enum_int_value(self, enum_name: &str, variant: &str) -> Option<i64> {
        self.session.enum_int_value(enum_name, variant)
    }

    #[must_use]
    pub fn enum_variants(self, enum_name: &str) -> Vec<String> {
        self.session.enum_variants(enum_name)
    }

    #[must_use]
    pub fn dimensions(self) -> Vec<DimensionInfo> {
        self.session.dimensions()
    }

    #[must_use]
    pub fn dimension(self, name: &str) -> Option<DimensionInfo> {
        self.session.dimension(name)
    }

    #[must_use]
    pub fn dimension_field_for_file(
        self,
        file_path: &str,
    ) -> Option<(DimensionInfo, String, String)> {
        let normalized_path = file_path.replace('\\', "/");
        let stem = std::path::Path::new(&normalized_path)
            .file_stem()
            .and_then(|value| value.to_str())?;
        for info in self.dimensions() {
            let Some(out_dir) = info.out_dir.as_ref() else {
                continue;
            };
            let out_dir = out_dir.replace('\\', "/");
            if !normalized_path.starts_with(&format!("{}/", out_dir.trim_end_matches('/'))) {
                continue;
            }
            let field = crate::dimensions::dimension_fields(self.session.schema())
                .into_iter()
                .find(|field| {
                    field.dimension.as_str() == info.name
                        && format!("{}_{}", field.bucket, field.source_field) == stem
                })?;
            return Some((
                info,
                field.source_type.to_string(),
                field.source_field.to_string(),
            ));
        }
        None
    }

    #[must_use]
    pub fn dimension_value(
        self,
        coordinate: &crate::DimensionValueCoordinate,
    ) -> Option<DimensionValueView> {
        let record_id = self.id_for_coordinate(
            coordinate.actual_type.as_str(),
            coordinate.record_key.as_str(),
        )?;
        match self
            .session
            .model()
            .dimension_field_value(
                self.session.schema(),
                record_id,
                coordinate.field.as_str(),
                coordinate.dimension.as_str(),
                coordinate.variant.as_str(),
            )
            .ok()?
        {
            DimensionValueLookup::Value { value, origin } => Some(DimensionValueView {
                state: DimensionValueState::Value(
                    dimension_value_at_path(value, &coordinate.path)?.clone(),
                ),
                origin: DimensionValueOrigin::from_record_origin(origin),
            }),
            DimensionValueLookup::ExplicitNull { origin } => {
                coordinate.path.is_empty().then(|| DimensionValueView {
                    state: DimensionValueState::Value(CfdValue::Null),
                    origin: DimensionValueOrigin::from_record_origin(origin),
                })
            }
            DimensionValueLookup::Missing => Some(DimensionValueView {
                state: DimensionValueState::Missing,
                origin: None,
            }),
        }
    }

    #[must_use]
    pub fn record_view(self, actual_type: &str, key: &str) -> Option<RecordView<'a>> {
        self.session.record_view(actual_type, key)
    }

    #[must_use]
    pub fn field_value(
        self,
        actual_type: &str,
        key: &str,
        path: &[CfdPathSegment],
    ) -> Option<&'a CfdValue> {
        self.session.field_value(actual_type, key, path)
    }

    #[must_use]
    pub fn effective_field_write(
        self,
        coordinate: &RecordCoordinate,
        path: &[CfdPathSegment],
    ) -> Option<EffectiveFieldWrite> {
        self.session.effective_field_write(coordinate, path)
    }

    #[must_use]
    pub fn ref_targets(self, expected_type: &str) -> Vec<RefTargetInfo> {
        self.session.ref_targets(expected_type)
    }

    #[must_use]
    pub fn field_shape(self, actual_type: &str, field_name: &str) -> Option<FieldShapeInfo> {
        let field = self.session.schema().field(actual_type, field_name)?;
        Some(field_shape(self.session.schema(), &field.ty_ref))
    }

    #[must_use]
    pub fn spread_source(
        self,
        coordinate: &RecordCoordinate,
        path: &CfdPath,
    ) -> Option<RecordCoordinate> {
        let host = self.id_for_coordinate(&coordinate.actual_type, &coordinate.key)?;
        let source = self.session.model().spread_source_at_path(host, path)?;
        self.coordinate_of(source)
    }

    #[must_use]
    pub fn resolved_ref_target(
        self,
        coordinate: &RecordCoordinate,
        path: &CfdPath,
    ) -> Option<RecordCoordinate> {
        let host = self.id_for_coordinate(&coordinate.actual_type, &coordinate.key)?;
        let target = self
            .session
            .model()
            .resolve_effective_ref(&RefSite::new(host, path.clone()))?;
        self.coordinate_of(target)
    }

    #[must_use]
    pub fn record_references(self, coordinate: &RecordCoordinate) -> Vec<RecordReferenceInfo> {
        let Some(host) = self.id_for_coordinate(&coordinate.actual_type, &coordinate.key) else {
            return Vec::new();
        };
        self.session
            .model()
            .direct_ref_edges_from_host(host)
            .filter_map(|edge| {
                Some(RecordReferenceInfo {
                    target: self.coordinate_of(edge.target)?,
                    path: edge.site.path.clone(),
                    dimension: edge.site.dimension.clone(),
                })
            })
            .collect()
    }

    #[must_use]
    pub fn id_as_enum_info(self) -> Vec<IdAsEnumInfo> {
        let schema = self.session.schema();
        let model = self.session.model();
        schema
            .all_types()
            .filter_map(|schema_type| {
                let enum_name = schema_type.id_as_enum.as_ref()?.to_string();
                let is_flags = schema
                    .resolve_enum(&enum_name)
                    .is_some_and(|schema_enum| schema_enum.is_flag);
                let ids = model.polymorphic_index(&schema_type.name).map_or_else(
                    || {
                        model
                            .records_of_type(&schema_type.name)
                            .map(|(_, record)| record.key().to_string())
                            .collect()
                    },
                    |index| index.records.keys().cloned().collect(),
                );
                Some(IdAsEnumInfo {
                    enum_name,
                    ids,
                    is_flags,
                })
            })
            .collect()
    }

    pub fn record_views_in_file(self, file: &str) -> impl Iterator<Item = RecordView<'a>> + 'a {
        self.session.record_views_in_file(file)
    }

    #[must_use]
    pub fn file_tree(self) -> Vec<FileTreeNode> {
        self.session.file_tree()
    }

    #[must_use]
    pub fn file_tree_with(self, options: FileTreeOptions) -> Vec<FileTreeNode> {
        self.session.file_tree_with(options)
    }

    #[must_use]
    pub const fn loader_extensions(self) -> &'a BTreeSet<String> {
        self.session.loader_extensions()
    }

    pub(crate) fn writer_capabilities_for_file(
        self,
        registry: &ProviderRegistry,
        file: &str,
    ) -> WriterCapabilities {
        let Some(entry) = self
            .files()
            .source_for_display(file)
            .and_then(|source_id| self.sources().entries().get(source_id.index()))
        else {
            return WriterCapabilities::read_only().with_provider_id("unknown");
        };
        registry.source_writer(&entry.provider_id).map_or_else(
            || WriterCapabilities::read_only().with_provider_id(entry.provider_id.clone()),
            |writer| {
                writer
                    .capabilities(&entry.source)
                    .with_provider_id(entry.provider_id.clone())
            },
        )
    }

    /// Return the provider-resolved table/sheet name for a record type in a
    /// source file. Non-table providers and unmapped types return `None`.
    ///
    /// # Errors
    ///
    /// Returns provider diagnostics when the table source options cannot be
    /// resolved or the type-to-sheet mapping is invalid.
    pub fn table_sheet_for_type(
        self,
        registry: &ProviderRegistry,
        file: &str,
        actual_type: &str,
    ) -> Result<Option<String>, coflow_api::DiagnosticSet> {
        let Some(entry) = self
            .files()
            .source_for_display(file)
            .and_then(|source_id| self.sources().entries().get(source_id.index()))
        else {
            return Ok(None);
        };
        let Some(manager) = registry.table_manager(&entry.provider_id) else {
            return Ok(None);
        };
        manager.sheet_for_type(&entry.source, actual_type)
    }
}

fn dimension_value_at_path<'a>(
    mut value: &'a CfdValue,
    path: &[CfdPathSegment],
) -> Option<&'a CfdValue> {
    for segment in path {
        value = match (segment, value) {
            (CfdPathSegment::Field(field), CfdValue::Object(object)) => {
                object.fields().get(field)?
            }
            (CfdPathSegment::Index(index), CfdValue::Array(items)) => items.get(*index)?,
            (CfdPathSegment::DictKey(key), CfdValue::Dict(entries)) => {
                entries.iter().find_map(|(candidate, value)| {
                    (coflow_data_model::format_cfd_dict_key(candidate) == *key).then_some(value)
                })?
            }
            _ => return None,
        };
    }
    Some(value)
}

fn field_shape(schema: &CftSchema, ty: &CftSchemaTypeRef) -> FieldShapeInfo {
    let non_nullable = non_nullable(ty);
    let ref_target_type = match non_nullable {
        CftSchemaTypeRef::RecordRef(name) => Some(name.to_string()),
        _ => None,
    };
    let enum_type = match non_nullable {
        CftSchemaTypeRef::Enum(name) => Some(name.to_string()),
        _ => None,
    };
    let polymorphic_types = match non_nullable {
        CftSchemaTypeRef::Object(name) => Some(name.as_str()),
        _ => None,
    }
    .and_then(|name| schema.resolve_type(name).map(|meta| (name, meta)))
    .filter(|(_, meta)| meta.is_abstract)
    .and_then(|(name, _)| schema.concrete_assignable_types(name))
    .filter(|types| types.len() >= 2)
    .unwrap_or_default()
    .into_iter()
    .map(|name| name.to_string())
    .collect();
    let collection_item = match non_nullable {
        CftSchemaTypeRef::Array(item) | CftSchemaTypeRef::Dict(_, item) => {
            Some(Box::new(field_shape(schema, item)))
        }
        _ => None,
    };
    FieldShapeInfo {
        display_label: ty.display_label(),
        ref_target_type,
        enum_type,
        nullable: matches!(ty, CftSchemaTypeRef::Nullable(_)),
        polymorphic_types,
        collection_item,
    }
}

fn non_nullable(ty: &CftSchemaTypeRef) -> &CftSchemaTypeRef {
    match ty {
        CftSchemaTypeRef::Nullable(inner) => non_nullable(inner),
        _ => ty,
    }
}
