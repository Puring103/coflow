use coflow_cft::ast::{CheckStmt, Item};
use coflow_cft::{
    CftSchemaEnum, CftSchemaEnumVariant, CftSchemaField, CftSchemaType, CftSchemaTypeRef, ModuleId,
};
use coflow_project::normalize_path;
use coflow_runtime::SchemaBuild;
use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::definition;
use crate::uri::{path_from_file_uri, path_to_file_uri};

pub(crate) struct LspBuild {
    pub(crate) schema: SchemaBuild,
    pub(crate) documents: BTreeMap<String, LspDocument>,
    pub(crate) cfd_definitions: definition::CfdDefinitionIndex,
    module_by_uri: BTreeMap<String, String>,
    module_by_path: BTreeMap<PathBuf, String>,
}

pub(crate) struct LspDocument {
    pub(crate) module_id: String,
    pub(crate) uri: String,
    pub(crate) source: String,
    pub(crate) ast: Option<coflow_cft::ast::ModuleAst>,
}

impl LspBuild {
    pub(crate) fn new(schema: SchemaBuild) -> Self {
        let mut documents = BTreeMap::new();
        let mut module_by_uri = BTreeMap::new();
        let mut module_by_path = BTreeMap::new();

        for (module_id, source) in &schema.sources {
            let path = schema
                .paths
                .get(module_id)
                .map_or_else(|| PathBuf::from(module_id), PathBuf::from);
            let uri = path_to_file_uri(&path);
            let ast =
                coflow_cft::parser::parse_module(&ModuleId::new(module_id.clone()), source).ok();
            module_by_uri.insert(uri.clone(), module_id.clone());
            module_by_path.insert(normalize_path(&path), module_id.clone());
            documents.insert(
                module_id.clone(),
                LspDocument {
                    module_id: module_id.clone(),
                    uri,
                    source: source.clone(),
                    ast,
                },
            );
        }

        Self {
            schema,
            documents,
            cfd_definitions: definition::CfdDefinitionIndex::default(),
            module_by_uri,
            module_by_path,
        }
    }

    pub(crate) fn with_cfd_definitions(
        mut self,
        definitions: definition::CfdDefinitionIndex,
    ) -> Self {
        self.cfd_definitions = definitions;
        self
    }

    pub(crate) const fn container(&self) -> Option<&coflow_cft::CftContainer> {
        self.schema.container.as_ref()
    }

    pub(crate) fn document_by_uri(&self, uri: &str) -> Option<&LspDocument> {
        if let Some(module_id) = self.module_by_uri.get(uri) {
            return self.documents.get(module_id);
        }
        let path = path_from_file_uri(uri)?;
        let module_id = self.module_by_path.get(&normalize_path(&path))?;
        self.documents.get(module_id)
    }

    pub(crate) fn document_by_module(&self, module_id: &ModuleId) -> Option<&LspDocument> {
        self.documents.get(module_id.as_str())
    }
}

pub(crate) fn current_type_at<'a>(
    build: &'a LspBuild,
    document: &LspDocument,
    offset: usize,
) -> Option<&'a CftSchemaType> {
    build.container()?.all_types().find(|ty| {
        ty.module.as_str() == document.module_id && ty.span.start <= offset && offset <= ty.span.end
    })
}

pub(crate) fn current_field_at(
    document: &LspDocument,
    offset: usize,
) -> Option<&coflow_cft::ast::FieldDef> {
    let ast = document.ast.as_ref()?;
    for item in &ast.items {
        if let Item::Type(ty) = item {
            if ty.span.start <= offset && offset <= ty.span.end {
                for field in &ty.fields {
                    if field.span.start <= offset && offset <= field.span.end {
                        return Some(field);
                    }
                }
            }
        }
    }
    None
}

pub(crate) fn type_of_chain(
    build: &LspBuild,
    document: &LspDocument,
    offset: usize,
    chain: &[String],
) -> Option<CftSchemaTypeRef> {
    let (first, rest) = chain.split_first()?;
    let mut ty_ref = type_of_name(build, document, offset, first)?;
    for part in rest {
        let type_name = type_name_of_schema_ref(&ty_ref)?;
        let (_, field) = field_by_type(build, type_name, part)?;
        ty_ref = field_receiver_type(field);
    }
    Some(ty_ref)
}

