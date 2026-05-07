//! Integration tests for the OCI Distribution registry.
//!
//! These tests run inside a QEMU/KVM virtual machine using the qlean crate,
//! following the same test flow as `project/distribution/test_registry.sh` but using HTTP APIs
//! instead of Docker commands.

use std::path::{Path, PathBuf};
use std::sync::Once;
use std::time::Duration;

use anyhow::{Context, Result};
use qlean::{Distro, Machine, MachineConfig, create_image, with_machine};
use serde_json::Value;
use tracing_subscriber::EnvFilter;

const REGISTRY_HOST: &str = "127.0.0.1";
const REGISTRY_PORT: u16 = 8968;
const POSTGRES_USER: &str = "postgres";
const POSTGRES_PASSWORD: &str = "password";
const POSTGRES_DB: &str = "postgres";

fn tracing_subscriber_init() {
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        tracing_subscriber::fmt()
            .with_env_filter(EnvFilter::from_default_env())
            .init();
    });
}

/// Helper to run a command and check its exit status
async fn exec_check(vm: &mut Machine, cmd: &str) -> Result<String> {
    let result = vm.exec(cmd).await?;
    if !result.status.success() {
        let stderr = String::from_utf8_lossy(&result.stderr);
        let stdout = String::from_utf8_lossy(&result.stdout);
        anyhow::bail!(
            "Command '{}' failed with exit code {:?}\nstdout: {}\nstderr: {}",
            cmd,
            result.status.code(),
            stdout,
            stderr
        );
    }
    Ok(String::from_utf8_lossy(&result.stdout).to_string())
}

/// Helper to run a command and expect it to fail
#[allow(dead_code)]
async fn exec_expect_fail(vm: &mut Machine, cmd: &str) -> Result<()> {
    let result = vm.exec(cmd).await?;
    if result.status.success() {
        anyhow::bail!("Command '{}' succeeded but was expected to fail", cmd);
    }
    Ok(())
}

