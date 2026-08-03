//! `coflow self-update`: replace the standalone binary with the latest release.
//!
//! This only applies to the standalone CLI installed from the `coflow-cli-*`
//! release assets. When `coflow` runs as the editor's bundled sidecar it is
//! replaced wholesale by the Tauri updater, so self-update is neither needed
//! nor wanted there.

use coflow_api::DiagnosticSet;
use std::io::{self, Write};

use crate::cli::SelfUpdateArgs;
use crate::diagnostics::cli_error;

const REPO_OWNER: &str = "Puring103";
const REPO_NAME: &str = "coflow";
const BIN_NAME: &str = "coflow";

/// Release asset stem for the current platform, e.g. `coflow-cli-macos-arm64`.
///
/// Returns `None` on platforms we do not publish a standalone CLI for.
fn asset_identifier() -> Option<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => Some("coflow-cli-macos-arm64"),
        ("macos", "x86_64") => Some("coflow-cli-macos-x64"),
        _ => None,
    }
}

pub(crate) fn run(args: &SelfUpdateArgs) -> Result<bool, DiagnosticSet> {
    let Some(identifier) = asset_identifier() else {
        return Err(cli_error(
            "SELF-UPDATE-PLATFORM",
            format!(
                "self-update is not supported on this platform ({} {}); reinstall manually.",
                std::env::consts::OS,
                std::env::consts::ARCH,
            ),
        ));
    };

    let current_version = env!("CARGO_PKG_VERSION");

    let mut builder = self_update::backends::github::Update::configure();
    builder
        .repo_owner(REPO_OWNER)
        .repo_name(REPO_NAME)
        .bin_name(BIN_NAME)
        .identifier(identifier)
        .current_version(current_version)
        .no_confirm(args.yes)
        .show_download_progress(!args.check);

    let updater = builder.build().map_err(|error| {
        cli_error(
            "SELF-UPDATE-CONFIG",
            format!("failed to configure self-update: {error}"),
        )
    })?;

    let latest = updater.get_latest_release().map_err(|error| {
        cli_error(
            "SELF-UPDATE-RELEASE",
            format!("failed to query latest release: {error}"),
        )
    })?;

    let up_to_date = !self_update::version::bump_is_greater(current_version, &latest.version)
        .map_err(|error| {
            cli_error(
                "SELF-UPDATE-VERSION",
                format!("failed to compare versions: {error}"),
            )
        })?;

    if up_to_date {
        println!("coflow {current_version} is already the latest release.");
        return Ok(true);
    }

    println!("A newer release is available: {current_version} -> {}", latest.version);

    if args.check {
        println!("Run `coflow self-update` to install it.");
        return Ok(true);
    }

    let status = updater.update().map_err(|error| {
        cli_error("SELF-UPDATE-INSTALL", format!("self-update failed: {error}"))
    })?;

    let _ = io::stdout().flush();
    println!("Updated coflow to {}.", status.version());
    Ok(true)
}
