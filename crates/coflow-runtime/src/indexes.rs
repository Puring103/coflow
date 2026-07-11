use std::collections::{BTreeMap, BTreeSet};

use coflow_api::{source_location_display_path, DiagnosticSet, FlatDiagnostic, ResolvedSource};
use coflow_data_model::RecordOrigin;
use coflow_data_model::{CfdDataModel, CfdRecordId};

use crate::RecordCoordinate;

#[derive(Debug, Clone, Default)]
pub struct DiagnosticsStore {
    diagnostics: DiagnosticSet,
    by_stage: BTreeMap<String, Vec<usize>>,
    by_file: BTreeMap<String, Vec<usize>>,
    by_record: BTreeMap<RecordCoordinate, Vec<usize>>,
    logical_locations: BTreeMap<usize, DiagnosticLogicalLocation>,
}

impl DiagnosticsStore {
    #[must_use]
    pub fn empty() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn from_set(diagnostics: DiagnosticSet) -> Self {
        Self::from_parts(diagnostics, BTreeMap::new())
    }

    #[must_use]
    pub fn from_parts(
        diagnostics: DiagnosticSet,
        logical_locations: BTreeMap<usize, DiagnosticLogicalLocation>,
    ) -> Self {
        let mut store = Self {
            diagnostics,
            by_stage: BTreeMap::new(),
            by_file: BTreeMap::new(),
            by_record: BTreeMap::new(),
            logical_locations,
        };
        store.rebuild_indexes();
        store
    }

    pub fn extend(&mut self, diagnostics: DiagnosticSet) {
        self.diagnostics.extend(diagnostics);
        self.rebuild_indexes();
    }

    pub fn extend_with_logical_locations(
        &mut self,
        diagnostics: DiagnosticSet,
        logical_locations: BTreeMap<usize, DiagnosticLogicalLocation>,
    ) {
        let offset = self.diagnostics.diagnostics.len();
        self.diagnostics.extend(diagnostics);
        for (index, location) in logical_locations {
            self.logical_locations.insert(offset + index, location);
        }
        self.rebuild_indexes();
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.diagnostics.is_empty()
    }

    #[must_use]
    pub const fn as_set(&self) -> &DiagnosticSet {
        &self.diagnostics
    }

    #[must_use]
    pub fn into_set(self) -> DiagnosticSet {
        self.diagnostics
    }

    #[must_use]
    pub fn logical_location(&self, index: usize) -> Option<&DiagnosticLogicalLocation> {
        self.logical_locations.get(&index)
    }

    #[must_use]
    pub fn flat_diagnostics(&self) -> Vec<FlatDiagnostic> {
        self.diagnostics
            .diagnostics
            .iter()
            .enumerate()
            .map(|(index, diagnostic)| {
                let location = self.logical_location(index);
                let actual_type = location.and_then(|location| location.actual_type.clone());
                let record_key = location.and_then(|location| location.record_key.clone());
                let field_path = location.and_then(|location| location.field_path.clone());
                diagnostic.flat_view(actual_type, record_key, field_path)
            })
            .collect()
    }

    #[must_use]
    pub fn by_stage(&self, stage: &str) -> &[usize] {
        self.by_stage.get(stage).map_or(&[], Vec::as_slice)
    }

    #[must_use]
    pub fn by_file(&self, file: &str) -> &[usize] {
        self.by_file.get(file).map_or(&[], Vec::as_slice)
    }

    #[must_use]
    pub fn by_record(&self, actual_type: &str, record_key: &str) -> &[usize] {
        self.by_record
            .get(&RecordCoordinate::new(actual_type, record_key))
            .map_or(&[], Vec::as_slice)
    }

