//! Executable architecture gate over the workspace dependency graph.
//!
//! Rules asserted (from the project-structure skill's dependency direction):
//! - `rs-broker-core` (domain + usecase + ports) imports no infrastructure crate
//!   and no framework driver.
//! - Driven adapters (`rs-broker-db`, `rs-broker-kafka`) never depend on the
//!   driving adapter or the composition root (`rs-broker-server`).
//! - `rs-broker-config` depends on no other workspace member.
//!
//! The gate reads Cargo's own dependency DAG (`cargo metadata --no-deps`)
//! rather than grepping source, so a violation fails the build.

use std::collections::HashSet;
use std::env;
use std::process::Command;

use serde_json::Value;

fn cargo_metadata() -> Value {
    let cargo = env::var("CARGO").unwrap_or_else(|_| String::from("cargo"));
    let output = Command::new(cargo)
        .args(["metadata", "--format-version", "1", "--no-deps"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("cargo metadata must run");
    assert!(
        output.status.success(),
        "cargo metadata exited with {}",
        output.status
    );
    serde_json::from_slice(&output.stdout).expect("cargo metadata must emit JSON")
}

/// Internal (workspace) dependencies of each package, by package name.
fn internal_dependencies() -> Vec<(String, HashSet<String>)> {
    let metadata = cargo_metadata();
    let members: HashSet<&str> = metadata["workspace_members"]
        .as_array()
        .expect("workspace_members is an array")
        .iter()
        .map(|id| id.as_str().expect("member id is a string"))
        .collect();

    let packages = metadata["packages"]
        .as_array()
        .expect("packages is an array");

    packages
        .iter()
        .filter(|package| {
            let id = package["id"].as_str().expect("package id is a string");
            members.contains(&id)
        })
        .map(|package| {
            let name = package["name"].as_str().expect("package name").to_string();
            let deps = package["dependencies"]
                .as_array()
                .expect("dependencies is an array")
                .iter()
                .filter_map(|dep| dep["name"].as_str())
                .filter(|dep| dep.starts_with("rs-broker-"))
                .map(str::to_string)
                .collect();
            (name, deps)
        })
        .collect()
}

fn dependencies_of(package: &str) -> HashSet<String> {
    internal_dependencies()
        .into_iter()
        .find(|(name, _)| name == package)
        .map(|(_, deps)| deps)
        .unwrap_or_else(|| panic!("workspace member {package} not found in cargo metadata"))
}

#[test]
fn core_imports_no_infrastructure_or_driver() {
    let deps = dependencies_of("rs-broker-core");

    for forbidden in [
        "rs-broker-db",
        "rs-broker-kafka",
        "rs-broker-proto",
        "rs-broker-server",
    ] {
        assert!(
            !deps.contains(forbidden),
            "rs-broker-core must not depend on {forbidden}"
        );
    }
}

#[test]
fn core_declares_no_framework_driver_dependency() {
    let cargo = env::var("CARGO").unwrap_or_else(|_| String::from("cargo"));
    let output = Command::new(cargo)
        .args(["metadata", "--format-version", "1", "--no-deps"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("cargo metadata must run");
    let metadata: Value = serde_json::from_slice(&output.stdout).expect("metadata JSON");

    let package = metadata["packages"]
        .as_array()
        .expect("packages is an array")
        .iter()
        .find(|package| package["name"] == "rs-broker-core")
        .expect("rs-broker-core package");

    let dependencies = package["dependencies"]
        .as_array()
        .expect("dependencies is an array");

    for dependency in dependencies {
        let name = dependency["name"].as_str().expect("dependency name");
        for forbidden in ["sqlx", "rdkafka", "tonic", "prost", "axum", "reqwest"] {
            assert!(
                name != forbidden,
                "rs-broker-core must not depend on the framework crate {forbidden}"
            );
        }
    }
}

#[test]
fn driven_adapters_do_not_depend_on_driving_adapter_or_composition_root() {
    for adapter in ["rs-broker-db", "rs-broker-kafka"] {
        let deps = dependencies_of(adapter);
        assert!(
            !deps.contains("rs-broker-server"),
            "{adapter} must not depend on the composition root rs-broker-server"
        );
    }
}

#[test]
fn config_depends_on_no_workspace_member() {
    let deps = dependencies_of("rs-broker-config");
    assert!(
        deps.is_empty(),
        "rs-broker-config must depend on no workspace member, found {deps:?}"
    );
}

#[test]
fn only_the_server_composes_adapters() {
    for consumer in ["rs-broker-db", "rs-broker-kafka", "rs-broker-config"] {
        let deps = dependencies_of(consumer);
        assert!(
            !deps.contains("rs-broker-server"),
            "{consumer} must not depend on rs-broker-server"
        );
    }
}
