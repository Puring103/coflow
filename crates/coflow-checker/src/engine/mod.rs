mod evaluator;
mod expressions;
mod runner;
mod statements;

use crate::dependencies as deps;
use crate::diagnostics;
use crate::diagnostics::{explanations, trace as evaluation_trace};
use crate::dimensions;
use crate::eval as value;
use crate::operations::{
    access, builtins, comparison as ops, predicates as type_predicates, quantifiers,
};

pub(crate) use runner::CheckRunner;
