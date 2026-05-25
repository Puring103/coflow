use std::cell::{Ref, RefCell};
use std::collections::BTreeMap;
use std::rc::Rc;

use crate::ModuleId;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CfcNominalType {
    pub module: ModuleId,
    pub name: String,
}

#[derive(Debug, Clone)]
pub enum CfcValue {
    Pending,
    Int(i64),
    Float(f64),
    Bool(bool),
    String(String),
    Enum {
        enum_type: CfcNominalType,
        variant: String,
        value: i64,
    },
    Object {
        type_name: Option<CfcNominalType>,
        fields: BTreeMap<String, CfcValueRef>,
    },
    Array(Vec<CfcValueRef>),
    Dict(Vec<(CfcValueRef, CfcValueRef)>),
}

#[derive(Debug, Clone)]
pub struct CfcValueRef(Rc<RefCell<CfcValue>>);

impl CfcValueRef {
    #[must_use]
    pub fn new(value: CfcValue) -> Self {
        Self(Rc::new(RefCell::new(value)))
    }

    pub(crate) fn pending() -> Self {
        Self::new(CfcValue::Pending)
    }

    #[must_use]
    pub fn borrow(&self) -> Ref<'_, CfcValue> {
        self.0.borrow()
    }

    pub(crate) fn replace(&self, value: CfcValue) {
        *self.0.borrow_mut() = value;
    }

    #[must_use]
    pub fn ptr_eq(a: &Self, b: &Self) -> bool {
        Rc::ptr_eq(&a.0, &b.0)
    }

    pub(crate) fn ptr_key(&self) -> usize {
        Rc::as_ptr(&self.0).cast::<()>() as usize
    }
}

impl CfcValue {
    #[must_use]
    pub fn type_name(&self) -> &'static str {
        match self {
            CfcValue::Pending => "pending",
            CfcValue::Int(_) => "int",
            CfcValue::Float(_) => "float",
            CfcValue::Bool(_) => "bool",
            CfcValue::String(_) => "string",
            CfcValue::Enum { .. } => "enum",
            CfcValue::Object { .. } => "object",
            CfcValue::Array(_) => "array",
            CfcValue::Dict(_) => "dict",
        }
    }
}
