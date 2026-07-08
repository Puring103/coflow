use std::collections::{BTreeSet, HashSet};

use crate::emit::backing_field_name;
use crate::emit::identifiers::{
    csharp_public_member_name, field_local_name, loader_reserved_local_names,
};
use crate::emit::readers::{
    read_field_expr, read_messagepack_expr, read_messagepack_field_expr, read_required_expr,
    read_token_expr,
};
use crate::emit::types::{
    collection_default_expr, csharp_property_type, csharp_type, default_value_expr,
};
use crate::model::{CsharpLoadField, CsharpLoader, CsharpPolymorphicCase};
use crate::names::escape_csharp_string;
use crate::schema_context::CsharpSchemaContext;
use crate::CsharpCodegenError;
use coflow_cft::CftFieldMeta;
use coflow_cft::CftSchemaTypeRef;

pub(super) fn loader_method(
    type_name: &str,
    view: &CsharpSchemaContext,
) -> Result<CsharpLoader, CsharpCodegenError> {
    let ty = view.type_meta(type_name)?;
    let mut used_local_names = loader_reserved_local_names(ty);
    let key_ty = view.key_field_type(type_name);
    let key_local_name = field_local_name("id", &mut used_local_names)?;
    let is_table = type_is_table(type_name, view);
    // Singletons are not regular tables (they don't get a `Table<TKey, T>`
    // accessor) but they do land on disk as a top-level array of records
    // — the database loader calls `LoadTable` on them just like a table.
    // Without `is_disk_loadable` the type-loader templates would skip
    // `LoadTable` for singletons and the shared `Load(dataDir)` body would
    // fail to compile.
    let is_disk_loadable = is_table || ty.is_singleton;
    let fields = ty
        .all_fields
        .iter()
        .map(|field| {
            load_field(
                field,
                type_name,
                ty.is_singleton,
                &mut used_local_names,
                view,
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    let requires_hydration = fields.iter().any(|field| field.requires_context);
    let polymorphic_cases = if view.range_is_polymorphic(type_name) {
        polymorphic_cases(type_name, view)?
    } else {
        Vec::new()
    };
    Ok(CsharpLoader {
        type_name: view.csharp_type_name(type_name),
        source_name: type_name.to_string(),
        key_type_name: csharp_type(&key_ty, view),
        key_local_name,
        key_property: "Id".to_string(),
        key_read_expr: read_required_expr(
            "id",
            "obj",
            &read_token_expr(&key_ty, "token", "context", view)?,
        ),
        key_messagepack_read_expr: read_messagepack_expr(&key_ty, "reader", "context", view)?,
        is_table,
        is_disk_loadable,
        is_struct: view.type_is_struct(ty),
        requires_hydration,
        fields,
        polymorphic_cases,
        is_polymorphic: false,
        expected: String::new(),
    })
}

pub(super) fn polymorphic_loader(
    type_name: &str,
    view: &CsharpSchemaContext,
) -> Result<CsharpLoader, CsharpCodegenError> {
    let is_table = type_is_table(type_name, view);
    let is_singleton = view.type_meta(type_name)?.is_singleton;
    Ok(CsharpLoader {
        type_name: view.csharp_type_name(type_name),
        source_name: type_name.to_string(),
        key_type_name: csharp_type(&view.key_field_type(type_name), view),
        key_local_name: String::new(),
        key_property: "Id".to_string(),
        key_read_expr: String::new(),
        key_messagepack_read_expr: String::new(),
        is_table,
        is_disk_loadable: is_table || is_singleton,
        is_struct: view.type_is_struct(view.type_meta(type_name)?),
        requires_hydration: false,
        fields: Vec::new(),
        polymorphic_cases: polymorphic_cases(type_name, view)?,
        is_polymorphic: true,
        expected: view.concrete_assignable_types(type_name)?.join(" | "),
    })
}

fn polymorphic_cases(
    type_name: &str,
    view: &CsharpSchemaContext,
) -> Result<Vec<CsharpPolymorphicCase>, CsharpCodegenError> {
    Ok(view
        .concrete_assignable_types(type_name)?
        .iter()
        .map(|case| CsharpPolymorphicCase {
            type_name: view.csharp_type_name(case),
            source_name: case.clone(),
        })
        .collect())
}

fn load_field(
    field: &CftFieldMeta,
    owner_type_name: &str,
    owner_is_singleton: bool,
    used_local_names: &mut HashSet<String>,
    view: &CsharpSchemaContext,
) -> Result<CsharpLoadField, CsharpCodegenError> {
    let local_name = field_local_name(&field.name, used_local_names)?;
    let default_expr = default_value_expr(field.default.as_ref(), &field.ty_ref, view)?;
    let missing_expr = default_expr.as_ref().map_or_else(
        || {
            if field.ty_ref.is_nullable() {
                None
            } else {
                collection_default_expr(field.ty_ref.non_nullable(), view).ok()
            }
        },
        |default| Some(default.clone()),
    );
    let inner_property_type = csharp_property_type(&field.ty_ref, view);
    let property_type = if field.dimension.is_some() {
        format!("Localized<{inner_property_type}>")
    } else {
        inner_property_type.clone()
    };
    let raw_read_expr = read_field_expr(field, "obj", "context", view, missing_expr.as_deref())?;
    let raw_msgpack_expr = read_messagepack_field_expr(field, "reader", "context", view)?;
    let (read_expr, messagepack_read_expr, inline_read_expr, inline_messagepack_read_expr) =
        if field.dimension.is_some() {
            let type_lit = escape_csharp_string(owner_type_name);
            let field_lit = escape_csharp_string(&field.name);
            let row_key_expr = if owner_is_singleton {
                format!("\"{type_lit}/{field_lit}\"")
            } else {
                format!("string.Concat(\"{type_lit}/{field_lit}/\", id.ToString())")
            };
            let inline_key_expr = format!("\"{type_lit}/{field_lit}\"");
            (
                format!("new Localized<{inner_property_type}>({row_key_expr}, {raw_read_expr})"),
                format!("new Localized<{inner_property_type}>({row_key_expr}, {raw_msgpack_expr})"),
                format!("new Localized<{inner_property_type}>({inline_key_expr}, {raw_read_expr})"),
                format!(
                    "new Localized<{inner_property_type}>({inline_key_expr}, {raw_msgpack_expr})"
                ),
            )
        } else {
            (
                raw_read_expr.clone(),
                raw_msgpack_expr.clone(),
                raw_read_expr,
                raw_msgpack_expr,
            )
        };
    Ok(CsharpLoadField {
        property: csharp_public_member_name(&field.name),
        source_name: field.name.clone(),
        local_name,
        type_name: property_type,
        assignment_target: backing_field_name(
            &csharp_public_member_name(&field.name),
            &field.ty_ref,
            view,
        )
        .unwrap_or_else(|| csharp_public_member_name(&field.name)),
        read_expr,
        inline_read_expr,
        messagepack_read_expr,
        inline_messagepack_read_expr,
        is_required: missing_expr.is_none(),
        default_expr,
        missing_expr,
        requires_context: field_type_requires_context(&field.ty_ref, view)?,
        has_name: format!("has{}", csharp_public_member_name(&field.name)),
    })
}

pub(super) fn field_type_requires_context(
    ty: &CftSchemaTypeRef,
    view: &CsharpSchemaContext,
) -> Result<bool, CsharpCodegenError> {
    let mut visited = BTreeSet::new();
    field_type_requires_context_inner(ty, view, &mut visited)
}

fn field_type_requires_context_inner(
    ty: &CftSchemaTypeRef,
    view: &CsharpSchemaContext,
    visited: &mut BTreeSet<String>,
) -> Result<bool, CsharpCodegenError> {
    match ty {
        CftSchemaTypeRef::Ref(name) => Ok(view.is_ref_target_loadable(name)),
        CftSchemaTypeRef::Named(name) if view.is_schema_enum(name) => Ok(false),
        CftSchemaTypeRef::Named(name) => {
            if !visited.insert(name.clone()) {
                return Ok(false);
            }
            for concrete in view.concrete_assignable_types(name)? {
                let meta = view.type_meta(&concrete)?;
                for field in &meta.all_fields {
                    if field_type_requires_context_inner(&field.ty_ref, view, visited)? {
                        return Ok(true);
                    }
                }
            }
            Ok(false)
        }
        CftSchemaTypeRef::Array(inner) | CftSchemaTypeRef::Nullable(inner) => {
            field_type_requires_context_inner(inner, view, visited)
        }
        CftSchemaTypeRef::Dict(_, value) => field_type_requires_context_inner(value, view, visited),
        CftSchemaTypeRef::Int
        | CftSchemaTypeRef::Float
        | CftSchemaTypeRef::Bool
        | CftSchemaTypeRef::String => Ok(false),
    }
}

fn type_is_table(type_name: &str, view: &CsharpSchemaContext) -> bool {
    view.is_ref_target_loadable(type_name)
}
