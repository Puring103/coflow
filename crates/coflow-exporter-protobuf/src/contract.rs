use crate::ProtobufExportError;
use coflow_cft::{CftSchema, CftValueType};
use std::collections::BTreeSet;

#[derive(Debug, Clone)]
pub(crate) struct Contract {
    pub(crate) records: Vec<Record>,
    pub(crate) helpers: Vec<Message>,
}

#[derive(Debug, Clone)]
pub(crate) struct Record {
    pub(crate) source_name: String,
    pub(crate) message_name: String,
    pub(crate) table_name: String,
    pub(crate) has_id: bool,
    pub(crate) fields: Vec<Field>,
}

#[derive(Debug, Clone)]
pub(crate) struct Message {
    pub(crate) name: String,
    pub(crate) fields: Vec<ProtoField>,
    pub(crate) oneof_name: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct Field {
    pub(crate) source_name: String,
    pub(crate) proto: ProtoField,
    pub(crate) value_type: CftValueType,
}

#[derive(Debug, Clone)]
pub(crate) struct ProtoField {
    pub(crate) name: String,
    pub(crate) number: u32,
    pub(crate) type_name: String,
    pub(crate) label: FieldLabel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FieldLabel {
    Singular,
    Optional,
    Repeated,
}

struct Builder<'a> {
    schema: &'a CftSchema,
    helpers: Vec<Message>,
    helper_names: BTreeSet<String>,
}

impl Contract {
    pub(crate) fn build(schema: &CftSchema) -> Result<Self, ProtobufExportError> {
        if schema.all_dimensions().next().is_some() {
            return Err(ProtobufExportError::new(
                "Protobuf export does not yet support localized dimension tables",
            ));
        }
        let mut builder = Builder {
            schema,
            helpers: Vec::new(),
            helper_names: BTreeSet::new(),
        };
        let mut records = Vec::new();
        for ty in schema.all_types().filter(|ty| !ty.is_abstract) {
            let message_name = pascal(ty.name.as_str());
            let mut fields = ty.all_fields().collect::<Vec<_>>();
            fields.sort_by(|left, right| left.name.cmp(&right.name));
            let fields = fields
                .into_iter()
                .enumerate()
                .map(|(index, field)| {
                    let number = u32::try_from(index)
                        .ok()
                        .and_then(|index| index.checked_add(16))
                        .ok_or_else(|| {
                            ProtobufExportError::new(format!(
                                "type `{}` has too many fields for Protobuf",
                                ty.name
                            ))
                        })?;
                    let context = format!("{}{}", message_name, pascal(field.name.as_str()));
                    let proto =
                        builder.field(&field.value_type, &context, field.name.as_str(), number)?;
                    Ok(Field {
                        source_name: field.name.to_string(),
                        proto,
                        value_type: field.value_type.clone(),
                    })
                })
                .collect::<Result<Vec<_>, ProtobufExportError>>()?;
            records.push(Record {
                source_name: ty.name.to_string(),
                table_name: format!("{message_name}Table"),
                message_name,
                has_id: !ty.is_struct,
                fields,
            });
        }
        Ok(Self {
            records,
            helpers: builder.helpers,
        })
    }

    pub(crate) fn record(&self, source_name: &str) -> Option<&Record> {
        self.records
            .iter()
            .find(|record| record.source_name == source_name)
    }
}

impl Builder<'_> {
    fn field(
        &mut self,
        value_type: &CftValueType,
        context: &str,
        source_name: &str,
        number: u32,
    ) -> Result<ProtoField, ProtobufExportError> {
        let (type_name, label) = match value_type {
            CftValueType::Nullable(inner) => {
                let type_name = self.singular_type(inner, &format!("{context}Value"))?;
                (type_name, FieldLabel::Optional)
            }
            CftValueType::Array(inner) => {
                let type_name = self.singular_type(inner, &format!("{context}Item"))?;
                (type_name, FieldLabel::Repeated)
            }
            CftValueType::Dict(key, value) => {
                let helper = format!("{context}Entry");
                let key_type = self.singular_type(key, &format!("{helper}Key"))?;
                let value_type = self.singular_type(value, &format!("{helper}Value"))?;
                self.add_helper(Message {
                    name: helper.clone(),
                    fields: vec![
                        ProtoField {
                            name: "key".to_string(),
                            number: 1,
                            type_name: key_type,
                            label: FieldLabel::Singular,
                        },
                        ProtoField {
                            name: "value".to_string(),
                            number: 2,
                            type_name: value_type,
                            label: FieldLabel::Singular,
                        },
                    ],
                    oneof_name: None,
                });
                (helper, FieldLabel::Repeated)
            }
            other => (self.singular_type(other, context)?, FieldLabel::Singular),
        };
        Ok(ProtoField {
            name: snake(source_name),
            number,
            type_name,
            label,
        })
    }