/// Wait for a service to be ready by polling a URL
/// The /v2/ endpoint returns 401 without auth, which is still a valid response indicating the service is up
async fn wait_for_service(vm: &mut Machine, url: &str, timeout_secs: u64) -> Result<()> {
    let start = std::time::Instant::now();
    let timeout = Duration::from_secs(timeout_secs);

    loop {
        // Use -w to get the HTTP status code, accept 200, 401, 403 as valid (service is running)
        let result = vm
            .exec(&format!(
                "curl -s -o /dev/null -w '%{{http_code}}' '{}'",
                url
            ))
            .await?;

        let status_code = String::from_utf8_lossy(&result.stdout).trim().to_string();
        tracing::debug!("Service check returned status: {}", status_code);

        // 200, 401, 403 all indicate the service is running
        if status_code == "200" || status_code == "401" || status_code == "403" {
            tracing::info!("Service is ready (status: {})", status_code);
            return Ok(());
        }

        if start.elapsed() > timeout {
            // Get more details about the failure
            let debug_result = vm.exec(&format!("curl -v '{}' 2>&1 || true", url)).await?;
            let debug_output = String::from_utf8_lossy(&debug_result.stdout);
            tracing::error!("Service check debug output:\n{}", debug_output);
            anyhow::bail!(
                "Timeout waiting for service at {} (last status: {})",
                url,
                status_code
            );
        }

        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

/// Setup PostgreSQL in the VM
async fn setup_postgres(vm: &mut Machine) -> Result<()> {
    tracing::info!("Installing PostgreSQL...");
    exec_check(vm, "apt-get update -qq").await?;
    exec_check(vm, "DEBIAN_FRONTEND=noninteractive apt-get install -y -qq postgresql postgresql-contrib curl jq").await?;

    // Start PostgreSQL
    tracing::info!("Starting PostgreSQL...");
    exec_check(vm, "service postgresql start").await?;

    // Wait for PostgreSQL to be ready
    tokio::time::sleep(Duration::from_secs(2)).await;

    // Setup PostgreSQL user and database
    tracing::info!("Configuring PostgreSQL...");
    exec_check(
        vm,
        &format!(
            "su - postgres -c \"psql -c \\\"ALTER USER {} WITH PASSWORD '{}';\\\"\"",
            POSTGRES_USER, POSTGRES_PASSWORD
        ),
    )
    .await?;

    // Configure pg_hba.conf for password authentication
    exec_check(
        vm,
        "echo 'host all all 127.0.0.1/32 md5' >> /etc/postgresql/*/main/pg_hba.conf",
    )
    .await?;
    exec_check(vm, "service postgresql restart").await?;
    tokio::time::sleep(Duration::from_secs(2)).await;

    tracing::info!("PostgreSQL setup complete.");
    Ok(())
}

/// Setup and start the distribution service
async fn setup_distribution(vm: &mut Machine, binary_path: &Path) -> Result<()> {
    // Create registry root directory
    tracing::info!("Creating registry directory...");
    exec_check(vm, "mkdir -p /var/lib/oci-registry").await?;

    // Upload the distribution binary
    tracing::info!("Uploading distribution binary...");
    vm.upload(binary_path, Path::new("/usr/local/bin")).await?;
    exec_check(vm, "chmod +x /usr/local/bin/distribution").await?;

    // Set environment variables and start the service
    tracing::info!("Starting distribution service...");

    // Create env file
    let env_file_content = format!(
        r#"OCI_REGISTRY_URL=0.0.0.0
OCI_REGISTRY_PORT={}
OCI_REGISTRY_STORAGE=FILESYSTEM
OCI_REGISTRY_ROOTDIR=/var/lib/oci-registry
OCI_REGISTRY_PUBLIC_URL=http://{}:{}
POSTGRES_HOST=127.0.0.1
POSTGRES_PORT=5432
POSTGRES_USER={}
POSTGRES_PASSWORD={}
POSTGRES_DB={}
JWT_SECRET=secret
JWT_LIFETIME_SECONDS=3600
OCI_REGISTRY_DEFAULT_USER=admin
RUST_LOG=info"#,
        REGISTRY_PORT, REGISTRY_HOST, REGISTRY_PORT, POSTGRES_USER, POSTGRES_PASSWORD, POSTGRES_DB
    );

    vm.write(
        std::path::Path::new("/etc/distribution.env"),
        env_file_content.as_bytes(),
    )
    .await?;

    // Create startup script
    let startup_script = r#"#!/bin/bash
set -a
source /etc/distribution.env
set +a
exec /usr/local/bin/distribution
"#;
    vm.write(
        std::path::Path::new("/usr/local/bin/start-distribution.sh"),
        startup_script.as_bytes(),
    )
    .await?;
    exec_check(vm, "chmod +x /usr/local/bin/start-distribution.sh").await?;

    // Start distribution in background using the script
    exec_check(
        vm,
        "nohup /usr/local/bin/start-distribution.sh > /var/log/distribution.log 2>&1 &",
    )
    .await?;

    // Give it a moment to start
    tokio::time::sleep(Duration::from_secs(3)).await;

    // Check if process is running
    let ps_output = exec_check(vm, "ps aux | grep distribution || true").await?;
    tracing::info!("Process status: {}", ps_output);

    // Check logs for errors
    let log_output = exec_check(
        vm,
        "cat /var/log/distribution.log 2>/dev/null || echo 'No log file'",
    )
    .await?;
    tracing::info!("Distribution log:\n{}", log_output);

    // Check if the binary can run
    let file_output = exec_check(vm, "file /usr/local/bin/distribution").await?;
    tracing::info!("Binary file type: {}", file_output);

    // Wait for service to be ready
    let api_url = format!("http://{}:{}/v2/", REGISTRY_HOST, REGISTRY_PORT);
    tracing::info!(
        "Waiting for distribution service to be ready at {}...",
        api_url
    );
    wait_for_service(vm, &api_url, 60).await?;

    tracing::info!("Distribution service is ready.");
    Ok(())
}

/// Test anonymous user permissions
async fn test_anonymous_user(vm: &mut Machine) -> Result<()> {
    tracing::info!("--- Running Test Case 1: No-Auth Admin Push Permissions ---");

    let api_url = format!("http://{}:{}", REGISTRY_HOST, REGISTRY_PORT);

    tracing::info!("Attempting to start blob upload without credentials...");
    let result = vm
        .exec(&format!(
            r#"curl -s -w "%{{http_code}}" -o /dev/null -X POST '{}/v2/admin/test/blobs/uploads/'"#,
            api_url
        ))
        .await?;

    let status_code = String::from_utf8_lossy(&result.stdout).trim().to_string();
    tracing::info!("No-auth push attempt returned status: {}", status_code);

    if status_code != "202" {
        anyhow::bail!("No-auth registry failed to initiate blob upload (status: {status_code})");
    }

    tracing::info!("[SUCCESS] No-auth registry accepted anonymous push.");
    Ok(())
}

/// Push a minimal blob and manifest to create a repository
async fn push_minimal_image(
    vm: &mut Machine,
    namespace: &str,
    repo: &str,
    tag: &str,
) -> Result<String> {
    let api_url = format!("http://{}:{}", REGISTRY_HOST, REGISTRY_PORT);

    // Create a deterministic per-repository blob so tags in the same repo share
    // a digest while different repos do not interfere with each other.
    exec_check(
        vm,
        &format!("printf '%s' '{namespace}/{repo}' > /tmp/repo_blob"),
    )
    .await?;
    let blob_size = exec_check(vm, "wc -c < /tmp/repo_blob").await?;
    let blob_digest_hex = exec_check(vm, "sha256sum /tmp/repo_blob | awk '{print $1}'").await?;
    let blob_digest = format!("sha256:{}", blob_digest_hex.trim());

    // Initiate blob upload
    tracing::info!("Initiating blob upload for {}/{}...", namespace, repo);
    let location = exec_check(
        vm,
        &format!(
            r#"curl -s -D - -X POST '{}/v2/{}/{}/blobs/uploads/' | grep -i '^location:' | tr -d '\r' | cut -d' ' -f2"#,
            api_url, namespace, repo
        ),
    ).await?;
    let location = location.trim();

    if location.is_empty() {
        anyhow::bail!("Failed to get upload location");
    }

    // Complete blob upload
    tracing::info!("Completing blob upload...");
    // Determine the separator: use '?' if location doesn't have query params, '&' otherwise
    let separator = if location.contains('?') { "&" } else { "?" };
    let upload_url = if location.starts_with("http") {
        format!("{}{}digest={}", location, separator, blob_digest)
    } else {
        format!("{}{}{}digest={}", api_url, location, separator, blob_digest)
    };
    tracing::debug!("Upload URL: {}", upload_url);

    exec_check(
        vm,
        &format!(
            r#"curl -sf -X PUT -H "Content-Type: application/octet-stream" --data-binary @/tmp/repo_blob '{}'"#,
            upload_url
        ),
    ).await?;

    // Create a minimal manifest
    let config_digest = blob_digest;
    let manifest = format!(
        r#"{{
  "schemaVersion": 2,
  "mediaType": "application/vnd.docker.distribution.manifest.v2+json",
  "config": {{
    "mediaType": "application/vnd.docker.container.image.v1+json",
    "size": {},
    "digest": "{}"
  }},
  "layers": []
}}"#,
        blob_size.trim(),
        config_digest
    );

    // Push the manifest
    tracing::info!("Pushing manifest for {}:{}", repo, tag);
    let manifest_digest = exec_check(
        vm,
        &format!(
            r#"curl -s -D - -o /dev/null -X PUT -H "Content-Type: application/vnd.docker.distribution.manifest.v2+json" -d '{}' '{}/v2/{}/{}/manifests/{}' | grep -i '^Docker-Content-Digest:' | tr -d '\r' | cut -d' ' -f2"#,
            manifest, api_url, namespace, repo, tag
        ),
    )
    .await?;

    tracing::info!("Successfully pushed {}/{}:{}", namespace, repo, tag);
    Ok(manifest_digest.trim().to_string())
}

