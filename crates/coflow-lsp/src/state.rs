use coflow_cft::syntax::ast::{CheckExpr, CheckStmt, Item, NameRef};
use coflow_cft::syntax::CheckVisitor;
use coflow_cft::{CftEnum, CftEnumVariant, CftField, CftType, CftValueType, ModuleId};
use coflow_project::normalize_path;
use coflow_runtime::ProjectSchemaSession;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use crate::definition;
use crate::uri::{path_from_file_uri, path_to_file_uri};

pub(crate) struct LspBuild {
    pub(crate) schema: ProjectSchemaSession,
    pub(crate) documents: BTreeMap<String, LspDocument>,
    pub(crate) cfd_definitions: definition::CfdDefinitionIndex,
    module_by_uri: BTreeMap<String, String>,
    module_by_path: BTreeMap<PathBuf, String>,
}

pub(crate) struct LspDocument {
    pub(crate) module_id: String,
    pub(crate) uri: String,
    pub(crate) source: Arc<str>,
    pub(crate) ast: Option<Arc<coflow_cft::syntax::ast::ModuleAst>>,
}

impl LspDocument {
    pub(crate) fn source(&self) -> &str {
        &self.source
    }

    pub(crate) fn ast(&self) -> Option<&coflow_cft::syntax::ast::ModuleAst> {
        self.ast.as_deref()
    }
}

impl LspBuild {
    pub(crate) fn new(schema: ProjectSchemaSession) -> Self {
        let mut documents = BTreeMap::new();
        let mut module_by_uri = BTreeMap::new();
        let mut module_by_path = BTreeMap::new();

        for (module_id, module) in schema.modules().modules() {
            let module_id = module_id.as_str().to_string();
            let path = module.path().to_path_buf();
            let uri = path_to_file_uri(&path);
            module_by_uri.insert(uri.clone(), module_id.clone());
            module_by_path.insert(normalize_path(&path), module_id.clone());
            documents.insert(
                module_id.clone(),
                LspDocument {
                    module_id,
                    uri,
                    source: module.shared_source(),
                    ast: module.shared_ast(),
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

    pub(crate) fn schema(&self) -> Option<&coflow_cft::CftSchema> {
        self.schema.schema()
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
) -> Option<&'a CftType> {
    build.schema()?.all_types().find(|ty| {
        ty.module.as_str() == document.module_id && ty.span.start <= offset && offset <= ty.span.end
    })
}

pub(crate) fn current_field_at(
    document: &LspDocument,
    offset: usize,
) -> Option<&coflow_cft::syntax::ast::FieldDef> {
    let ast = document.ast()?;
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
) -> Option<CftValueType> {
    let (first, rest) = chain.split_first()?;
    let mut value_type = type_of_name(build, document, offset, first)?;
    for part in rest {
        let type_name = type_name_of_schema_ref(&value_type)?;
        let (_, field) = field_by_type(build, type_name, part)?;
        value_type = field_receiver_type(field);
    }
    Some(value_type)
}

fn type_of_name(
    build: &LspBuild,
    document: &LspDocument,
    offset: usize,
    name: &str,
) -> Option<CftValueType> {
    let current_type = current_type_at(build, document, offset)?;
    let field = current_type
        .all_fields()
        .find(|field| field.name.as_str() == name)?;
    Some(field_receiver_type(field))
}

pub(crate) fn field_by_type<'a>(
    build: &'a LspBuild,
    type_name: &str,
    field_name: &str,
) -> Option<(&'a CftType, &'a CftField)> {
    let schema = build.schema()?;
    let mut current = schema.resolve_type(type_name);
    while let Some(ty) = current {
        if let Some(field) = ty
            .own_fields()
            .find(|field| field.name.as_str() == field_name)
        {
            return Some((ty, field));
        }
        current = ty
            .parent
            .as_deref()
            .and_then(|parent| schema.resolve_type(parent));
    }
    None
}

fn field_receiver_type(field: &CftField) -> CftValueType {
    field.value_type.clone()
}

pub(crate) fn type_name_of_schema_ref(ty: &CftValueType) -> Option<&str> {
    match ty {
        CftValueType::Object(name) => Some(name),
        CftValueType::Nullable(inner) => type_name_of_schema_ref(inner),
        _ => None,
    }
}

pub(crate) fn field_by_chain<'a>(
    build: &'a LspBuild,
    document: &LspDocument,
    offset: usize,
    chain: &[String],
) -> Option<(String, &'a CftField)> {
    let (field_name, receiver) = chain.split_last()?;
    let receiver_type = type_of_chain(build, document, offset, receiver)?;
    let type_name = type_name_of_schema_ref(&receiver_type)?;
    let (_, field) = field_by_type(build, type_name, field_name)?;
    Some((type_name.to_string(), field))
}

pub(crate) fn enum_variant_by_chain<'a>(
    build: &'a LspBuild,
    chain: &[String],
) -> Option<(&'a CftEnum, &'a CftEnumVariant)> {
    if chain.len() != 2 {
        return None;
    }
    let enum_def = build.schema()?.resolve_enum(&chain[0])?;
    let variant = enum_def
        .variants
        .iter()
        .find(|variant| variant.name.as_str() == chain[1])?;
    Some((enum_def, variant))
}

pub(crate) fn enum_name_exists(build: &LspBuild, enum_name: &str) -> bool {
    build
        .schema()
        .is_some_and(|schema| schema.resolve_enum(enum_name).is_some())
        || ast_enum_name_exists(build, enum_name)
}

pub(crate) fn enum_variant_exists(build: &LspBuild, enum_name: &str, variant_name: &str) -> bool {
    enum_variant_by_chain(build, &[enum_name.to_string(), variant_name.to_string()]).is_some()
        || definition::ast_enum_variant_location(build, enum_name, variant_name).is_some()
}

fn ast_enum_name_exists(build: &LspBuild, enum_name: &str) -> bool {
    build.documents.values().any(|document| {
        document.ast().is_some_and(|ast| {
            ast.items
                .iter()
                .any(|item| matches!(item, Item::Enum(enum_def) if enum_def.name == enum_name))
        })
    })
}

pub(crate) fn quantifier_bindings_at(document: &LspDocument, offset: usize) -> Vec<String> {
    struct BindingVisitor {
        offset: usize,
        bindings: Vec<String>,
    }

    impl CheckVisitor for BindingVisitor {
        type Error = std::convert::Infallible;

        fn visit_stmt(&mut self, stmt: &CheckStmt) -> Result<(), Self::Error> {
            let span = stmt.span();
            if span.start <= self.offset && self.offset <= span.end {
                self.walk_stmt(stmt)?;
            }
            Ok(())
        }

        fn visit_expr(&mut self, _expr: &CheckExpr) -> Result<(), Self::Error> {
            Ok(())
        }

        fn enter_quantifier_body(&mut self, bindings: &[NameRef]) -> Result<(), Self::Error> {
            self.bindings
                .extend(bindings.iter().map(|binding| binding.name.clone()));
            Ok(())
        }
    }

    let mut visitor = BindingVisitor {
        offset,
        bindings: Vec::new(),
    };
    let Some(ast) = document.ast() else {
        return visitor.bindings;
    };
    for item in &ast.items {
        match item {
            Item::Type(ty) => {
                if let Some(check) = &ty.check {
                    let result = visitor.visit_block(check);
                    debug_assert!(result.is_ok());
                }
            }
            Item::Check(check) => {
                let result = visitor.visit_block(&check.block);
                debug_assert!(result.is_ok());
            }
            Item::Const(_) | Item::Enum(_) => {}
        }
    }
    visitor.bindings
}
