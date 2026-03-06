use anyhow::{Result, anyhow};
use std::net::{TcpListener, UdpSocket};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

/// A running QEMU process. Kills the child on Drop.
pub struct QemuProcess {
    label: String,
    child: Child,
}

impl QemuProcess {
    /// Spawn a QEMU process with the given arguments.
    pub fn spawn(label: &str, args: &[String]) -> Result<Self> {
        let binary = find_qemu_binary()?;
        let child = Command::new(&binary)
            .args(args)
            .stdout(Stdio::null())
            .stdin(Stdio::null())
            .spawn()
            .map_err(|e| anyhow!("Failed to spawn QEMU ({}): {}", binary, e))?;
        Ok(Self {
            label: label.to_string(),
            child,
        })
    }

    /// Returns true if the process is still running.
    pub fn is_running(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }
}

impl Drop for QemuProcess {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Configuration for the director VM.
pub struct DirectorVmConfig {
    pub kernel: PathBuf,
    pub initramfs: PathBuf,
    /// UDP port the director VM listens on for the point-to-point tunnel with the agent.
    /// The agent sends to this port; the director receives here.
    pub net_port: u16,
    /// UDP port the agent VM listens on. The director sends to this port.
    pub agent_net_port: u16,
    /// TCP port on host to forward to VM's port 3000
    pub host_http_port: u16,
    pub memory_mb: u32,
    /// If Some, write serial output to this file; if None, use stdio
    pub serial_log: Option<PathBuf>,
    /// If Some, capture NIC0 traffic to this pcap file via filter-dump
    pub pcap_log: Option<PathBuf>,
}

/// Configuration for the agent VM.
pub struct AgentVmConfig {
    /// UDP port the agent VM listens on for the point-to-point tunnel with the director.
    /// The director sends to this port; the agent receives here.
    pub net_port: u16,
    /// UDP port the director VM listens on. The agent sends to this port.
    pub director_net_port: u16,
    /// Paths to pre-created raw disk images
    pub disk_paths: Vec<PathBuf>,
    pub memory_mb: u32,
    /// If Some, write serial output to this file; if None, discard
    pub serial_log: Option<PathBuf>,
}

/// Build QEMU argument list for the director VM.
///
/// The director VM has two NICs:
/// - NIC0: UDP multicast socket network (230.0.0.1:{net_port}, internal, static 10.0.0.1/24)
/// - NIC1: user networking with hostfwd for HTTP access
///
/// Uses UEFI firmware (edk2-x86_64-code.fd) if available to bypass the ~512 MiB
/// SeaBIOS initrd size limit. The director initramfs embeds the squashfs and can
/// exceed 512 MiB.
pub fn director_vm_args(config: &DirectorVmConfig) -> Vec<String> {
    let mut args = vec![
        "-machine".into(),
        "q35".into(),
        "-cpu".into(),
        "Broadwell".into(),
        "-smp".into(),
        "2".into(),
        "-nographic".into(),
        "-no-reboot".into(),
        "-m".into(),
        config.memory_mb.to_string(),
    ];

    args.extend([
        "-kernel".into(),
        config.kernel.to_string_lossy().into_owned(),
        "-initrd".into(),
        config.initramfs.to_string_lossy().into_owned(),
        "-append".into(),
        // no_timer_check: skip the IO-APIC timer test that panics under TCG.
        // Unlike noapic, this keeps the APIC enabled so virtio MSI/MSI-X works.
        "console=ttyS0 quiet no_timer_check earlyprintk=serial,ttyS0".into(),
    ]);

    args.extend(acceleration_args());

    // NIC0: internal network via UDP unicast point-to-point tunnel.
    // UDP avoids TCP connection state loss after a guest reboot (TCP socket,connect
    // drops received frames after the guest resets the NIC). UDP datagrams are
    // stateless and continue working across guest reboots.
    // Director listens on net_port, sends to agent_net_port.
    args.extend([
        "-netdev".into(),
        format!(
            "socket,id=net0,udp=127.0.0.1:{},localaddr=127.0.0.1:{}",
            config.agent_net_port, config.net_port
        ),
        "-device".into(),
        "virtio-net-pci,netdev=net0,mac=52:54:00:00:00:01".into(),
    ]);

    if let Some(pcap) = &config.pcap_log {
        let pcap_str = pcap.to_string_lossy().replace('\\', "/");
        args.extend([
            "-object".into(),
            format!("filter-dump,id=fd0,netdev=net0,file={}", pcap_str),
        ]);
    }

    // NIC1: user networking with hostfwd
    args.extend([
        "-netdev".into(),
        format!(
            "user,id=net1,hostfwd=tcp:127.0.0.1:{}-:3000",
            config.host_http_port
        ),
        "-device".into(),
        "virtio-net-pci,netdev=net1".into(),
    ]);

    // Serial output
    let serial_dest = match &config.serial_log {
        Some(path) => format!("file:{}", path.to_string_lossy().replace('\\', "/")),
        None => "stdio".into(),
    };
    args.extend(["-serial".into(), serial_dest]);

    args
}

/// Build QEMU argument list for the agent VM.
///
/// The agent VM has one NIC on the UDP multicast network (joins 230.0.0.1:{net_port},
/// the same group as the director VM) and one or more virtio disks. The agent boots
/// via PXE from SeaBIOS, which hands off to iPXE served by the director VM.
pub fn agent_vm_args(config: &AgentVmConfig) -> Vec<String> {
    let mut args = vec![
        "-machine".into(),
        "q35".into(),
        "-cpu".into(),
        "Broadwell,pcid=on,tsc-deadline=on".into(),
        "-smp".into(),
        "2".into(),
        "-nographic".into(),
        "-m".into(),
        config.memory_mb.to_string(),
    ];

    args.extend(acceleration_args());

    // Serial output
    let serial_dest = match &config.serial_log {
        Some(path) => format!("file:{}", path.to_string_lossy().replace('\\', "/")),
        None => "null".into(),
    };
    args.extend(["-serial".into(), serial_dest]);

    // NIC0: UDP multicast network (joins 230.0.0.1:{net_port}, same group as director).
    // filter-dump captures all frames to/from net0 to a pcap for debugging.
    let pcap_path = config
        .serial_log
        .as_ref()
        .map(|p| {
            let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or("agent");
            p.parent()
                .unwrap_or(std::path::Path::new("."))
                .join(format!("{}-net0.pcap", stem))
                .to_string_lossy()
                .replace('\\', "/")
        })
        .unwrap_or_else(|| ".build/agent-net0.pcap".to_string());
    // NIC0: UDP unicast tunnel. Agent listens on net_port, sends to director_net_port.
    args.extend([
        "-netdev".into(),
        format!(
            "socket,id=net0,udp=127.0.0.1:{},localaddr=127.0.0.1:{}",
            config.director_net_port, config.net_port
        ),
        "-object".into(),
        format!("filter-dump,id=f1,netdev=net0,file={}", pcap_path),
        "-device".into(),
        "virtio-net-pci,netdev=net0,mac=52:54:00:00:00:02".into(),
    ]);

    // Virtio disks
    for disk_path in &config.disk_paths {
        args.extend([
            "-drive".into(),
            format!("file={},if=virtio,format=raw", disk_path.to_string_lossy()),
        ]);
    }

    args
}

/// Create a sparse raw disk image of the given size.
pub fn create_disk_image(path: &Path, size_bytes: u64) -> Result<()> {
    use std::io::{Seek, SeekFrom, Write};
    let mut f = std::fs::File::create(path)
        .map_err(|e| anyhow!("Failed to create disk image {:?}: {}", path, e))?;
    f.seek(SeekFrom::Start(size_bytes - 1))
        .map_err(|e| anyhow!("Failed to seek in disk image: {}", e))?;
    f.write_all(&[0u8])
        .map_err(|e| anyhow!("Failed to write disk image: {}", e))?;
    Ok(())
}

/// Find an available UDP port in the given range.
pub fn find_available_udp_port(range_start: u16, range_end: u16) -> Result<u16> {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    if range_end <= range_start {
        return Err(anyhow!("Invalid port range: {}-{}", range_start, range_end));
    }
    for _ in 0..100 {
        let port = rng.gen_range(range_start..range_end);
        if UdpSocket::bind(("127.0.0.1", port)).is_ok() {
            return Ok(port);
        }
    }
    Err(anyhow!(
        "Could not find available UDP port in range {}-{}",
        range_start,
        range_end
    ))
}

/// Find an available TCP port in the given range.
pub fn find_available_tcp_port(range_start: u16, range_end: u16) -> Result<u16> {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    if range_end <= range_start {
        return Err(anyhow!("Invalid port range: {}-{}", range_start, range_end));
    }
    for _ in 0..100 {
        let port = rng.gen_range(range_start..range_end);
        if TcpListener::bind(("127.0.0.1", port)).is_ok() {
            return Ok(port);
        }
    }
    Err(anyhow!(
        "Could not find available TCP port in range {}-{}",
        range_start,
        range_end
    ))
}

/// Find the OVMF/EDK2 x86_64 UEFI firmware file for direct-kernel boot.
///
/// Returns `None` if not found; in that case the caller falls back to SeaBIOS.
pub fn find_ovmf_firmware() -> Option<String> {
    let candidates = [
        // QEMU Windows installer default location
        r"C:\Program Files\qemu\share\edk2-x86_64-code.fd",
        // Linux/Fedora/RHEL
        "/usr/share/edk2/x64/OVMF_CODE.fd",
        "/usr/share/OVMF/OVMF_CODE.fd",
        // Debian/Ubuntu
        "/usr/share/ovmf/x64/OVMF.fd",
    ];
    for path in &candidates {
        if std::path::Path::new(path).exists() {
            return Some(path.to_string());
        }
    }
    None
}

/// Return acceleration arguments appropriate for the current platform.
///
/// On Linux with `/dev/kvm`, uses KVM with `-cpu host` for near-native performance.
///
/// On Windows (and other platforms), uses TCG with an Icelake-Server CPU model.
/// WHPX is not used because its XCR0/XSAVE emulation is incomplete (the patch
/// adding XCR0 support was never merged into mainline QEMU), which prevents AVX2
/// from being enabled in the guest. CentOS 10 glibc requires x86-64-v3 (which
/// needs AVX2 via XCR0.YMM), so WHPX guests always panic with
/// "CPU does not support x86-64-v3". TCG fully emulates XCR0 and works correctly.
pub fn acceleration_args() -> Vec<String> {
    if cfg!(target_os = "linux") && std::path::Path::new("/dev/kvm").exists() {
        vec!["-enable-kvm".into(), "-cpu".into(), "host".into()]
    } else {
        // TCG with Icelake-Server for x86-64-v3 support (required by CentOS 10 glibc).
        // Slower than KVM/WHPX but correct on all platforms.
        vec![
            "-accel".into(),
            "tcg".into(),
            "-cpu".into(),
            "Icelake-Server-noTSX".into(),
        ]
    }
}

/// Find the QEMU x86_64 binary in PATH.
pub fn find_qemu_binary() -> Result<String> {
    let candidates = ["qemu-system-x86_64", "qemu-kvm"];
    for candidate in &candidates {
        if which_binary(candidate).is_ok() {
            return Ok(candidate.to_string());
        }
    }
    // Check well-known Windows install path
    #[cfg(target_os = "windows")]
    {
        let windows_path = r"C:\Program Files\qemu\qemu-system-x86_64.exe";
        if std::path::Path::new(windows_path).exists() {
            return Ok(windows_path.to_string());
        }
    }
    Err(anyhow!(
        "QEMU not found in PATH. Install qemu-system-x86_64."
    ))
}

fn which_binary(name: &str) -> Result<()> {
    let cmd = if cfg!(windows) { "where" } else { "which" };
    let status = Command::new(cmd)
        .arg(name)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|e| anyhow!("Failed to run {}: {}", cmd, e))?;
    if status.success() {
        Ok(())
    } else {
        Err(anyhow!("Not found"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_director_vm_args_contains_key_elements() {
        let config = DirectorVmConfig {
            kernel: PathBuf::from("/boot/vmlinuz"),
            initramfs: PathBuf::from("/boot/initramfs.img"),
            net_port: 20000,
            agent_net_port: 20001,
            host_http_port: 30000,
            memory_mb: 256,
            serial_log: None,
            pcap_log: None,
        };
        let args = director_vm_args(&config);
        let args_str = args.join(" ");
        assert!(args_str.contains("-nographic"));
        assert!(args_str.contains("udp=127.0.0.1:20001"));
        assert!(args_str.contains("localaddr=127.0.0.1:20000"));
        assert!(args_str.contains("hostfwd=tcp:127.0.0.1:30000-:3000"));
        assert!(args_str.contains("52:54:00:00:00:01"));
        assert!(args_str.contains("/boot/vmlinuz"));
        assert!(args_str.contains("/boot/initramfs.img"));
        assert!(args_str.contains("console=ttyS0 quiet"));
    }

    #[test]
    fn test_agent_vm_args_contains_key_elements() {
        let config = AgentVmConfig {
            net_port: 20001,
            director_net_port: 20000,
            disk_paths: vec![PathBuf::from("/tmp/disk0.img")],
            memory_mb: 512,
            serial_log: None,
        };
        let args = agent_vm_args(&config);
        let args_str = args.join(" ");
        assert!(args_str.contains("q35"));
        assert!(args_str.contains("udp=127.0.0.1:20000"));
        assert!(args_str.contains("localaddr=127.0.0.1:20001"));
        assert!(args_str.contains("52:54:00:00:00:02"));
        assert!(args_str.contains("/tmp/disk0.img"));
        assert!(args_str.contains("serial"));
        assert!(args_str.contains("null"));
    }

    #[test]
    fn test_create_disk_image() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.img");
        let size = 10 * 1024 * 1024; // 10 MiB
        create_disk_image(&path, size).unwrap();
        let metadata = std::fs::metadata(&path).unwrap();
        assert_eq!(metadata.len(), size);
    }

    #[test]
    fn test_find_available_udp_port() {
        let port = find_available_udp_port(20000, 29999).unwrap();
        assert!((20000..=29999).contains(&port));
        // Verify it's actually bindable
        UdpSocket::bind(("127.0.0.1", port)).unwrap();
    }

    #[test]
    fn test_find_available_tcp_port() {
        let port = find_available_tcp_port(30000, 39999).unwrap();
        assert!((30000..=39999).contains(&port));
    }
}