fn type_of_name(
    build: &LspBuild,
    document: &LspDocument,
    offset: usize,
    name: &str,
) -> Option<CftSchemaTypeRef> {
    let current_type = current_type_at(build, document, offset)?;
    let field = current_type
        .all_fields
        .iter()
        .find(|field| field.name == name)?;
    Some(field_receiver_type(field))
}

pub(crate) fn field_by_type<'a>(
    build: &'a LspBuild,
    type_name: &str,
    field_name: &str,
) -> Option<(&'a CftSchemaType, &'a CftSchemaField)> {
    let container = build.container()?;
    let mut current = container.resolve_type(type_name);
    while let Some(ty) = current {
        if let Some(field) = ty.fields.iter().find(|field| field.name == field_name) {
            return Some((ty, field));
        }
        current = ty
            .parent
            .as_deref()
            .and_then(|parent| container.resolve_type(parent));
    }
    None
}

fn field_receiver_type(field: &CftSchemaField) -> CftSchemaTypeRef {
    field.ty_ref.clone()
}

pub(crate) fn type_name_of_schema_ref(ty: &CftSchemaTypeRef) -> Option<&str> {
    match ty {
        CftSchemaTypeRef::Named(name) => Some(name),
        CftSchemaTypeRef::Nullable(inner) => type_name_of_schema_ref(inner),
        _ => None,
    }
}

pub(crate) fn field_by_chain<'a>(
    build: &'a LspBuild,
    document: &LspDocument,
    offset: usize,
    chain: &[String],
) -> Option<(String, &'a CftSchemaField)> {
    let (field_name, receiver) = chain.split_last()?;
    let receiver_type = type_of_chain(build, document, offset, receiver)?;
    let type_name = type_name_of_schema_ref(&receiver_type)?;
    let (_, field) = field_by_type(build, type_name, field_name)?;
    Some((type_name.to_string(), field))
}

pub(crate) fn enum_variant_by_chain<'a>(
    build: &'a LspBuild,
    chain: &[String],
) -> Option<(&'a CftSchemaEnum, &'a CftSchemaEnumVariant)> {
    if chain.len() != 2 {
        return None;
    }
    let enum_def = build.container()?.resolve_enum(&chain[0])?;
    let variant = enum_def
        .variants
        .iter()
        .find(|variant| variant.name == chain[1])?;
    Some((enum_def, variant))
}

pub(crate) fn enum_name_exists(build: &LspBuild, enum_name: &str) -> bool {
    build
        .container()
        .is_some_and(|container| container.resolve_enum(enum_name).is_some())
        || ast_enum_name_exists(build, enum_name)
}

pub(crate) fn enum_variant_exists(build: &LspBuild, enum_name: &str, variant_name: &str) -> bool {
    enum_variant_by_chain(build, &[enum_name.to_string(), variant_name.to_string()]).is_some()
        || definition::ast_enum_variant_location(build, enum_name, variant_name).is_some()
}

fn ast_enum_name_exists(build: &LspBuild, enum_name: &str) -> bool {
    build.documents.values().any(|document| {
        document.ast.as_ref().is_some_and(|ast| {
            ast.items
                .iter()
                .any(|item| matches!(item, Item::Enum(enum_def) if enum_def.name == enum_name))
        })
    })
}

pub(crate) fn quantifier_bindings_at(document: &LspDocument, offset: usize) -> Vec<String> {
    let mut bindings = Vec::new();
    let Some(ast) = &document.ast else {
        return bindings;
    };
    for item in &ast.items {
        if let Item::Type(ty) = item {
            if let Some(check) = &ty.check {
                collect_quantifier_bindings(&check.stmts, offset, &mut bindings);
            }
        }
    }
    bindings
}

fn collect_quantifier_bindings(stmts: &[CheckStmt], offset: usize, bindings: &mut Vec<String>) {
    for stmt in stmts {
        match stmt {
            CheckStmt::Quantifier {
                binding,
                body,
                span,
                ..
            } => {
                if span.start <= offset && offset <= span.end {
                    bindings.push(binding.name.clone());
                    collect_quantifier_bindings(body, offset, bindings);
                }
            }
            CheckStmt::When { body, span, .. } => {
                if span.start <= offset && offset <= span.end {
                    collect_quantifier_bindings(body, offset, bindings);
                }
            }
            CheckStmt::Expr(_) => {}
        }
    }
}
