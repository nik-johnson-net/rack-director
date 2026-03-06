use anyhow::Result;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::task::JoinSet;

use crate::e2e::agent_vm::AgentVm;
use crate::e2e::build::{ensure_agent_images, ensure_director_images};
use crate::e2e::config::TestConfig;
use crate::e2e::director::DirectorVm;
use crate::e2e::lifecycle;
use crate::output::Output;
use crate::vm::qemu::find_available_tcp_port;

/// Configuration for running e2e tests.
pub struct TestRunConfig {
    pub agent_kernel: PathBuf,
    pub agent_initramfs: PathBuf,
    pub director_kernel: PathBuf,
    pub director_initramfs: PathBuf,
    /// If Some, write per-VM serial logs to named files in this directory.
    pub serial_logs_dir: Option<PathBuf>,
}

/// Result of a single e2e test run.
pub struct TestResult {
    pub name: String,
    pub passed: bool,
    pub error: Option<String>,
    pub duration_secs: u64,
}

/// Run a single e2e test from a TOML file.
pub async fn run_test(
    test_path: &Path,
    run_config: &TestRunConfig,
    output: &Output,
) -> Result<TestResult> {
    let config = TestConfig::load(test_path)?;
    let name = config.test.name.clone();
    let start = Instant::now();

    output.step(&format!("Running test: {}", name));

    ensure_director_images(
        &run_config.director_kernel,
        &run_config.director_initramfs,
        output,
    )
    .await?;
    ensure_agent_images(
        &run_config.agent_kernel,
        &run_config.agent_initramfs,
        output,
    )
    .await?;

    let test_future = run_test_inner(config, run_config, output);
    let result = tokio::select! {
        r = test_future => r,
        _ = tokio::signal::ctrl_c() => {
            Err(anyhow::anyhow!("Test interrupted by Ctrl+C"))
        }
    };
    match result {
        Ok(()) => {
            let duration_secs = start.elapsed().as_secs();
            output.success(&format!("Test '{}' passed in {}s", name, duration_secs));
            Ok(TestResult {
                name,
                passed: true,
                error: None,
                duration_secs,
            })
        }
        Err(e) => {
            let duration_secs = start.elapsed().as_secs();
            let error = e.to_string();
            output.error(&format!("Test '{}' failed: {}", name, error));
            Ok(TestResult {
                name,
                passed: false,
                error: Some(error),
                duration_secs,
            })
        }
    }
}

/// Run all TOML test files in a directory sequentially.
pub async fn run_all(
    tests_dir: &Path,
    run_config: &TestRunConfig,
    output: &Output,
) -> Result<Vec<TestResult>> {
    ensure_director_images(
        &run_config.director_kernel,
        &run_config.director_initramfs,
        output,
    )
    .await?;
    ensure_agent_images(
        &run_config.agent_kernel,
        &run_config.agent_initramfs,
        output,
    )
    .await?;

    let test_files = collect_test_files(tests_dir)?;
    let mut results = Vec::new();
    for test_file in test_files {
        let result = run_test(&test_file, run_config, output).await?;
        results.push(result);
    }
    Ok(results)
}

/// Run all TOML test files in a directory in parallel using tokio tasks.
pub async fn run_all_parallel(
    tests_dir: &Path,
    run_config: &TestRunConfig,
    output: &Output,
) -> Result<Vec<TestResult>> {
    // Build images once before spawning parallel tasks to avoid concurrent docker builds.
    ensure_director_images(
        &run_config.director_kernel,
        &run_config.director_initramfs,
        output,
    )
    .await?;
    ensure_agent_images(
        &run_config.agent_kernel,
        &run_config.agent_initramfs,
        output,
    )
    .await?;

    let test_files = collect_test_files(tests_dir)?;
    let output = Arc::new(output.clone());
    let mut join_set: JoinSet<Result<TestResult>> = JoinSet::new();

    for test_file in test_files {
        let run_config = TestRunConfig {
            agent_kernel: run_config.agent_kernel.clone(),
            agent_initramfs: run_config.agent_initramfs.clone(),
            director_kernel: run_config.director_kernel.clone(),
            director_initramfs: run_config.director_initramfs.clone(),
            serial_logs_dir: run_config.serial_logs_dir.clone(),
        };
        let output = Arc::clone(&output);
        join_set.spawn(async move { run_test(&test_file, &run_config, &output).await });
    }

    let mut results = Vec::new();
    while let Some(result) = join_set.join_next().await {
        results.push(result??);
    }
    Ok(results)
}