    fn rebuild_indexes(&mut self) {
        self.by_stage.clear();
        self.by_file.clear();
        self.by_record.clear();
        for (index, diagnostic) in self.diagnostics.diagnostics.iter().enumerate() {
            self.by_stage
                .entry(diagnostic.stage.clone())
                .or_default()
                .push(index);
            if let Some(file) = diagnostic
                .primary
                .as_ref()
                .map(|label| source_location_display_path(&label.location))
            {
                self.by_file.entry(file).or_default().push(index);
            }
            if let Some(location) = self.logical_locations.get(&index) {
                if let Some(coordinate) = location.coordinate() {
                    self.by_record.entry(coordinate).or_default().push(index);
                }
            }
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DiagnosticLogicalLocation {
    pub actual_type: Option<String>,
    pub record_key: Option<String>,
    pub field_path: Option<String>,
}

impl DiagnosticLogicalLocation {
    #[must_use]
    pub fn coordinate(&self) -> Option<RecordCoordinate> {
        Some(RecordCoordinate::new(
            self.actual_type.clone()?,
            self.record_key.clone()?,
        ))
    }
}

#[derive(Debug, Clone, Default)]
pub struct SourceIndex {
    pub(crate) entries: Vec<ResolvedSourceEntry>,
}

impl SourceIndex {
    #[must_use]
    pub fn entries(&self) -> &[ResolvedSourceEntry] {
        &self.entries
    }

    pub(crate) fn push(&mut self, entry: ResolvedSourceEntry) {
        self.entries.push(entry);
    }
}

#[derive(Debug, Clone)]
pub struct ResolvedSourceEntry {
    pub id: SourceId,
    pub provider_id: String,
    pub source: ResolvedSource,
    pub display_path: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SourceId(pub(crate) usize);

impl SourceId {
    #[must_use]
    pub const fn index(self) -> usize {
        self.0
    }
}

/// Index of every top-level record in the project.
///
/// The authoritative key is `(actual_type, key)` so synthetic records that
/// share a key with their source record do not collide.
///
/// Loaders push `PendingRecordRef` entries during the load pass; after
/// `model.build()` returns, [`RecordIndex::finalize_with_model`] walks
/// `model.records()` and matches each `CfdRecord` back to its pending entry
/// by `(actual_type, key)`, producing a fully-populated [`RecordRef`] per id.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RecordIndex {
    by_id: BTreeMap<CfdRecordId, RecordRef>,
    by_coordinate: BTreeMap<RecordCoordinate, CfdRecordId>,
    files: BTreeMap<String, Vec<CfdRecordId>>,
    rejected: Vec<RejectedRecordRef>,
    rejected_files: BTreeMap<String, Vec<usize>>,
    rejected_by_coordinate: BTreeMap<RecordCoordinate, Vec<usize>>,
    pending: Vec<PendingRecordRef>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PendingRecordRef {
    pub coordinate: RecordCoordinate,
    pub origin: RecordOrigin,
    pub source_id: SourceId,
    pub provider_id: String,
    pub display_path: String,
}

impl RecordIndex {
    #[must_use]
    pub fn get(&self, id: CfdRecordId) -> Option<&RecordRef> {
        self.by_id.get(&id)
    }

    #[must_use]
    pub fn get_by_coordinate(&self, actual_type: &str, key: &str) -> Option<&RecordRef> {
        let id = self
            .by_coordinate
            .get(&RecordCoordinate::new(actual_type, key))?;
        self.by_id.get(id)
    }

    #[must_use]
    pub fn id_for_coordinate(&self, actual_type: &str, key: &str) -> Option<CfdRecordId> {
        self.by_coordinate
            .get(&RecordCoordinate::new(actual_type, key))
            .copied()
    }

    #[must_use]
    pub fn ids_in_file(&self, file: &str) -> &[CfdRecordId] {
        self.files.get(file).map_or(&[], Vec::as_slice)
    }

    pub fn coordinates_in_file<'a>(
        &'a self,
        file: &str,
    ) -> impl Iterator<Item = &'a RecordCoordinate> + 'a {
        self.ids_in_file(file)
            .iter()
            .filter_map(move |id| self.by_id.get(id).map(|r| &r.coordinate))
    }

    #[must_use]
    pub fn file_for_id(&self, id: CfdRecordId) -> Option<&str> {
        self.by_id.get(&id).map(|r| r.display_path.as_str())
    }

    #[must_use]
    pub fn file_for_coordinate(&self, actual_type: &str, key: &str) -> Option<&str> {
        self.get_by_coordinate(actual_type, key)
            .map(|r| r.display_path.as_str())
    }

    #[must_use]
    pub const fn by_id(&self) -> &BTreeMap<CfdRecordId, RecordRef> {
        &self.by_id
    }

    #[must_use]
    pub const fn by_file(&self) -> &BTreeMap<String, Vec<CfdRecordId>> {
        &self.files
    }

    #[must_use]
    pub fn rejected(&self) -> &[RejectedRecordRef] {
        &self.rejected
    }

    pub fn rejected_in_file(&self, file: &str) -> impl Iterator<Item = &RejectedRecordRef> {
        self.rejected_files
            .get(file)
            .into_iter()
            .flatten()
            .filter_map(|index| self.rejected.get(*index))
    }

    pub fn rejected_by_coordinate(
        &self,
        actual_type: &str,
        key: &str,
    ) -> impl Iterator<Item = &RejectedRecordRef> {
        self.rejected_by_coordinate
            .get(&RecordCoordinate::new(actual_type, key))
            .into_iter()
            .flatten()
            .filter_map(|index| self.rejected.get(*index))
    }

    pub(crate) fn push_pending(&mut self, pending: PendingRecordRef) {
        self.pending.push(pending);
    }

    /// After `model.build()` succeeds, match each model record back to a
    /// pending entry by `(actual_type, key)`. Pending entries that do not
    /// match a model record are kept as rejected source rows so hosts can
    /// still show invalid input alongside diagnostics.
    pub(crate) fn finalize_with_model(&mut self, model: &CfdDataModel) {
        self.by_id.clear();
        self.by_coordinate.clear();
        self.files.clear();
        self.rejected.clear();
        self.rejected_files.clear();
        self.rejected_by_coordinate.clear();
        // Index pending by coordinate, popping each entry as it's matched so
        // duplicate loader output (theoretically impossible since model
        // build rejects duplicates) doesn't reuse the same metadata twice.
        let mut pending_by_coordinate: BTreeMap<RecordCoordinate, Vec<PendingRecordRef>> =
            BTreeMap::new();
        for pending in std::mem::take(&mut self.pending) {
            pending_by_coordinate
                .entry(pending.coordinate.clone())
                .or_default()
                .push(pending);
        }
        for (id, record) in model.records() {
            let coordinate = RecordCoordinate::new(record.actual_type(), record.key.clone());
            let Some(mut candidates) = pending_by_coordinate.remove(&coordinate) else {
                continue;
            };
            let pending = candidates.remove(0);
            for duplicate in candidates {
                self.push_rejected(duplicate);
            }
            self.files
                .entry(pending.display_path.clone())
                .or_default()
                .push(id);
            self.by_coordinate.insert(coordinate.clone(), id);
            self.by_id.insert(
                id,
                RecordRef {
                    id,
                    coordinate,
                    origin: pending.origin,
                    source_id: pending.source_id,
                    provider_id: pending.provider_id,
                    display_path: pending.display_path,
                },
            );
        }
        for pending in pending_by_coordinate.into_values().flatten() {
            self.push_rejected(pending);
        }
    }

    pub(crate) fn finalize_rejected_pending(&mut self) {
        self.by_id.clear();
        self.by_coordinate.clear();
        self.files.clear();
        self.rejected.clear();
        self.rejected_files.clear();
        self.rejected_by_coordinate.clear();
        for pending in std::mem::take(&mut self.pending) {
            self.push_rejected(pending);
        }
    }

    fn push_rejected(&mut self, pending: PendingRecordRef) {
        let index = self.rejected.len();
        self.rejected_files
            .entry(pending.display_path.clone())
            .or_default()
            .push(index);
        self.rejected_by_coordinate
            .entry(pending.coordinate.clone())
            .or_default()
            .push(index);
        self.rejected.push(RejectedRecordRef {
            coordinate: pending.coordinate,
            origin: pending.origin,
            source_id: pending.source_id,
            provider_id: pending.provider_id,
            display_path: pending.display_path,
        });
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordRef {
    pub id: CfdRecordId,
    pub coordinate: RecordCoordinate,
    pub origin: RecordOrigin,
    pub source_id: SourceId,
    pub provider_id: String,
    pub display_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RejectedRecordRef {
    pub coordinate: RecordCoordinate,
    pub origin: RecordOrigin,
    pub source_id: SourceId,
    pub provider_id: String,
    pub display_path: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FileIndex {
    source_files: BTreeSet<String>,
    display_to_source: BTreeMap<String, SourceId>,
}

impl FileIndex {
    #[must_use]
    pub const fn source_files(&self) -> &BTreeSet<String> {
        &self.source_files
    }

    #[must_use]
    pub fn source_for_display(&self, display_path: &str) -> Option<SourceId> {
        self.display_to_source.get(display_path).copied()
    }

    pub(crate) fn add_source_file(&mut self, display_path: String, source_id: SourceId) {
        self.source_files.insert(display_path.clone());
        self.display_to_source.insert(display_path, source_id);
    }
}
