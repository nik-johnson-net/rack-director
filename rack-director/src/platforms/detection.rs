use crate::database::Connection;
use crate::platforms::cpu_model::platform_name_processor_version;

use super::{DiskType, PlatformAttributes, PlatformCpu, PlatformDisk, PlatformNic};
use anyhow::Result;
use common::device_attributes::{CpuInfo, DiskInfo, MemoryInfo, NetworkInterface};

/// Auto-detect or create a platform for a device based on its hardware attributes
///
/// This function:
/// 1. Converts device hardware attributes to platform format
/// 2. Attempts to find an existing matching platform
/// 3. If no match found, creates a new platform with auto-generated name
/// 4. Assigns labels to disks and NICs based on heuristics
///
/// Returns the platform ID
pub async fn detect_or_create_platform(
    conn: &Connection,
    disks: &[DiskInfo],
    nics: &[NetworkInterface],
    cpus: &[CpuInfo],
    memory: &[MemoryInfo],
) -> Result<i64> {
    // Convert device hardware to platform attributes
    let mut platform_attrs = convert_device_hardware_to_platform(disks, nics, cpus, memory);

    // Assign labels to disks and NICs
    assign_disk_labels(&mut platform_attrs.disks);
    assign_nic_labels(&mut platform_attrs.nics);

    // Try to find existing matching platform
    if let Some(platform_id) = find_matching_platform(conn, &platform_attrs).await? {
        log::info!("Found matching platform ID: {}", platform_id);
        return Ok(platform_id);
    }

    // No match found, create new platform with auto-generated name
    let platform_name = generate_platform_name(&platform_attrs);
    log::info!("Creating new platform: {}", platform_name);

    let platform = super::store::create(
        conn,
        &platform_name,
        Some("Auto-detected platform"),
        &platform_attrs,
    )
    .await?;

    Ok(platform.id.unwrap())
}

/// Find a matching platform based on hardware attributes
/// Used for auto-detection during hardware discovery
/// Returns platform ID if a match is found
pub async fn find_matching_platform(
    conn: &Connection,
    device_attrs: &PlatformAttributes,
) -> Result<Option<i64>> {
    let platforms = super::store::list(conn).await?;

    for platform in platforms {
        if is_platform_match(&platform.attributes, device_attrs) {
            return Ok(platform.id);
        }
    }

    Ok(None)
}

/// Sort disks by canonical hardware characteristics, not by bus path.
/// This ensures consistent ordering across devices with identical hardware
/// in different physical slots.
///
/// Sort order: (disk_type.priority(), size_gb)
/// - Primary: Disk type (NVMe < SSD < HDD) — ensures fastest disks come first
/// - Secondary: Size (smaller first) — ROOT should be smallest
///
/// For disks with identical type and size, the stable sort preserves discovery order.
/// We intentionally do NOT use path as a tiebreaker, since path varies by PCI slot
/// and would make label assignment inconsistent across identical hardware.
pub(crate) fn sort_disks_canonical(disks: &mut [PlatformDisk]) {
    disks.sort_by(|a, b| {
        a.disk_type
            .priority()
            .cmp(&b.disk_type.priority())
            .then_with(|| a.size_gb.cmp(&b.size_gb))
    });
}

