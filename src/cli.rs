// SPDX-FileCopyrightText: 2026 tao3k team and Contributors
//
// SPDX-License-Identifier: Apache-2.0 AND LGPL-2.1-or-later

//! Command-line interface for Marlin Cove (`check-config`, `preflight`, `plan-live`, `features`, `serve`).

use std::path::PathBuf;
use std::process::ExitCode;

use crate::config::{
    CoveConfig, CoveConfigSummary, LaunchPlan, PreflightReport, compiled_features,
};

/// Execute the Marlin Cove CLI from an argument iterator.
#[must_use]
pub fn run(args: impl IntoIterator<Item = String>) -> ExitCode {
    let tokens: Vec<String> = args.into_iter().skip(1).collect();
    let Some(command) = tokens.first().map(String::as_str) else {
        print_help();
        return ExitCode::SUCCESS;
    };
    let config_path = parse_config_flag(&tokens[1..]);

    match command {
        "--help" | "-h" | "help" => {
            print_help();
            ExitCode::SUCCESS
        }
        "features" => {
            println!("compiled_features={}", compiled_features().join(","));
            ExitCode::SUCCESS
        }
        "check-config" => match load_config(config_path.as_deref()) {
            Ok(config) => {
                for line in CoveConfigSummary::from_config(&config).render_lines() {
                    println!("{line}");
                }
                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("{error}");
                ExitCode::FAILURE
            }
        },
        "preflight" => match load_config(config_path.as_deref()) {
            Ok(config) => {
                for line in CoveConfigSummary::from_config(&config).render_lines() {
                    println!("{line}");
                }
                let report = PreflightReport::evaluate(&config);
                for line in report.render_lines() {
                    println!("{line}");
                }
                if report.is_ready() {
                    ExitCode::SUCCESS
                } else {
                    ExitCode::FAILURE
                }
            }
            Err(error) => {
                eprintln!("{error}");
                ExitCode::FAILURE
            }
        },
        "plan-live" => match load_config(config_path.as_deref()) {
            Ok(config) => {
                let report = PreflightReport::evaluate(&config);
                for line in report.render_lines() {
                    println!("{line}");
                }
                let plan = LaunchPlan::from_config(&config);
                for line in plan.render_lines() {
                    println!("{line}");
                }
                if report.is_ready() && plan.is_runnable() {
                    ExitCode::SUCCESS
                } else {
                    ExitCode::FAILURE
                }
            }
            Err(error) => {
                eprintln!("{error}");
                ExitCode::FAILURE
            }
        },
        "serve" => run_serve(config_path.as_deref()),
        other => {
            eprintln!("unknown command: {other}");
            print_help();
            ExitCode::FAILURE
        }
    }
}

#[cfg(feature = "relay")]
fn run_serve(config_path: Option<&std::path::Path>) -> ExitCode {
    let config = match load_config(config_path) {
        Ok(config) => config,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    let report = PreflightReport::evaluate(&config);
    if !report.is_ready() {
        for line in report.render_lines() {
            eprintln!("{line}");
        }
        return ExitCode::FAILURE;
    }

    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .try_init();

    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("failed to start tokio runtime: {error}");
            return ExitCode::FAILURE;
        }
    };

    match runtime.block_on(crate::relay::serve(config)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("marlin-cove relay exited with error: {error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(not(feature = "relay"))]
fn run_serve(_config_path: Option<&std::path::Path>) -> ExitCode {
    eprintln!("`serve` requires compiling marlin-cove with `--features relay`");
    ExitCode::FAILURE
}

fn load_config(
    path: Option<&std::path::Path>,
) -> Result<CoveConfig, crate::config::CoveConfigError> {
    let base = match path {
        Some(path) => CoveConfig::from_toml_file(path)?,
        None => CoveConfig::default(),
    };
    base.with_env_lookup(|key| std::env::var(key).ok())
}

fn parse_config_flag(args: &[String]) -> Option<PathBuf> {
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if arg == "--config" {
            return iter.next().map(PathBuf::from);
        }
        if let Some(rest) = arg.strip_prefix("--config=") {
            return Some(PathBuf::from(rest));
        }
    }
    None
}

fn print_help() {
    println!(
        "usage: marlin-cove <check-config|preflight|plan-live|features|serve> [--config <cove.toml>]"
    );
}
