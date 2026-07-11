use crate::names::csharp_type_name;
use crate::CsharpCodegenError;
use coflow_cft::{CftEnumMeta, CftFieldMeta, CftSchemaTypeRef, CftTypeMeta, CompiledSchema};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug)]
pub struct CsharpLoweringPlan<'a> {
    pub int_32: bool,
    pub float_32: bool,
    schema: &'a CompiledSchema,
    types: Vec<&'a CftTypeMeta>,
    enums: Vec<&'a CftEnumMeta>,
    fields: BTreeMap<&'a str, &'a [CftFieldMeta]>,
    csharp_types: BTreeMap<String, String>,
    csharp_enums: BTreeMap<String, String>,
    declared_tables: Vec<String>,
    loadable_tables: Vec<String>,
    loadable_table_set: BTreeSet<String>,
    singleton_types: Vec<String>,
    polymorphic_types: Vec<String>,
    polymorphic_type_set: BTreeSet<String>,
    ref_targets: Vec<String>,
    schema_enums: BTreeSet<String>,
    id_as_enum_names: BTreeSet<String>,
    type_id_as_enum: BTreeMap<String, String>,
    assignable_types: BTreeMap<String, Vec<String>>,
    struct_types: BTreeSet<String>,
    types_with_descendants: BTreeSet<String>,
    messagepack_table_order: Result<Vec<String>, String>,
    uses_localization: bool,
}

impl<'a> CsharpLoweringPlan<'a> {
    pub fn lower(
        schema: &'a CompiledSchema,
        int_32: bool,
        float_32: bool,
        non_empty_tables: Option<&BTreeSet<String>>,
    ) -> Result<Self, CsharpCodegenError> {
        let enums = schema.enum_metas().collect::<Vec<_>>();
        let schema_enums = enums
            .iter()
            .map(|schema_enum| schema_enum.name.clone())
            .collect::<BTreeSet<_>>();
        let csharp_enums = enums
            .iter()
            .map(|schema_enum| {
                (
                    schema_enum.name.clone(),
                    csharp_type_name(&schema_enum.name),
                )
            })
            .collect::<BTreeMap<_, _>>();

        let types = schema.type_metas().collect::<Vec<_>>();
        let mut fields = BTreeMap::new();
        let mut csharp_types = BTreeMap::new();
        let mut declared_tables = Vec::new();
        let mut singleton_types = Vec::new();
        let mut polymorphic_types = Vec::new();
        let mut polymorphic_type_set = BTreeSet::new();
        let mut ref_targets = BTreeSet::new();
        let mut id_as_enum_names = BTreeSet::new();
        let mut type_id_as_enum = BTreeMap::new();
        let mut assignable_types = BTreeMap::new();
        let mut struct_types = BTreeSet::new();
        let mut types_with_descendants = BTreeSet::new();
        let mut uses_localization = false;

        for ty in &types {
            let type_fields = schema.fields_slice(&ty.name).ok_or_else(|| {
                CsharpCodegenError::new(format!("unknown CFT type `{}`", ty.name))
            })?;
            fields.insert(ty.name.as_str(), type_fields);
            csharp_types.insert(ty.name.clone(), csharp_type_name(&ty.name));
            if !ty.is_abstract && !ty.is_singleton {
                declared_tables.push(ty.name.clone());
            }
            if ty.is_singleton {
                singleton_types.push(ty.name.clone());
            }
            if schema.range_is_polymorphic(&ty.name) {
                polymorphic_types.push(ty.name.clone());
                polymorphic_type_set.insert(ty.name.clone());
            }
            if schema.type_is_struct(&ty.name) {
                struct_types.insert(ty.name.clone());
            }
            if let Some(parent) = &ty.parent {
                types_with_descendants.insert(parent.clone());
            }
            if let Some(enum_name) = schema.inherited_id_as_enum(&ty.name) {
                id_as_enum_names.insert(enum_name.clone());
                type_id_as_enum.insert(ty.name.clone(), enum_name);
            }
            let assignable = schema.concrete_assignable_types(&ty.name).ok_or_else(|| {
                CsharpCodegenError::new(format!("unknown CFT type `{}`", ty.name))
            })?;
            assignable_types.insert(ty.name.clone(), assignable);
            for field in &ty.own_fields {
                uses_localization |= field.dimension.is_some();
                collect_ref_targets(&field.ty_ref, &mut ref_targets);
            }
        }

        let loadable_tables = declared_tables
            .iter()
            .filter(|name| non_empty_tables.is_none_or(|tables| tables.contains(*name)))
            .cloned()
            .collect::<Vec<_>>();
        let loadable_table_set = loadable_tables.iter().cloned().collect();
        let ref_targets = ref_targets.into_iter().collect::<Vec<_>>();
        let mut plan = Self {
            int_32,
            float_32,
            schema,
            types,
            enums,
            fields,
            csharp_types,
            csharp_enums,
            declared_tables,
            loadable_tables,
            loadable_table_set,
            singleton_types,
            polymorphic_types,
            polymorphic_type_set,
            ref_targets,
            schema_enums,
            id_as_enum_names,
            type_id_as_enum,
            assignable_types,
            struct_types,
            types_with_descendants,
            messagepack_table_order: Ok(Vec::new()),
            uses_localization,
        };
        plan.messagepack_table_order = plan.compile_messagepack_table_order();
        Ok(plan)
    }