/// Check if device hardware matches a platform configuration
/// Applies tolerances for size and memory comparisons
fn is_platform_match(platform: &PlatformAttributes, device: &PlatformAttributes) -> bool {
    // Check disk count and configuration
    if platform.disks.len() != device.disks.len() {
        return false;
    }

    // Sort both disk lists by canonical hardware characteristics
    let mut platform_disks = platform.disks.clone();
    let mut device_disks = device.disks.clone();
    sort_disks_canonical(&mut platform_disks);
    sort_disks_canonical(&mut device_disks);

    // Check each disk matches (type and size with tolerance)
    // Note: We do NOT compare paths - only hardware characteristics
    for (p_disk, d_disk) in platform_disks.iter().zip(device_disks.iter()) {
        if p_disk.disk_type != d_disk.disk_type {
            return false;
        }

        // Size tolerance: +/- 5%
        let size_diff = p_disk.size_gb.abs_diff(d_disk.size_gb);
        let tolerance = (p_disk.size_gb as f64 * 0.05) as u64;
        if size_diff > tolerance {
            return false;
        }
    }

    // Check NIC count
    if platform.nics.len() != device.nics.len() {
        return false;
    }

    // Check NIC speeds (if specified)
    let mut platform_nics = platform.nics.clone();
    let mut device_nics = device.nics.clone();
    platform_nics.sort_by(|a, b| a.logical.cmp(&b.logical));
    device_nics.sort_by(|a, b| a.logical.cmp(&b.logical));

    for (p_nic, d_nic) in platform_nics.iter().zip(device_nics.iter()) {
        // Only compare speeds if both are specified
        if let (Some(p_speed), Some(d_speed)) = (p_nic.speed_mbps, d_nic.speed_mbps) {
            // Allow 10% tolerance for speed comparison (e.g., 1000 vs 1100 Mbps)
            let diff = p_speed.abs_diff(d_speed);
            let tolerance = (p_speed as f64 * 0.10) as u32;
            if diff > tolerance {
                return false;
            }
        }
    }

    // Check CPU count and configuration
    if platform.cpus.len() != device.cpus.len() {
        return false;
    }

    // Sort CPUs for comparison
    let mut platform_cpus = platform.cpus.clone();
    let mut device_cpus = device.cpus.clone();
    platform_cpus.sort_by(|a, b| a.model.cmp(&b.model));
    device_cpus.sort_by(|a, b| a.model.cmp(&b.model));

    for (p_cpu, d_cpu) in platform_cpus.iter().zip(device_cpus.iter()) {
        if p_cpu.brand != d_cpu.brand || p_cpu.model != d_cpu.model || p_cpu.cores != d_cpu.cores {
            return false;
        }
    }

    // Check memory with tolerance (+/- 1 GiB)
    let memory_diff = platform.memory_gib.abs_diff(device.memory_gib);
    if memory_diff > 1 {
        return false;
    }

    true
}

/// Convert device hardware attributes to platform attributes
fn convert_device_hardware_to_platform(
    disks: &[DiskInfo],
    nics: &[NetworkInterface],
    cpus: &[CpuInfo],
    memory: &[MemoryInfo],
) -> PlatformAttributes {
    let platform_disks = disks
        .iter()
        .filter_map(|disk| {
            // Get size in GB (already parsed as u64 by rack-agent)
            let size_gb = disk.size?;

            // Get disk type (required)
            let disk_type = disk.disk_type?;

            Some(PlatformDisk {
                size_gb,
                disk_type,
                label: None, // Will be assigned later
            })
        })
        .collect();

    let platform_nics = nics
        .iter()
        .map(|nic| PlatformNic {
            logical: nic.interface_name.clone(),
            speed_mbps: nic.speed_mbps, // Read from sysfs during hardware scan
            label: None,                // Will be assigned later
        })
        .collect();

    let platform_cpus = cpus
        .iter()
        .filter_map(|cpu| {
            let brand = extract_cpu_brand(cpu.manufacturer.as_ref()?)?;
            let model = cpu.model.clone()?;
            let cores = cpu.cores?;

            Some(PlatformCpu {
                brand: brand.to_string(),
                model,
                cores,
            })
        })
        .collect();

    // Calculate total memory in GiB
    let memory_gib = calculate_total_memory_gib(memory);

    PlatformAttributes {
        disks: platform_disks,
        nics: platform_nics,
        cpus: platform_cpus,
        memory_gib,
    }
}

/// Assign labels to disks based on heuristics
/// ROOT = smallest + fastest disk (first disk after canonical sort)
/// Others = DATA1, DATA2, ... in canonical order
fn assign_disk_labels(disks: &mut [PlatformDisk]) {
    if disks.is_empty() {
        return;
    }

    // Sort disks by canonical hardware characteristics
    sort_disks_canonical(disks);

    // After canonical sort, the first disk is the smallest + fastest (ROOT)
    disks[0].label = Some("ROOT".to_string());

    // Assign DATA labels to remaining disks in canonical order
    for (i, disk) in disks.iter_mut().enumerate().skip(1) {
        disk.label = Some(format!("DATA{}", i));
    }
}

/// Assign labels to NICs in bus order
/// NIC1, NIC2, ... sorted by logical name
fn assign_nic_labels(nics: &mut [PlatformNic]) {
    // Sort NICs by logical name for consistent ordering
    nics.sort_by(|a, b| a.logical.cmp(&b.logical));

    for (i, nic) in nics.iter_mut().enumerate() {
        nic.label = Some(format!("NIC{}", i + 1));
    }
}

