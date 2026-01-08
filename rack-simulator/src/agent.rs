use anyhow::Result;
use serde_json::json;

use crate::ConnectionConfig;
use crate::hardware_profiles::HardwareConfig;
use crate::http::HttpClient;
use crate::output::Output;
use crate::server::ServerState;

pub async fn run(conn: &ConnectionConfig, state: &ServerState, output: &Output) -> Result<()> {
    output.step("AGENT SIMULATION");
    output.detail("UUID", &state.uuid);
    output.detail(
        "Manufacturer",
        state.hardware.manufacturer.as_deref().unwrap_or("Unknown"),
    );
    output.detail(
        "Product",
        state.hardware.product_name.as_deref().unwrap_or("Unknown"),
    );

    let http = HttpClient::new(conn);

    output.info("Building hardware attributes...");
    let attributes = build_attributes(&state.hardware, &state.server_name);

    output.info(&format!("Uploading {} attributes...", attributes.len()));
    http.update_attributes(&state.uuid, attributes, output)
        .await?;

    output.info("Reporting action success...");
    http.action_success(&state.uuid, output).await?;

    output.success("Agent simulation complete");

    Ok(())
}

fn build_attributes(
    hardware: &HardwareConfig,
    server_name: &str,
) -> serde_json::Map<String, serde_json::Value> {
    let mut attrs = serde_json::Map::new();

    if let Some(manufacturer) = &hardware.manufacturer {
        attrs.insert("manufacturer".to_string(), json!(manufacturer));
    }

    if let Some(product_name) = &hardware.product_name {
        attrs.insert("product_name".to_string(), json!(product_name));
    }

    let serial = hardware
        .serial_number
        .clone()
        .unwrap_or_else(|| generate_serial(server_name));
    attrs.insert("serial_number".to_string(), json!(serial));

    if let Some(bios_version) = &hardware.bios_version {
        attrs.insert("bios_version".to_string(), json!(bios_version));
    }

    if let Some(bios_vendor) = &hardware.bios_vendor {
        attrs.insert("bios_vendor".to_string(), json!(bios_vendor));
    }

    let processor_count = hardware.processor_count.unwrap_or(1);
    let cores = hardware.cores_per_processor.unwrap_or(8);
    let threads = hardware.threads_per_core.unwrap_or(2);

    let processors: Vec<_> = (0..processor_count)
        .map(|i| {
            json!({
                "designation": format!("CPU{}", i),
                "manufacturer": null,
                "version": null,
                "max_speed_mhz": 2400,
                "core_count": cores,
                "thread_count": cores * threads,
            })
        })
        .collect();
    attrs.insert("processors".to_string(), json!(processors));

    let dimm_count = hardware.memory_dimm_count.unwrap_or(4);
    let dimm_size = hardware.memory_dimm_size_mb.unwrap_or(8192);
    let dimm_speed = hardware.memory_speed_mhz.unwrap_or(2400);

    let memory_devices: Vec<_> = (0..dimm_count)
        .map(|i| {
            json!({
                "size_mb": dimm_size,
                "speed_mhz": dimm_speed,
                "manufacturer": "Samsung",
                "part_number": format!("M393A2K40DB3-CWE-{}", i),
            })
        })
        .collect();
    attrs.insert("memory_devices".to_string(), json!(memory_devices));

    let total_memory = hardware
        .total_memory_mb
        .unwrap_or((dimm_count as u64) * (dimm_size as u64));
    attrs.insert("total_memory_mb".to_string(), json!(total_memory));

    attrs
}

fn generate_serial(seed: &str) -> String {
    let hash = simple_hash(seed.as_bytes());
    format!("SN{:08X}", hash)
}

fn simple_hash(data: &[u8]) -> u32 {
    let mut hash: u32 = 5381;
    for byte in data {
        hash = hash.wrapping_mul(33).wrapping_add(*byte as u32);
    }
    hash
}
