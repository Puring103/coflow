//! 名称校验、契约与代码产物组装。
use super::names::{identifier, qualified, quoted, short};
use super::{CsharpCodegenError, CsharpIdAsEnumVariant, GeneratedFile};
use coflow_core::schema::{CftSchema, CftValueType};
use std::collections::{BTreeMap, BTreeSet};
pub(super) fn generate(
    schema: &CftSchema,
    ids: &BTreeMap<String, Vec<CsharpIdAsEnumVariant>>,
    root: &str,
) -> Result<(Vec<GeneratedFile>, Vec<u8>), CsharpCodegenError> {
    if root.is_empty() || !root.split('.').all(coflow_language::lexical::is_identifier) {
        return Err(CsharpCodegenError::new("invalid C# namespace"));
    }
    let escaped_root = root
        .split('.')
        .map(identifier)
        .collect::<Vec<_>>()
        .join(".");
    let root = escaped_root.as_str();
    // C# 的类型和命名空间共享声明空间，元数据、enum 和 Host 接口也参与冲突检查。
    let mut symbols =
        BTreeSet::from(["Generated".to_string(), "GeneratedHostBindings".to_string()]);
    let mut names = schema
        .all_types()
        .map(|ty| ty.name.to_string())
        .chain(schema.all_enums().map(|ty| ty.name.to_string()))
        .collect::<Vec<_>>();
    names.extend(schema.all_types().filter(|ty| ty.is_host).map(|ty| {
        let interface = format!("I{}", short(&ty.name));
        ty.name
            .rsplit_once("::")
            .map_or_else(|| interface.clone(), |(ns, _)| format!("{ns}::{interface}"))
    }));
    for name in &names {
        if !symbols.insert(name.clone()) {
            return Err(CsharpCodegenError::new(format!(
                "{name} conflicts with a generated C# type"
            )));
        }
    }
    for name in &names {
        let mut parent = name.as_str();
        while let Some((namespace, _)) = parent.rsplit_once("::") {
            if symbols.contains(namespace) {
                return Err(CsharpCodegenError::new(format!(
                    "{namespace} is both a C# type and namespace"
                )));
            }
            parent = namespace;
        }
    }
    for ty in schema.all_types() {
        if ["Generated", "GeneratedHostBindings"].contains(&ty.name.as_str()) {
            return Err(CsharpCodegenError::new(
                "type conflicts with generated contract metadata",
            ));
        }
        let mut reserved: BTreeSet<String> = [
            "Value",
            "Read",
            "ActualType",
            "Dispose",
            "Projection",
            "__CoflowCodecs",
            "ValueEquals",
            "Wrap",
        ]
        .into_iter()
        .map(str::to_string)
        .collect();
        reserved.insert(short(&ty.name).to_string());
        if ty.kind != coflow_language::cft::syntax::ast::TypeKind::Data {
            reserved.insert("Id".into());
        }
        for field in ty
            .all_fields()
            .filter(|field| matches!(field.value_type, CftValueType::FString))
        {
            reserved.insert(format!("Render{}", field.name));
        }
        for field in ty.all_fields().filter(|field| {
            matches!(field.value_type, CftValueType::Function(..)) && field.dimension.is_none()
        }) {
            reserved.insert(format!("{}Function", field.name));
        }
        for field in ty.all_fields() {
            if reserved.contains(field.name.as_str()) {
                return Err(CsharpCodegenError::new(format!(
                    "{}.{} conflicts with generated C# members",
                    ty.name, field.name
                )));
            }
        }
    }
    let mut files = Vec::new();
    super::declarations::render(schema, ids, root, &mut files)?;
    let contract = coflow_core::contract::Contract::new(schema.clone())
        .map_err(|e| CsharpCodegenError::new(e.to_string()))?;
    let bytes = contract
        .to_bytes()
        .map_err(|e| CsharpCodegenError::new(e.to_string()))?;
    let identity = contract
        .identity()
        .iter()
        .map(u8::to_string)
        .collect::<Vec<_>>()
        .join(",");
    let bindings = schema
        .all_types()
        .map(|ty| {
            let factory = if ty.is_struct {
                format!("value => new {}(value)", qualified(root, &ty.name))
            } else {
                format!(
                    "value => new {}(new Record(value))",
                    qualified(root, &ty.name)
                )
            };
            format!(
                "        new TypeBinding<{}>({}, {factory})",
                qualified(root, &ty.name),
                quoted(&ty.name)
            )
        })
        .collect::<Vec<_>>()
        .join(",\n");
    files.push(GeneratedFile {
        relative_path: "Coflow.Bindings.cs".into(),
        contents: format!(
            "using Coflow;\n\nnamespace {root}\n{{\npublic static class Generated\n{{\n    internal static byte[] ContractIdentity {{ get; }} = new byte[] {{ {identity} }};\n\n    private static TypeBinding[] Bindings {{ get; }} = new TypeBinding[]\n    {{\n{bindings}\n    }};\n\n    public static Contract LoadContract(byte[] bytes) => new Contract(bytes, ContractIdentity, Bindings);\n}}\n}}\n"
        ),
    });
    super::hosts::render(schema, root, &mut files)?;
    Ok((files, bytes))
}

pub(super) fn file(source: &str, contents: String) -> GeneratedFile {
    GeneratedFile {
        relative_path: format!("{}.cs", source.replace("::", "/")).into(),
        contents,
    }
}
