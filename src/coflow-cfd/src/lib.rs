//! Source-neutral runtime data model for Coflow data.
//!
//! This crate is deliberately below every concrete loader. Excel, JSON, tests,
//! and editor integrations should all translate their input into
//! [`CfdInputRecord`] / [`CfdInputValue`] and then build a [`CfdDataModel`].
//! `check` execution is also outside this crate: a future evaluator should take
//! compiled CFT schema plus [`CfdDataModel`] and must not depend on Excel.

use coflow_cft::{
    CftAnnotation, CftAnnotationValue, CftConstValue, CftContainer, CftSchemaBinOp,
    CftSchemaCheckBlock, CftSchemaCheckExpr, CftSchemaCheckExprKind, CftSchemaCheckStmt,
    CftSchemaCmpOp, CftSchemaDefaultValue, CftSchemaEnum, CftSchemaField, CftSchemaQuantifierKind,
    CftSchemaType, CftSchemaTypePredicate, CftSchemaUnaryOp, Span,
};
use regex::Regex;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub struct CfdDataModel {
    pub tables: BTreeMap<String, CfdTable>,
    pub inheritance_index: BTreeMap<String, CfdPolymorphicIndex>,
    records: Vec<CfdRecord>,
}

impl CfdDataModel {
    #[must_use]
    pub fn builder(schema: &CftContainer) -> CfdModelBuilder<'_> {
        CfdModelBuilder::new(schema)
    }

    /// Executes CFT `check` blocks against this already-built data model.
    ///
    /// # Errors
    ///
    /// Returns runtime check diagnostics for false conditions or evaluation
    /// errors. This method is source-neutral and never inspects Excel/JSON
    /// loader state.
    pub fn run_checks(&self, schema: &CftContainer) -> Result<(), CfdDiagnostics> {
        CheckRunner::new(schema, self).run()
    }

    #[must_use]
    pub fn record(&self, id: CfdRecordId) -> Option<&CfdRecord> {
        self.records.get(id.0)
    }

    pub fn records(&self) -> impl Iterator<Item = (CfdRecordId, &CfdRecord)> {
        self.records
            .iter()
            .enumerate()
            .map(|(index, record)| (CfdRecordId(index), record))
    }

    #[must_use]
    pub fn table(&self, type_name: &str) -> Option<&CfdTable> {
        self.tables.get(type_name)
    }
}

#[derive(Debug)]
pub struct CfdModelBuilder<'a> {
    schema: &'a CftContainer,
    records: Vec<CfdInputRecord>,
}

impl<'a> CfdModelBuilder<'a> {
    #[must_use]
    pub fn new(schema: &'a CftContainer) -> Self {
        Self {
            schema,
            records: Vec::new(),
        }
    }

    pub fn add_record(
        &mut self,
        actual_type: impl Into<String>,
        fields: impl IntoIterator<Item = (impl Into<String>, CfdInputValue)>,
    ) -> &mut Self {
        self.records.push(CfdInputRecord::new(actual_type, fields));
        self
    }

    pub fn add_input_record(&mut self, record: CfdInputRecord) -> &mut Self {
        self.records.push(record);
        self
    }

