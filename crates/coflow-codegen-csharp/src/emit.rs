mod database;
mod identifiers;
mod loaders;
mod readers;
mod types;

use crate::lowering::CsharpLoweringPlan;
use crate::model::{
    CsharpConstructorAssignment, CsharpEnum, CsharpEnumVariant, CsharpEquality, CsharpParameter,
    CsharpProperty, CsharpType,
};
use crate::names::camel_case;
use crate::CsharpCodegenError;
use coflow_cft::{CftEnum, CftField, CftSchemaTypeRef, CftType};
use std::collections::{BTreeSet, HashSet};

pub use database::build_csharp_database;
use identifiers::{csharp_public_member_name, csharp_public_type_name, field_local_name};
use loaders::{field_type_requires_context, loader_method, polymorphic_loader};
use types::{csharp_field_property_type, csharp_type};

pub fn build_csharp_enum(schema_enum: &CftEnum) -> CsharpEnum {
    CsharpEnum {
        name: csharp_public_type_name(&schema_enum.name),
        is_flags: schema_enum.is_flag,
        summary: None,
        obsolete: false,
        variants: schema_enum
            .variants
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
    schema_type: &CftType,
    view: &CsharpLoweringPlan<'_>,
) -> Result<CsharpType, CsharpCodegenError> {
    let ty = view.resolve_type(&schema_type.name)?;
    let mut constructor_parameters = Vec::new();
    let mut base_constructor_args = Vec::new();
    let mut assignments = Vec::new();
    let mut properties = Vec::new();

    let is_struct = schema_type.is_struct;
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
        .own_fields()
        .map(|field| field.name.clone())
        .collect::<BTreeSet<_>>();

    for field in view.fields(&ty.name)? {
        let local_name = field_local_name(&field.name, &mut HashSet::new())?;
        let property_type = csharp_field_property_type(field, view);
        constructor_parameters.push(CsharpParameter {
            ty: property_type.clone(),
            name: local_name.clone(),
        });
        if !is_struct && schema_type.parent.is_some() && !own_field_names.contains(&field.name) {
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

    let all_field_props = view
        .fields(&ty.name)?
        .map(|f| csharp_public_member_name(&f.name))
        .collect::<Vec<_>>();
    let equality = (!schema_type.is_abstract).then_some({
        CsharpEquality {
            key_property: "Id".to_string(),
            is_struct,
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
    schema_type: &CftType,
    view: &CsharpLoweringPlan<'_>,
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
    field: &CftField,
    property_type: String,
    local_name: String,
    view: &CsharpLoweringPlan<'_>,
    properties: &mut Vec<CsharpProperty>,
    assignments: &mut Vec<CsharpConstructorAssignment>,
) {
    let property_name = csharp_public_member_name(&field.name);
    let backing_field = backing_field_name(&property_name, &field.ty_ref, view);
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

fn type_is_table(type_name: &str, view: &CsharpLoweringPlan<'_>) -> bool {
    view.is_ref_target_loadable(type_name)
}

fn has_concrete_parent(type_name: &str, view: &CsharpLoweringPlan<'_>) -> bool {
    let mut parent = view
        .resolve_type(type_name)
        .ok()
        .and_then(|ty| ty.parent.as_deref());
    while let Some(parent_name) = parent {
        let Ok(parent_ty) = view.resolve_type(parent_name) else {
            return false;
        };
        if !parent_ty.is_abstract {
            return true;
        }
        parent = parent_ty.parent.as_deref();
    }
    false
}

pub(super) fn backing_field_name(
    property_name: &str,
    ty: &CftSchemaTypeRef,
    view: &CsharpLoweringPlan<'_>,
) -> Option<String> {
    field_type_requires_context(ty, view)
        .ok()
        .filter(|requires_context| *requires_context)
        .map(|_| format!("_{}", camel_case(property_name)))
}

fn type_declaration(schema_type: &CftType, view: &CsharpLoweringPlan<'_>) -> String {
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
