// SPDX-FileCopyrightText: 2026 tao3k team and Contributors
//
// SPDX-License-Identifier: Apache-2.0 AND LGPL-2.1-or-later

//! Compiles `marlin.cove.v1` and `marlin.app.v1` protobuf contracts into typed
//! `prost` + `tonic` HTTP/2 gRPC code and runs the build-time harness gate.

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = rust_lang_project_harness::default_rust_harness_config()
        .with_verification_profile_hint(
            rust_lang_project_harness::RustVerificationProfileHint::new(
                "src/lib.rs",
                [rust_lang_project_harness::RustOwnerResponsibility::PublicApi],
            )
            .with_rationale("crate root owns the public package API for cargo-test verification"),
        );
    rust_lang_project_harness::assert_rust_project_harness_build_clean_from_env_with_config(
        &config,
    );

    let schemas = [
        "proto/marlin/cove/v1/cove.proto",
        "proto/marlin/app/v1/app_server.proto",
    ];
    for schema in schemas {
        println!("cargo:rerun-if-changed={schema}");
    }
    tonic_prost_build::configure()
        .build_server(true)
        .build_client(true)
        .compile_protos(&schemas, &["proto"])?;
    Ok(())
}
