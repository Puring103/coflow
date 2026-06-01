use std::cell::{Ref, RefCell};
use std::collections::BTreeMap;
use std::rc::Rc;

use crate::ModuleId;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CfdNominalType {
    pub module: ModuleId,
    pub name: String,
}

#[derive(Debug, Clone)]
pub enum CfdValue {
    Null,
    Int(i64),
    Float(f64),
    Bool(bool),
    String(String),
    Enum {
        enum_type: CfdNominalType,
        variant: String,
        value: i64,
    },
    Object {
        type_name: Option<CfdNominalType>,
        fields: BTreeMap<String, CfdValueRef>,
    },
    Union {
        union_type: CfdNominalType,
        value: CfdValueRef,
    },
    Array(Vec<CfdValueRef>),
    Dict(Vec<(CfdValueRef, CfdValueRef)>),
}

#[derive(Debug, Clone)]
pub struct CfdValueRef(Rc<RefCell<ValueSlot>>);

#[derive(Debug, Clone)]
pub(crate) enum ValueSlot {
    Pending(CfdValue),
    Ready(CfdValue),
}

impl CfdValueRef {
    #[must_use]
    pub fn new(value: CfdValue) -> Self {
        Self(Rc::new(RefCell::new(ValueSlot::Ready(value))))
    }

    pub(crate) fn pending(value: CfdValue) -> Self {
        Self(Rc::new(RefCell::new(ValueSlot::Pending(value))))
    }

    #[must_use]
    pub fn borrow(&self) -> Ref<'_, CfdValue> {
        Ref::map(self.0.borrow(), |slot| match slot {
            ValueSlot::Pending(value) | ValueSlot::Ready(value) => value,
        })
    }

    pub(crate) fn replace(&self, value: CfdValue) {
        *self.0.borrow_mut() = ValueSlot::Ready(value);
    }

    pub(crate) fn is_pending(&self) -> bool {
        matches!(&*self.0.borrow(), ValueSlot::Pending(_))
    }

    #[must_use]
    pub fn ptr_eq(a: &Self, b: &Self) -> bool {
        Rc::ptr_eq(&a.0, &b.0)
    }

    #[allow(dead_code)]
    pub(crate) fn ptr_key(&self) -> usize {
        Rc::as_ptr(&self.0).cast::<()>() as usize
    }
}

impl CfdValue {
    #[must_use]
    pub fn type_name(&self) -> &'static str {
        match self {
            CfdValue::Null => "null",
            CfdValue::Int(_) => "int",
            CfdValue::Float(_) => "float",
            CfdValue::Bool(_) => "bool",
            CfdValue::String(_) => "string",
            CfdValue::Enum { .. } => "enum",
            CfdValue::Object { .. } => "object",
            CfdValue::Union { .. } => "union",
            CfdValue::Array(_) => "array",
            CfdValue::Dict(_) => "dict",
        }
    }
}
