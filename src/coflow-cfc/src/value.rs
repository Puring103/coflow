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
    Null,
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
    Union {
        union_type: CfcNominalType,
        value: CfcValueRef,
    },
    Array(Vec<CfcValueRef>),
    Dict(Vec<(CfcValueRef, CfcValueRef)>),
}

#[derive(Debug, Clone)]
pub struct CfcValueRef(Rc<RefCell<ValueSlot>>);

#[derive(Debug, Clone)]
pub(crate) enum ValueSlot {
    Pending(CfcValue),
    Ready(CfcValue),
}

impl CfcValueRef {
    #[must_use]
    pub fn new(value: CfcValue) -> Self {
        Self(Rc::new(RefCell::new(ValueSlot::Ready(value))))
    }

    pub(crate) fn pending(value: CfcValue) -> Self {
        Self(Rc::new(RefCell::new(ValueSlot::Pending(value))))
    }

    #[must_use]
    pub fn borrow(&self) -> Ref<'_, CfcValue> {
        Ref::map(self.0.borrow(), |slot| match slot {
            ValueSlot::Pending(value) | ValueSlot::Ready(value) => value,
        })
    }

    pub(crate) fn replace(&self, value: CfcValue) {
        *self.0.borrow_mut() = ValueSlot::Ready(value);
    }

    pub(crate) fn is_pending(&self) -> bool {
        matches!(&*self.0.borrow(), ValueSlot::Pending(_))
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
            CfcValue::Null => "null",
            CfcValue::Int(_) => "int",
            CfcValue::Float(_) => "float",
            CfcValue::Bool(_) => "bool",
            CfcValue::String(_) => "string",
            CfcValue::Enum { .. } => "enum",
            CfcValue::Object { .. } => "object",
            CfcValue::Union { .. } => "union",
            CfcValue::Array(_) => "array",
            CfcValue::Dict(_) => "dict",
        }
    }
}
