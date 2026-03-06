use anyhow::Result;
use std::path::{Path, PathBuf};

use crate::e2e::config::DiskSpec;
use crate::vm::qemu::{AgentVmConfig, QemuProcess, agent_vm_args, create_disk_image};

/// A running agent VM.
pub struct AgentVm {
    _process: QemuProcess,
    pub disk_paths: Vec<PathBuf>,
}

impl AgentVm {
    /// Start the agent VM, creating disk images in `disk_dir`.
    ///
    /// `net_port` is the UDP port the agent listens on.
    /// `director_net_port` is the UDP port the director listens on; the agent sends to it.
    pub fn start(
        net_port: u16,
        director_net_port: u16,
        disk_specs: &[DiskSpec],
        disk_dir: &Path,
        memory_mb: u32,
        serial_log: Option<PathBuf>,
    ) -> Result<Self> {
        let mut disk_paths = Vec::new();
        for (i, spec) in disk_specs.iter().enumerate() {
            let path = disk_dir.join(format!("disk{}.img", i));
            let size_bytes = spec.size_gb * 1024 * 1024 * 1024;
            create_disk_image(&path, size_bytes)?;
            disk_paths.push(path);
        }

        let config = AgentVmConfig {
            net_port,
            director_net_port,
            disk_paths: disk_paths.clone(),
            memory_mb,
            serial_log,
        };

        let args = agent_vm_args(&config);
        let process = QemuProcess::spawn("agent", &args)?;

        Ok(Self {
            _process: process,
            disk_paths,
        })
    }

    /// Returns true if the VM process is still running.
    pub fn is_running(&mut self) -> bool {
        self._process.is_running()
    }
}
