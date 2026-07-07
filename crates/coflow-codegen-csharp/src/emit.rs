mod database;
mod identifiers;
mod readers;
mod types;

use crate::model::{
    CsharpConstructorAssignment, CsharpEnum, CsharpEnumVariant, CsharpEquality, CsharpLoadField,
    CsharpLoader, CsharpParameter, CsharpPolymorphicCase, CsharpProperty, CsharpType,
};
use crate::names::{camel_case, escape_csharp_string, has_annotation};
use crate::schema_view::{FieldMeta, FieldType, SchemaView, TypeMeta};
use crate::CsharpCodegenError;
use coflow_cft::CftEnumMeta;
use std::collections::{BTreeSet, HashSet};

pub use database::build_csharp_database;
use identifiers::{
    csharp_public_member_name, csharp_public_type_name, field_local_name,
    loader_reserved_local_names,
};
use readers::{
    read_field_expr, read_messagepack_expr, read_messagepack_field_expr, read_required_expr,
    read_token_expr,
};
use types::{
    collection_default_expr, csharp_field_property_type, csharp_property_type, csharp_type,
    default_value_expr,
};

pub fn build_csharp_enum(schema_enum: &CftEnumMeta) -> CsharpEnum {
    CsharpEnum {
        name: csharp_public_type_name(&schema_enum.name),
        is_flags: has_annotation(&schema_enum.annotations, "flag"),
        summary: None,
        obsolete: false,
        variants: schema_enum
            .all_variants
            .iter()
            .map(|variant| CsharpEnumVariant {
                name: csharp_public_member_name(&variant.name),
                value: variant.value,
                summary: None,
                obsolete: false,
            })
            .collect(),
    }
}

pub fn build_csharp_type(
    schema_type: &TypeMeta,
    view: &SchemaView,
) -> Result<CsharpType, CsharpCodegenError> {
    let ty = view.type_meta(&schema_type.name)?;
    let mut constructor_parameters = Vec::new();
    let mut base_constructor_args = Vec::new();
    let mut assignments = Vec::new();
    let mut properties = Vec::new();

    let is_table = !schema_type.is_abstract && type_is_table(&schema_type.name, view);
    if is_table {
        add_id_constructor_member(
            schema_type,
            view,
            &mut constructor_parameters,
            &mut base_constructor_args,
            &mut properties,
            &mut assignments,
        );
    }

    let own_field_names = schema_type
        .own_fields
        .iter()
        .map(|field| field.name.clone())
        .collect::<BTreeSet<_>>();

    for field in &ty.all_fields {
        let local_name = field_local_name(&field.name, &mut HashSet::new())?;
        let property_type = csharp_field_property_type(field, view);
        constructor_parameters.push(CsharpParameter {
            ty: property_type.clone(),
            name: local_name.clone(),
        });
        if !schema_type.is_struct
            && schema_type.parent.is_some()
            && !own_field_names.contains(&field.name)
        {
            base_constructor_args.push(local_name);
            continue;
        }

        add_field_constructor_member(
            field,
            property_type,
            local_name,
            view,
            &mut properties,
            &mut assignments,
        );
    }

    let loader = if schema_type.is_abstract {
        Some(polymorphic_loader(&schema_type.name, view)?)
    } else {
        Some(loader_method(&schema_type.name, view)?)
    };

    let equality = (!schema_type.is_abstract).then(|| {
        let all_field_props: Vec<String> = ty
            .all_fields
            .iter()
            .map(|f| csharp_public_member_name(&f.name))
            .collect();
        CsharpEquality {
            key_property: "Id".to_string(),
            is_struct: schema_type.is_struct,
            by_fields: !is_table,
            fields: all_field_props,
        }
    });

    Ok(CsharpType {
        name: view.csharp_type_name(&schema_type.name),
        declaration: type_declaration(schema_type, view),
        constructor_visibility: if schema_type.is_abstract {
            "protected".to_string()
        } else {
            "public".to_string()
        },
        summary: None,
        obsolete: false,
        properties,
        constructor_parameters,
        base_constructor_call: (!base_constructor_args.is_empty())
            .then(|| format!(" : base({})", base_constructor_args.join(", "))),
        base_constructor_args,
        assignments,
        loader,
        equality,
    })
}

fn add_id_constructor_member(
    schema_type: &TypeMeta,
    view: &SchemaView,
    constructor_parameters: &mut Vec<CsharpParameter>,
    base_constructor_args: &mut Vec<String>,
    properties: &mut Vec<CsharpProperty>,
    assignments: &mut Vec<CsharpConstructorAssignment>,
) {
    let key_ty = view.key_field_type(&schema_type.name);
    constructor_parameters.push(CsharpParameter {
        ty: csharp_type(&key_ty, view),
        name: "id".to_string(),
    });
    if has_concrete_parent(&schema_type.name, view) {
        base_constructor_args.push("id".to_string());
        return;
    }
    properties.push(CsharpProperty {
        visibility: "public".to_string(),
        name: "Id".to_string(),
        type_name: csharp_type(&key_ty, view),
        backing_field: None,
        summary: None,
        obsolete: false,
    });
    assignments.push(CsharpConstructorAssignment {
        property: "Id".to_string(),
        target: "Id".to_string(),
        parameter: "id".to_string(),
    });
}

fn add_field_constructor_member(
    field: &FieldMeta,
    property_type: String,
    local_name: String,
    view: &SchemaView,
    properties: &mut Vec<CsharpProperty>,
    assignments: &mut Vec<CsharpConstructorAssignment>,
) {
    let property_name = csharp_public_member_name(&field.name);
    let backing_field = backing_field_name(&property_name, &field.ty, view);
    properties.push(CsharpProperty {
        visibility: "public".to_string(),
        name: property_name.clone(),
        type_name: property_type,
        backing_field: backing_field.clone(),
        summary: None,
        obsolete: false,
    });
    assignments.push(CsharpConstructorAssignment {
        target: backing_field.unwrap_or_else(|| property_name.clone()),
        property: property_name,
        parameter: local_name,
    });
}

