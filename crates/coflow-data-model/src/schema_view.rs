use crate::model::{CfdDictKey, CfdIdValue, CfdIndexKey, CfdInputValue, CfdValue};
use coflow_cft::{
    CftAnnotation, CftAnnotationValue, CftContainer, CftSchemaDefaultValue, CftSchemaEnum,
    CftSchemaField, CftSchemaType,
};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RecordDraft {
    pub(crate) actual_type: String,
    pub(crate) fields: BTreeMap<String, CfdValueDraft>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum CfdValueDraft {
    Value(CfdValue),
    Object(Box<RecordDraft>),
    PendingRef { target_type: String, id: CfdIdValue },
    Array(Vec<CfdValueDraft>),
    Dict(Vec<(CfdDictKey, CfdValueDraft)>),
}

#[derive(Debug, Clone)]
pub(crate) struct SchemaView {
    pub(crate) types: BTreeMap<String, TypeMeta>,
    pub(crate) enums: BTreeMap<String, EnumMeta>,
    children: BTreeMap<String, BTreeSet<String>>,
}

impl SchemaView {
    pub(crate) fn new(schema: &CftContainer) -> Self {
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
            types,
            enums,
            children,
        }
    }

    pub(crate) fn full_fields(&self, type_name: &str) -> Vec<FieldMeta> {
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

    pub(crate) fn is_assignable(&self, actual_type: &str, expected_type: &str) -> bool {
        let mut current = Some(actual_type);
        while let Some(name) = current {
            if name == expected_type {
                return true;
            }
            current = self.types.get(name).and_then(|meta| meta.parent.as_deref());
        }
        false
    }

    pub(crate) fn range_is_polymorphic(&self, type_name: &str) -> bool {
        self.types
            .get(type_name)
            .is_some_and(|meta| meta.is_abstract || self.has_descendants(type_name))
    }

    fn has_descendants(&self, type_name: &str) -> bool {
        self.children
            .get(type_name)
            .is_some_and(|children| !children.is_empty())
    }

    pub(crate) fn assignable_target_names(&self, actual_type: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut current = Some(actual_type);
        while let Some(name) = current {
            out.push(name.to_string());
            current = self.types.get(name).and_then(|meta| meta.parent.as_deref());
        }
        out
    }

    pub(crate) fn id_field_for_actual(&self, actual_type: &str) -> Option<FieldMeta> {
        self.full_fields(actual_type)
            .into_iter()
            .find(|field| field.is_id)
    }

    pub(crate) fn index_fields_for_actual(&self, actual_type: &str) -> Vec<FieldMeta> {
        self.full_fields(actual_type)
            .into_iter()
            .filter(|field| field.is_index)
            .collect()
    }

    pub(crate) fn range_has_id(&self, target_type: &str) -> bool {
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

    pub(crate) fn enum_variant_value(&self, enum_name: &str, variant: &str) -> Option<i64> {
        self.enums
            .get(enum_name)
            .and_then(|meta| meta.variants.get(variant))
            .copied()
    }
}

#[derive(Debug, Clone)]
pub(crate) struct TypeMeta {
    pub(crate) name: String,
    pub(crate) parent: Option<String>,
    pub(crate) is_abstract: bool,
    fields: Vec<FieldMeta>,
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
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct FieldMeta {
    pub(crate) name: String,
    pub(crate) ty: CfdType,
    pub(crate) default: Option<CftSchemaDefaultValue>,
    pub(crate) ref_target: Option<String>,
    pub(crate) is_id: bool,
    pub(crate) is_index: bool,
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
pub(crate) struct EnumMeta {
    pub(crate) variants: BTreeMap<String, i64>,
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
pub(crate) enum CfdType {
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

    pub(crate) fn is_nullable(&self) -> bool {
        matches!(self, Self::Nullable(_))
    }

    pub(crate) fn display(&self) -> String {
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

pub(crate) fn type_accepts_default(expected: &CfdType, actual: &CfdType) -> bool {
    match expected {
        CfdType::Nullable(inner) => type_accepts_default(inner, actual),
        _ => expected == actual,
    }
}

pub(crate) fn id_matches_type(id: &CfdIdValue, ty: &CfdType) -> bool {
    match ty {
        CfdType::Nullable(inner) => id_matches_type(id, inner),
        CfdType::String => matches!(id, CfdIdValue::String(_)),
        CfdType::Int => matches!(id, CfdIdValue::Int(_)),
        _ => false,
    }
}

pub(crate) fn id_from_fields(
    fields: &BTreeMap<String, CfdValueDraft>,
    name: &str,
) -> Option<CfdIdValue> {
    match fields.get(name) {
        Some(CfdValueDraft::Value(CfdValue::String(value))) => {
            Some(CfdIdValue::String(value.clone()))
        }
        Some(CfdValueDraft::Value(CfdValue::Int(value))) => Some(CfdIdValue::Int(*value)),
        _ => None,
    }
}

pub(crate) fn index_key_from_draft(value: &CfdValueDraft) -> Option<CfdIndexKey> {
    match value {
        CfdValueDraft::Value(CfdValue::String(value)) => Some(CfdIndexKey::String(value.clone())),
        CfdValueDraft::Value(CfdValue::Int(value)) => Some(CfdIndexKey::Int(*value)),
        CfdValueDraft::Value(CfdValue::Enum(value)) => Some(CfdIndexKey::Enum(value.clone())),
        CfdValueDraft::Value(CfdValue::Null) => None,
        _ => None,
    }
}

pub(crate) fn input_value_kind(value: &CfdInputValue) -> &'static str {
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
