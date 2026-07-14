use std::collections::BTreeSet;
use std::path::Path;
use std::sync::Arc;

use coflow_api::{ArtifactSet, CodeGenerator, CodegenContext, DecodedOutputOptions, DiagnosticSet};
use coflow_cft::CftSchema;
use coflow_cft::{parse_modules, CftFile, CftModuleSet};
use coflow_data_model::{CfdDataModel, CfdPath, CfdPathSegment, CfdRecordId, CfdValue};
use coflow_project::{path_to_slash, Project};
use serde::{Deserialize, Serialize};

use crate::checks::CheckState;
use crate::dimensions::{self, dimensions_for_project, DimensionInfo};
use crate::files::{self, DimensionGroup, FileTreeNode, FileTreeOptions};
use crate::indexes::{DiagnosticsStore, FileIndex, RecordIndex, SourceIndex};
use crate::load::SourceDataCache;
use crate::records::{EffectiveFieldWrite, RecordView, RefTargetInfo};
use crate::writes::record_value_at_path;

/// Stable, wire-friendly coordinate of a top-level record.
///
/// Top-level records always have an `(actual_type, key)` pair that uniquely
/// identifies them inside a model build, even when synthetic dimension
/// records share keys with their source records.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
#[cfg_attr(
    feature = "ts-export",
    ts(export, export_to = "../../frontend/src/bindings/")
)]
pub struct RecordCoordinate {
    pub actual_type: String,
    pub key: String,
}

impl RecordCoordinate {
    #[must_use]
    pub fn new(actual_type: impl Into<String>, key: impl Into<String>) -> Self {
        Self {
            actual_type: actual_type.into(),
            key: key.into(),
        }
    }
}

#[derive(Debug)]
pub(crate) struct ProjectSession {
    pub(crate) project: Project,
    pub(crate) schema: Arc<CftSchema>,
    pub(crate) model: CfdDataModel,
    pub(crate) diagnostics: DiagnosticsStore,
    pub(crate) sources: SourceIndex,
    pub(crate) records: RecordIndex,
    pub(crate) files: FileIndex,
    pub(crate) loader_extensions: BTreeSet<String>,
    pub(crate) source_data: SourceDataCache,
    pub(crate) check_state: CheckState,
}

impl ProjectSession {
    #[must_use]
    pub fn schema(&self) -> &CftSchema {
        &self.schema
    }

    #[must_use]
    pub const fn model(&self) -> &CfdDataModel {
        &self.model
    }

    #[must_use]
    pub const fn diagnostics(&self) -> &DiagnosticsStore {
        &self.diagnostics
    }

    #[must_use]
    pub const fn sources(&self) -> &SourceIndex {
        &self.sources
    }

    #[must_use]
    pub const fn records(&self) -> &RecordIndex {
        &self.records
    }

    #[must_use]
    pub const fn files(&self) -> &FileIndex {
        &self.files
    }

    #[must_use]
    pub fn into_diagnostics(self) -> DiagnosticSet {
        self.diagnostics.into_set()
    }

    #[must_use]
    pub fn into_schema_session(self) -> ProjectSchemaSession {
        ProjectSchemaSession {
            project: self.project,
            modules: Arc::new(parse_modules(std::iter::empty::<CftFile>())),
            schema: self.schema,
            diagnostics: self.diagnostics,
        }
    }

    #[must_use]
    pub fn has_diagnostics(&self) -> bool {
        !self.diagnostics.is_empty()
    }

    /// Resolve a wire `(actual_type, key)` coordinate to its internal model
    /// id. Returns `None` when no record matches — callers surface an
    /// `EditorError::NotFound` rather than panic.
    #[must_use]
    pub fn id_for_coordinate(&self, actual_type: &str, key: &str) -> Option<CfdRecordId> {
        self.records.id_for_coordinate(actual_type, key)
    }

    /// Inverse of [`Self::id_for_coordinate`]: given an internal record id,
    /// return the wire coordinate. Lives here so model id leakage stays
    /// confined to the engine boundary.
    #[must_use]
    pub fn coordinate_of(&self, id: CfdRecordId) -> Option<RecordCoordinate> {
        self.records.get(id).map(|r| r.coordinate.clone())
    }

    /// Look up the project-relative file that backs a record, addressed by
    /// its wire coordinate.
    #[must_use]
    pub fn file_for_record(&self, actual_type: &str, key: &str) -> Option<&str> {
        self.records.file_for_coordinate(actual_type, key)
    }