    pub fn cft_enum_meta(&self, name: &str) -> Option<&CftEnumMeta> {
        self.schema.enum_meta(name)
    }

    pub fn enum_names(&self) -> impl Iterator<Item = &String> {
        self.enums.iter().map(|schema_enum| &schema_enum.name)
    }

    pub fn cft_enum_metas(&self) -> impl Iterator<Item = &CftEnumMeta> {
        self.enums.iter().copied()
    }

    pub fn type_metas(&self) -> impl Iterator<Item = &CftTypeMeta> {
        self.types.iter().copied()
    }

    pub fn is_schema_enum(&self, name: &str) -> bool {
        self.schema_enums.contains(name)
    }

    pub const fn uses_localization(&self) -> bool {
        self.uses_localization
    }

    pub const fn id_as_enum_names(&self) -> &BTreeSet<String> {
        &self.id_as_enum_names
    }

    pub fn is_ref_target_loadable(&self, name: &str) -> bool {
        self.loadable_table_set.contains(name)
            || self
                .assignable_types
                .get(name)
                .is_some_and(|types| types.iter().any(|ty| self.loadable_table_set.contains(ty)))
    }

    pub fn type_meta(&self, name: &str) -> Result<&CftTypeMeta, CsharpCodegenError> {
        self.schema
            .type_meta(name)
            .ok_or_else(|| CsharpCodegenError::new(format!("unknown CFT type `{name}`")))
    }

    pub fn fields(
        &self,
        name: &str,
    ) -> Result<impl Iterator<Item = &CftFieldMeta>, CsharpCodegenError> {
        self.fields
            .get(name)
            .copied()
            .map(<[CftFieldMeta]>::iter)
            .ok_or_else(|| CsharpCodegenError::new(format!("unknown CFT type `{name}`")))
    }

    pub fn all_type_names(&self) -> impl Iterator<Item = &String> {
        self.types.iter().map(|ty| &ty.name)
    }

    pub fn declared_table_names(&self) -> &[String] {
        &self.declared_tables
    }

    pub fn table_names(&self) -> &[String] {
        &self.loadable_tables
    }

    pub fn singleton_type_names(&self) -> &[String] {
        &self.singleton_types
    }

    pub fn polymorphic_type_names(&self) -> &[String] {
        &self.polymorphic_types
    }

    pub fn range_is_polymorphic(&self, type_name: &str) -> bool {
        self.polymorphic_type_set.contains(type_name)
    }

    pub fn csharp_type_name(&self, type_name: &str) -> String {
        self.csharp_types
            .get(type_name)
            .cloned()
            .unwrap_or_else(|| csharp_type_name(type_name))
    }

    pub fn csharp_enum_name(&self, enum_name: &str) -> String {
        self.csharp_enums
            .get(enum_name)
            .cloned()
            .unwrap_or_else(|| csharp_type_name(enum_name))
    }