fn type_is_table(type_name: &str, view: &SchemaView) -> bool {
    view.is_ref_target_loadable(type_name)
}

fn has_concrete_parent(type_name: &str, view: &SchemaView) -> bool {
    let mut parent = view
        .type_meta(type_name)
        .ok()
        .and_then(|ty| ty.parent.as_deref());
    while let Some(parent_name) = parent {
        let Ok(parent_ty) = view.type_meta(parent_name) else {
            return false;
        };
        if !parent_ty.is_abstract {
            return true;
        }
        parent = parent_ty.parent.as_deref();
    }
    false
}

fn loader_method(type_name: &str, view: &SchemaView) -> Result<CsharpLoader, CsharpCodegenError> {
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
        is_struct: ty.is_struct,
        requires_hydration,
        fields,
        polymorphic_cases,
        is_polymorphic: false,
        expected: String::new(),
    })
}

fn polymorphic_loader(
    type_name: &str,
    view: &SchemaView,
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
        is_struct: view.type_meta(type_name)?.is_struct,
        requires_hydration: false,
        fields: Vec::new(),
        polymorphic_cases: polymorphic_cases(type_name, view)?,
        is_polymorphic: true,
        expected: view.concrete_assignable_types(type_name)?.join(" | "),
    })
}

fn polymorphic_cases(
    type_name: &str,
    view: &SchemaView,
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
    field: &FieldMeta,
    owner_type_name: &str,
    owner_is_singleton: bool,
    used_local_names: &mut HashSet<String>,
    view: &SchemaView,
) -> Result<CsharpLoadField, CsharpCodegenError> {
    let local_name = field_local_name(&field.name, used_local_names)?;
    let default_expr = default_value_expr(field.default.as_ref(), &field.ty, view)?;
    let missing_expr = default_expr.as_ref().map_or_else(
        || {
            if field.ty.is_nullable() {
                None
            } else {
                collection_default_expr(field.ty.non_nullable(), view).ok()
            }
        },
        |default| Some(default.clone()),
    );
    let inner_property_type = csharp_property_type(&field.ty, view);
    let property_type = if field.is_dimensional {
        format!("Localized<{inner_property_type}>")
    } else {
        inner_property_type.clone()
    };
    let raw_read_expr = read_field_expr(field, "obj", "context", view, missing_expr.as_deref())?;
    let raw_msgpack_expr = read_messagepack_field_expr(field, "reader", "context", view)?;
    let (read_expr, messagepack_read_expr, inline_read_expr, inline_messagepack_read_expr) =
        if field.is_dimensional {
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
            &field.ty,
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
        requires_context: field_type_requires_context(&field.ty, view)?,
        has_name: format!("has{}", csharp_public_member_name(&field.name)),
    })
}

fn field_type_requires_context(
    ty: &FieldType,
    view: &SchemaView,
) -> Result<bool, CsharpCodegenError> {
    let mut visited = BTreeSet::new();
    field_type_requires_context_inner(ty, view, &mut visited)
}

fn field_type_requires_context_inner(
    ty: &FieldType,
    view: &SchemaView,
    visited: &mut BTreeSet<String>,
) -> Result<bool, CsharpCodegenError> {
    match ty {
        FieldType::Ref(name) => Ok(view.is_ref_target_loadable(name)),
        FieldType::Type(name) => {
            if !visited.insert(name.clone()) {
                return Ok(false);
            }
            for concrete in view.concrete_assignable_types(name)? {
                let meta = view.type_meta(&concrete)?;
                for field in &meta.all_fields {
                    if field_type_requires_context_inner(&field.ty, view, visited)? {
                        return Ok(true);
                    }
                }
            }
            Ok(false)
        }
        FieldType::Array(inner) | FieldType::Nullable(inner) => {
            field_type_requires_context_inner(inner, view, visited)
        }
        FieldType::Dict(_, value) => field_type_requires_context_inner(value, view, visited),
        FieldType::Int
        | FieldType::Float
        | FieldType::Bool
        | FieldType::String
        | FieldType::Enum(_) => Ok(false),
    }
}

fn backing_field_name(property_name: &str, ty: &FieldType, view: &SchemaView) -> Option<String> {
    field_type_requires_context(ty, view)
        .ok()
        .filter(|requires_context| *requires_context)
        .map(|_| format!("_{}", camel_case(property_name)))
}

fn type_declaration(schema_type: &TypeMeta, view: &SchemaView) -> String {
    let prefix = if schema_type.is_abstract {
        "public abstract partial class"
    } else if schema_type.is_struct {
        "public partial struct"
    } else if schema_type.is_sealed || !view.type_has_descendants(&schema_type.name) {
        "public sealed partial class"
    } else {
        "public partial class"
    };

    let mut interfaces = Vec::new();
    if let Some(parent) = schema_type
        .parent
        .as_ref()
        .filter(|_| !schema_type.is_struct)
    {
        interfaces.push(view.csharp_type_name(parent));
    }
    if !schema_type.is_abstract {
        interfaces.push(format!(
            "IEquatable<{}>",
            view.csharp_type_name(&schema_type.name)
        ));
    }
    let suffix = if interfaces.is_empty() {
        String::new()
    } else {
        format!(" : {}", interfaces.join(", "))
    };

    format!(
        "{prefix} {}{suffix}",
        view.csharp_type_name(&schema_type.name)
    )
}