    /// Iterate the coordinates of every top-level record in `file`. Used by
    /// hosts that render per-file record lists without exposing internal ids.
    pub fn coordinates_in_file<'a>(
        &'a self,
        file: &str,
    ) -> impl Iterator<Item = &'a RecordCoordinate> + 'a {
        self.records.coordinates_in_file(file)
    }

    /// Integer value of an enum variant declared in the project schema.
    /// Returns `None` for unknown enum names or variants.
    #[must_use]
    pub fn enum_int_value(&self, enum_name: &str, variant: &str) -> Option<i64> {
        self.schema()
            .enum_variant_value(enum_name, variant)
    }

    #[must_use]
    pub fn enum_variants(&self, enum_name: &str) -> Vec<String> {
        self.schema()
            .enum_meta(enum_name)
            .map(|meta| {
                meta.all_variants
                    .iter()
                    .map(|variant| variant.name.clone())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Resolved dimension metadata for the project.
    #[must_use]
    pub fn dimensions(&self) -> Vec<DimensionInfo> {
        let view = self.schema();
        let fields = dimensions::dimension_fields(view);
        dimensions_for_project(&self.project, &fields)
    }

    /// Set of dimension-synthesized runtime type names (e.g.
    /// `"Item_nameVariants"`). Hosts use this to mark synthesized
    /// records so their `default` slot renders as read-only in editors,
    /// without re-deriving the naming convention themselves.
    #[must_use]
    pub fn dimension_synthesized_types(&self) -> BTreeSet<String> {
        let view = self.schema();
        dimensions::dimension_fields(view)
            .into_iter()
            .map(|field| field.synthesized_type)
            .collect()
    }

    /// Lookup a single dimension by name.
    #[must_use]
    pub fn dimension(&self, name: &str) -> Option<DimensionInfo> {
        self.dimensions().into_iter().find(|d| d.name == name)
    }

    /// Compose a read-only [`RecordView`] for a coordinate. Returns `None`
    /// when no record matches — typically a stale coordinate after a rename.
    #[must_use]
    pub fn record_view(&self, actual_type: &str, key: &str) -> Option<RecordView<'_>> {
        let record_ref = self.records.get_by_coordinate(actual_type, key)?;
        let record = self.model.record(record_ref.id)?;
        Some(RecordView {
            coordinate: record_ref.coordinate.clone(),
            display_path: record_ref.display_path.as_str(),
            record,
            origin: &record_ref.origin,
            provider_id: record_ref.provider_id.as_str(),
        })
    }

    /// Read a record field by model path through the same path resolver the
    /// write engine uses for current-value checks.
    #[must_use]
    pub fn field_value(
        &self,
        actual_type: &str,
        key: &str,
        path: &[CfdPathSegment],
    ) -> Option<&CfdValue> {
        let record = self.record_view(actual_type, key)?;
        record_value_at_path(
            record.record,
            &CfdPath {
                segments: path.to_vec(),
            },
        )
    }

    #[must_use]
    pub fn effective_field_write(
        &self,
        coordinate: &RecordCoordinate,
        path: &[CfdPathSegment],
    ) -> Option<EffectiveFieldWrite> {
        let record_ref = self
            .records
            .get_by_coordinate(&coordinate.actual_type, &coordinate.key)?;
        let host_path = CfdPath {
            segments: path.to_vec(),
        };
        let (target_ref, target_path) = if let Some((source_id, source_path)) =
            self.model.spread_source_path(record_ref.id, &host_path)
        {
            (self.records.get(source_id)?, source_path.segments)
        } else {
            (record_ref, path.to_vec())
        };
        let target_record = self.model.record(target_ref.id)?;
        let old_value = record_value_at_path(
            target_record,
            &CfdPath {
                segments: target_path.clone(),
            },
        )
        .cloned();
        Some(EffectiveFieldWrite {
            host: coordinate.clone(),
            target: target_ref.coordinate.clone(),
            file_path: target_ref.display_path.clone(),
            field_path: target_path,
            old_value,
        })
    }

    #[must_use]
    pub fn ref_targets(&self, expected_type: &str) -> Vec<RefTargetInfo> {
        let mut targets = Vec::new();
        let schema = self.schema();
        let Some(domain_id) = self.model.type_domain_id(expected_type) else {
            return targets;
        };
        let Some(members) = self.model.domain_members(domain_id) else {
            return targets;
        };
        for type_id in members {
            let Some(type_name) = self.model.type_name(*type_id) else {
                continue;
            };
            if !schema.is_assignable(type_name, expected_type) {
                continue;
            }
            for (_, record) in self.model.records_of_type(type_name) {
                let Some(file_path) = self.file_for_record(record.actual_type(), &record.key)
                else {
                    continue;
                };
                targets.push(RefTargetInfo {
                    coordinate: RecordCoordinate::new(record.actual_type(), record.key.clone()),
                    file_path: file_path.to_string(),
                });
            }
        }
        targets.sort_by(|a, b| {
            a.coordinate
                .actual_type
                .cmp(&b.coordinate.actual_type)
                .then_with(|| a.coordinate.key.cmp(&b.coordinate.key))
        });
        targets.dedup_by(|a, b| a.coordinate == b.coordinate);
        targets
    }

    /// Iterate read-only views of every record backed by `file`.
    pub fn record_views_in_file<'a>(
        &'a self,
        file: &str,
    ) -> impl Iterator<Item = RecordView<'a>> + 'a {
        self.records.ids_in_file(file).iter().filter_map(move |id| {
            let record_ref = self.records.get(*id)?;
            let record = self.model.record(*id)?;
            Some(RecordView {
                coordinate: record_ref.coordinate.clone(),
                display_path: record_ref.display_path.as_str(),
                record,
                origin: &record_ref.origin,
                provider_id: record_ref.provider_id.as_str(),
            })
        })
    }

    /// File-tree view of the project using default options (every
    /// loader-registered extension is walked, dimension `out_dirs` become
    /// virtual subtrees).
    #[must_use]
    pub fn file_tree(&self) -> Vec<FileTreeNode> {
        let mut options = FileTreeOptions {
            extra_extensions: self.loader_extensions.iter().cloned().collect(),
            dimension_groups: Vec::new(),
            in_sources: BTreeSet::new(),
        };
        for source in self.files.source_files() {
            options.in_sources.insert(display_source_path(source));
        }
        for info in self.dimensions() {
            if let Some(out_dir) = info.out_dir.as_ref() {
                let absolute = self.project.resolve_path(Path::new(out_dir));
                options.dimension_groups.push(DimensionGroup {
                    display_name: info.display_name.clone(),
                    dir: absolute,
                });
            }
        }
        self.file_tree_with(options)
    }

    /// File-tree view using caller-supplied options. The options carry the
    /// extension whitelist and any dimension groups that should be lifted to
    /// the top of the tree.
    #[must_use]
    pub fn file_tree_with(&self, options: FileTreeOptions) -> Vec<FileTreeNode> {
        let ext_whitelist: BTreeSet<String> = options.extra_extensions.into_iter().collect();
        let mut skip: BTreeSet<String> = BTreeSet::new();
        for group in &options.dimension_groups {
            if let Ok(rel) = group.dir.strip_prefix(&self.project.root_dir) {
                let slash = path_to_slash(rel);
                if !slash.is_empty() {
                    skip.insert(slash);
                }
            }
        }
        let mut tree = files::build_file_tree(
            &self.project.root_dir,
            &options.in_sources,
            &ext_whitelist,
            &skip,
        );
        for group in options.dimension_groups.iter().rev() {
            if let Some(node) = files::build_dimension_subtree(
                &self.project.root_dir,
                group.display_name.clone(),
                &group.dir,
                &options.in_sources,
                &ext_whitelist,
            ) {
                tree.insert(0, node);
            }
        }
        tree
    }

    #[must_use]
    pub const fn loader_extensions(&self) -> &BTreeSet<String> {
        &self.loader_extensions
    }
}

