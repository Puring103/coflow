use super::runtime::EvalValue;
use std::collections::BTreeMap;

pub(super) struct CheckScope {
    layers: Vec<BTreeMap<String, EvalValue>>,
}

impl CheckScope {
    pub(super) fn new() -> Self {
        Self { layers: Vec::new() }
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
