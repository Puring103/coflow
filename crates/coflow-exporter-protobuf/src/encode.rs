use crate::contract::{Contract, Record};
use crate::ProtobufExportError;
use coflow_api::ArtifactFile;
use coflow_cft::{CftSchema, CftValueType};
use coflow_data_model::{CfdDataModel, CfdDictKey, CfdObject, CfdValue};

pub(crate) fn encode_tables(
    contract: &Contract,
    schema: &CftSchema,
    model: &CfdDataModel,
) -> Result<Vec<ArtifactFile>, ProtobufExportError> {
    let mut files = Vec::new();
    for record_contract in &contract.records {
        let mut table = Vec::new();
        for (_, record) in model.records_of_type(&record_contract.source_name) {
            let mut payload = Vec::new();
            if record_contract.has_id {
                write_string(1, record.key(), &mut payload);
            }
            encode_fields(
                contract,
                schema,
                record_contract,
                &record.object,
                &mut payload,
            )?;
            write_bytes(1, &payload, &mut table);
        }
        files.push(ArtifactFile::bytes(
            format!("{}.pb", record_contract.source_name),
            table,
        ));
    }
    Ok(files)
}

fn encode_fields(
    contract: &Contract,
    schema: &CftSchema,
    record: &Record,
    object: &CfdObject,
    out: &mut Vec<u8>,
) -> Result<(), ProtobufExportError> {
    for field in &record.fields {
        let value = object.field(&field.source_name).ok_or_else(|| {
            ProtobufExportError::new(format!(
                "{} is missing field `{}`",
                object.actual_type(),
                field.source_name
            ))
        })?;
        encode_field_value(
            contract,
            schema,
            &field.value_type,
            value,
            field.proto.number,
            out,
        )?;
    }
    Ok(())
}

fn encode_field_value(
    contract: &Contract,
    schema: &CftSchema,
    ty: &CftValueType,
    value: &CfdValue,
    tag: u32,
    out: &mut Vec<u8>,
) -> Result<(), ProtobufExportError> {
    if let CftValueType::Nullable(inner) = ty {
        if matches!(value, CfdValue::Null) {
            return Ok(());
        }
        if matches!(
            inner.as_ref(),
            CftValueType::Array(_) | CftValueType::Dict(_, _)
        ) {
            let mut wrapper = Vec::new();
            encode_field_value(contract, schema, inner, value, 1, &mut wrapper)?;
            write_bytes(tag, &wrapper, out);
            return Ok(());
        }
        return encode_field_value(contract, schema, inner, value, tag, out);
    }
    match (ty, value) {
        (CftValueType::Int | CftValueType::Enum(_), CfdValue::Int(value)) => {
            write_sint64(tag, *value, out);
        }
        (CftValueType::Enum(_), CfdValue::Enum(value)) => write_sint64(tag, value.value, out),
        (CftValueType::Float, CfdValue::Float(value)) => write_double(tag, *value, out),
        (CftValueType::Bool, CfdValue::Bool(value)) => {
            write_varint_field(tag, u64::from(*value), out)
        }
        (CftValueType::String, CfdValue::String(value)) => write_string(tag, value, out),
        (CftValueType::RecordRef(_), CfdValue::Ref(value)) => write_string(tag, value, out),
        (CftValueType::Object(declared), CfdValue::Object(object)) => {
            let actual = contract.record(object.actual_type()).ok_or_else(|| {
                ProtobufExportError::new(format!("unknown object type `{}`", object.actual_type()))
            })?;
            let mut payload = Vec::new();
            encode_fields(contract, schema, actual, object, &mut payload)?;
            if schema.range_is_polymorphic(declared) {
                let concrete = schema.concrete_assignable_types(declared).ok_or_else(|| {
                    ProtobufExportError::new(format!("unknown declared type `{declared}`"))
                })?;
                let index = concrete
                    .iter()
                    .position(|name| name.as_str() == object.actual_type())
                    .ok_or_else(|| {
                        ProtobufExportError::new(format!(
                            "type `{}` is not assignable to `{declared}`",
                            object.actual_type()
                        ))
                    })?;
                let mut wrapper = Vec::new();
                write_bytes(
                    u32::try_from(index).unwrap_or(u32::MAX).saturating_add(1),
                    &payload,
                    &mut wrapper,
                );
                write_bytes(tag, &wrapper, out);
            } else {
                write_bytes(tag, &payload, out);
            }
        }
        (CftValueType::Array(inner), CfdValue::Array(items)) => {
            for item in items {
                encode_repeated_value(contract, schema, inner, item, tag, out)?;
            }
        }
        (CftValueType::Dict(key_ty, value_ty), CfdValue::Dict(entries)) => {
            for (key, value) in entries {
                let mut entry = Vec::new();
                encode_dict_key(key_ty, key, 1, &mut entry)?;
                encode_repeated_value(contract, schema, value_ty, value, 2, &mut entry)?;
                write_bytes(tag, &entry, out);
            }
        }
        (_, CfdValue::Null) => {}
        _ => {
            return Err(ProtobufExportError::new(format!(
                "value does not match Protobuf field type `{}`",
                ty.display_label()
            )))
        }
    }
    Ok(())
}

