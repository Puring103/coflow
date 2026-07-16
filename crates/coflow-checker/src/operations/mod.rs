pub(crate) mod access;
pub(crate) mod builtins;
pub(crate) mod comparison;
pub(crate) mod predicates;
pub(crate) mod quantifiers;

use crate::diagnostics;
use crate::eval as value;
use comparison as ops;