async fn run_test_inner(
    config: TestConfig,
    run_config: &TestRunConfig,
    output: &Output,
) -> Result<()> {
    // Use .build/ under the current working directory for temp files to keep VM disk images
    // out of the Docker build context and avoid filling the system temp drive.
    let build_dir = std::env::current_dir()?.join(".build");
    std::fs::create_dir_all(&build_dir)?;
    let temp_dir = tempfile::tempdir_in(&build_dir)?;
    // Two UDP ports needed: one for director (director_net_port), one for agent (agent_net_port).
    // Each VM sends to the other's port and receives on its own port.
    let director_net_port = find_available_tcp_port(20000, 29998)?;
    let agent_net_port = director_net_port + 1;
    let timeout = Duration::from_secs(config.test.timeout_seconds);

    // Resolve serial log directory: use the user-supplied path if given, otherwise default to
    // .build/console-logs/ so logs are always captured and never interleaved with stdout.
    let logs_dir = resolve_logs_dir(run_config.serial_logs_dir.as_deref(), &build_dir)?;

    // Determine director serial log path (must be absolute for QEMU on Windows)
    let director_serial = logs_dir.join(format!("{}-director-serial.log", config.test.name));

    // Start director VM. Uses UDP unicast networking:
    // director listens on director_net_port, agent listens on agent_net_port.
    // 2048 MiB: the initramfs is ~450 MiB (squashfs embedded); the kernel loads
    // it into RAM before extracting, so peak RAM usage is ~1 GiB during boot.
    output.step("Starting Rack Director VM...");
    let director = DirectorVm::start(
        run_config.director_kernel.clone(),
        run_config.director_initramfs.clone(),
        director_net_port,
        agent_net_port,
        2048,
        Some(director_serial),
    )
    .await?;

    output.detail("Director URL", &director.host_url());

    // Set up rack-director data
    output.step("Creating Operating System in Rack Director");
    let os_id = director.create_stub_os().await?;

    output.step("Creating Platforms in Rack Director");
    let mut platform_ids: HashMap<String, i64> = HashMap::new();
    for platform_spec in &config.rack_director.platforms {
        let id = director.create_platform(platform_spec).await?;
        platform_ids.insert(platform_spec.name.clone(), id);
    }

    output.step("Creating Roles in Rack Director");
    let mut role_ids: HashMap<String, i64> = HashMap::new();
    for role_spec in &config.rack_director.roles {
        let id = director.create_role(role_spec, os_id).await?;
        role_ids.insert(role_spec.name.clone(), id);
    }

    // Configure DHCP so the agent VM can get a 10.0.0.x address
    output.step("Creating Networks in Rack Director");
    director.create_rack_dhcp_network().await?;

    // Determine agent serial log path (must be absolute for QEMU on Windows)
    let serial_log = logs_dir.join(format!("{}-agent-serial.log", config.test.name));

    // Start agent VM. Uses UDP unicast: agent listens on agent_net_port, sends to director_net_port.
    output.step("Starting Agent VM...");
    let _agent_vm = AgentVm::start(
        agent_net_port,
        director_net_port,
        &config.vm.disks,
        temp_dir.path(),
        config.vm.memory_mb,
        Some(serial_log),
    )?;

    // Wait for device to appear
    output.step("Waiting for Agent to register");
    let device_uuid = director.wait_for_device(timeout).await?;

    // Assign platform and role using the first role's configuration
    output.step("Assigning Role to Agent");
    let first_role = config
        .rack_director
        .roles
        .first()
        .ok_or_else(|| anyhow::anyhow!("No roles defined in test config"))?;

    if let Some(platform_name) = &first_role.platform {
        let platform_id = platform_ids
            .get(platform_name)
            .copied()
            .ok_or_else(|| anyhow::anyhow!("Platform '{}' not found", platform_name))?;
        director.assign_platform(&device_uuid, platform_id).await?;
    }

    let role_id = role_ids
        .get(&first_role.name)
        .copied()
        .ok_or_else(|| anyhow::anyhow!("Role '{}' not found", first_role.name))?;
    director.assign_role(&device_uuid, role_id).await?;

    // Drive lifecycle
    output.step("Driving lifecycle");
    lifecycle::drive_lifecycle(&director, &device_uuid, &config.lifecycle.steps, timeout).await?;

    // Verify final state
    let final_state = director.get_lifecycle_state(&device_uuid).await?;
    if final_state != config.lifecycle.expect_final_state {
        return Err(anyhow::anyhow!(
            "Expected final state '{}' but device is in '{}'",
            config.lifecycle.expect_final_state,
            final_state
        ));
    }

    Ok(())
}

/// Determine the directory to write serial console logs into.
///
/// When the caller supplies an explicit directory, that path is made absolute if relative.
/// When no directory is supplied, defaults to `<build_dir>/console-logs/` so that logs are
/// always captured to disk rather than going to stdio or being discarded.
///
/// The directory is created if it does not already exist.
fn resolve_logs_dir(serial_logs_dir: Option<&Path>, build_dir: &Path) -> Result<PathBuf> {
    let logs_dir = match serial_logs_dir {
        Some(dir) => {
            if dir.is_absolute() {
                dir.to_path_buf()
            } else {
                std::env::current_dir()?.join(dir)
            }
        }
        None => build_dir.join("console-logs"),
    };
    std::fs::create_dir_all(&logs_dir)?;
    Ok(logs_dir)
}

fn collect_test_files(tests_dir: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for entry in std::fs::read_dir(tests_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("toml") {
            files.push(path);
        }
    }
    files.sort();
    Ok(files)
}