fn encode_repeated_value(
    contract: &Contract,
    schema: &CftSchema,
    ty: &CftValueType,
    value: &CfdValue,
    tag: u32,
    out: &mut Vec<u8>,
) -> Result<(), ProtobufExportError> {
    match ty {
        CftValueType::Nullable(inner) => {
            let mut wrapper = Vec::new();
            if !matches!(value, CfdValue::Null) {
                encode_field_value(contract, schema, inner, value, 1, &mut wrapper)?;
            }
            write_bytes(tag, &wrapper, out);
            Ok(())
        }
        CftValueType::Array(_) | CftValueType::Dict(_, _) => {
            let mut wrapper = Vec::new();
            encode_field_value(contract, schema, ty, value, 1, &mut wrapper)?;
            write_bytes(tag, &wrapper, out);
            Ok(())
        }
        _ => encode_field_value(contract, schema, ty, value, tag, out),
    }
}

fn encode_dict_key(
    ty: &CftValueType,
    key: &CfdDictKey,
    tag: u32,
    out: &mut Vec<u8>,
) -> Result<(), ProtobufExportError> {
    match (ty.non_nullable(), key) {
        (CftValueType::String, CfdDictKey::String(value)) => write_string(tag, value, out),
        (CftValueType::Int, CfdDictKey::Int(value)) => write_sint64(tag, *value, out),
        (CftValueType::Enum(_), CfdDictKey::Enum(value)) => write_sint64(tag, value.value, out),
        _ => {
            return Err(ProtobufExportError::new(
                "dictionary key does not match its CFT type",
            ))
        }
    }
    Ok(())
}

fn write_sint64(tag: u32, value: i64, out: &mut Vec<u8>) {
    let zigzag = ((value << 1) ^ (value >> 63)) as u64;
    write_varint_field(tag, zigzag, out);
}

fn write_double(tag: u32, value: f64, out: &mut Vec<u8>) {
    write_key(tag, 1, out);
    out.extend_from_slice(&value.to_le_bytes());
}

fn write_string(tag: u32, value: &str, out: &mut Vec<u8>) {
    write_bytes(tag, value.as_bytes(), out);
}

fn write_bytes(tag: u32, value: &[u8], out: &mut Vec<u8>) {
    write_key(tag, 2, out);
    write_varint(value.len() as u64, out);
    out.extend_from_slice(value);
}

fn write_varint_field(tag: u32, value: u64, out: &mut Vec<u8>) {
    write_key(tag, 0, out);
    write_varint(value, out);
}

fn write_key(tag: u32, wire_type: u8, out: &mut Vec<u8>) {
    write_varint((u64::from(tag) << 3) | u64::from(wire_type), out);
}

fn write_varint(mut value: u64, out: &mut Vec<u8>) {
    while value >= 0x80 {
        out.push((value as u8 & 0x7f) | 0x80);
        value >>= 7;
    }
    out.push(value as u8);
}