    pub fn csharp_named_type(&self, name: &str) -> String {
        if self.is_schema_enum(name) {
            self.csharp_enum_name(name)
        } else {
            self.csharp_type_name(name)
        }
    }

    pub fn id_as_enum(&self, type_name: &str) -> Option<String> {
        self.type_id_as_enum.get(type_name).cloned()
    }

    pub fn is_id_as_enum(&self, enum_name: &str) -> bool {
        self.id_as_enum_names.contains(enum_name)
    }

    pub fn key_field_type(&self, type_name: &str) -> CftSchemaTypeRef {
        self.id_as_enum(type_name)
            .map_or_else(|| CftSchemaTypeRef::String, CftSchemaTypeRef::Named)
    }

    pub fn type_has_descendants(&self, type_name: &str) -> bool {
        self.types_with_descendants.contains(type_name)
    }

    pub fn concrete_assignable_types(
        &self,
        type_name: &str,
    ) -> Result<&[String], CsharpCodegenError> {
        self.assignable_types
            .get(type_name)
            .map(Vec::as_slice)
            .ok_or_else(|| CsharpCodegenError::new(format!("unknown CFT type `{type_name}`")))
    }

    pub fn ref_target_names(&self) -> &[String] {
        &self.ref_targets
    }

    pub fn type_is_struct(&self, ty: &CftTypeMeta) -> bool {
        self.struct_types.contains(&ty.name)
    }

    pub fn messagepack_table_order(&self) -> Result<&[String], CsharpCodegenError> {
        self.messagepack_table_order
            .as_deref()
            .map_err(|message| CsharpCodegenError::new(message.clone()))
    }

    fn compile_messagepack_table_order(&self) -> Result<Vec<String>, String> {
        let dependencies = self.table_dependencies()?;
        topological_order(&self.loadable_tables, &dependencies)
    }

    fn table_dependencies(&self) -> Result<BTreeMap<String, BTreeSet<String>>, String> {
        let mut dependencies = BTreeMap::new();
        for table in &self.loadable_tables {
            let mut table_dependencies = BTreeSet::new();
            let mut pending_types = vec![table.clone()];
            let mut visited_types = BTreeSet::new();
            while let Some(type_name) = pending_types.pop() {
                if !visited_types.insert(type_name.clone()) {
                    continue;
                }
                let fields = self
                    .fields
                    .get(type_name.as_str())
                    .copied()
                    .ok_or_else(|| format!("unknown CFT type `{type_name}`"))?;
                for field in fields {
                    self.collect_type_dependencies(
                        &field.ty_ref,
                        &mut table_dependencies,
                        &mut pending_types,
                    )?;
                }
            }
            dependencies.insert(table.clone(), table_dependencies);
        }
        Ok(dependencies)
    }

    fn collect_type_dependencies(
        &self,
        ty: &CftSchemaTypeRef,
        table_dependencies: &mut BTreeSet<String>,
        pending_types: &mut Vec<String>,
    ) -> Result<(), String> {
        match ty {
            CftSchemaTypeRef::Ref(name) => {
                let assignable = self
                    .assignable_types
                    .get(name)
                    .ok_or_else(|| format!("unknown CFT type `{name}`"))?;
                let mut hit_table = false;
                for concrete in assignable {
                    if self.loadable_table_set.contains(concrete) {
                        table_dependencies.insert(concrete.clone());
                        hit_table = true;
                    }
                }
                if !hit_table {
                    pending_types.extend(assignable.iter().cloned());
                }
            }
            CftSchemaTypeRef::Named(name) if self.schema_enums.contains(name) => {}
            CftSchemaTypeRef::Named(name) => pending_types.push(name.clone()),
            CftSchemaTypeRef::Array(inner) | CftSchemaTypeRef::Nullable(inner) => {
                self.collect_type_dependencies(inner, table_dependencies, pending_types)?;
            }
            CftSchemaTypeRef::Dict(_, value) => {
                self.collect_type_dependencies(value, table_dependencies, pending_types)?;
            }
            CftSchemaTypeRef::Int
            | CftSchemaTypeRef::Float
            | CftSchemaTypeRef::Bool
            | CftSchemaTypeRef::String => {}
        }
        Ok(())
    }
}

