use crate::schema_view::{FieldMeta, FieldType, SchemaView};
use crate::CsharpCodegenError;

pub(super) fn read_field_expr(
    field: &FieldMeta,
    obj: &str,
    context: &str,
    view: &SchemaView,
    missing_expr: Option<&str>,
) -> Result<String, CsharpCodegenError> {
    let name = &field.name;
    let reader = read_token_expr(field.ty.non_nullable(), "token", context, view)?;
    if field.ty.is_nullable() {
        return Ok(format!(
            "CoflowJson.ReadNullable({obj}, \"{name}\", (token) => {reader})"
        ));
    }
    if let Some(missing_expr) = missing_expr {
        return Ok(format!(
            "CoflowJson.ReadOptional({obj}, \"{name}\", (token) => {reader}, {missing_expr})"
        ));
    }
    Ok(read_required_expr(name, obj, &reader))
}

pub(super) fn read_required_expr(name: &str, obj: &str, reader: &str) -> String {
    format!("CoflowJson.ReadRequired({obj}, \"{name}\", (token) => {reader})")
}

pub(super) fn read_token_expr(
    ty: &FieldType,
    token: &str,
    context: &str,
    view: &SchemaView,
) -> Result<String, CsharpCodegenError> {
    match ty {
        FieldType::Int => Ok(format!("CoflowJson.ReadInt({token})")),
        FieldType::Float => Ok(format!("CoflowJson.ReadFloat({token})")),
        FieldType::Bool => Ok(format!("CoflowJson.ReadBool({token})")),
        FieldType::String => Ok(format!("CoflowJson.ReadString({token})")),
        FieldType::Enum(name) if view.is_id_as_enum(name) => Ok(format!(
            "CoflowJson.ReadStringEnum<{}>({token})",
            view.csharp_enum_name(name)
        )),
        FieldType::Enum(name) if view.is_schema_enum(name) => Ok(format!(
            "CoflowJson.ReadEnum<{}>({token})",
            view.csharp_enum_name(name)
        )),
        FieldType::Enum(name) => Ok(format!(
            "CoflowJson.ReadStringEnum<{}>({token})",
            view.csharp_enum_name(name)
        )),
        FieldType::Ref(name) => {
            let csharp_name = view.csharp_type_name(name);
            let key_reader = read_token_expr(&view.key_field_type(name), token, context, view)?;
            Ok(format!("{context}.Get{csharp_name}({key_reader})"))
        }
        FieldType::Type(name) => {
            let csharp_name = view.csharp_type_name(name);
            let inline_reader = if view.range_is_polymorphic(name) {
                format!("{csharp_name}.LoadPolymorphic({token}, {context})")
            } else {
                format!("{csharp_name}.LoadInline({token}, {context})")
            };
            Ok(inline_reader)
        }
        FieldType::Array(inner) => Ok(format!(
            "CoflowJson.ReadArray({token}, (item) => {})",
            read_token_expr(inner, "item", context, view)?
        )),
        FieldType::Dict(key, value) => Ok(format!(
            "CoflowJson.ReadDict({token}, (key) => {}, (value) => {})",
            read_dict_key_expr(key, "key", view)?,
            read_token_expr(value, "value", context, view)?
        )),
        FieldType::Nullable(inner) => Ok(format!(
            "{token}.Type == JTokenType.Null ? null : {}",
            read_token_expr(inner, token, context, view)?
        )),
    }
}

fn read_dict_key_expr(
    ty: &FieldType,
    key: &str,
    view: &SchemaView,
) -> Result<String, CsharpCodegenError> {
    match ty.non_nullable() {
        FieldType::String => Ok(key.to_string()),
        FieldType::Int => Ok(format!("CoflowJson.ReadIntKey({key})")),
        FieldType::Enum(name) => Ok(format!(
            "CoflowJson.ReadEnumKey<{}>({key})",
            view.csharp_enum_name(name)
        )),
        _ => Err(CsharpCodegenError::new(
            "dictionary key type must be string, int, or enum",
        )),
    }
}

pub(super) fn read_messagepack_field_expr(
    field: &FieldMeta,
    reader: &str,
    context: &str,
    view: &SchemaView,
) -> Result<String, CsharpCodegenError> {
    read_messagepack_expr(&field.ty, reader, context, view)
}

pub(super) fn read_messagepack_expr(
    ty: &FieldType,
    reader: &str,
    context: &str,
    view: &SchemaView,
) -> Result<String, CsharpCodegenError> {
    match ty {
        FieldType::Int => Ok(format!("CoflowMessagePack.ReadInt(ref {reader})")),
        FieldType::Float => Ok(format!("CoflowMessagePack.ReadFloat(ref {reader})")),
        FieldType::Bool => Ok(format!("CoflowMessagePack.ReadBool(ref {reader})")),
        FieldType::String => Ok(format!("CoflowMessagePack.ReadString(ref {reader})")),
        FieldType::Enum(name) if view.is_id_as_enum(name) => Ok(format!(
            "CoflowMessagePack.ReadStringEnum<{}>(ref {reader})",
            view.csharp_enum_name(name)
        )),
        FieldType::Enum(name) if view.is_schema_enum(name) => Ok(format!(
            "CoflowMessagePack.ReadEnum<{}>(ref {reader})",
            view.csharp_enum_name(name)
        )),
        FieldType::Enum(name) => Ok(format!(
            "CoflowMessagePack.ReadStringEnum<{}>(ref {reader})",
            view.csharp_enum_name(name)
        )),
        FieldType::Ref(name) => {
            let csharp_name = view.csharp_type_name(name);
            let key_reader =
                read_messagepack_expr(&view.key_field_type(name), reader, context, view)?;
            Ok(format!("{context}.Get{csharp_name}({key_reader})"))
        }
        FieldType::Type(name) => {
            let csharp_name = view.csharp_type_name(name);
            let inline_reader = if view.range_is_polymorphic(name) {
                format!("{csharp_name}.LoadPolymorphic(ref {reader}, {context})")
            } else {
                format!("{csharp_name}.LoadInline(ref {reader}, {context})")
            };
            Ok(inline_reader)
        }
        FieldType::Array(inner) => Ok(format!(
            "CoflowMessagePack.ReadArray(ref {reader}, {context}, static (ref MessagePackReader itemReader, CoflowTables.LoadContext context) => {})",
            read_messagepack_expr(inner, "itemReader", "context", view)?
        )),
        FieldType::Dict(key, value) => Ok(format!(
            "CoflowMessagePack.ReadDict(ref {reader}, {context}, static (key) => {}, static (ref MessagePackReader valueReader, CoflowTables.LoadContext context) => {})",
            read_messagepack_dict_key_expr(key, "key", view)?,
            read_messagepack_expr(value, "valueReader", "context", view)?
        )),
        FieldType::Nullable(inner) => Ok(format!(
            "CoflowMessagePack.ReadNil(ref {reader}) ? null : {}",
            read_messagepack_expr(inner, reader, context, view)?
        )),
    }
}

fn read_messagepack_dict_key_expr(
    ty: &FieldType,
    key: &str,
    view: &SchemaView,
) -> Result<String, CsharpCodegenError> {
    match ty.non_nullable() {
        FieldType::String => Ok(key.to_string()),
        FieldType::Int => Ok(format!("CoflowMessagePack.ReadIntKey({key})")),
        FieldType::Enum(name) => Ok(format!(
            "CoflowMessagePack.ReadEnumKey<{}>({key})",
            view.csharp_enum_name(name)
        )),
        _ => Err(CsharpCodegenError::new(
            "dictionary key type must be string, int, or enum",
        )),
    }
}
