//! enum 与数据投影声明。
use super::generate::file;
use super::names::{identifier, namespace, qualified, quoted, short};
use super::value_codecs::{append_materialization, codec, cs_type, decode, pack};
use super::{CsharpCodegenError, CsharpIdAsEnumVariant, GeneratedFile};
use coflow_core::schema::{CftSchema, CftValueType};
use std::collections::{BTreeMap, BTreeSet};
pub(super) fn render(
    schema: &CftSchema,
    ids: &BTreeMap<String, Vec<CsharpIdAsEnumVariant>>,
    root: &str,
    files: &mut Vec<GeneratedFile>,
) -> Result<(), CsharpCodegenError> {
    for en in schema.all_enums() {
        let enum_name = identifier(short(&en.name));
        let mut body = format!(
            "using System;\n\nnamespace {}\n{{\n{}public enum {} : {}\n{{\n",
            namespace(root, &en.name),
            if en.is_flag { "[Flags]\n" } else { "" },
            enum_name,
            if en.is_flag { "uint" } else { "int" }
        );
        if let Some(variants) = ids.get(en.name.as_str()) {
            for v in variants {
                body.push_str(&format!("    {} = {},\n", identifier(&v.name), v.value));
            }
        } else {
            for v in &en.variants {
                body.push_str(&format!("    {} = {},\n", identifier(&v.name), v.value));
            }
        }
        body.push_str("}\n}\n");
        files.push(file(&en.name, body));
    }
    for ty in schema.all_types() {
        let type_name = identifier(short(&ty.name));
        let base = ty
            .parent
            .as_ref()
            .map_or_else(|| "CoflowObject".into(), |parent| qualified(root, parent));
        let mut body = format!(
            "#nullable enable\nusing System;\nusing Coflow;\n\nnamespace {}\n{{\n",
            namespace(root, &ty.name)
        );
        if ty.is_struct {
            body.push_str(&format!(
                "public readonly struct {type_name} : ICoflowValue\n{{\n    private readonly Projection _value;\n"
            ));
        } else {
            body.push_str(&format!(
                "public {}class {type_name} : {base}\n{{\n",
                if ty.is_abstract {
                    "abstract "
                } else if ty.is_sealed || ty.is_singleton {
                    "sealed "
                } else {
                    ""
                }
            ));
        }

        let mut assignments = Vec::new();
        let mut decoders = Vec::new();
        let mut host_codecs = Vec::new();
        if ty.kind != coflow_language::cft::syntax::ast::TypeKind::Data && ty.parent.is_none() {
            if ty.is_host {
                body.push_str("    public string Id => Read(\"id\", __Codecs.Id);\n");
                host_codecs.push("        internal static readonly Func<Projection, string> Id = ValueCodecs.String;\n".to_string());
            } else {
                body.push_str("    public string Id { get; private set; } = default!;\n");
                assignments
                    .push("        Id = ValueCodecs.String(record.Field(\"id\"));\n".to_string());
            }
        }

        for field in ty.own_fields() {
            let field_name = identifier(&field.name);
            let member_name = if matches!(field.value_type, CftValueType::Function(..))
                && field.dimension.is_none()
            {
                format!("{field_name}Function")
            } else {
                field_name.clone()
            };
            let property_type = if field.dimension.is_some() {
                format!("CoflowDimension<{}>", cs_type(&field.value_type, root)?)
            } else {
                cs_type(&field.value_type, root)?
            };
            let reader = if field.dimension.is_some() {
                format!(
                    "value => new {property_type}(value, {})",
                    codec(schema, &field.value_type, root, 1)?
                )
            } else {
                codec(schema, &field.value_type, root, 0)?
            };
            let value = if field.dimension.is_some() {
                format!(
                    "new {property_type}(__VALUE__, {})",
                    codec(schema, &field.value_type, root, 0)?
                )
            } else {
                decode(schema, &field.value_type, root, "__VALUE__")?
            };

            if ty.is_host {
                body.push_str(&format!(
                    "    public {property_type} {member_name} => Read({}, __Codecs.{field_name});\n",
                    quoted(&field.name)
                ));
                host_codecs.push(format!(
                    "        internal static readonly Func<Projection, {property_type}> {field_name} = {reader};\n"
                ));
            } else if ty.is_struct {
                body.push_str(&format!(
                    "    public {property_type} {member_name} {{ get; }}\n"
                ));
                append_materialization(
                    &mut assignments,
                    &mut decoders,
                    &member_name,
                    &field_name,
                    &property_type,
                    &reader,
                    &value,
                    &format!("projection.Field({})", quoted(&field.name)),
                );
            } else {
                body.push_str(&format!(
                    "    public {property_type} {member_name} {{ get; private set; }} = default!;\n"
                ));
                let projection = format!("record.Field({})", quoted(&field.name));
                let materialized = if field.dimension.is_some() {
                    format!(
                        "new {property_type}({projection}, {})",
                        codec(schema, &field.value_type, root, 0)?
                    )
                } else {
                    decode(schema, &field.value_type, root, &projection)?
                };
                assignments.push(format!("        {member_name} = {materialized};\n"));
            }

            if let CftValueType::Function(parameters, result) = &field.value_type {
                if field.dimension.is_none() {
                    let mut used = parameters
                        .iter()
                        .filter_map(|parameter| parameter.name.clone())
                        .collect::<BTreeSet<_>>();
                    let names = parameters
                        .iter()
                        .enumerate()
                        .map(|(index, parameter)| {
                            let raw = parameter.name.clone().unwrap_or_else(|| {
                                let mut candidate = format!("a{index}");
                                while used.contains(&candidate) {
                                    candidate.push('_');
                                }
                                used.insert(candidate.clone());
                                candidate
                            });
                            identifier(&raw)
                        })
                        .collect::<Vec<_>>();
                    let arguments = parameters
                        .iter()
                        .zip(&names)
                        .map(|(parameter, name)| {
                            Ok(format!("{} {name}", cs_type(&parameter.value_type, root)?))
                        })
                        .collect::<Result<Vec<_>, CsharpCodegenError>>()?;
                    body.push_str(&format!(
                        "    public {} {field_name}({}) => {member_name}.Invoke({});\n",
                        cs_type(result, root)?,
                        arguments.join(", "),
                        names.join(", ")
                    ));
                }
            }
            if matches!(field.value_type, CftValueType::FString) {
                if field.dimension.is_some() {
                    body.push_str(&format!(
                        "    public string Render{}(string? variant = null) => (variant == null ? {field_name}.Default() : {field_name}.For(variant)).Render();\n",
                        field.name
                    ));
                } else {
                    body.push_str(&format!(
                        "    public string Render{}() => {field_name}.Render();\n",
                        field.name
                    ));
                }
            }
        }

        if ty.is_struct {
            body.push_str(&format!(
                "\n    internal {type_name}(Projection projection)\n    {{\n        projection.RequireContract(global::{root}.Generated.ContractIdentity);\n        _value = projection;\n{}    }}\n\n    void ICoflowValue.Encode(ArgumentWriter writer) => writer.Write(_value);\n",
                assignments.join("")
            ));
        } else {
            body.push_str(&format!(
                "\n    {} {type_name}(Record record) : base(record)\n    {{\n{}    }}\n",
                if ty.is_abstract {
                    "protected"
                } else {
                    "internal"
                },
                assignments.join("")
            ));
        }

        if ty.kind == coflow_language::cft::syntax::ast::TypeKind::Data && !ty.is_abstract {
            let fields = ty.all_fields().collect::<Vec<_>>();
            let parameters = fields
                .iter()
                .map(|field| {
                    Ok(format!(
                        "{} {}",
                        cs_type(&field.value_type, root)?,
                        identifier(&field.name)
                    ))
                })
                .collect::<Result<Vec<_>, CsharpCodegenError>>()?;
            let names = fields
                .iter()
                .map(|field| quoted(&field.name))
                .collect::<Vec<_>>();
            let values = fields
                .iter()
                .map(|field| pack(&field.value_type, &identifier(&field.name)))
                .collect::<Vec<_>>();
            let detached = format!(
                "Projection.Data(global::{root}.Generated.ContractIdentity, {}, new[] {{ {} }}, new[] {{ {} }})",
                quoted(&ty.name),
                names.join(", "),
                values.join(", ")
            );
            body.push_str(&format!(
                "\n    public {type_name}({}) : this({}) {{ }}\n",
                parameters.join(", "),
                if ty.is_struct {
                    detached
                } else {
                    format!("new Record({detached})")
                }
            ));
        }

        if !decoders.is_empty() {
            body.push('\n');
            for decoder in decoders {
                body.push_str(&decoder);
            }
        }

        if !host_codecs.is_empty() {
            body.push_str("\n    private static class __Codecs\n    {\n");
            for codec in host_codecs {
                body.push_str(&codec);
            }
            body.push_str("    }\n");
        }

        body.push_str("}\n");
        body.push_str("}\n");
        files.push(file(&ty.name, body));
    }
    Ok(())
}
