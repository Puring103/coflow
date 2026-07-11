#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::single_element_loop,
    clippy::too_many_lines,
    unused_imports
)]

use std::process::Command;
use toml::{Table, Value};

#[path = "repo_hygiene/api.rs"]
mod api;
#[path = "repo_hygiene/architecture.rs"]
mod architecture;
#[path = "repo_hygiene/checker.rs"]
mod checker;
#[path = "repo_hygiene/cli.rs"]
mod cli;
#[path = "repo_hygiene/hosts.rs"]
mod hosts;
#[path = "repo_hygiene/lark_table.rs"]
mod lark_table;
#[path = "repo_hygiene/loaders.rs"]
mod loaders;
#[path = "repo_hygiene/lsp.rs"]
mod lsp;
#[path = "repo_hygiene/project.rs"]
mod project;
#[path = "repo_hygiene/runtime.rs"]
mod runtime;
#[path = "repo_hygiene/schema_model.rs"]
mod schema_model;
#[path = "repo_hygiene/workspace.rs"]
mod workspace;

fn struct_block<'a>(source: &'a str, marker: &str) -> Option<&'a str> {
    let start = source.find(marker)?;
    let tail = &source[start..];
    let end = tail.find('}')?;
    Some(&tail[..=end])
}