    /// Builds a validated in-memory data model from source-neutral records.
    ///
    /// # Errors
    ///
    /// Returns data-model diagnostics for schema/type mismatches, missing
    /// fields, duplicate ids, duplicate dict keys, or unresolved references.
    pub fn build(self) -> Result<CfdDataModel, CfdDiagnostics> {
        ModelCompiler::new(self.schema, self.records).build()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CfdTable {
    pub type_name: String,
    pub records: Vec<CfdRecordId>,
    pub primary_index: BTreeMap<CfdIdValue, CfdRecordId>,
    pub secondary_indexes: BTreeMap<String, BTreeMap<CfdIndexKey, Vec<CfdRecordId>>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CfdPolymorphicIndex {
    pub root_type: String,
    pub records: BTreeMap<CfdIdValue, CfdRecordId>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CfdRecord {
    pub actual_type: String,
    pub fields: BTreeMap<String, CfdValue>,
}

impl CfdRecord {
    #[must_use]
    pub fn field(&self, name: &str) -> Option<&CfdValue> {
        self.fields.get(name)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CfdRecordId(usize);

impl CfdRecordId {
    #[must_use]
    pub fn index(self) -> usize {
        self.0
    }
}

impl fmt::Display for CfdRecordId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum CfdValue {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
    Enum(CfdEnumValue),
    Object(Box<CfdRecord>),
    Ref { id: CfdIdValue, target: CfdRecordId },
    Array(Vec<CfdValue>),
    Dict(Vec<(CfdDictKey, CfdValue)>),
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CfdIdValue {
    String(String),
    Int(i64),
}

impl From<&str> for CfdIdValue {
    fn from(value: &str) -> Self {
        Self::String(value.to_string())
    }
}

impl From<String> for CfdIdValue {
    fn from(value: String) -> Self {
        Self::String(value)
    }
}

impl From<i64> for CfdIdValue {
    fn from(value: i64) -> Self {
        Self::Int(value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CfdDictKey {
    String(String),
    Int(i64),
    Enum(CfdEnumValue),
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CfdIndexKey {
    String(String),
    Int(i64),
    Enum(CfdEnumValue),
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CfdEnumValue {
    pub enum_name: String,
    pub variant: String,
    pub value: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CfdInputRecord {
    pub actual_type: String,
    pub fields: BTreeMap<String, CfdInputValue>,
}

impl CfdInputRecord {
    #[must_use]
    pub fn new(
        actual_type: impl Into<String>,
        fields: impl IntoIterator<Item = (impl Into<String>, CfdInputValue)>,
    ) -> Self {
        Self {
            actual_type: actual_type.into(),
            fields: fields
                .into_iter()
                .map(|(name, value)| (name.into(), value))
                .collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum CfdInputValue {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
    EnumVariant {
        enum_name: String,
        variant: String,
    },
    Object {
        actual_type: Option<String>,
        fields: BTreeMap<String, CfdInputValue>,
    },
    Ref(CfdIdValue),
    Array(Vec<CfdInputValue>),
    Dict(Vec<(CfdInputDictKey, CfdInputValue)>),
}

impl CfdInputValue {
    #[must_use]
    pub fn enum_variant(enum_name: impl Into<String>, variant: impl Into<String>) -> Self {
        Self::EnumVariant {
            enum_name: enum_name.into(),
            variant: variant.into(),
        }
    }

    #[must_use]
    pub fn object(
        actual_type: impl Into<String>,
        fields: impl IntoIterator<Item = (impl Into<String>, CfdInputValue)>,
    ) -> Self {
        Self::Object {
            actual_type: Some(actual_type.into()),
            fields: fields
                .into_iter()
                .map(|(name, value)| (name.into(), value))
                .collect(),
        }
    }

    #[must_use]
    pub fn object_with_declared_type(
        fields: impl IntoIterator<Item = (impl Into<String>, CfdInputValue)>,
    ) -> Self {
        Self::Object {
            actual_type: None,
            fields: fields
                .into_iter()
                .map(|(name, value)| (name.into(), value))
                .collect(),
        }
    }

    #[must_use]
    pub fn dict(entries: impl IntoIterator<Item = (CfdInputDictKey, CfdInputValue)>) -> Self {
        Self::Dict(entries.into_iter().collect())
    }
}

impl From<bool> for CfdInputValue {
    fn from(value: bool) -> Self {
        Self::Bool(value)
    }
}

impl From<i64> for CfdInputValue {
    fn from(value: i64) -> Self {
        Self::Int(value)
    }
}

impl From<f64> for CfdInputValue {
    fn from(value: f64) -> Self {
        Self::Float(value)
    }
}

impl From<&str> for CfdInputValue {
    fn from(value: &str) -> Self {
        Self::String(value.to_string())
    }
}

impl From<String> for CfdInputValue {
    fn from(value: String) -> Self {
        Self::String(value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CfdInputDictKey {
    String(String),
    Int(i64),
    EnumVariant { enum_name: String, variant: String },
}

impl CfdInputDictKey {
    #[must_use]
    pub fn enum_variant(enum_name: impl Into<String>, variant: impl Into<String>) -> Self {
        Self::EnumVariant {
            enum_name: enum_name.into(),
            variant: variant.into(),
        }
    }
}

impl From<&str> for CfdInputDictKey {
    fn from(value: &str) -> Self {
        Self::String(value.to_string())
    }
}

impl From<String> for CfdInputDictKey {
    fn from(value: String) -> Self {
        Self::String(value)
    }
}

impl From<i64> for CfdInputDictKey {
    fn from(value: i64) -> Self {
        Self::Int(value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CfdDiagnostics {
    pub diagnostics: Vec<CfdDiagnostic>,
}

impl CfdDiagnostics {
    #[must_use]
    pub fn new(diagnostics: Vec<CfdDiagnostic>) -> Self {
        Self { diagnostics }
    }

    #[must_use]
    pub fn one(diagnostic: CfdDiagnostic) -> Self {
        Self {
            diagnostics: vec![diagnostic],
        }
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.diagnostics.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CfdDiagnostic {
    pub code: CfdErrorCode,
    pub stage: CfdStage,
    pub severity: CfdSeverity,
    pub message: String,
    pub primary: Option<CfdLabel>,
    pub related: Vec<CfdLabel>,
}

impl CfdDiagnostic {
    #[must_use]
    pub fn error(code: CfdErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            stage: code.stage(),
            severity: CfdSeverity::Error,
            message: message.into(),
            primary: None,
            related: Vec::new(),
        }
    }

    #[must_use]
    pub fn with_primary(mut self, record: Option<CfdRecordId>, path: CfdPath) -> Self {
        self.primary = Some(CfdLabel {
            record,
            path,
            message: None,
        });
        self
    }

    #[must_use]
    pub fn with_primary_message(mut self, message: impl Into<String>) -> Self {
        if let Some(primary) = &mut self.primary {
            primary.message = Some(message.into());
        }
        self
    }

    #[must_use]
    pub fn with_related(
        mut self,
        record: Option<CfdRecordId>,
        path: CfdPath,
        message: impl Into<String>,
    ) -> Self {
        self.related.push(CfdLabel {
            record,
            path,
            message: Some(message.into()),
        });
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CfdLabel {
    pub record: Option<CfdRecordId>,
    pub path: CfdPath,
    pub message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CfdPath {
    pub segments: Vec<CfdPathSegment>,
}

impl CfdPath {
    #[must_use]
    pub fn root() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn field(mut self, name: impl Into<String>) -> Self {
        self.segments.push(CfdPathSegment::Field(name.into()));
        self
    }

    #[must_use]
    pub fn index(mut self, index: usize) -> Self {
        self.segments.push(CfdPathSegment::Index(index));
        self
    }

    #[must_use]
    pub fn dict_key(mut self, key: impl Into<String>) -> Self {
        self.segments.push(CfdPathSegment::DictKey(key.into()));
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CfdPathSegment {
    Field(String),
    Index(usize),
    DictKey(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CfdStage {
    DataModel,
    Reference,
    Check,
}

impl fmt::Display for CfdStage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::DataModel => "DATA",
            Self::Reference => "REF",
            Self::Check => "CHECK",
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CfdSeverity {
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CfdErrorCode {
    UnknownType,
    AbstractRecordType,
    MissingObjectType,
    ObjectTypeMismatch,
    UnknownField,
    MissingRequiredField,
    TypeMismatch,
    InvalidEnumVariant,
    DuplicateDictKey,
    MissingIdField,
    DuplicateId,
    DuplicatePolymorphicId,
    RefTargetHasNoId,
    RefTargetNotFound,
    CheckFailed,
    CheckEvalTypeError,
    CheckNullAccess,
    CheckIndexOutOfBounds,
    CheckMissingDictKey,
    CheckEmptyMinMax,
    CheckInvalidRegex,
}

impl CfdErrorCode {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::UnknownType => "CFD-DATA-001",
            Self::AbstractRecordType => "CFD-DATA-002",
            Self::MissingObjectType => "CFD-DATA-003",
            Self::ObjectTypeMismatch => "CFD-DATA-004",
            Self::UnknownField => "CFD-DATA-005",
            Self::MissingRequiredField => "CFD-DATA-006",
            Self::TypeMismatch => "CFD-DATA-007",
            Self::InvalidEnumVariant => "CFD-DATA-008",
            Self::DuplicateDictKey => "CFD-DATA-009",
            Self::MissingIdField => "CFD-DATA-010",
            Self::DuplicateId => "CFD-DATA-011",
            Self::DuplicatePolymorphicId => "CFD-DATA-012",
            Self::RefTargetHasNoId => "CFD-REF-001",
            Self::RefTargetNotFound => "CFD-REF-002",
            Self::CheckFailed => "CFD-CHECK-001",
            Self::CheckEvalTypeError => "CFD-CHECK-002",
            Self::CheckNullAccess => "CFD-CHECK-003",
            Self::CheckIndexOutOfBounds => "CFD-CHECK-004",
            Self::CheckMissingDictKey => "CFD-CHECK-005",
            Self::CheckEmptyMinMax => "CFD-CHECK-006",
            Self::CheckInvalidRegex => "CFD-CHECK-007",
        }
    }

    #[must_use]
    pub fn stage(self) -> CfdStage {
        match self {
            Self::RefTargetHasNoId | Self::RefTargetNotFound => CfdStage::Reference,
            Self::CheckFailed
            | Self::CheckEvalTypeError
            | Self::CheckNullAccess
            | Self::CheckIndexOutOfBounds
            | Self::CheckMissingDictKey
            | Self::CheckEmptyMinMax
            | Self::CheckInvalidRegex => CfdStage::Check,
            _ => CfdStage::DataModel,
        }
    }
}

impl fmt::Display for CfdErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

struct ModelCompiler<'a> {
    schema: SchemaView,
    input: Vec<CfdInputRecord>,
    diagnostics: Vec<CfdDiagnostic>,
    schema_source: &'a CftContainer,
}

impl<'a> ModelCompiler<'a> {
    fn new(schema_source: &'a CftContainer, input: Vec<CfdInputRecord>) -> Self {
        Self {
            schema: SchemaView::new(schema_source),
            input,
            diagnostics: Vec::new(),
            schema_source,
        }
    }

    fn build(mut self) -> Result<CfdDataModel, CfdDiagnostics> {
        let mut drafts = Vec::new();
        let input = std::mem::take(&mut self.input);
        for record in input {
            let id = CfdRecordId(drafts.len());
            if let Some(draft) = self.validate_record(
                None,
                &record.actual_type,
                &record.fields,
                Some(id),
                CfdPath::root(),
            ) {
                drafts.push(draft);
            }
        }

        if !self.diagnostics.is_empty() {
            return Err(CfdDiagnostics::new(self.diagnostics));
        }

        let (tables, inheritance_index) = self.build_indexes(&drafts);
        if !self.diagnostics.is_empty() {
            return Err(CfdDiagnostics::new(self.diagnostics));
        }

        let mut records = Vec::with_capacity(drafts.len());
        for (index, draft) in drafts.iter().enumerate() {
            let record_id = CfdRecordId(index);
            let Some(fields) = self.resolve_fields(
                &draft.fields,
                Some(record_id),
                CfdPath::root(),
                &tables,
                &inheritance_index,
            ) else {
                continue;
            };
            records.push(CfdRecord {
                actual_type: draft.actual_type.clone(),
                fields,
            });
        }

        if !self.diagnostics.is_empty() {
            return Err(CfdDiagnostics::new(self.diagnostics));
        }

        let _ = self.schema_source;
        Ok(CfdDataModel {
            tables,
            inheritance_index,
            records,
        })
    }

    fn validate_record(
        &mut self,
        expected_type: Option<&str>,
        actual_type: &str,
        input_fields: &BTreeMap<String, CfdInputValue>,
        record: Option<CfdRecordId>,
        path: CfdPath,
    ) -> Option<RecordDraft> {
        let diagnostic_start = self.diagnostics.len();
        let Some(actual_meta) = self.schema.types.get(actual_type).cloned() else {
            self.push(
                CfdDiagnostic::error(
                    CfdErrorCode::UnknownType,
                    format!("unknown type `{actual_type}`"),
                )
                .with_primary(record, path),
            );
            return None;
        };
        if actual_meta.is_abstract {
            self.push(
                CfdDiagnostic::error(
                    CfdErrorCode::AbstractRecordType,
                    format!("abstract type `{actual_type}` cannot be instantiated"),
                )
                .with_primary(record, path),
            );
            return None;
        }
        if let Some(expected) = expected_type {
            if !self.schema.is_assignable(actual_type, expected) {
                self.push(
                    CfdDiagnostic::error(
                        CfdErrorCode::ObjectTypeMismatch,
                        format!("type `{actual_type}` is not assignable to `{expected}`"),
                    )
                    .with_primary(record, path),
                );
                return None;
            }
        }

        let fields = self.schema.full_fields(actual_type);
        let known_fields = fields
            .iter()
            .map(|field| field.name.as_str())
            .collect::<BTreeSet<_>>();
        for name in input_fields.keys() {
            if !known_fields.contains(name.as_str()) {
                self.push(
                    CfdDiagnostic::error(
                        CfdErrorCode::UnknownField,
                        format!("unknown field `{name}` on type `{actual_type}`"),
                    )
                    .with_primary(record, path.clone().field(name.clone())),
                );
            }
        }

        let mut out = BTreeMap::new();
        for field in fields {
            let field_path = path.clone().field(field.name.clone());
            let value = if let Some(value) = input_fields.get(&field.name) {
                self.validate_field_value(&field, value, record, field_path)
            } else if let Some(default) = &field.default {
                self.default_field_value(&field, default, record, field_path)
            } else {
                self.push(
                    CfdDiagnostic::error(
                        CfdErrorCode::MissingRequiredField,
                        format!("missing required field `{}`", field.name),
                    )
                    .with_primary(record, field_path),
                );
                None
            };
            if let Some(value) = value {
                out.insert(field.name, value);
            }
        }

        if self.diagnostics.len() == diagnostic_start {
            Some(RecordDraft {
                actual_type: actual_type.to_string(),
                fields: out,
            })
        } else {
            None
        }
    }

    fn validate_field_value(
        &mut self,
        field: &FieldMeta,
        value: &CfdInputValue,
        record: Option<CfdRecordId>,
        path: CfdPath,
    ) -> Option<CfdValueDraft> {
        if let Some(target_type) = &field.ref_target {
            return self.validate_ref_field(field, target_type, value, record, path);
        }
        self.validate_value(&field.ty, value, record, path)
    }

    fn validate_ref_field(
        &mut self,
        field: &FieldMeta,
        target_type: &str,
        value: &CfdInputValue,
        record: Option<CfdRecordId>,
        path: CfdPath,
    ) -> Option<CfdValueDraft> {
        if matches!(value, CfdInputValue::Null) {
            if field.ty.is_nullable() {
                return Some(CfdValueDraft::Value(CfdValue::Null));
            }
            self.type_mismatch("non-null @ref id", value, record, path);
            return None;
        }

        let id = match value {
            CfdInputValue::Ref(id) => id.clone(),
            CfdInputValue::String(value) => CfdIdValue::String(value.clone()),
            CfdInputValue::Int(value) => CfdIdValue::Int(*value),
            _ => {
                self.type_mismatch("@ref id", value, record, path);
                return None;
            }
        };

        if !id_matches_type(&id, &field.ty) {
            self.push(
                CfdDiagnostic::error(
                    CfdErrorCode::TypeMismatch,
                    "@ref id does not match the field id type",
                )
                .with_primary(record, path),
            );
            return None;
        }

        Some(CfdValueDraft::PendingRef {
            target_type: target_type.to_string(),
            id,
        })
    }

    fn validate_value(
        &mut self,
        ty: &CfdType,
        value: &CfdInputValue,
        record: Option<CfdRecordId>,
        path: CfdPath,
    ) -> Option<CfdValueDraft> {
        if let CfdType::Nullable(inner) = ty {
            return if matches!(value, CfdInputValue::Null) {
                Some(CfdValueDraft::Value(CfdValue::Null))
            } else {
                self.validate_value(inner, value, record, path)
            };
        }

        match (ty, value) {
            (CfdType::Int, CfdInputValue::Int(value)) => {
                Some(CfdValueDraft::Value(CfdValue::Int(*value)))
            }
            (CfdType::Float, CfdInputValue::Float(value)) => {
                Some(CfdValueDraft::Value(CfdValue::Float(*value)))
            }
            (CfdType::Bool, CfdInputValue::Bool(value)) => {
                Some(CfdValueDraft::Value(CfdValue::Bool(*value)))
            }
            (CfdType::String, CfdInputValue::String(value)) => {
                Some(CfdValueDraft::Value(CfdValue::String(value.clone())))
            }
            (CfdType::Enum(expected), CfdInputValue::EnumVariant { enum_name, variant }) => {
                if enum_name != expected {
                    self.push(
                        CfdDiagnostic::error(
                            CfdErrorCode::TypeMismatch,
                            format!("expected enum `{expected}`, got `{enum_name}`"),
                        )
                        .with_primary(record, path),
                    );
                    return None;
                }
                let enum_value = self.resolve_enum_value(enum_name, variant, record, path)?;
                Some(CfdValueDraft::Value(CfdValue::Enum(enum_value)))
            }
            (
                CfdType::Type(expected),
                CfdInputValue::Object {
                    actual_type,
                    fields,
                },
            ) => {
                let actual = if let Some(actual) = actual_type {
                    actual.clone()
                } else if self.schema.range_is_polymorphic(expected) {
                    self.push(
                        CfdDiagnostic::error(
                            CfdErrorCode::MissingObjectType,
                            format!("field of polymorphic type `{expected}` needs an actual type"),
                        )
                        .with_primary(record, path),
                    );
                    return None;
                } else {
                    expected.clone()
                };
                let draft = self.validate_record(Some(expected), &actual, fields, record, path)?;
                Some(CfdValueDraft::Object(Box::new(draft)))
            }
            (CfdType::Array(inner), CfdInputValue::Array(items)) => {
                let mut out = Vec::with_capacity(items.len());
                for (index, item) in items.iter().enumerate() {
                    let Some(value) =
                        self.validate_value(inner, item, record, path.clone().index(index))
                    else {
                        continue;
                    };
                    out.push(value);
                }
                Some(CfdValueDraft::Array(out))
            }
            (CfdType::Dict(key_ty, value_ty), CfdInputValue::Dict(entries)) => {
                let mut seen = BTreeMap::<CfdDictKey, CfdPath>::new();
                let mut out = Vec::with_capacity(entries.len());
                for (index, (key, value)) in entries.iter().enumerate() {
                    let key_path = path.clone().dict_key(index.to_string());
                    let Some(key) = self.validate_dict_key(key_ty, key, record, key_path.clone())
                    else {
                        continue;
                    };
                    if let Some(first) = seen.get(&key) {
                        self.push(
                            CfdDiagnostic::error(
                                CfdErrorCode::DuplicateDictKey,
                                "duplicate dict key",
                            )
                            .with_primary(record, key_path)
                            .with_related(
                                record,
                                first.clone(),
                                "first key is here",
                            ),
                        );
                        continue;
                    }
                    seen.insert(key.clone(), key_path);
                    let Some(value) = self.validate_value(
                        value_ty,
                        value,
                        record,
                        path.clone().dict_key(index.to_string()),
                    ) else {
                        continue;
                    };
                    out.push((key, value));
                }
                Some(CfdValueDraft::Dict(out))
            }
            _ => {
                self.type_mismatch(&ty.display(), value, record, path);
                None
            }
        }
    }

    fn validate_dict_key(
        &mut self,
        ty: &CfdType,
        key: &CfdInputDictKey,
        record: Option<CfdRecordId>,
        path: CfdPath,
    ) -> Option<CfdDictKey> {
        match (ty, key) {
            (CfdType::String, CfdInputDictKey::String(value)) => {
                Some(CfdDictKey::String(value.clone()))
            }
            (CfdType::Int, CfdInputDictKey::Int(value)) => Some(CfdDictKey::Int(*value)),
            (CfdType::Enum(expected), CfdInputDictKey::EnumVariant { enum_name, variant }) => {
                if enum_name != expected {
                    self.push(
                        CfdDiagnostic::error(
                            CfdErrorCode::TypeMismatch,
                            format!("expected enum key `{expected}`, got `{enum_name}`"),
                        )
                        .with_primary(record, path),
                    );
                    return None;
                }
                let value = self.resolve_enum_value(enum_name, variant, record, path)?;
                Some(CfdDictKey::Enum(value))
            }
            _ => {
                self.push(
                    CfdDiagnostic::error(CfdErrorCode::TypeMismatch, "dict key type mismatch")
                        .with_primary(record, path),
                );
                None
            }
        }
    }

    fn default_field_value(
        &mut self,
        field: &FieldMeta,
        value: &CftSchemaDefaultValue,
        record: Option<CfdRecordId>,
        path: CfdPath,
    ) -> Option<CfdValueDraft> {
        let Some(target_type) = &field.ref_target else {
            return self.default_value(&field.ty, value, record, path);
        };

        match value {
            CftSchemaDefaultValue::Null if field.ty.is_nullable() => {
                Some(CfdValueDraft::Value(CfdValue::Null))
            }
            CftSchemaDefaultValue::String(value)
                if id_matches_type(&CfdIdValue::String(value.clone()), &field.ty) =>
            {
                Some(CfdValueDraft::PendingRef {
                    target_type: target_type.clone(),
                    id: CfdIdValue::String(value.clone()),
                })
            }
            CftSchemaDefaultValue::Int(value)
                if id_matches_type(&CfdIdValue::Int(*value), &field.ty) =>
            {
                Some(CfdValueDraft::PendingRef {
                    target_type: target_type.clone(),
                    id: CfdIdValue::Int(*value),
                })
            }
            _ => {
                self.push(
                    CfdDiagnostic::error(
                        CfdErrorCode::TypeMismatch,
                        "schema default does not match @ref field type",
                    )
                    .with_primary(record, path),
                );
                None
            }
        }
    }

    fn default_value(
        &mut self,
        ty: &CfdType,
        value: &CftSchemaDefaultValue,
        record: Option<CfdRecordId>,
        path: CfdPath,
    ) -> Option<CfdValueDraft> {
        let out = match value {
            CftSchemaDefaultValue::Null if ty.is_nullable() => CfdValue::Null,
            CftSchemaDefaultValue::Int(value) if type_accepts_default(ty, &CfdType::Int) => {
                CfdValue::Int(*value)
            }
            CftSchemaDefaultValue::Float(value) if type_accepts_default(ty, &CfdType::Float) => {
                CfdValue::Float(*value)
            }
            CftSchemaDefaultValue::Bool(value) if type_accepts_default(ty, &CfdType::Bool) => {
                CfdValue::Bool(*value)
            }
            CftSchemaDefaultValue::String(value) if type_accepts_default(ty, &CfdType::String) => {
                CfdValue::String(value.clone())
            }
            CftSchemaDefaultValue::Enum {
                enum_name,
                variant,
                value,
            } if type_accepts_default(ty, &CfdType::Enum(enum_name.clone())) => {
                CfdValue::Enum(CfdEnumValue {
                    enum_name: enum_name.clone(),
                    variant: variant.clone(),
                    value: *value,
                })
            }
            CftSchemaDefaultValue::EmptyArray if matches!(ty, CfdType::Array(_)) => {
                CfdValue::Array(Vec::new())
            }
            CftSchemaDefaultValue::EmptyObject if matches!(ty, CfdType::Dict(_, _)) => {
                CfdValue::Dict(Vec::new())
            }
            _ => {
                self.push(
                    CfdDiagnostic::error(
                        CfdErrorCode::TypeMismatch,
                        "schema default does not match field type",
                    )
                    .with_primary(record, path),
                );
                return None;
            }
        };
        Some(CfdValueDraft::Value(out))
    }

    fn build_indexes(
        &mut self,
        drafts: &[RecordDraft],
    ) -> (
        BTreeMap<String, CfdTable>,
        BTreeMap<String, CfdPolymorphicIndex>,
    ) {
        let mut tables = BTreeMap::<String, CfdTable>::new();
        let mut inheritance_index = BTreeMap::<String, CfdPolymorphicIndex>::new();

        for (index, draft) in drafts.iter().enumerate() {
            let record_id = CfdRecordId(index);
            let table = tables
                .entry(draft.actual_type.clone())
                .or_insert_with(|| CfdTable {
                    type_name: draft.actual_type.clone(),
                    records: Vec::new(),
                    primary_index: BTreeMap::new(),
                    secondary_indexes: BTreeMap::new(),
                });
            table.records.push(record_id);

            if let Some(id_field) = self.schema.id_field_for_actual(&draft.actual_type) {
                if let Some(id) = id_from_fields(&draft.fields, &id_field.name) {
                    if let Some(first) = table.primary_index.insert(id.clone(), record_id) {
                        self.push(
                            CfdDiagnostic::error(
                                CfdErrorCode::DuplicateId,
                                format!("duplicate id in table `{}`", draft.actual_type),
                            )
                            .with_primary(
                                Some(record_id),
                                CfdPath::root().field(id_field.name.clone()),
                            )
                            .with_related(
                                Some(first),
                                CfdPath::root().field(id_field.name.clone()),
                                "first id is here",
                            ),
                        );
                    }
                    self.add_polymorphic_ids(
                        &mut inheritance_index,
                        &draft.actual_type,
                        id,
                        record_id,
                        &id_field.name,
                    );
                } else {
                    self.push(
                        CfdDiagnostic::error(
                            CfdErrorCode::MissingIdField,
                            format!("record `{}` has no usable @id field", draft.actual_type),
                        )
                        .with_primary(
                            Some(record_id),
                            CfdPath::root().field(id_field.name.clone()),
                        ),
                    );
                }
            }

            for field in self.schema.index_fields_for_actual(&draft.actual_type) {
                let Some(value) = draft.fields.get(&field.name) else {
                    continue;
                };
                let Some(key) = index_key_from_draft(value) else {
                    continue;
                };
                if let Some(table) = tables.get_mut(&draft.actual_type) {
                    table
                        .secondary_indexes
                        .entry(field.name.clone())
                        .or_default()
                        .entry(key)
                        .or_default()
                        .push(record_id);
                }
            }
        }

        (tables, inheritance_index)
    }

    fn add_polymorphic_ids(
        &mut self,
        inheritance_index: &mut BTreeMap<String, CfdPolymorphicIndex>,
        actual_type: &str,
        id: CfdIdValue,
        record_id: CfdRecordId,
        id_field_name: &str,
    ) {
        for target_type in self.schema.assignable_target_names(actual_type) {
            if !self.schema.range_is_polymorphic(&target_type) {
                continue;
            }
            let index = inheritance_index
                .entry(target_type.clone())
                .or_insert_with(|| CfdPolymorphicIndex {
                    root_type: target_type.clone(),
                    records: BTreeMap::new(),
                });
            if let Some(first) = index.records.insert(id.clone(), record_id) {
                self.push(
                    CfdDiagnostic::error(
                        CfdErrorCode::DuplicatePolymorphicId,
                        format!("duplicate id in polymorphic range `{target_type}`"),
                    )
                    .with_primary(
                        Some(record_id),
                        CfdPath::root().field(id_field_name.to_string()),
                    )
                    .with_related(
                        Some(first),
                        CfdPath::root().field(id_field_name.to_string()),
                        "first id is here",
                    ),
                );
            }
        }
    }

    fn resolve_fields(
        &mut self,
        fields: &BTreeMap<String, CfdValueDraft>,
        record: Option<CfdRecordId>,
        path: CfdPath,
        tables: &BTreeMap<String, CfdTable>,
        inheritance_index: &BTreeMap<String, CfdPolymorphicIndex>,
    ) -> Option<BTreeMap<String, CfdValue>> {
        let diagnostic_start = self.diagnostics.len();
        let mut out = BTreeMap::new();
        for (name, value) in fields {
            let value_path = path.clone().field(name.clone());
            let Some(value) =
                self.resolve_value(value, record, value_path, tables, inheritance_index)
            else {
                continue;
            };
            out.insert(name.clone(), value);
        }
        if self.diagnostics.len() == diagnostic_start {
            Some(out)
        } else {
            None
        }
    }

    fn resolve_value(
        &mut self,
        value: &CfdValueDraft,
        record: Option<CfdRecordId>,
        path: CfdPath,
        tables: &BTreeMap<String, CfdTable>,
        inheritance_index: &BTreeMap<String, CfdPolymorphicIndex>,
    ) -> Option<CfdValue> {
        match value {
            CfdValueDraft::Value(value) => Some(value.clone()),
            CfdValueDraft::PendingRef { target_type, id } => {
                let target = self.resolve_ref_target(
                    target_type,
                    id,
                    tables,
                    inheritance_index,
                    record,
                    path,
                )?;
                Some(CfdValue::Ref {
                    id: id.clone(),
                    target,
                })
            }
            CfdValueDraft::Object(record_draft) => {
                let fields = self.resolve_fields(
                    &record_draft.fields,
                    record,
                    path,
                    tables,
                    inheritance_index,
                )?;
                Some(CfdValue::Object(Box::new(CfdRecord {
                    actual_type: record_draft.actual_type.clone(),
                    fields,
                })))
            }
            CfdValueDraft::Array(items) => {
                let mut out = Vec::with_capacity(items.len());
                for (index, item) in items.iter().enumerate() {
                    let Some(value) = self.resolve_value(
                        item,
                        record,
                        path.clone().index(index),
                        tables,
                        inheritance_index,
                    ) else {
                        continue;
                    };
                    out.push(value);
                }
                Some(CfdValue::Array(out))
            }
            CfdValueDraft::Dict(entries) => {
                let mut out = Vec::with_capacity(entries.len());
                for (index, (key, value)) in entries.iter().enumerate() {
                    let Some(value) = self.resolve_value(
                        value,
                        record,
                        path.clone().dict_key(index.to_string()),
                        tables,
                        inheritance_index,
                    ) else {
                        continue;
                    };
                    out.push((key.clone(), value));
                }
                Some(CfdValue::Dict(out))
            }
        }
    }

    fn resolve_ref_target(
        &mut self,
        target_type: &str,
        id: &CfdIdValue,
        tables: &BTreeMap<String, CfdTable>,
        inheritance_index: &BTreeMap<String, CfdPolymorphicIndex>,
        record: Option<CfdRecordId>,
        path: CfdPath,
    ) -> Option<CfdRecordId> {
        if !self.schema.range_has_id(target_type) {
            self.push(
                CfdDiagnostic::error(
                    CfdErrorCode::RefTargetHasNoId,
                    format!("ref target `{target_type}` has no @id field"),
                )
                .with_primary(record, path),
            );
            return None;
        }

        let target = if self.schema.range_is_polymorphic(target_type) {
            inheritance_index
                .get(target_type)
                .and_then(|index| index.records.get(id))
                .copied()
        } else {
            tables
                .get(target_type)
                .and_then(|table| table.primary_index.get(id))
                .copied()
        };

        if target.is_none() {
            self.push(
                CfdDiagnostic::error(
                    CfdErrorCode::RefTargetNotFound,
                    format!("ref target `{target_type}` with id was not found"),
                )
                .with_primary(record, path),
            );
        }
        target
    }

    fn resolve_enum_value(
        &mut self,
        enum_name: &str,
        variant: &str,
        record: Option<CfdRecordId>,
        path: CfdPath,
    ) -> Option<CfdEnumValue> {
        let Some(value) = self.schema.enum_variant_value(enum_name, variant) else {
            self.push(
                CfdDiagnostic::error(
                    CfdErrorCode::InvalidEnumVariant,
                    format!("unknown enum variant `{enum_name}.{variant}`"),
                )
                .with_primary(record, path),
            );
            return None;
        };
        Some(CfdEnumValue {
            enum_name: enum_name.to_string(),
            variant: variant.to_string(),
            value,
        })
    }

    fn type_mismatch(
        &mut self,
        expected: &str,
        value: &CfdInputValue,
        record: Option<CfdRecordId>,
        path: CfdPath,
    ) {
        self.push(
            CfdDiagnostic::error(
                CfdErrorCode::TypeMismatch,
                format!("expected {expected}, got {}", input_value_kind(value)),
            )
            .with_primary(record, path),
        );
    }

    fn push(&mut self, diagnostic: CfdDiagnostic) {
        self.diagnostics.push(diagnostic);
    }
}

struct CheckRunner<'a> {
    schema: SchemaView,
    model: &'a CfdDataModel,
    diagnostics: Vec<CfdDiagnostic>,
}

impl<'a> CheckRunner<'a> {
    fn new(schema: &'a CftContainer, model: &'a CfdDataModel) -> Self {
        Self {
            schema: SchemaView::new(schema),
            model,
            diagnostics: Vec::new(),
        }
    }

    fn run(mut self) -> Result<(), CfdDiagnostics> {
        for (record_id, record) in self.model.records() {
            let checks = self.schema.checks_for_actual(&record.actual_type);
            let root = CheckValue::Record(CheckRecordRef::Top(record_id));
            let mut evaluator = CheckEvaluator::new(
                &self.schema,
                self.model,
                Some(record_id),
                CfdPath::root(),
                root,
            );
            for check in checks {
                evaluator.eval_check_block(&check);
            }
            self.diagnostics.extend(evaluator.diagnostics);
        }

        if self.diagnostics.is_empty() {
            Ok(())
        } else {
            Err(CfdDiagnostics::new(self.diagnostics))
        }
    }
}

struct CheckEvaluator<'a> {
    schema: &'a SchemaView,
    model: &'a CfdDataModel,
    root_record: Option<CfdRecordId>,
    root_path: CfdPath,
    current: CheckValue,
    scopes: Vec<BTreeMap<String, CheckValue>>,
    diagnostics: Vec<CfdDiagnostic>,
}

impl<'a> CheckEvaluator<'a> {
    fn new(
        schema: &'a SchemaView,
        model: &'a CfdDataModel,
        root_record: Option<CfdRecordId>,
        root_path: CfdPath,
        current: CheckValue,
    ) -> Self {
        Self {
            schema,
            model,
            root_record,
            root_path,
            current,
            scopes: Vec::new(),
            diagnostics: Vec::new(),
        }
    }

    fn eval_check_block(&mut self, check: &CftSchemaCheckBlock) {
        self.eval_stmts(&check.stmts);
    }

    fn eval_stmts(&mut self, stmts: &[CftSchemaCheckStmt]) {
        for stmt in stmts {
            self.eval_stmt(stmt);
        }
    }

    fn eval_stmt(&mut self, stmt: &CftSchemaCheckStmt) {
        match stmt {
            CftSchemaCheckStmt::Expr(expr) => match self.eval_expr(expr) {
                Ok(CheckValue::Bool(true)) => {}
                Ok(CheckValue::Bool(false)) => self.diag(
                    CfdErrorCode::CheckFailed,
                    expr.span,
                    "check condition evaluated to false",
                ),
                Ok(_) => self.diag(
                    CfdErrorCode::CheckEvalTypeError,
                    expr.span,
                    "check expression did not evaluate to bool",
                ),
                Err(()) => {}
            },
            CftSchemaCheckStmt::When {
                condition, body, ..
            } => match self.eval_expr(condition) {
                Ok(CheckValue::Bool(true)) => self.eval_stmts(body),
                Ok(CheckValue::Bool(false)) => {}
                Ok(_) => self.diag(
                    CfdErrorCode::CheckEvalTypeError,
                    condition.span,
                    "when condition did not evaluate to bool",
                ),
                Err(()) => {}
            },
            CftSchemaCheckStmt::Quantifier {
                kind,
                binding,
                collection,
                body,
                span,
            } => {
                let Ok(collection) = self.eval_expr(collection) else {
                    return;
                };
                let Some(items) = self.quantifier_items(collection, *span) else {
                    return;
                };
                self.eval_quantifier(*kind, binding, &items, body, *span);
            }
        }
    }

    fn eval_quantifier(
        &mut self,
        kind: CftSchemaQuantifierKind,
        binding: &str,
        items: &[CheckValue],
        body: &[CftSchemaCheckStmt],
        span: Span,
    ) {
        let mut matched = 0_usize;
        for item in items {
            let diagnostic_start = self.diagnostics.len();
            let mut scope = BTreeMap::new();
            scope.insert(binding.to_string(), item.clone());
            self.scopes.push(scope);
            self.eval_stmts(body);
            let passed = self.diagnostics.len() == diagnostic_start;
            let _ = self.scopes.pop();

            if passed {
                matched += 1;
            }
        }

        match kind {
            CftSchemaQuantifierKind::All => {}
            CftSchemaQuantifierKind::Any if matched == 0 => self.diag(
                CfdErrorCode::CheckFailed,
                span,
                "any quantifier did not match any element",
            ),
            CftSchemaQuantifierKind::Any => {}
            CftSchemaQuantifierKind::None if matched > 0 => self.diag(
                CfdErrorCode::CheckFailed,
                span,
                "none quantifier matched at least one element",
            ),
            CftSchemaQuantifierKind::None => {}
        }
    }

    fn quantifier_items(&mut self, collection: CheckValue, span: Span) -> Option<Vec<CheckValue>> {
        match collection {
            CheckValue::Array(items) => Some(items),
            CheckValue::Dict(entries) => Some(
                entries
                    .into_iter()
                    .map(|entry| CheckValue::Entry(Box::new(entry)))
                    .collect(),
            ),
            _ => {
                self.diag(
                    CfdErrorCode::CheckEvalTypeError,
                    span,
                    "quantifier target is not a collection",
                );
                None
            }
        }
    }

    #[allow(clippy::too_many_lines)]
    fn eval_expr(&mut self, expr: &CftSchemaCheckExpr) -> Result<CheckValue, ()> {
        match &expr.kind {
            CftSchemaCheckExprKind::Int(value) => Ok(CheckValue::Int(*value)),
            CftSchemaCheckExprKind::Float(value) => Ok(CheckValue::Float(*value)),
            CftSchemaCheckExprKind::Bool(value) => Ok(CheckValue::Bool(*value)),
            CftSchemaCheckExprKind::Null => Ok(CheckValue::Null),
            CftSchemaCheckExprKind::String(value) => Ok(CheckValue::String(value.clone())),
            CftSchemaCheckExprKind::Name(name) => self.eval_name(name, expr.span),
            CftSchemaCheckExprKind::Field { expr: inner, name } => {
                if let CftSchemaCheckExprKind::Name(enum_name) = &inner.kind {
                    if let Some(enum_value) = self.schema.enum_variant_value(enum_name, name) {
                        return Ok(CheckValue::Enum(CfdEnumValue {
                            enum_name: enum_name.clone(),
                            variant: name.clone(),
                            value: enum_value,
                        }));
                    }
                }
                let target = self.eval_expr(inner)?;
                self.eval_field(target, name, expr.span)
            }
            CftSchemaCheckExprKind::Index { expr: inner, index } => {
                let target = self.eval_expr(inner)?;
                let index = self.eval_expr(index)?;
                self.eval_index(target, index, expr.span)
            }
            CftSchemaCheckExprKind::Is {
                expr: inner,
                predicate,
            } => {
                let value = self.eval_expr(inner)?;
                Ok(CheckValue::Bool(self.eval_is(&value, predicate)))
            }
            CftSchemaCheckExprKind::Call { name, args } => self.eval_call(name, args, expr.span),
            CftSchemaCheckExprKind::BinOp { op, lhs, rhs } => {
                self.eval_bin_op(*op, lhs, rhs, expr.span)
            }
            CftSchemaCheckExprKind::Unary { op, expr: inner } => {
                let value = self.eval_expr(inner)?;
                self.eval_unary(*op, value, expr.span)
            }
            CftSchemaCheckExprKind::CmpChain { first, rest } => {
                let mut lhs = self.eval_expr(first)?;
                for (op, rhs_expr) in rest {
                    let rhs = self.eval_expr(rhs_expr)?;
                    if !self.compare(*op, &lhs, &rhs, rhs_expr.span)? {
                        return Ok(CheckValue::Bool(false));
                    }
                    lhs = rhs;
                }
                Ok(CheckValue::Bool(true))
            }
        }
    }

    fn eval_name(&mut self, name: &str, span: Span) -> Result<CheckValue, ()> {
        for scope in self.scopes.iter().rev() {
            if let Some(value) = scope.get(name) {
                return Ok(value.clone());
            }
        }
        if let Some(value) = self.current.field(self.model, name) {
            return Ok(value);
        }
        if let Some(value) = self.schema.consts.get(name) {
            return Ok(CheckValue::from_const(value));
        }
        if self.schema.enums.contains_key(name) {
            return Ok(CheckValue::EnumNamespace(name.to_string()));
        }
        self.diag(
            CfdErrorCode::CheckEvalTypeError,
            span,
            format!("unknown check value `{name}`"),
        );
        Err(())
    }

    fn eval_field(&mut self, target: CheckValue, name: &str, span: Span) -> Result<CheckValue, ()> {
        if matches!(target, CheckValue::Null) {
            self.diag(
                CfdErrorCode::CheckNullAccess,
                span,
                "field access on null value",
            );
            return Err(());
        }
        match target {
            CheckValue::Record(record) => record.field(self.model, name).ok_or_else(|| {
                self.diag(
                    CfdErrorCode::CheckEvalTypeError,
                    span,
                    format!("record has no field `{name}`"),
                );
            }),
            CheckValue::Entry(entry) => match name {
                "key" => Ok(*entry.key),
                "value" => Ok(entry.value),
                _ => {
                    self.diag(
                        CfdErrorCode::CheckEvalTypeError,
                        span,
                        format!("dict entry has no field `{name}`"),
                    );
                    Err(())
                }
            },
            _ => {
                self.diag(
                    CfdErrorCode::CheckEvalTypeError,
                    span,
                    "field access target is not an object",
                );
                Err(())
            }
        }
    }

    fn eval_index(
        &mut self,
        target: CheckValue,
        index: CheckValue,
        span: Span,
    ) -> Result<CheckValue, ()> {
        if matches!(target, CheckValue::Null) {
            self.diag(
                CfdErrorCode::CheckNullAccess,
                span,
                "index access on null value",
            );
            return Err(());
        }
        match target {
            CheckValue::Array(items) => {
                let CheckValue::Int(index) = index else {
                    self.diag(
                        CfdErrorCode::CheckEvalTypeError,
                        span,
                        "array index is not int",
                    );
                    return Err(());
                };
                let Ok(index) = usize::try_from(index) else {
                    self.diag(
                        CfdErrorCode::CheckIndexOutOfBounds,
                        span,
                        "array index is negative",
                    );
                    return Err(());
                };
                items.get(index).cloned().ok_or_else(|| {
                    self.diag(
                        CfdErrorCode::CheckIndexOutOfBounds,
                        span,
                        "array index is out of bounds",
                    );
                })
            }
            CheckValue::Dict(entries) => {
                let Some(key) = dict_key_from_check_value(&index) else {
                    self.diag(
                        CfdErrorCode::CheckEvalTypeError,
                        span,
                        "dict index is not a valid key",
                    );
                    return Err(());
                };
                entries
                    .into_iter()
                    .find(|entry| entry.key_key().is_some_and(|entry_key| entry_key == key))
                    .map(|entry| entry.value)
                    .ok_or_else(|| {
                        self.diag(
                            CfdErrorCode::CheckMissingDictKey,
                            span,
                            "dict key is missing",
                        );
                    })
            }
            _ => {
                self.diag(
                    CfdErrorCode::CheckEvalTypeError,
                    span,
                    "index target is not a collection",
                );
                Err(())
            }
        }
    }

    fn eval_is(&self, value: &CheckValue, predicate: &CftSchemaTypePredicate) -> bool {
        match predicate {
            CftSchemaTypePredicate::Null => matches!(value, CheckValue::Null),
            CftSchemaTypePredicate::Type(type_name) => value
                .actual_type(self.model)
                .is_some_and(|actual| self.schema.is_assignable(actual, type_name)),
        }
    }

    fn eval_call(
        &mut self,
        name: &str,
        args: &[CftSchemaCheckExpr],
        span: Span,
    ) -> Result<CheckValue, ()> {
        if self.schema.enums.contains_key(name) {
            let Some(arg) = args.first() else {
                self.diag(
                    CfdErrorCode::CheckEvalTypeError,
                    span,
                    "missing enum constructor arg",
                );
                return Err(());
            };
            let CheckValue::Int(value) = self.eval_expr(arg)? else {
                self.diag(
                    CfdErrorCode::CheckEvalTypeError,
                    arg.span,
                    "enum constructor arg is not int",
                );
                return Err(());
            };
            return Ok(CheckValue::Enum(
                self.schema
                    .enum_value_from_int(name, value)
                    .unwrap_or(CfdEnumValue {
                        enum_name: name.to_string(),
                        variant: value.to_string(),
                        value,
                    }),
            ));
        }

        match name {
            "len" => {
                let Some(arg) = args.first() else {
                    self.diag(
                        CfdErrorCode::CheckEvalTypeError,
                        span,
                        "len expects one argument",
                    );
                    return Err(());
                };
                match self.eval_expr(arg)? {
                    CheckValue::Array(items) => Ok(CheckValue::Int(items.len() as i64)),
                    CheckValue::Dict(entries) => Ok(CheckValue::Int(entries.len() as i64)),
                    _ => {
                        self.diag(
                            CfdErrorCode::CheckEvalTypeError,
                            arg.span,
                            "len expects array or dict",
                        );
                        Err(())
                    }
                }
            }
            "contains" => {
                let [collection, value] = args else {
                    self.diag(
                        CfdErrorCode::CheckEvalTypeError,
                        span,
                        "contains expects two arguments",
                    );
                    return Err(());
                };
                let collection = self.eval_expr(collection)?;
                let value = self.eval_expr(value)?;
                Ok(CheckValue::Bool(self.contains_value(&collection, &value)))
            }
            "unique" => {
                let Some(arg) = args.first() else {
                    self.diag(
                        CfdErrorCode::CheckEvalTypeError,
                        span,
                        "unique expects one argument",
                    );
                    return Err(());
                };
                let CheckValue::Array(items) = self.eval_expr(arg)? else {
                    self.diag(
                        CfdErrorCode::CheckEvalTypeError,
                        arg.span,
                        "unique expects array",
                    );
                    return Err(());
                };
                let mut seen = BTreeSet::new();
                for item in items {
                    let Some(key) = comparable_key(&item) else {
                        self.diag(
                            CfdErrorCode::CheckEvalTypeError,
                            arg.span,
                            "unique element is not comparable",
                        );
                        return Err(());
                    };
                    if !seen.insert(key) {
                        return Ok(CheckValue::Bool(false));
                    }
                }
                Ok(CheckValue::Bool(true))
            }
            "min" | "max" => self.eval_min_max(name, args, span),
            "sum" => self.eval_sum(args, span),
            "keys" => {
                let Some(arg) = args.first() else {
                    self.diag(
                        CfdErrorCode::CheckEvalTypeError,
                        span,
                        "keys expects one argument",
                    );
                    return Err(());
                };
                let CheckValue::Dict(entries) = self.eval_expr(arg)? else {
                    self.diag(
                        CfdErrorCode::CheckEvalTypeError,
                        arg.span,
                        "keys expects dict",
                    );
                    return Err(());
                };
                Ok(CheckValue::Array(
                    entries.into_iter().map(|entry| *entry.key).collect(),
                ))
            }
            "values" => {
                let Some(arg) = args.first() else {
                    self.diag(
                        CfdErrorCode::CheckEvalTypeError,
                        span,
                        "values expects one argument",
                    );
                    return Err(());
                };
                let CheckValue::Dict(entries) = self.eval_expr(arg)? else {
                    self.diag(
                        CfdErrorCode::CheckEvalTypeError,
                        arg.span,
                        "values expects dict",
                    );
                    return Err(());
                };
                Ok(CheckValue::Array(
                    entries.into_iter().map(|entry| entry.value).collect(),
                ))
            }
            "matches" => {
                let [value, pattern_expr] = args else {
                    self.diag(
                        CfdErrorCode::CheckEvalTypeError,
                        span,
                        "matches expects two arguments",
                    );
                    return Err(());
                };
                let CheckValue::String(value) = self.eval_expr(value)? else {
                    self.diag(
                        CfdErrorCode::CheckEvalTypeError,
                        value.span,
                        "matches value is not string",
                    );
                    return Err(());
                };
                let CftSchemaCheckExprKind::String(pattern) = &pattern_expr.kind else {
                    self.diag(
                        CfdErrorCode::CheckEvalTypeError,
                        pattern_expr.span,
                        "matches pattern is not literal",
                    );
                    return Err(());
                };
                let regex = Regex::new(pattern).map_err(|_| {
                    self.diag(
                        CfdErrorCode::CheckInvalidRegex,
                        pattern_expr.span,
                        "regex pattern cannot be compiled",
                    );
                })?;
                Ok(CheckValue::Bool(regex.is_match(&value)))
            }
            _ => {
                self.diag(
                    CfdErrorCode::CheckEvalTypeError,
                    span,
                    format!("unknown function `{name}`"),
                );
                Err(())
            }
        }
    }

    fn eval_min_max(
        &mut self,
        name: &str,
        args: &[CftSchemaCheckExpr],
        span: Span,
    ) -> Result<CheckValue, ()> {
        let Some(arg) = args.first() else {
            self.diag(
                CfdErrorCode::CheckEvalTypeError,
                span,
                "min/max expects one argument",
            );
            return Err(());
        };
        let CheckValue::Array(items) = self.eval_expr(arg)? else {
            self.diag(
                CfdErrorCode::CheckEvalTypeError,
                arg.span,
                "min/max expects array",
            );
            return Err(());
        };
        let Some(mut out) = items.first().cloned() else {
            self.diag(
                CfdErrorCode::CheckEmptyMinMax,
                span,
                "min/max called on empty array",
            );
            return Err(());
        };
        for item in items.iter().skip(1) {
            let ord = self.compare_order(&out, item, span)?;
            if (name == "min" && ord.is_gt()) || (name == "max" && ord.is_lt()) {
                out = item.clone();
            }
        }
        Ok(out)
    }

    fn eval_sum(&mut self, args: &[CftSchemaCheckExpr], span: Span) -> Result<CheckValue, ()> {
        let Some(arg) = args.first() else {
            self.diag(
                CfdErrorCode::CheckEvalTypeError,
                span,
                "sum expects one argument",
            );
            return Err(());
        };
        let CheckValue::Array(items) = self.eval_expr(arg)? else {
            self.diag(
                CfdErrorCode::CheckEvalTypeError,
                arg.span,
                "sum expects array",
            );
            return Err(());
        };
        let mut int_sum = 0_i64;
        let mut float_sum = 0.0_f64;
        let mut saw_float = false;
        for item in items {
            match item {
                CheckValue::Int(value) if !saw_float => int_sum = int_sum.saturating_add(value),
                CheckValue::Int(value) => float_sum += value as f64,
                CheckValue::Float(value) => {
                    if !saw_float {
                        saw_float = true;
                        float_sum = int_sum as f64;
                    }
                    float_sum += value;
                }
                _ => {
                    self.diag(
                        CfdErrorCode::CheckEvalTypeError,
                        arg.span,
                        "sum item is not numeric",
                    );
                    return Err(());
                }
            }
        }
        if saw_float {
            Ok(CheckValue::Float(float_sum))
        } else {
            Ok(CheckValue::Int(int_sum))
        }
    }

    fn contains_value(&mut self, collection: &CheckValue, value: &CheckValue) -> bool {
        match collection {
            CheckValue::Array(items) => items.iter().any(|item| values_equal(item, value)),
            CheckValue::Dict(entries) => dict_key_from_check_value(value).is_some_and(|key| {
                entries
                    .iter()
                    .any(|entry| entry.key_key() == Some(key.clone()))
            }),
            _ => false,
        }
    }

    fn eval_unary(
        &mut self,
        op: CftSchemaUnaryOp,
        value: CheckValue,
        span: Span,
    ) -> Result<CheckValue, ()> {
        match (op, value) {
            (CftSchemaUnaryOp::Not, CheckValue::Bool(value)) => Ok(CheckValue::Bool(!value)),
            (CftSchemaUnaryOp::Neg, CheckValue::Int(value)) => Ok(CheckValue::Int(-value)),
            (CftSchemaUnaryOp::Neg, CheckValue::Float(value)) => Ok(CheckValue::Float(-value)),
            (CftSchemaUnaryOp::BitNot, CheckValue::Int(value)) => Ok(CheckValue::Int(!value)),
            (CftSchemaUnaryOp::BitNot, CheckValue::Enum(value)) => Ok(CheckValue::Enum(
                self.enum_with_value(&value.enum_name, !value.value),
            )),
            _ => {
                self.diag(
                    CfdErrorCode::CheckEvalTypeError,
                    span,
                    "unsupported unary operation",
                );
                Err(())
            }
        }
    }

    fn eval_bin_op(
        &mut self,
        op: CftSchemaBinOp,
        lhs: &CftSchemaCheckExpr,
        rhs: &CftSchemaCheckExpr,
        span: Span,
    ) -> Result<CheckValue, ()> {
        match op {
            CftSchemaBinOp::Or => {
                let lhs = self.eval_expr(lhs)?;
                let CheckValue::Bool(lhs) = lhs else {
                    self.diag(CfdErrorCode::CheckEvalTypeError, span, "lhs is not bool");
                    return Err(());
                };
                if lhs {
                    return Ok(CheckValue::Bool(true));
                }
                let rhs = self.eval_expr(rhs)?;
                let CheckValue::Bool(rhs) = rhs else {
                    self.diag(CfdErrorCode::CheckEvalTypeError, span, "rhs is not bool");
                    return Err(());
                };
                Ok(CheckValue::Bool(rhs))
            }
            CftSchemaBinOp::And => {
                let lhs = self.eval_expr(lhs)?;
                let CheckValue::Bool(lhs) = lhs else {
                    self.diag(CfdErrorCode::CheckEvalTypeError, span, "lhs is not bool");
                    return Err(());
                };
                if !lhs {
                    return Ok(CheckValue::Bool(false));
                }
                let rhs = self.eval_expr(rhs)?;
                let CheckValue::Bool(rhs) = rhs else {
                    self.diag(CfdErrorCode::CheckEvalTypeError, span, "rhs is not bool");
                    return Err(());
                };
                Ok(CheckValue::Bool(rhs))
            }
            _ => {
                let lhs = self.eval_expr(lhs)?;
                let rhs = self.eval_expr(rhs)?;
                self.eval_eager_bin_op(op, lhs, rhs, span)
            }
        }
    }

    fn eval_eager_bin_op(
        &mut self,
        op: CftSchemaBinOp,
        lhs: CheckValue,
        rhs: CheckValue,
        span: Span,
    ) -> Result<CheckValue, ()> {
        match (op, lhs, rhs) {
            (CftSchemaBinOp::Add, CheckValue::Int(lhs), CheckValue::Int(rhs)) => {
                Ok(CheckValue::Int(lhs + rhs))
            }
            (CftSchemaBinOp::Sub, CheckValue::Int(lhs), CheckValue::Int(rhs)) => {
                Ok(CheckValue::Int(lhs - rhs))
            }
            (CftSchemaBinOp::Mul, CheckValue::Int(lhs), CheckValue::Int(rhs)) => {
                Ok(CheckValue::Int(lhs * rhs))
            }
            (CftSchemaBinOp::Div, CheckValue::Int(lhs), CheckValue::Int(rhs)) => {
                Ok(CheckValue::Int(lhs / rhs))
            }
            (CftSchemaBinOp::IntDiv, CheckValue::Int(lhs), CheckValue::Int(rhs)) => {
                Ok(CheckValue::Int(lhs / rhs))
            }
            (CftSchemaBinOp::Mod, CheckValue::Int(lhs), CheckValue::Int(rhs)) => {
                Ok(CheckValue::Int(lhs % rhs))
            }
            (CftSchemaBinOp::Pow, CheckValue::Int(lhs), CheckValue::Int(rhs)) => {
                Ok(CheckValue::Int(lhs.pow(rhs as u32)))
            }
            (CftSchemaBinOp::Shl, CheckValue::Int(lhs), CheckValue::Int(rhs)) => {
                Ok(CheckValue::Int(lhs << rhs))
            }
            (CftSchemaBinOp::Shr, CheckValue::Int(lhs), CheckValue::Int(rhs)) => {
                Ok(CheckValue::Int(lhs >> rhs))
            }
            (CftSchemaBinOp::Add, CheckValue::Float(lhs), CheckValue::Float(rhs)) => {
                Ok(CheckValue::Float(lhs + rhs))
            }
            (CftSchemaBinOp::Sub, CheckValue::Float(lhs), CheckValue::Float(rhs)) => {
                Ok(CheckValue::Float(lhs - rhs))
            }
            (CftSchemaBinOp::Mul, CheckValue::Float(lhs), CheckValue::Float(rhs)) => {
                Ok(CheckValue::Float(lhs * rhs))
            }
            (CftSchemaBinOp::Div, CheckValue::Float(lhs), CheckValue::Float(rhs)) => {
                Ok(CheckValue::Float(lhs / rhs))
            }
            (CftSchemaBinOp::Pow, CheckValue::Float(lhs), CheckValue::Float(rhs)) => {
                Ok(CheckValue::Float(lhs.powf(rhs)))
            }
            (CftSchemaBinOp::BitOr, CheckValue::Int(lhs), CheckValue::Int(rhs)) => {
                Ok(CheckValue::Int(lhs | rhs))
            }
            (CftSchemaBinOp::BitXor, CheckValue::Int(lhs), CheckValue::Int(rhs)) => {
                Ok(CheckValue::Int(lhs ^ rhs))
            }
            (CftSchemaBinOp::BitAnd, CheckValue::Int(lhs), CheckValue::Int(rhs)) => {
                Ok(CheckValue::Int(lhs & rhs))
            }
            (
                op @ (CftSchemaBinOp::BitOr | CftSchemaBinOp::BitXor | CftSchemaBinOp::BitAnd),
                CheckValue::Enum(lhs),
                CheckValue::Enum(rhs),
            ) if lhs.enum_name == rhs.enum_name => {
                let value = match op {
                    CftSchemaBinOp::BitOr => lhs.value | rhs.value,
                    CftSchemaBinOp::BitXor => lhs.value ^ rhs.value,
                    CftSchemaBinOp::BitAnd => lhs.value & rhs.value,
                    _ => unreachable!(),
                };
                Ok(CheckValue::Enum(
                    self.enum_with_value(&lhs.enum_name, value),
                ))
            }
            _ => {
                self.diag(
                    CfdErrorCode::CheckEvalTypeError,
                    span,
                    "unsupported binary operation",
                );
                Err(())
            }
        }
    }

    fn compare(
        &mut self,
        op: CftSchemaCmpOp,
        lhs: &CheckValue,
        rhs: &CheckValue,
        span: Span,
    ) -> Result<bool, ()> {
        Ok(match op {
            CftSchemaCmpOp::Eq => values_equal(lhs, rhs),
            CftSchemaCmpOp::Ne => !values_equal(lhs, rhs),
            CftSchemaCmpOp::Lt => self.compare_order(lhs, rhs, span)?.is_lt(),
            CftSchemaCmpOp::Le => !self.compare_order(lhs, rhs, span)?.is_gt(),
            CftSchemaCmpOp::Gt => self.compare_order(lhs, rhs, span)?.is_gt(),
            CftSchemaCmpOp::Ge => !self.compare_order(lhs, rhs, span)?.is_lt(),
        })
    }

    fn compare_order(
        &mut self,
        lhs: &CheckValue,
        rhs: &CheckValue,
        span: Span,
    ) -> Result<std::cmp::Ordering, ()> {
        match (lhs, rhs) {
            (CheckValue::Int(lhs), CheckValue::Int(rhs)) => Ok(lhs.cmp(rhs)),
            (CheckValue::Float(lhs), CheckValue::Float(rhs)) => {
                lhs.partial_cmp(rhs).ok_or_else(|| {
                    self.diag(
                        CfdErrorCode::CheckEvalTypeError,
                        span,
                        "float comparison failed",
                    );
                })
            }
            (CheckValue::Enum(lhs), CheckValue::Enum(rhs)) if lhs.enum_name == rhs.enum_name => {
                Ok(lhs.value.cmp(&rhs.value))
            }
            _ => {
                self.diag(
                    CfdErrorCode::CheckEvalTypeError,
                    span,
                    "values are not ordered comparable",
                );
                Err(())
            }
        }
    }

    fn enum_with_value(&self, enum_name: &str, value: i64) -> CfdEnumValue {
        self.schema
            .enum_value_from_int(enum_name, value)
            .unwrap_or(CfdEnumValue {
                enum_name: enum_name.to_string(),
                variant: value.to_string(),
                value,
            })
    }

    fn diag(&mut self, code: CfdErrorCode, _span: Span, message: impl Into<String>) {
        self.diagnostics.push(
            CfdDiagnostic::error(code, message)
                .with_primary(self.root_record, self.root_path.clone()),
        );
    }
}

#[derive(Debug, Clone, PartialEq)]
enum CheckValue {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
    Enum(CfdEnumValue),
    EnumNamespace(String),
    Record(CheckRecordRef),
    Entry(Box<CheckEntry>),
    Array(Vec<CheckValue>),
    Dict(Vec<CheckEntry>),
}

impl CheckValue {
    fn from_const(value: &CftConstValue) -> Self {
        match value {
            CftConstValue::Int(value) => Self::Int(*value),
            CftConstValue::Float(value) => Self::Float(*value),
            CftConstValue::Bool(value) => Self::Bool(*value),
            CftConstValue::String(value) => Self::String(value.clone()),
        }
    }

    fn field(&self, model: &CfdDataModel, name: &str) -> Option<CheckValue> {
        let Self::Record(record) = self else {
            return None;
        };
        record
            .fields(model)?
            .get(name)
            .map(CheckValue::from_cfd_value)
    }

    fn from_cfd_value(value: &CfdValue) -> Self {
        match value {
            CfdValue::Null => Self::Null,
            CfdValue::Bool(value) => Self::Bool(*value),
            CfdValue::Int(value) => Self::Int(*value),
            CfdValue::Float(value) => Self::Float(*value),
            CfdValue::String(value) => Self::String(value.clone()),
            CfdValue::Enum(value) => Self::Enum(value.clone()),
            CfdValue::Object(record) => {
                Self::Record(CheckRecordRef::Inline(record.as_ref().clone()))
            }
            CfdValue::Ref { target, .. } => Self::Record(CheckRecordRef::Top(*target)),
            CfdValue::Array(items) => Self::Array(items.iter().map(Self::from_cfd_value).collect()),
            CfdValue::Dict(entries) => Self::Dict(
                entries
                    .iter()
                    .map(|(key, value)| CheckEntry {
                        key: Box::new(Self::from_dict_key(key)),
                        value: Self::from_cfd_value(value),
                    })
                    .collect(),
            ),
        }
    }

    fn from_dict_key(key: &CfdDictKey) -> Self {
        match key {
            CfdDictKey::String(value) => Self::String(value.clone()),
            CfdDictKey::Int(value) => Self::Int(*value),
            CfdDictKey::Enum(value) => Self::Enum(value.clone()),
        }
    }

    fn actual_type<'a>(&'a self, model: &'a CfdDataModel) -> Option<&'a str> {
        match self {
            Self::Record(record) => record.actual_type(model),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
enum CheckRecordRef {
    Top(CfdRecordId),
    Inline(CfdRecord),
}

impl CheckRecordRef {
    fn fields<'a>(&'a self, model: &'a CfdDataModel) -> Option<&'a BTreeMap<String, CfdValue>> {
        match self {
            Self::Top(id) => model.record(*id).map(|record| &record.fields),
            Self::Inline(record) => Some(&record.fields),
        }
    }

    fn actual_type<'a>(&'a self, model: &'a CfdDataModel) -> Option<&'a str> {
        match self {
            Self::Top(id) => model.record(*id).map(|record| record.actual_type.as_str()),
            Self::Inline(record) => Some(&record.actual_type),
        }
    }

    fn field(&self, model: &CfdDataModel, name: &str) -> Option<CheckValue> {
        self.fields(model)?
            .get(name)
            .map(CheckValue::from_cfd_value)
    }
}

#[derive(Debug, Clone, PartialEq)]
struct CheckEntry {
    key: Box<CheckValue>,
    value: CheckValue,
}

impl CheckEntry {
    fn key_key(&self) -> Option<ComparableKey> {
        comparable_key(&self.key)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum ComparableKey {
    Null,
    Bool(bool),
    Int(i64),
    String(String),
    Enum(CfdEnumValue),
}

fn values_equal(lhs: &CheckValue, rhs: &CheckValue) -> bool {
    match (lhs, rhs) {
        (CheckValue::Null, CheckValue::Null) => true,
        (CheckValue::Bool(lhs), CheckValue::Bool(rhs)) => lhs == rhs,
        (CheckValue::Int(lhs), CheckValue::Int(rhs)) => lhs == rhs,
        (CheckValue::Float(lhs), CheckValue::Float(rhs)) => lhs == rhs,
        (CheckValue::String(lhs), CheckValue::String(rhs)) => lhs == rhs,
        (CheckValue::Enum(lhs), CheckValue::Enum(rhs)) => {
            lhs.enum_name == rhs.enum_name && lhs.value == rhs.value
        }
        (CheckValue::Record(lhs), CheckValue::Record(rhs)) => lhs == rhs,
        _ => false,
    }
}

fn comparable_key(value: &CheckValue) -> Option<ComparableKey> {
    match value {
        CheckValue::Null => Some(ComparableKey::Null),
        CheckValue::Bool(value) => Some(ComparableKey::Bool(*value)),
        CheckValue::Int(value) => Some(ComparableKey::Int(*value)),
        CheckValue::String(value) => Some(ComparableKey::String(value.clone())),
        CheckValue::Enum(value) => Some(ComparableKey::Enum(value.clone())),
        _ => None,
    }
}

fn dict_key_from_check_value(value: &CheckValue) -> Option<ComparableKey> {
    match value {
        CheckValue::Int(_) | CheckValue::String(_) | CheckValue::Enum(_) => comparable_key(value),
        _ => None,
    }
}

#[derive(Debug, Clone, PartialEq)]
struct RecordDraft {
    actual_type: String,
    fields: BTreeMap<String, CfdValueDraft>,
}

#[derive(Debug, Clone, PartialEq)]
enum CfdValueDraft {
    Value(CfdValue),
    Object(Box<RecordDraft>),
    PendingRef { target_type: String, id: CfdIdValue },
    Array(Vec<CfdValueDraft>),
    Dict(Vec<(CfdDictKey, CfdValueDraft)>),
}

#[derive(Debug, Clone)]
struct SchemaView {
    consts: BTreeMap<String, CftConstValue>,
    types: BTreeMap<String, TypeMeta>,
    enums: BTreeMap<String, EnumMeta>,
    children: BTreeMap<String, BTreeSet<String>>,
}

impl SchemaView {
    fn new(schema: &CftContainer) -> Self {
        let consts = schema
            .module_ids()
            .filter_map(|id| schema.schema(id))
            .flat_map(|module| module.consts.iter())
            .map(|schema_const| (schema_const.name.clone(), schema_const.value.clone()))
            .collect::<BTreeMap<_, _>>();

        let enums = schema
            .all_enums()
            .map(|schema_enum| (schema_enum.name.clone(), EnumMeta::from_schema(schema_enum)))
            .collect::<BTreeMap<_, _>>();

        let mut types = BTreeMap::new();
        let mut children = BTreeMap::<String, BTreeSet<String>>::new();
        for schema_type in schema.all_types() {
            let meta = TypeMeta::from_schema(schema, schema_type);
            if let Some(parent) = &meta.parent {
                children
                    .entry(parent.clone())
                    .or_default()
                    .insert(meta.name.clone());
            }
            types.insert(meta.name.clone(), meta);
        }

        Self {
            consts,
            types,
            enums,
            children,
        }
    }

    fn full_fields(&self, type_name: &str) -> Vec<FieldMeta> {
        let mut out = Vec::new();
        self.fill_fields(type_name, &mut out, &mut BTreeSet::new());
        out
    }

    fn fill_fields(&self, type_name: &str, out: &mut Vec<FieldMeta>, seen: &mut BTreeSet<String>) {
        if !seen.insert(type_name.to_string()) {
            return;
        }
        let Some(meta) = self.types.get(type_name) else {
            return;
        };
        if let Some(parent) = &meta.parent {
            self.fill_fields(parent, out, seen);
        }
        out.extend(meta.fields.clone());
    }

    fn is_assignable(&self, actual_type: &str, expected_type: &str) -> bool {
        let mut current = Some(actual_type);
        while let Some(name) = current {
            if name == expected_type {
                return true;
            }
            current = self.types.get(name).and_then(|meta| meta.parent.as_deref());
        }
        false
    }

    fn range_is_polymorphic(&self, type_name: &str) -> bool {
        self.types
            .get(type_name)
            .is_some_and(|meta| meta.is_abstract || self.has_descendants(type_name))
    }

    fn has_descendants(&self, type_name: &str) -> bool {
        self.children
            .get(type_name)
            .is_some_and(|children| !children.is_empty())
    }

    fn assignable_target_names(&self, actual_type: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut current = Some(actual_type);
        while let Some(name) = current {
            out.push(name.to_string());
            current = self.types.get(name).and_then(|meta| meta.parent.as_deref());
        }
        out
    }

    fn id_field_for_actual(&self, actual_type: &str) -> Option<FieldMeta> {
        self.full_fields(actual_type)
            .into_iter()
            .find(|field| field.is_id)
    }

    fn index_fields_for_actual(&self, actual_type: &str) -> Vec<FieldMeta> {
        self.full_fields(actual_type)
            .into_iter()
            .filter(|field| field.is_index)
            .collect()
    }

    fn range_has_id(&self, target_type: &str) -> bool {
        if self.id_field_for_actual(target_type).is_some() {
            return true;
        }
        self.descendants(target_type)
            .iter()
            .any(|descendant| self.id_field_for_actual(descendant).is_some())
    }

    fn descendants(&self, type_name: &str) -> Vec<String> {
        let mut out = Vec::new();
        self.fill_descendants(type_name, &mut out);
        out
    }

    fn fill_descendants(&self, type_name: &str, out: &mut Vec<String>) {
        let Some(children) = self.children.get(type_name) else {
            return;
        };
        for child in children {
            out.push(child.clone());
            self.fill_descendants(child, out);
        }
    }

    fn enum_variant_value(&self, enum_name: &str, variant: &str) -> Option<i64> {
        self.enums
            .get(enum_name)
            .and_then(|meta| meta.variants.get(variant))
            .copied()
    }

    fn enum_value_from_int(&self, enum_name: &str, value: i64) -> Option<CfdEnumValue> {
        let meta = self.enums.get(enum_name)?;
        meta.variants
            .iter()
            .find(|(_, variant_value)| **variant_value == value)
            .map(|(variant, variant_value)| CfdEnumValue {
                enum_name: enum_name.to_string(),
                variant: variant.clone(),
                value: *variant_value,
            })
    }

    fn checks_for_actual(&self, actual_type: &str) -> Vec<CftSchemaCheckBlock> {
        let mut chain = Vec::new();
        let mut current = Some(actual_type);
        while let Some(name) = current {
            let Some(meta) = self.types.get(name) else {
                break;
            };
            chain.push(meta);
            current = meta.parent.as_deref();
        }
        chain.reverse();
        chain
            .into_iter()
            .filter_map(|meta| meta.check.clone())
            .collect()
    }
}

#[derive(Debug, Clone)]
struct TypeMeta {
    name: String,
    parent: Option<String>,
    is_abstract: bool,
    fields: Vec<FieldMeta>,
    check: Option<CftSchemaCheckBlock>,
}

impl TypeMeta {
    fn from_schema(schema: &CftContainer, schema_type: &CftSchemaType) -> Self {
        Self {
            name: schema_type.name.clone(),
            parent: schema_type.parent.clone(),
            is_abstract: schema_type.is_abstract,
            fields: schema_type
                .fields
                .iter()
                .map(|field| FieldMeta::from_schema(schema, field))
                .collect(),
            check: schema_type.check.clone(),
        }
    }
}

#[derive(Debug, Clone)]
struct FieldMeta {
    name: String,
    ty: CfdType,
    default: Option<CftSchemaDefaultValue>,
    ref_target: Option<String>,
    is_id: bool,
    is_index: bool,
}

impl FieldMeta {
    fn from_schema(schema: &CftContainer, field: &CftSchemaField) -> Self {
        Self {
            name: field.name.clone(),
            ty: CfdType::parse(&field.ty, schema),
            default: field.default.clone(),
            ref_target: annotation_name_arg(&field.annotations, "ref"),
            is_id: has_annotation(&field.annotations, "id"),
            is_index: has_annotation(&field.annotations, "index"),
        }
    }
}

#[derive(Debug, Clone)]
struct EnumMeta {
    variants: BTreeMap<String, i64>,
}

impl EnumMeta {
    fn from_schema(schema_enum: &CftSchemaEnum) -> Self {
        Self {
            variants: schema_enum
                .variants
                .iter()
                .map(|variant| (variant.name.clone(), variant.value))
                .collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum CfdType {
    Int,
    Float,
    Bool,
    String,
    Type(String),
    Enum(String),
    Array(Box<CfdType>),
    Dict(Box<CfdType>, Box<CfdType>),
    Nullable(Box<CfdType>),
}

impl CfdType {
    fn parse(text: &str, schema: &CftContainer) -> Self {
        let mut parser = TypeParser::new(text, schema);
        parser.parse_type()
    }

    fn is_nullable(&self) -> bool {
        matches!(self, Self::Nullable(_))
    }

    fn display(&self) -> String {
        match self {
            Self::Int => "int".to_string(),
            Self::Float => "float".to_string(),
            Self::Bool => "bool".to_string(),
            Self::String => "string".to_string(),
            Self::Type(name) | Self::Enum(name) => name.clone(),
            Self::Array(inner) => format!("[{}]", inner.display()),
            Self::Dict(key, value) => format!("{{{}: {}}}", key.display(), value.display()),
            Self::Nullable(inner) => format!("{}?", inner.display()),
        }
    }
}

struct TypeParser<'a> {
    text: &'a str,
    pos: usize,
    schema: &'a CftContainer,
}

impl<'a> TypeParser<'a> {
    fn new(text: &'a str, schema: &'a CftContainer) -> Self {
        Self {
            text,
            pos: 0,
            schema,
        }
    }

    fn parse_type(&mut self) -> CfdType {
        self.skip_ws();
        let mut ty = self.parse_primary();
        self.skip_ws();
        while self.eat('?') {
            ty = CfdType::Nullable(Box::new(ty));
            self.skip_ws();
        }
        ty
    }

    fn parse_primary(&mut self) -> CfdType {
        self.skip_ws();
        if self.eat('[') {
            let inner = self.parse_type();
            self.skip_ws();
            let _ = self.eat(']');
            return CfdType::Array(Box::new(inner));
        }
        if self.eat('{') {
            let key = self.parse_type();
            self.skip_ws();
            let _ = self.eat(':');
            let value = self.parse_type();
            self.skip_ws();
            let _ = self.eat('}');
            return CfdType::Dict(Box::new(key), Box::new(value));
        }

        let name = self.parse_name();
        match name.as_str() {
            "int" => CfdType::Int,
            "float" => CfdType::Float,
            "bool" => CfdType::Bool,
            "string" => CfdType::String,
            other if self.schema.has_enum(other) => CfdType::Enum(other.to_string()),
            other => CfdType::Type(other.to_string()),
        }
    }

    fn parse_name(&mut self) -> String {
        self.skip_ws();
        let start = self.pos;
        while let Some(ch) = self.peek() {
            if matches!(
                ch,
                '[' | ']' | '{' | '}' | ':' | '?' | ' ' | '\t' | '\r' | '\n'
            ) {
                break;
            }
            self.pos += ch.len_utf8();
        }
        self.text[start..self.pos].to_string()
    }

    fn skip_ws(&mut self) {
        while let Some(ch) = self.peek() {
            if ch.is_whitespace() {
                self.pos += ch.len_utf8();
            } else {
                break;
            }
        }
    }

    fn eat(&mut self, expected: char) -> bool {
        if self.peek() == Some(expected) {
            self.pos += expected.len_utf8();
            true
        } else {
            false
        }
    }

    fn peek(&self) -> Option<char> {
        self.text[self.pos..].chars().next()
    }
}

fn has_annotation(annotations: &[CftAnnotation], name: &str) -> bool {
    annotations.iter().any(|annotation| annotation.name == name)
}

fn annotation_name_arg(annotations: &[CftAnnotation], name: &str) -> Option<String> {
    annotations
        .iter()
        .find(|annotation| annotation.name == name)
        .and_then(|annotation| annotation.args.first())
        .and_then(|arg| match arg {
            CftAnnotationValue::Name(name) => Some(name.clone()),
            _ => None,
        })
}

fn type_accepts_default(expected: &CfdType, actual: &CfdType) -> bool {
    match expected {
        CfdType::Nullable(inner) => type_accepts_default(inner, actual),
        _ => expected == actual,
    }
}

fn id_matches_type(id: &CfdIdValue, ty: &CfdType) -> bool {
    match ty {
        CfdType::Nullable(inner) => id_matches_type(id, inner),
        CfdType::String => matches!(id, CfdIdValue::String(_)),
        CfdType::Int => matches!(id, CfdIdValue::Int(_)),
        _ => false,
    }
}

fn id_from_fields(fields: &BTreeMap<String, CfdValueDraft>, name: &str) -> Option<CfdIdValue> {
    match fields.get(name) {
        Some(CfdValueDraft::Value(CfdValue::String(value))) => {
            Some(CfdIdValue::String(value.clone()))
        }
        Some(CfdValueDraft::Value(CfdValue::Int(value))) => Some(CfdIdValue::Int(*value)),
        _ => None,
    }
}

fn index_key_from_draft(value: &CfdValueDraft) -> Option<CfdIndexKey> {
    match value {
        CfdValueDraft::Value(CfdValue::String(value)) => Some(CfdIndexKey::String(value.clone())),
        CfdValueDraft::Value(CfdValue::Int(value)) => Some(CfdIndexKey::Int(*value)),
        CfdValueDraft::Value(CfdValue::Enum(value)) => Some(CfdIndexKey::Enum(value.clone())),
        CfdValueDraft::Value(CfdValue::Null) => None,
        _ => None,
    }
}

fn input_value_kind(value: &CfdInputValue) -> &'static str {
    match value {
        CfdInputValue::Null => "null",
        CfdInputValue::Bool(_) => "bool",
        CfdInputValue::Int(_) => "int",
        CfdInputValue::Float(_) => "float",
        CfdInputValue::String(_) => "string",
        CfdInputValue::EnumVariant { .. } => "enum",
        CfdInputValue::Object { .. } => "object",
        CfdInputValue::Ref(_) => "ref",
        CfdInputValue::Array(_) => "array",
        CfdInputValue::Dict(_) => "dict",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use coflow_cft::{CftContainer, ModuleId};

    fn compile_schema(source: &str) -> CftContainer {
        let mut container = CftContainer::new();
        container
            .add_module(ModuleId::from("main"), source)
            .expect("schema should parse");
        container.compile().expect("schema should compile");
        container
    }

    fn assert_has_code(diags: &CfdDiagnostics, code: CfdErrorCode) {
        assert!(
            diags.diagnostics.iter().any(|diag| diag.code == code),
            "expected {code}, got {:?}",
            diags
                .diagnostics
                .iter()
                .map(|diag| diag.code)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn data_model_applies_defaults_and_builds_indexes_without_running_check() {
        let schema = compile_schema(
            r#"
                const DEFAULT_NAME = "unknown";
                enum Rarity { Common = 0, Rare = 10, }
                type Item {
                    @id
                    id: string;
                    name: string = DEFAULT_NAME;
                    @index
                    rarity: Rarity = Rarity.Common;
                    tags: [string] = [];
                    attrs: {string: int} = {};
                    check { id != ""; }
                }
            "#,
        );

        let mut builder = CfdDataModel::builder(&schema);
        builder.add_record("Item", [("id", CfdInputValue::from(""))]);
        let model = builder.build().expect("data model should build");

        let table = model.table("Item").expect("item table");
        assert_eq!(table.records, vec![CfdRecordId(0)]);
        assert!(table.primary_index.contains_key(&CfdIdValue::from("")));
        assert!(
            table.secondary_indexes["rarity"].contains_key(&CfdIndexKey::Enum(CfdEnumValue {
                enum_name: "Rarity".to_string(),
                variant: "Common".to_string(),
                value: 0,
            }))
        );

        let record = model.record(CfdRecordId(0)).expect("record");
        assert_eq!(
            record.field("name"),
            Some(&CfdValue::String("unknown".to_string()))
        );
        assert_eq!(record.field("tags"), Some(&CfdValue::Array(Vec::new())));
        assert_eq!(record.field("attrs"), Some(&CfdValue::Dict(Vec::new())));
    }

    #[test]
    fn polymorphic_refs_resolve_against_the_data_model() {
        let schema = compile_schema(
            r#"
                abstract type Reward { @id id: string; }
                type ItemReward : Reward { count: int; }
                type CurrencyReward : Reward { amount: int; }
                type Drop {
                    @ref(Reward)
                    reward_id: string;
                }
            "#,
        );

        let mut builder = CfdDataModel::builder(&schema);
        builder.add_record(
            "ItemReward",
            [
                ("id", CfdInputValue::from("reward_1")),
                ("count", CfdInputValue::from(1_i64)),
            ],
        );
        builder.add_record(
            "Drop",
            [(
                "reward_id",
                CfdInputValue::Ref(CfdIdValue::from("reward_1")),
            )],
        );
        let model = builder.build().expect("data model should build");

        assert!(model.inheritance_index["Reward"]
            .records
            .contains_key(&CfdIdValue::from("reward_1")));
        assert_eq!(
            model
                .record(CfdRecordId(1))
                .and_then(|record| record.field("reward_id")),
            Some(&CfdValue::Ref {
                id: CfdIdValue::from("reward_1"),
                target: CfdRecordId(0),
            })
        );
    }

    #[test]
    fn ref_field_defaults_are_resolved_as_references() {
        let schema = compile_schema(
            r#"
                type Item { @id id: string; }
                type Drop {
                    @ref(Item)
                    item_id: string = "default_item";
                }
            "#,
        );

        let mut builder = CfdDataModel::builder(&schema);
        builder.add_record("Item", [("id", CfdInputValue::from("default_item"))]);
        builder.add_input_record(CfdInputRecord::new(
            "Drop",
            std::iter::empty::<(&str, CfdInputValue)>(),
        ));
        let model = builder.build().expect("data model should build");

        assert_eq!(
            model
                .record(CfdRecordId(1))
                .and_then(|record| record.field("item_id")),
            Some(&CfdValue::Ref {
                id: CfdIdValue::from("default_item"),
                target: CfdRecordId(0),
            })
        );
    }

    #[test]
    fn duplicate_ids_are_checked_inside_polymorphic_ranges() {
        let schema = compile_schema(
            r#"
                abstract type Reward { @id id: string; }
                type ItemReward : Reward { count: int; }
                type CurrencyReward : Reward { amount: int; }
            "#,
        );

        let mut builder = CfdDataModel::builder(&schema);
        builder.add_record(
            "ItemReward",
            [
                ("id", CfdInputValue::from("same")),
                ("count", CfdInputValue::from(1_i64)),
            ],
        );
        builder.add_record(
            "CurrencyReward",
            [
                ("id", CfdInputValue::from("same")),
                ("amount", CfdInputValue::from(10_i64)),
            ],
        );

        let err = builder.build().expect_err("duplicate polymorphic id");
        assert_has_code(&err, CfdErrorCode::DuplicatePolymorphicId);
        let diag = err
            .diagnostics
            .iter()
            .find(|diag| diag.code == CfdErrorCode::DuplicatePolymorphicId)
            .expect("diag");
        assert!(!diag.related.is_empty());
    }

    #[test]
    fn inline_objects_use_declared_type_when_not_polymorphic() {
        let schema = compile_schema(
            r#"
                type Stats { hp: int; speed: float = 1.0; }
                type Monster { stats: Stats; }
            "#,
        );

        let mut builder = CfdDataModel::builder(&schema);
        builder.add_record(
            "Monster",
            [(
                "stats",
                CfdInputValue::object_with_declared_type([("hp", CfdInputValue::from(100_i64))]),
            )],
        );
        let model = builder.build().expect("data model should build");
        let Some(CfdValue::Object(stats)) = model
            .record(CfdRecordId(0))
            .and_then(|record| record.field("stats"))
        else {
            panic!("expected stats object");
        };
        assert_eq!(stats.actual_type, "Stats");
        assert_eq!(stats.field("speed"), Some(&CfdValue::Float(1.0)));
    }

    #[test]
    fn semantic_edges_report_data_model_diagnostics() {
        let schema = compile_schema(
            r#"
                enum Rarity { Common, Rare, }
                type Item {
                    @id
                    id: string;
                    rarity: Rarity;
                    maybe: int?;
                    attrs: {string: int};
                }
            "#,
        );

        let mut builder = CfdDataModel::builder(&schema);
        builder.add_record(
            "Item",
            [
                ("id", CfdInputValue::from("item_1")),
                ("unknown", CfdInputValue::from(1_i64)),
                ("rarity", CfdInputValue::enum_variant("Rarity", "Missing")),
                (
                    "attrs",
                    CfdInputValue::dict([
                        (CfdInputDictKey::from("x"), CfdInputValue::from(1_i64)),
                        (CfdInputDictKey::from("x"), CfdInputValue::from(2_i64)),
                    ]),
                ),
            ],
        );

        let err = builder.build().expect_err("data errors");
        assert_has_code(&err, CfdErrorCode::UnknownField);
        assert_has_code(&err, CfdErrorCode::MissingRequiredField);
        assert_has_code(&err, CfdErrorCode::InvalidEnumVariant);
        assert_has_code(&err, CfdErrorCode::DuplicateDictKey);
    }

    #[test]
    fn build_collects_diagnostics_across_multiple_records() {
        let schema = compile_schema(
            r#"
                type Item { id: string; value: int; }
            "#,
        );

        let mut builder = CfdDataModel::builder(&schema);
        builder.add_record(
            "Item",
            [
                ("id", CfdInputValue::from("a")),
                ("value", CfdInputValue::from("not_int")),
            ],
        );
        builder.add_record("MissingType", [("id", CfdInputValue::from("b"))]);

        let err = builder.build().expect_err("data errors");
        assert_has_code(&err, CfdErrorCode::TypeMismatch);
        assert_has_code(&err, CfdErrorCode::UnknownType);
    }

    #[test]
    fn polymorphic_object_fields_need_actual_type_markers() {
        let schema = compile_schema(
            r#"
                abstract type Reward { id: string; }
                type CurrencyReward : Reward { amount: int; }
                type Drop { reward: Reward; }
            "#,
        );

        let mut builder = CfdDataModel::builder(&schema);
        builder.add_record(
            "Drop",
            [(
                "reward",
                CfdInputValue::object_with_declared_type([
                    ("id", CfdInputValue::from("r1")),
                    ("amount", CfdInputValue::from(10_i64)),
                ]),
            )],
        );

        let err = builder.build().expect_err("missing object type");
        assert_has_code(&err, CfdErrorCode::MissingObjectType);
    }

    #[test]
    fn ref_resolution_reports_missing_targets_and_targets_without_id() {
        let missing_schema = compile_schema(
            r#"
                type Item { @id id: string; }
                type Drop { @ref(Item) item_id: string; }
            "#,
        );
        let mut missing_builder = CfdDataModel::builder(&missing_schema);
        missing_builder.add_record(
            "Drop",
            [("item_id", CfdInputValue::Ref(CfdIdValue::from("missing")))],
        );
        let missing = missing_builder.build().expect_err("missing ref target");
        assert_has_code(&missing, CfdErrorCode::RefTargetNotFound);

        let no_id_schema = compile_schema(
            r#"
                type Item { name: string; }
                type Drop { @ref(Item) item_id: string; }
            "#,
        );
        let mut no_id_builder = CfdDataModel::builder(&no_id_schema);
        no_id_builder.add_record("Item", [("name", CfdInputValue::from("Potion"))]);
        no_id_builder.add_record(
            "Drop",
            [("item_id", CfdInputValue::Ref(CfdIdValue::from("potion")))],
        );
        let no_id = no_id_builder.build().expect_err("ref target without id");
        assert_has_code(&no_id, CfdErrorCode::RefTargetHasNoId);
    }

    #[test]
    fn check_runner_accepts_core_expressions_refs_and_quantifiers() {
        let schema = compile_schema(
            r#"
                const MIN_LEVEL = 1;
                enum Rarity { Common, Rare, }

                type Item {
                    @id
                    id: string;
                    rarity: Rarity;
                }

                type Drop {
                    @ref(Item)
                    item_id: string;
                    weights: [int];
                    resistances: {Rarity: float};

                    check {
                        item_id.id != "";
                        item_id.rarity >= Rarity.Common;
                        len(weights) >= MIN_LEVEL;
                        sum(weights) == 100;
                        all entry in resistances {
                            entry.key >= Rarity.Common;
                            entry.value >= 0.0;
                        }
                        contains(keys(resistances), Rarity.Rare);
                    }
                }
            "#,
        );

        let mut builder = CfdDataModel::builder(&schema);
        builder.add_record(
            "Item",
            [
                ("id", CfdInputValue::from("item_1")),
                ("rarity", CfdInputValue::enum_variant("Rarity", "Rare")),
            ],
        );
        builder.add_record(
            "Drop",
            [
                ("item_id", CfdInputValue::from("item_1")),
                (
                    "weights",
                    CfdInputValue::Array(vec![
                        CfdInputValue::from(40_i64),
                        CfdInputValue::from(60_i64),
                    ]),
                ),
                (
                    "resistances",
                    CfdInputValue::dict([
                        (
                            CfdInputDictKey::enum_variant("Rarity", "Common"),
                            CfdInputValue::from(0.5_f64),
                        ),
                        (
                            CfdInputDictKey::enum_variant("Rarity", "Rare"),
                            CfdInputValue::from(1.0_f64),
                        ),
                    ]),
                ),
            ],
        );
        let model = builder.build().expect("data model should build");
        model.run_checks(&schema).expect("checks should pass");
    }

    #[test]
    fn check_runner_reports_false_conditions() {
        let schema = compile_schema(
            r#"
                type Item {
                    value: int;
                    check { value > 0; }
                }
            "#,
        );

        let mut builder = CfdDataModel::builder(&schema);
        builder.add_record("Item", [("value", CfdInputValue::from(0_i64))]);
        let model = builder.build().expect("data model should build");
        let err = model.run_checks(&schema).expect_err("check should fail");
        assert_has_code(&err, CfdErrorCode::CheckFailed);
    }

    #[test]
    fn check_runner_short_circuits_nullable_guards_and_reports_null_access() {
        let guarded = compile_schema(
            r#"
                type Child { id: string; }
                type Holder {
                    child: Child? = null;
                    check { child == null || child.id != ""; }
                }
            "#,
        );
        let mut guarded_builder = CfdDataModel::builder(&guarded);
        guarded_builder.add_input_record(CfdInputRecord::new(
            "Holder",
            std::iter::empty::<(&str, CfdInputValue)>(),
        ));
        let guarded_model = guarded_builder.build().expect("data model should build");
        guarded_model
            .run_checks(&guarded)
            .expect("guarded check should pass");

        let unguarded = compile_schema(
            r#"
                type Child { id: string; }
                type Holder {
                    child: Child? = null;
                    check { child.id != ""; }
                }
            "#,
        );
        let mut unguarded_builder = CfdDataModel::builder(&unguarded);
        unguarded_builder.add_input_record(CfdInputRecord::new(
            "Holder",
            std::iter::empty::<(&str, CfdInputValue)>(),
        ));
        let unguarded_model = unguarded_builder.build().expect("data model should build");
        let err = unguarded_model
            .run_checks(&unguarded)
            .expect_err("null access");
        assert_has_code(&err, CfdErrorCode::CheckNullAccess);
    }

    #[test]
    fn check_runner_executes_inherited_checks() {
        let schema = compile_schema(
            r#"
                abstract type Reward {
                    id: string;
                    check { id != ""; }
                }
                type CurrencyReward : Reward {
                    amount: int;
                    check { amount > 0; }
                }
            "#,
        );

        let mut builder = CfdDataModel::builder(&schema);
        builder.add_record(
            "CurrencyReward",
            [
                ("id", CfdInputValue::from("")),
                ("amount", CfdInputValue::from(0_i64)),
            ],
        );
        let model = builder.build().expect("data model should build");
        let err = model
            .run_checks(&schema)
            .expect_err("inherited checks fail");
        let failures = err
            .diagnostics
            .iter()
            .filter(|diag| diag.code == CfdErrorCode::CheckFailed)
            .count();
        assert_eq!(failures, 2);
    }

    #[test]
    fn check_runner_reports_index_and_empty_minmax_eval_errors() {
        let schema = compile_schema(
            r#"
                type Item {
                    xs: [int];
                    check {
                        xs[1] > 0;
                        min(xs) > 0;
                    }
                }
            "#,
        );

        let mut builder = CfdDataModel::builder(&schema);
        builder.add_record("Item", [("xs", CfdInputValue::Array(Vec::new()))]);
        let model = builder.build().expect("data model should build");
        let err = model.run_checks(&schema).expect_err("eval errors");
        assert_has_code(&err, CfdErrorCode::CheckIndexOutOfBounds);
        assert_has_code(&err, CfdErrorCode::CheckEmptyMinMax);
    }
}
