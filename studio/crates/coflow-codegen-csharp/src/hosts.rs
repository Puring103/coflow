//! Host 接口和回调绑定。
use super::generate::file;
use super::names::{identifier, namespace, qualified, quoted, short};
use super::value_codecs::{cs_type, invocation_codec};
use super::{CsharpCodegenError, GeneratedFile};
use coflow_core::schema::{CftSchema, CftValueType};
pub(super) fn render(
    schema: &CftSchema,
    root: &str,
    files: &mut Vec<GeneratedFile>,
) -> Result<(), CsharpCodegenError> {
    let mut hosts = format!("#nullable enable\nusing System;\nusing Coflow;\n\nnamespace {root}\n{{\npublic static class GeneratedHostBindings\n{{\n");
    for ty in schema.all_types().filter(|ty| ty.is_host) {
        let interface_name = format!("I{}", short(&ty.name));
        let source_interface = ty.name.rsplit_once("::").map_or_else(
            || interface_name.clone(),
            |(ns, _)| format!("{ns}::{interface_name}"),
        );
        if schema.resolve_type(&source_interface).is_some()
            || schema.resolve_enum(&source_interface).is_some()
        {
            return Err(CsharpCodegenError::new(
                "Host interface name conflicts with a declaration",
            ));
        }
        let interface_type = qualified(root, &source_interface);
        let short_name = short(&ty.name);
        let binding_name = if schema
            .all_types()
            .filter(|candidate| candidate.is_host && short(&candidate.name) == short_name)
            .count()
            == 1
        {
            format!("{}Binding", identifier(short_name))
        } else {
            format!("{}Binding", identifier(&ty.name.replace("::", "_")))
        };
        let mut interface = format!(
            "#nullable enable\nusing Coflow;\n\nnamespace {}\n{{\npublic interface {}\n{{\n",
            namespace(root, &ty.name),
            identifier(&interface_name)
        );
        for field in ty.all_fields() {
            if let CftValueType::Function(parameters, result) = &field.value_type {
                let arguments = parameters
                    .iter()
                    .enumerate()
                    .map(|(parameter_index, parameter)| {
                        Ok(format!(
                            "{} {}",
                            cs_type(&parameter.value_type, root)?,
                            identifier(
                                parameter
                                    .name
                                    .as_deref()
                                    .unwrap_or(&format!("arg{parameter_index}"))
                            )
                        ))
                    })
                    .collect::<Result<Vec<_>, CsharpCodegenError>>()?;
                interface.push_str(&format!(
                    "    {} {}({});\n",
                    cs_type(result, root)?,
                    identifier(&field.name),
                    arguments.join(", ")
                ));
            } else {
                interface.push_str(&format!(
                    "    {} {} {{ get; }}\n",
                    cs_type(&field.value_type, root)?,
                    identifier(&field.name)
                ));
            }
        }
        interface.push_str("}\n}\n");
        files.push(file(&source_interface, interface));
        hosts.push_str(&format!("    public static RuntimeBuilder BindHost(this RuntimeBuilder builder, {interface_type} host)\n        => builder.BindHost(new {binding_name}(host));\n\n    private sealed class {binding_name} : HostBinding\n    {{\n        private readonly {interface_type} host;\n\n        public {binding_name}({interface_type} host) : base({})\n        {{\n            this.host = host ?? throw new ArgumentNullException(nameof(host));\n        }}\n\n        public override object? Read(string field) => field switch\n        {{\n", quoted(&ty.name)));
        for field in ty
            .all_fields()
            .filter(|field| !matches!(field.value_type, CftValueType::Function(..)))
        {
            let value = match &field.value_type {
                CftValueType::Enum(name) => {
                    format!(
                        "new HostEnum({}, (uint)host.{})",
                        quoted(name),
                        identifier(&field.name)
                    )
                }
                CftValueType::Option(inner) if matches!(inner.as_ref(), CftValueType::Enum(_)) => {
                    let CftValueType::Enum(name) = inner.as_ref() else {
                        return Err(CsharpCodegenError::new("invalid optional enum"));
                    };
                    format!(
                        "host.{0} is {{ }} __{1} ? (object)new HostEnum({2}, (uint)__{1}) : null",
                        identifier(&field.name),
                        field.name,
                        quoted(name)
                    )
                }
                _ => format!("host.{}", identifier(&field.name)),
            };
            hosts.push_str(&format!(
                "            {} => {value},\n",
                quoted(&field.name)
            ));
        }
        hosts.push_str("            _ => throw new CoflowException(\"Host function members cannot be read as data.\"),\n        };\n\n        public override void Call(string field, HostCall call)\n        {\n            switch (field)\n            {\n");
        for field in ty
            .all_fields()
            .filter(|field| matches!(field.value_type, CftValueType::Function(..)))
        {
            let CftValueType::Function(parameters, result) = &field.value_type else {
                unreachable!();
            };
            let arguments = parameters
                .iter()
                .map(|parameter| {
                    Ok(format!(
                        "call.Argument({})",
                        invocation_codec(schema, &parameter.value_type, root, 0)?
                    ))
                })
                .collect::<Result<Vec<_>, CsharpCodegenError>>()?;
            hosts.push_str(&format!(
                "                case {}:\n                    call.Return({}, host.{}({}));\n                    return;\n",
                quoted(&field.name),
                invocation_codec(schema, result, root, 0)?,
                identifier(&field.name),
                arguments.join(", ")
            ));
        }
        hosts.push_str("                default: throw new CoflowException(\"Unknown Host function.\");\n            }\n        }\n    }\n\n");
    }
    hosts.push_str("}\n}\n");
    files.push(GeneratedFile {
        relative_path: "Coflow.Host.cs".into(),
        contents: hosts,
    });
    Ok(())
}