#[derive(Debug, Clone)]
pub struct ProjectSchemaSession {
    pub(crate) project: Project,
    pub(crate) modules: Arc<CftModuleSet>,
    pub(crate) schema: Arc<CftSchema>,
    pub(crate) diagnostics: DiagnosticsStore,
}

impl ProjectSchemaSession {
    #[must_use]
    pub const fn project(&self) -> &Project {
        &self.project
    }

    #[must_use]
    pub fn schema(&self) -> &CftSchema {
        &self.schema
    }

    /// Parsed CFT modules paired with this schema attempt for language hosts.
    #[must_use]
    pub fn modules(&self) -> &CftModuleSet {
        &self.modules
    }

    #[must_use]
    pub const fn diagnostics(&self) -> &DiagnosticsStore {
        &self.diagnostics
    }

    #[must_use]
    pub fn into_diagnostics(self) -> DiagnosticSet {
        self.diagnostics.into_set()
    }

    #[must_use]
    pub fn has_diagnostics(&self) -> bool {
        !self.diagnostics.is_empty()
    }

    /// Generates schema-only code artifacts from this session.
    ///
    /// # Errors
    ///
    /// Returns provider diagnostics when the generator rejects its options or schema.
    pub fn codegen_artifacts(
        &self,
        codegen: &dyn CodeGenerator,
        options: &DecodedOutputOptions,
        data_format: &str,
        id_as_enum_variants: &serde_json::Value,
    ) -> Result<ArtifactSet, DiagnosticSet> {
        codegen.generate(
            CodegenContext {
                schema: self.schema(),
                model: None,
                data_format,
                id_as_enum_variants,
            },
            options,
        )
    }
}

fn display_source_path(source: &str) -> String {
    if source.contains("://") {
        source.to_string()
    } else {
        path_to_slash(Path::new(source))
    }
}