async fn get_visible_repos(vm: &mut Machine) -> Result<Value> {
    let api_url = format!("http://{}:{}/api/v1/repo", REGISTRY_HOST, REGISTRY_PORT);
    let output = exec_check(vm, &format!(r#"curl -sf '{}'"#, api_url)).await?;
    serde_json::from_str(&output).context("Failed to parse repo list JSON")
}

fn repo_entry<'a>(repos: &'a Value, namespace: &str, repo: &str) -> Result<&'a Value> {
    repos["data"]
        .as_array()
        .and_then(|items| {
            items.iter().find(|item| {
                item["namespace"].as_str() == Some(namespace) && item["name"].as_str() == Some(repo)
            })
        })
        .with_context(|| format!("Repository {namespace}/{repo} not found in repo list"))
}

fn repo_tags(repo: &Value) -> Vec<String> {
    repo["tags"]
        .as_array()
        .map(|values| {
            values
                .iter()
                .filter_map(|value| value.as_str().map(ToOwned::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

async fn test_repo_list_metadata(vm: &mut Machine, user: &str) -> Result<()> {
    tracing::info!("--- Running Test Case 4: Repo List Metadata ---");

    let api_url = format!("http://{}:{}", REGISTRY_HOST, REGISTRY_PORT);
    let latest_repo = "repo-list-latest";
    let recent_repo = "repo-list-recent";

    let _ = push_minimal_image(vm, user, latest_repo, "v1").await?;
    tokio::time::sleep(Duration::from_secs(1)).await;
    let _ = push_minimal_image(vm, user, latest_repo, "latest").await?;
    tokio::time::sleep(Duration::from_secs(1)).await;
    let latest_digest = push_minimal_image(vm, user, latest_repo, "v2").await?;

    tokio::time::sleep(Duration::from_secs(1)).await;
    let _ = push_minimal_image(vm, user, recent_repo, "v1").await?;
    tokio::time::sleep(Duration::from_secs(1)).await;
    let _ = push_minimal_image(vm, user, recent_repo, "v2").await?;

    let repos = get_visible_repos(vm).await?;

    let latest_entry = repo_entry(&repos, user, latest_repo)?;
    assert_eq!(
        repo_tags(latest_entry),
        vec!["latest".to_string(), "v2".to_string(), "v1".to_string()]
    );
    assert_eq!(latest_entry["size_tag"].as_str(), Some("latest"));
    assert_eq!(
        latest_entry["size_bytes"].as_u64(),
        Some(format!("{user}/{latest_repo}").len() as u64)
    );
    assert!(latest_entry["last_pushed_at"].is_string());

    let recent_entry = repo_entry(&repos, user, recent_repo)?;
    assert_eq!(
        repo_tags(recent_entry),
        vec!["v2".to_string(), "v1".to_string()]
    );
    assert_eq!(recent_entry["size_tag"].as_str(), Some("v2"));
    assert_eq!(
        recent_entry["size_bytes"].as_u64(),
        Some(format!("{user}/{recent_repo}").len() as u64)
    );
    assert!(recent_entry["last_pushed_at"].is_string());

    tracing::info!("Deleting {}:{} tag v1...", recent_repo, "v1");
    exec_check(
        vm,
        &format!(
            r#"curl -sf -X DELETE '{}/v2/{}/{}/manifests/v1'"#,
            api_url, user, recent_repo
        ),
    )
    .await?;

    let repos = get_visible_repos(vm).await?;
    let recent_entry = repo_entry(&repos, user, recent_repo)?;
    assert_eq!(repo_tags(recent_entry), vec!["v2".to_string()]);
    assert_eq!(recent_entry["size_tag"].as_str(), Some("v2"));
    assert_eq!(
        recent_entry["size_bytes"].as_u64(),
        Some(format!("{user}/{recent_repo}").len() as u64)
    );

    tracing::info!("Deleting digest-backed tags from {}...", latest_repo);
    exec_check(
        vm,
        &format!(
            r#"curl -sf -X DELETE '{}/v2/{}/{}/manifests/{}'"#,
            api_url, user, latest_repo, latest_digest
        ),
    )
    .await?;

    let repos = get_visible_repos(vm).await?;
    let latest_entry = repo_entry(&repos, user, latest_repo)?;
    assert_eq!(repo_tags(latest_entry), Vec::<String>::new());
    assert!(latest_entry["size_tag"].is_null());
    assert!(latest_entry["size_bytes"].is_null());
    assert!(latest_entry["last_pushed_at"].is_null());

    Ok(())
}

/// Get the path to the distribution binary.
fn get_distribution_binary_path() -> Result<PathBuf> {
    // First, try CARGO_BIN_EXE_distribution which Cargo sets for integration tests
    if let Ok(path) = std::env::var("CARGO_BIN_EXE_distribution") {
        let path = PathBuf::from(path);
        if path.exists() {
            return Ok(path);
        }
    }

    // Fall back to manual path resolution, respecting CARGO_TARGET_DIR
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").context("CARGO_MANIFEST_DIR not set")?;

    let target_dir = std::env::var("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            PathBuf::from(&manifest_dir)
                .parent()
                .unwrap()
                .join("target")
        });

    let debug_path = target_dir.join("debug/distribution");
    if debug_path.exists() {
        return Ok(debug_path);
    }

    let release_path = target_dir.join("release/distribution");
    if release_path.exists() {
        return Ok(release_path);
    }

    anyhow::bail!(
        "Distribution binary not found at {:?} or {:?}. \
        Please build it with 'cargo build -p distribution'.",
        debug_path,
        release_path
    );
}

#[tokio::test]
async fn test_registry_integration() -> Result<()> {
    tracing_subscriber_init();

    // Load .env file
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").context("CARGO_MANIFEST_DIR not set")?;
    let env_path = PathBuf::from(&manifest_dir).join(".env");
    if env_path.exists() {
        dotenvy::from_path(&env_path).ok();
        tracing::info!("Loaded .env from {:?}", env_path);
    }

    // Get distribution binary path
    let binary_path = get_distribution_binary_path()?;
    tracing::info!("Using distribution binary at {:?}", binary_path);

    // Create VM image and config
    tracing::info!("Creating VM image...");
    let image = create_image(Distro::Debian, "debian-13-generic-amd64").await?;
    let config = MachineConfig {
        core: 2,
        mem: 2048,
        disk: Some(10),
        clear: true,
    };

    // Execute tests in the virtual machine
    with_machine(&image, &config, |vm| {
        Box::pin(async move {
            tracing::info!("VM started successfully");

            // Setup PostgreSQL
            setup_postgres(vm).await?;

            // Setup and start distribution
            setup_distribution(vm, &binary_path).await?;

            // Run test cases
            test_anonymous_user(vm).await?;
            test_repo_list_metadata(vm, "admin").await?;

            tracing::info!("");
            tracing::info!("=================================================");
            tracing::info!("[SUCCESS] All integration tests passed!");
            tracing::info!("=================================================");

            Ok(())
        })
    })
    .await?;

    Ok(())
}