/// Extract CPU brand from manufacturer string
fn extract_cpu_brand(manufacturer: &str) -> Option<&str> {
    let lower = manufacturer.to_lowercase();
    if lower.contains("intel") {
        Some("intel")
    } else if lower.contains("amd") {
        Some("amd")
    } else {
        None
    }
}

/// Calculate total memory in GiB from memory modules
fn calculate_total_memory_gib(memory: &[MemoryInfo]) -> u32 {
    let total_mb: u64 = memory
        .iter()
        .filter_map(|mem| mem.size_mb.map(|s| s as u64))
        .sum();

    // Convert MB to GiB (1 GiB = 1024 MiB)
    (total_mb / 1024) as u32
}

/// Generate a platform name based on hardware configuration
fn generate_platform_name(attrs: &PlatformAttributes) -> String {
    let cpu_desc = if !attrs.cpus.is_empty() {
        let model = platform_name_processor_version(&attrs.cpus[0].model);
        format!("{}x{}", attrs.cpus.len(), model)
    } else {
        "UnknownCPU".to_string()
    };

    let mem_desc = format!("{}GB", attrs.memory_gib);

    let disk_summary = if !attrs.disks.is_empty() {
        // Create descriptors with size+type (e.g., "480SSD", "1000HDD")
        let disk_descriptors: Vec<String> = attrs
            .disks
            .iter()
            .map(|d| {
                let type_str = match d.disk_type {
                    DiskType::Nvme => "NVMe",
                    DiskType::Ssd => "SSD",
                    DiskType::Hdd => "HDD",
                };
                format!("{}{}", d.size_gb, type_str)
            })
            .collect();

        // Group descriptors and count occurrences
        let mut descriptor_counts = std::collections::HashMap::new();
        for descriptor in disk_descriptors {
            *descriptor_counts.entry(descriptor).or_insert(0) += 1;
        }

        // Format as "{count}x{descriptor}" and sort for consistency
        let mut disk_desc: Vec<String> = descriptor_counts
            .iter()
            .map(|(descriptor, count)| format!("{}x{}", count, descriptor))
            .collect();
        disk_desc.sort();

        disk_desc.join("+")
    } else {
        "NoDisks".to_string()
    };

    format!("{}-{}-{}", cpu_desc, mem_desc, disk_summary)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{database, test_database_path};
    use std::sync::Arc;

    async fn setup_db(path: String) -> Arc<crate::database::Connection> {
        let factory = database::DatabaseConnectionFactory::new(std::path::PathBuf::from(path));
        Arc::new(database::run_migrations(&factory).await.unwrap())
    }

    fn sample_platform_attributes() -> PlatformAttributes {
        PlatformAttributes {
            disks: vec![
                PlatformDisk {
                    size_gb: 480,
                    disk_type: DiskType::Ssd,
                    label: Some("ROOT".to_string()),
                },
                PlatformDisk {
                    size_gb: 2000,
                    disk_type: DiskType::Hdd,
                    label: Some("DATA1".to_string()),
                },
            ],
            nics: vec![
                PlatformNic {
                    logical: "eno1".to_string(),
                    speed_mbps: Some(10000),
                    label: Some("NIC1".to_string()),
                },
                PlatformNic {
                    logical: "eno2".to_string(),
                    speed_mbps: Some(10000),
                    label: Some("NIC2".to_string()),
                },
            ],
            cpus: vec![PlatformCpu {
                brand: "intel".to_string(),
                model: "E3-1240 v3".to_string(),
                cores: 4,
            }],
            memory_gib: 32,
        }
    }

    #[test]
    fn test_calculate_total_memory_gib() {
        let memory = vec![
            MemoryInfo {
                size_mb: Some(16384),
                speed_mhz: None,
                manufacturer: None,
                part_number: None,
            },
            MemoryInfo {
                size_mb: Some(16384),
                speed_mhz: None,
                manufacturer: None,
                part_number: None,
            },
        ];

        // 16384 MB * 2 = 32768 MB = 32 GiB
        assert_eq!(calculate_total_memory_gib(&memory), 32);
    }

    #[test]
    fn test_extract_cpu_brand() {
        assert_eq!(extract_cpu_brand("Intel Corporation"), Some("intel"));
        assert_eq!(extract_cpu_brand("AMD"), Some("amd"));
        assert_eq!(extract_cpu_brand("GenuineIntel"), Some("intel"));
        assert_eq!(extract_cpu_brand("Unknown"), None);
    }

    #[test]
    fn test_assign_disk_labels() {
        let mut disks = vec![
            PlatformDisk {
                size_gb: 2000,
                disk_type: DiskType::Hdd,
                label: None,
            },
            PlatformDisk {
                size_gb: 480,
                disk_type: DiskType::Nvme,
                label: None,
            },
            PlatformDisk {
                size_gb: 2000,
                disk_type: DiskType::Hdd,
                label: None,
            },
        ];

        assign_disk_labels(&mut disks);

        // After canonical sort: NVMe (fastest) should be ROOT
        let nvme_disk = disks
            .iter()
            .find(|d| d.disk_type == DiskType::Nvme)
            .unwrap();
        assert_eq!(nvme_disk.label, Some("ROOT".to_string()));

        // HDDs should be DATA1 and DATA2
        let hdd_disks: Vec<_> = disks
            .iter()
            .filter(|d| d.disk_type == DiskType::Hdd)
            .collect();
        assert_eq!(hdd_disks.len(), 2);
        for hdd in &hdd_disks {
            assert!(
                hdd.label == Some("DATA1".to_string()) || hdd.label == Some("DATA2".to_string())
            );
        }
    }

    #[test]
    fn test_assign_nic_labels() {
        let mut nics = vec![
            PlatformNic {
                logical: "eno2".to_string(),
                speed_mbps: None,
                label: None,
            },
            PlatformNic {
                logical: "eno1".to_string(),
                speed_mbps: None,
                label: None,
            },
        ];

        assign_nic_labels(&mut nics);

        // Should be sorted and labeled in order
        assert_eq!(nics[0].label, Some("NIC1".to_string()));
        assert_eq!(nics[1].label, Some("NIC2".to_string()));
        assert_eq!(nics[0].logical, "eno1"); // Sorted alphabetically
    }

    #[test]
    fn test_generate_platform_name() {
        let attrs = PlatformAttributes {
            disks: vec![
                PlatformDisk {
                    size_gb: 480,
                    disk_type: DiskType::Ssd,
                    label: Some("ROOT".to_string()),
                },
                PlatformDisk {
                    size_gb: 2000,
                    disk_type: DiskType::Hdd,
                    label: Some("DATA1".to_string()),
                },
            ],
            nics: vec![],
            cpus: vec![PlatformCpu {
                brand: "intel".to_string(),
                model: "E3-1240 v3".to_string(),
                cores: 4,
            }],
            memory_gib: 32,
        };

        let name = generate_platform_name(&attrs);
        // Expected format: "1xE31240v3-32GB-1x2000HDD+1x480SSD"
        // (sorted alphabetically: HDD before SSD)
        assert_eq!(name, "1xE31240v3-32GB-1x2000HDD+1x480SSD");
    }

    #[test]
    fn test_generate_platform_name_single_disk() {
        let attrs = PlatformAttributes {
            disks: vec![PlatformDisk {
                size_gb: 480,
                disk_type: DiskType::Ssd,
                label: Some("ROOT".to_string()),
            }],
            nics: vec![],
            cpus: vec![PlatformCpu {
                brand: "intel".to_string(),
                model: "Intel(R) Xeon(R) E3-1240 v3 @ 3.40 GHz".to_string(),
                cores: 4,
            }],
            memory_gib: 32,
        };

        let name = generate_platform_name(&attrs);
        assert_eq!(name, "1xIntelXeonE31240v3-32GB-1x480SSD");
    }

    #[test]
    fn test_generate_platform_name_multiple_identical_disks() {
        let attrs = PlatformAttributes {
            disks: vec![
                PlatformDisk {
                    size_gb: 960,
                    disk_type: DiskType::Nvme,
                    label: Some("ROOT".to_string()),
                },
                PlatformDisk {
                    size_gb: 960,
                    disk_type: DiskType::Nvme,
                    label: Some("DATA1".to_string()),
                },
            ],
            nics: vec![],
            cpus: vec![PlatformCpu {
                brand: "amd".to_string(),
                model: "EPYC 7502".to_string(),
                cores: 32,
            }],
            memory_gib: 128,
        };

        let name = generate_platform_name(&attrs);
        assert_eq!(name, "1xEPYC7502-128GB-2x960NVMe");
    }

    #[test]
    fn test_generate_platform_name_no_disks() {
        let attrs = PlatformAttributes {
            disks: vec![],
            nics: vec![],
            cpus: vec![PlatformCpu {
                brand: "intel".to_string(),
                model: "Intel(R) Xeon(R) E3-1240 v3".to_string(),
                cores: 4,
            }],
            memory_gib: 32,
        };

        let name = generate_platform_name(&attrs);
        assert_eq!(name, "1xIntelXeonE31240v3-32GB-NoDisks");
    }

    #[test]
    fn test_generate_platform_name_mixed_disks() {
        let attrs = PlatformAttributes {
            disks: vec![
                PlatformDisk {
                    size_gb: 480,
                    disk_type: DiskType::Ssd,
                    label: Some("ROOT".to_string()),
                },
                PlatformDisk {
                    size_gb: 1000,
                    disk_type: DiskType::Hdd,
                    label: Some("DATA1".to_string()),
                },
                PlatformDisk {
                    size_gb: 960,
                    disk_type: DiskType::Nvme,
                    label: Some("DATA2".to_string()),
                },
            ],
            nics: vec![],
            cpus: vec![PlatformCpu {
                brand: "intel".to_string(),
                model: "Intel(R) Xeon(R) E3-1240 v3".to_string(),
                cores: 4,
            }],
            memory_gib: 64,
        };

        let name = generate_platform_name(&attrs);
        // Sorted alphabetically: 1000HDD, 480SSD, 960NVMe
        assert_eq!(
            name,
            "1xIntelXeonE31240v3-64GB-1x1000HDD+1x480SSD+1x960NVMe"
        );
    }

    #[tokio::test]
    async fn test_find_matching_platform_exact_match() {
        let db = setup_db(test_database_path!()).await;

        let attrs = sample_platform_attributes();
        let platform = crate::platforms::store::create(&db, "Test Platform", None::<&str>, &attrs)
            .await
            .unwrap();

        // Search with exact same attributes
        let device_attrs = sample_platform_attributes();
        let found = find_matching_platform(&db, &device_attrs).await.unwrap();

        assert_eq!(found, platform.id);
    }

    #[tokio::test]
    async fn test_find_matching_platform_with_size_tolerance() {
        let db = setup_db(test_database_path!()).await;

        let attrs = sample_platform_attributes();
        let platform = crate::platforms::store::create(&db, "Test Platform", None::<&str>, &attrs)
            .await
            .unwrap();

        // Device with slightly different disk size (within 5% tolerance)
        let mut device_attrs = sample_platform_attributes();
        device_attrs.disks[0].size_gb = 475; // 480 - 5 = within 5%
        device_attrs.disks[1].size_gb = 2050; // 2000 + 50 = within 5%

        let found = find_matching_platform(&db, &device_attrs).await.unwrap();

        assert_eq!(found, platform.id);
    }

    #[tokio::test]
    async fn test_find_matching_platform_with_memory_tolerance() {
        let db = setup_db(test_database_path!()).await;

        let attrs = sample_platform_attributes();
        let platform = crate::platforms::store::create(&db, "Test Platform", None::<&str>, &attrs)
            .await
            .unwrap();

        // Device with slightly different memory (within 1 GiB tolerance)
        let mut device_attrs = sample_platform_attributes();
        device_attrs.memory_gib = 33; // 32 + 1 = within tolerance

        let found = find_matching_platform(&db, &device_attrs).await.unwrap();

        assert_eq!(found, platform.id);
    }

    #[tokio::test]
    async fn test_find_matching_platform_no_match_disk_count() {
        let db = setup_db(test_database_path!()).await;

        let attrs = sample_platform_attributes();
        crate::platforms::store::create(&db, "Test Platform", None::<&str>, &attrs)
            .await
            .unwrap();

        // Device with different number of disks
        let mut device_attrs = sample_platform_attributes();
        device_attrs.disks.pop();

        let found = find_matching_platform(&db, &device_attrs).await.unwrap();

        assert!(found.is_none());
    }

    #[tokio::test]
    async fn test_find_matching_platform_no_match_disk_type() {
        let db = setup_db(test_database_path!()).await;

        let attrs = sample_platform_attributes();
        crate::platforms::store::create(&db, "Test Platform", None::<&str>, &attrs)
            .await
            .unwrap();

        // Device with different disk type
        let mut device_attrs = sample_platform_attributes();
        device_attrs.disks[0].disk_type = DiskType::Nvme;

        let found = find_matching_platform(&db, &device_attrs).await.unwrap();

        assert!(found.is_none());
    }

    #[tokio::test]
    async fn test_find_matching_platform_no_match_nic_count() {
        let db = setup_db(test_database_path!()).await;

        let attrs = sample_platform_attributes();
        crate::platforms::store::create(&db, "Test Platform", None::<&str>, &attrs)
            .await
            .unwrap();

        // Device with different number of NICs
        let mut device_attrs = sample_platform_attributes();
        device_attrs.nics.pop();

        let found = find_matching_platform(&db, &device_attrs).await.unwrap();

        assert!(found.is_none());
    }

    #[tokio::test]
    async fn test_find_matching_platform_no_match_cpu_config() {
        let db = setup_db(test_database_path!()).await;

        let attrs = sample_platform_attributes();
        crate::platforms::store::create(&db, "Test Platform", None::<&str>, &attrs)
            .await
            .unwrap();

        // Device with different CPU cores
        let mut device_attrs = sample_platform_attributes();
        device_attrs.cpus[0].cores = 8;

        let found = find_matching_platform(&db, &device_attrs).await.unwrap();

        assert!(found.is_none());
    }

    #[test]
    fn test_is_platform_match_exact() {
        let platform = sample_platform_attributes();
        let device = sample_platform_attributes();

        assert!(is_platform_match(&platform, &device));
    }

    #[test]
    fn test_is_platform_match_disk_size_tolerance() {
        let platform = sample_platform_attributes();
        let mut device = sample_platform_attributes();

        // Within 5% tolerance
        device.disks[0].size_gb = 475; // 480 - 5
        assert!(is_platform_match(&platform, &device));

        // Outside tolerance
        device.disks[0].size_gb = 450; // More than 5% diff
        assert!(!is_platform_match(&platform, &device));
    }

    #[test]
    fn test_is_platform_match_memory_tolerance() {
        let platform = sample_platform_attributes();
        let mut device = sample_platform_attributes();

        // Within 1 GiB tolerance
        device.memory_gib = 33;
        assert!(is_platform_match(&platform, &device));

        // Outside tolerance
        device.memory_gib = 35;
        assert!(!is_platform_match(&platform, &device));
    }

    #[test]
    fn test_is_platform_match_different_disk_count() {
        let platform = sample_platform_attributes();
        let mut device = sample_platform_attributes();
        device.disks.pop();

        assert!(!is_platform_match(&platform, &device));
    }

    #[test]
    fn test_is_platform_match_different_disk_type() {
        let platform = sample_platform_attributes();
        let mut device = sample_platform_attributes();
        device.disks[0].disk_type = DiskType::Nvme;

        assert!(!is_platform_match(&platform, &device));
    }

    #[test]
    fn test_canonical_disk_sorting() {
        let mut disks = vec![
            PlatformDisk {
                size_gb: 2000,
                disk_type: DiskType::Hdd,
                label: None,
            },
            PlatformDisk {
                size_gb: 480,
                disk_type: DiskType::Ssd,
                label: None,
            },
            PlatformDisk {
                size_gb: 1000,
                disk_type: DiskType::Nvme,
                label: None,
            },
        ];

        sort_disks_canonical(&mut disks);

        // After canonical sort: NVMe (priority=1) < SSD (priority=2) < HDD (priority=3)
        assert_eq!(disks[0].disk_type, DiskType::Nvme);
        assert_eq!(disks[0].size_gb, 1000);
        assert_eq!(disks[1].disk_type, DiskType::Ssd);
        assert_eq!(disks[1].size_gb, 480);
        assert_eq!(disks[2].disk_type, DiskType::Hdd);
        assert_eq!(disks[2].size_gb, 2000);
    }

    /// Two identical hardware configurations with different PCIe bus topologies must still match.
    ///
    /// Since `path` is no longer stored in `PlatformDisk`, matching is purely by hardware
    /// characteristics (disk_type and size_gb), so topology differences are invisible to
    /// the matching algorithm.
    #[test]
    fn test_platform_match_identical_hardware_different_topology() {
        let platform_disks = vec![
            PlatformDisk {
                size_gb: 480,
                disk_type: DiskType::Ssd,
                label: Some("ROOT".to_string()),
            },
            PlatformDisk {
                size_gb: 2000,
                disk_type: DiskType::Hdd,
                label: Some("DATA1".to_string()),
            },
        ];

        // Identical hardware, but would have been different PCI paths on the old implementation
        let device_disks = vec![
            PlatformDisk {
                size_gb: 480,
                disk_type: DiskType::Ssd,
                label: None,
            },
            PlatformDisk {
                size_gb: 2000,
                disk_type: DiskType::Hdd,
                label: None,
            },
        ];

        let platform_attrs = PlatformAttributes {
            disks: platform_disks,
            nics: vec![],
            cpus: vec![],
            memory_gib: 32,
        };
        let device_attrs = PlatformAttributes {
            disks: device_disks,
            nics: vec![],
            cpus: vec![],
            memory_gib: 32,
        };

        // Should match because hardware class (type + size) is identical
        assert!(is_platform_match(&platform_attrs, &device_attrs));
    }

    #[test]
    fn test_assign_disk_labels_canonical_order() {
        let mut attrs = PlatformAttributes {
            disks: vec![
                PlatformDisk {
                    size_gb: 2000,
                    disk_type: DiskType::Hdd,
                    label: None,
                },
                PlatformDisk {
                    size_gb: 480,
                    disk_type: DiskType::Ssd,
                    label: None,
                },
            ],
            nics: vec![],
            cpus: vec![],
            memory_gib: 32,
        };

        assign_disk_labels(&mut attrs.disks);

        // ROOT should be assigned to the SSD (smallest + fastest)
        let root_disk = attrs
            .disks
            .iter()
            .find(|d| d.label.as_deref() == Some("ROOT"));
        assert!(root_disk.is_some());
        assert_eq!(root_disk.unwrap().disk_type, DiskType::Ssd);
        assert_eq!(root_disk.unwrap().size_gb, 480);

        // DATA1 should be the HDD
        let data1_disk = attrs
            .disks
            .iter()
            .find(|d| d.label.as_deref() == Some("DATA1"));
        assert!(data1_disk.is_some());
        assert_eq!(data1_disk.unwrap().disk_type, DiskType::Hdd);
        assert_eq!(data1_disk.unwrap().size_gb, 2000);
    }

    /// Verify that platform creation produces PlatformDisk structs with no path field.
    ///
    /// This tests the full `detect_or_create_platform` path to confirm that the
    /// conversion from `DiskInfo` to `PlatformDisk` never stores a path.
    #[tokio::test]
    async fn test_detect_or_create_platform_produces_no_path() {
        use common::device_attributes::{DiskInfo, MemoryInfo, NetworkInterface};

        let db = setup_db(test_database_path!()).await;

        let disks = vec![DiskInfo {
            name: "sda".to_string(),
            path: Some("/dev/disk/by-path/pci-0000:00:1f.2-ata-1".to_string()),
            size: Some(480),
            disk_type: Some(DiskType::Ssd),
            model: None,
            serial: None,
            vendor: None,
            uuid: None,
        }];
        let nics = vec![NetworkInterface {
            interface_name: "eth0".to_string(),
            mac_address: "aa:bb:cc:dd:ee:ff".to_string(),
            speed_mbps: Some(1000),
            ip_address: None,
            network_id: None,
            disabled: false,
            warning_label: None,
        }];
        let cpus = vec![CpuInfo {
            manufacturer: Some("Intel Corporation".to_string()),
            model: Some("E3-1240 v3".to_string()),
            cores: Some(4),
            designation: None,
            threads: None,
            speed_mhz: None,
        }];
        let memory = vec![MemoryInfo {
            size_mb: Some(32768),
            speed_mhz: None,
            manufacturer: None,
            part_number: None,
        }];

        let platform_id = detect_or_create_platform(&db, &disks, &nics, &cpus, &memory)
            .await
            .unwrap();

        let platform = crate::platforms::store::get(&db, platform_id)
            .await
            .unwrap();
        assert_eq!(platform.attributes.disks.len(), 1);

        // The created PlatformDisk must carry hardware class only — no path
        let disk = &platform.attributes.disks[0];
        assert_eq!(disk.size_gb, 480);
        assert_eq!(disk.disk_type, DiskType::Ssd);
        // label is assigned by assign_disk_labels
        assert_eq!(disk.label, Some("ROOT".to_string()));
    }
}