fn collect_ref_targets(ty: &CftSchemaTypeRef, out: &mut BTreeSet<String>) {
    match ty {
        CftSchemaTypeRef::Ref(name) => {
            out.insert(name.clone());
        }
        CftSchemaTypeRef::Array(inner) | CftSchemaTypeRef::Nullable(inner) => {
            collect_ref_targets(inner, out);
        }
        CftSchemaTypeRef::Dict(key, value) => {
            collect_ref_targets(key, out);
            collect_ref_targets(value, out);
        }
        CftSchemaTypeRef::Int
        | CftSchemaTypeRef::Float
        | CftSchemaTypeRef::Bool
        | CftSchemaTypeRef::String
        | CftSchemaTypeRef::Named(_) => {}
    }
}

fn topological_order(
    tables: &[String],
    dependencies: &BTreeMap<String, BTreeSet<String>>,
) -> Result<Vec<String>, String> {
    let mut remaining = dependencies.clone();
    let mut ordered = Vec::with_capacity(tables.len());
    while !remaining.is_empty() {
        let ready = tables
            .iter()
            .find(|table| {
                remaining
                    .get(*table)
                    .is_some_and(BTreeSet::is_empty)
            })
            .cloned();
        let Some(ready) = ready else {
            let component = first_cyclic_component(&remaining);
            return Err(format!(
                "C# read-only immediate reference loading does not support cyclic table reference component: {}",
                component.join(" -> ")
            ));
        };
        remaining.remove(&ready);
        for deps in remaining.values_mut() {
            deps.remove(&ready);
        }
        ordered.push(ready);
    }
    Ok(ordered)
}

fn first_cyclic_component(graph: &BTreeMap<String, BTreeSet<String>>) -> Vec<String> {
    let mut reversed = BTreeMap::<String, BTreeSet<String>>::new();
    for node in graph.keys() {
        reversed.entry(node.clone()).or_default();
    }
    for (node, dependencies) in graph {
        for dependency in dependencies {
            if graph.contains_key(dependency) {
                reversed
                    .entry(dependency.clone())
                    .or_default()
                    .insert(node.clone());
            }
        }
    }

    let mut visited = BTreeSet::new();
    let mut finish_order = Vec::new();
    for node in graph.keys() {
        finish_dfs(node, graph, &mut visited, &mut finish_order);
    }

    visited.clear();
    for node in finish_order.into_iter().rev() {
        if visited.contains(&node) {
            continue;
        }
        let mut component = Vec::new();
        let mut stack = vec![node];
        while let Some(current) = stack.pop() {
            if !visited.insert(current.clone()) {
                continue;
            }
            component.push(current.clone());
            stack.extend(reversed.get(&current).into_iter().flatten().cloned());
        }
        component.sort();
        let self_cycle = component.len() == 1
            && graph
                .get(&component[0])
                .is_some_and(|deps| deps.contains(&component[0]));
        if component.len() > 1 || self_cycle {
            component.push(component[0].clone());
            return component;
        }
    }
    graph.keys().next().cloned().into_iter().collect()
}

fn finish_dfs(
    start: &str,
    graph: &BTreeMap<String, BTreeSet<String>>,
    visited: &mut BTreeSet<String>,
    finish_order: &mut Vec<String>,
) {
    if visited.contains(start) {
        return;
    }
    let mut stack = vec![(start.to_string(), false)];
    while let Some((node, expanded)) = stack.pop() {
        if expanded {
            finish_order.push(node);
            continue;
        }
        if !visited.insert(node.clone()) {
            continue;
        }
        stack.push((node.clone(), true));
        if let Some(dependencies) = graph.get(&node) {
            for dependency in dependencies.iter().rev() {
                if graph.contains_key(dependency) && !visited.contains(dependency) {
                    stack.push((dependency.clone(), false));
                }
            }
        }
    }
}