    fn singular_type(
        &mut self,
        value_type: &CftValueType,
        context: &str,
    ) -> Result<String, ProtobufExportError> {
        match value_type {
            CftValueType::Int | CftValueType::Enum(_) => Ok("sint64".to_string()),
            CftValueType::Float => Ok("double".to_string()),
            CftValueType::Bool => Ok("bool".to_string()),
            CftValueType::String | CftValueType::RecordRef(_) => Ok("string".to_string()),
            CftValueType::Object(name) => {
                if self.schema.range_is_polymorphic(name) {
                    let helper = format!("{}Value", pascal(name));
                    if !self.helper_names.contains(&helper) {
                        let concrete =
                            self.schema.concrete_assignable_types(name).ok_or_else(|| {
                                ProtobufExportError::new(format!("unknown type `{name}`"))
                            })?;
                        let fields = concrete
                            .iter()
                            .enumerate()
                            .map(|(index, actual)| ProtoField {
                                name: snake(actual),
                                number: u32::try_from(index).unwrap_or(u32::MAX).saturating_add(1),
                                type_name: pascal(actual),
                                label: FieldLabel::Singular,
                            })
                            .collect();
                        self.add_helper(Message {
                            name: helper.clone(),
                            fields,
                            oneof_name: Some("value".to_string()),
                        });
                    }
                    Ok(helper)
                } else {
                    Ok(pascal(name))
                }
            }
            CftValueType::Nullable(inner) => {
                let inner_type = self.singular_type(inner, &format!("{context}Value"))?;
                let helper = format!("{context}Nullable");
                self.add_helper(Message {
                    name: helper.clone(),
                    fields: vec![ProtoField {
                        name: "value".to_string(),
                        number: 1,
                        type_name: inner_type,
                        label: FieldLabel::Optional,
                    }],
                    oneof_name: None,
                });
                Ok(helper)
            }
            CftValueType::Array(inner) => {
                let inner_type = self.singular_type(inner, &format!("{context}Item"))?;
                let helper = format!("{context}Array");
                self.add_helper(Message {
                    name: helper.clone(),
                    fields: vec![ProtoField {
                        name: "items".to_string(),
                        number: 1,
                        type_name: inner_type,
                        label: FieldLabel::Repeated,
                    }],
                    oneof_name: None,
                });
                Ok(helper)
            }
            CftValueType::Dict(key, value) => {
                let key_type = self.singular_type(key, &format!("{context}Key"))?;
                let value_type = self.singular_type(value, &format!("{context}Value"))?;
                let entry = format!("{context}Entry");
                self.add_helper(Message {
                    name: entry.clone(),
                    fields: vec![
                        ProtoField {
                            name: "key".to_string(),
                            number: 1,
                            type_name: key_type,
                            label: FieldLabel::Singular,
                        },
                        ProtoField {
                            name: "value".to_string(),
                            number: 2,
                            type_name: value_type,
                            label: FieldLabel::Singular,
                        },
                    ],
                    oneof_name: None,
                });
                let helper = format!("{context}Dict");
                self.add_helper(Message {
                    name: helper.clone(),
                    fields: vec![ProtoField {
                        name: "entries".to_string(),
                        number: 1,
                        type_name: entry,
                        label: FieldLabel::Repeated,
                    }],
                    oneof_name: None,
                });
                Ok(helper)
            }
        }
    }

    fn add_helper(&mut self, message: Message) {
        if self.helper_names.insert(message.name.clone()) {
            self.helpers.push(message);
        }
    }
}

pub(crate) fn pascal(value: &str) -> String {
    let mut out = String::new();
    let mut upper = true;
    for character in value.chars() {
        if character == '_' || character == '-' || character == ' ' {
            upper = true;
        } else if upper {
            out.extend(character.to_uppercase());
            upper = false;
        } else {
            out.push(character);
        }
    }
    out
}

pub(crate) fn snake(value: &str) -> String {
    let mut out = String::new();
    for (index, character) in value.chars().enumerate() {
        if character.is_uppercase() && index > 0 {
            out.push('_');
        }
        if character == '-' || character == ' ' {
            out.push('_');
        } else {
            out.extend(character.to_lowercase());
        }
    }
    out
}
