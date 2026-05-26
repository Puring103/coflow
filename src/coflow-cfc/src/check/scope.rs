use super::runtime::EvalValue;
use crate::container::ModuleId;
use crate::value::CfcValueRef;
use std::collections::{BTreeMap, HashMap};

pub(super) struct CheckScope<'a> {
    layers: Vec<BTreeMap<String, EvalValue>>,
    pub(super) enum_values: &'a HashMap<(ModuleId, String, String), CfcValueRef>,
}

impl<'a> CheckScope<'a> {
    pub(super) fn new(
        enum_values: &'a HashMap<(ModuleId, String, String), CfcValueRef>,
        base: BTreeMap<String, EvalValue>,
    ) -> Self {
        let mut scope = Self {
            layers: Vec::new(),
            enum_values,
        };
        scope.push(base);
        scope
    }

    pub(super) fn push(&mut self, layer: BTreeMap<String, EvalValue>) {
        self.layers.push(layer);
    }

    pub(super) fn pop(&mut self) {
        self.layers.pop();
    }

    pub(super) fn lookup(&self, name: &str) -> Option<EvalValue> {
        self.layers
            .iter()
            .rev()
            .find_map(|layer| layer.get(name).cloned())
    }
}

pub(super) fn enum_type_value(module: &ModuleId, name: &str) -> EvalValue {
    EvalValue::EnumType {
        module: module.clone(),
        name: name.to_string(),
    }
}

pub(super) fn module_namespace_value(module: &ModuleId, allow_data: bool) -> EvalValue {
    EvalValue::ModuleNamespace {
        module: module.clone(),
        allow_data,
    }
}

pub(super) fn ref_layer(
    values: impl IntoIterator<Item = (String, CfcValueRef)>,
) -> BTreeMap<String, EvalValue> {
    values
        .into_iter()
        .map(|(name, value)| (name, EvalValue::Ref(value)))
        .collect()
}
