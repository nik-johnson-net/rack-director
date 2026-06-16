import { redirect } from "react-router";

export type Plan = {

}

export type ValidationErrors = Record<string, string>;

export class ValidationError extends Error {
  errors: ValidationErrors;

  constructor(errors: ValidationErrors) {
    super('Validation failed');
    this.name = 'ValidationError';
    this.errors = errors;
  }
}

/**
 * Generic error handler for API responses
 * Parses validation errors from 400 responses
 * Can be reused by all API client functions
 */
export async function handleApiError(response: Response, defaultMessage: string): Promise<never> {
  if (response.status === 400) {
    // Try to parse as validation error
    const errorData = await response.json().catch(() => null);
    if (errorData && errorData.errors) {
      throw new ValidationError(errorData.errors);
    }
    if (errorData && errorData.error) {
      throw new Error(errorData.error);
    }
  }

  // Fallback to generic error
  console.error(`API error (${response.status}):`, response.statusText);
  throw new Error(defaultMessage);
}

export type DeviceLifecycle = "new" | "unprovisioned" | "provisioned" | "removed" | "broken";

export type NetworkInterface = {
  interface_name: string;
  mac_address: string;
  ip_address?: string;
  network_id?: number;
  disabled?: boolean;
  warning_label?: string;
}

export type BmcInfo = {
  mac_address: string;
  ip_address?: string;
  ip_address_source: string;
}

export type BmcConfig = {
  ip_address_source: string; // "static" or "dhcp"
  ip_address?: string;
  netmask?: string;
  gateway?: string;
}

export type Device = {
  uuid: string;
  architecture: Architecture;
  lifecycle?: DeviceLifecycle;
  role_id?: number;
  platform_id?: number;
  attributes: {
    hostname?: string;
    network_interfaces?: NetworkInterface[];
    bmc?: BmcInfo;
    bmc_config?: BmcConfig;
    disk_label_overrides?: Record<string, string>;
    // Legacy fields
    mac_address?: string;
    static_ip?: string;
    [key: string]: any;
  };
  created_at?: string;
  first_seen_at?: string;
  last_seen_at?: string;
}

export type DeviceWithRole = Device & {
  role_name?: string;
  role_description?: string;
  os_name?: string;
  os_version?: string;
}

export type DevicesIndex = {
  devices: Device[]
}

export type DhcpLease = {
  id: number;
  mac_address: string;
  ip_address: string;
  device_uuid?: string;
  lease_start?: string;
  lease_end?: string;
  state?: string;
  network_id?: number;
  hostname?: string;
}

export type PendingDevice = {
  id: number;
  mac_address: string;
  device_uuid?: string;
  network_id: number;
  created_at: string;
  completed_at?: string;
}

export type CreatePendingDeviceRequest = {
  mac_address: string;
  network_id: number;
}

export type DhcpNetwork = {
  id: number;
  name: string;
  subnet: string;
  gateway: string;
  dns_servers: string[];
  lease_duration: number;
  relay_agent_address?: string;
  enable_autodiscovery: boolean;
  created_at: string;
  updated_at: string;
}

export type DhcpPool = {
  id: number;
  network_id: number;
  name: string;
  range_start: string;
  range_end: string;
  created_at: string;
  updated_at: string;
}

export type StaticReservation = {
  id: number;
  network_id: number;
  mac_address: string;
  ip_address: string;
  hostname?: string;
  created_at: string;
  updated_at: string;
}

export type CreateDhcpNetworkRequest = {
  name: string;
  subnet: string;
  gateway: string;
  dns_servers: string[];
  lease_duration: number;
  relay_agent_address?: string;
  enable_autodiscovery?: boolean;
}

export type UpdateDhcpNetworkRequest = {
  name?: string;
  subnet?: string;
  gateway?: string;
  dns_servers?: string[];
  lease_duration?: number;
  relay_agent_address?: string;
  enable_autodiscovery?: boolean;
}

export type CreateDhcpPoolRequest = {
  name: string;
  range_start: string;
  range_end: string;
}

export type CreateStaticReservationRequest = {
  mac_address: string;
  ip_address: string;
  hostname?: string;
}

export type MakeStaticRequest = {
  ip_address?: string;
  hostname?: string;
}

export type LifecycleTransition = {
  id: number;
  device_uuid: string;
  from_state: DeviceLifecycle;
  to_state: DeviceLifecycle;
  started_at: string;
  completed_at?: string;
  plan_id?: number;
  success?: boolean;
  error_message?: string;
}

export type DeviceStatus = {
  device_uuid: string;
  current_lifecycle: DeviceLifecycle;
  active_transition?: LifecycleTransition;
}

export type TransitionRequest = {
  to_state: DeviceLifecycle;
}

export type Architecture = "x86-64";


export async function getDevicesIndex(): Promise<DevicesIndex> {
  return fetch('/ui/devices').then((response) => {
    if (response.ok) {
      return response.json()
    } else {
      console.log('Error getting /ui/devices:', response.statusText)
      return redirect('error')
    }
  });
}

export async function getAllDevices(): Promise<Device[]> {
  return fetch('/ui/devices').then((response) => {
    if (response.ok) {
      return response.json().then((data: DevicesIndex) => data.devices);
    } else {
      console.error('Error getting devices:', response.statusText);
      throw new Error('Failed to fetch devices');
    }
  });
}


// Roles Types

export type FirmwareMode = "bios" | "uefi";

export type PartitionConfig = {
  label: string;
  size: string;              // "512MiB", "50%", "rest" or "*"
  filesystem?: string;       // undefined if lvm consumer
  mount_point?: string;
  flags?: string[];          // "boot", "esp", "bios_grub", "lvm"
  volume_group?: string;     // name of VG this partition feeds (when lvm flag set)
};

export type DiskConfig = {
  device: string;            // Platform label "ROOT" or raw path (free text)
  partition_table: string;   // "gpt" or "msdos"
  partitions: PartitionConfig[];
};

export type LogicalVolume = {
  name: string;
  size: string;              // "50G", "100%FREE"
  filesystem?: string;       // undefined for raw LVs (e.g. Ceph OSDs)
  mount_point?: string;
};

export type VolumeGroup = {
  name: string;
  logical_volumes: LogicalVolume[];
};

export type DiskLayout = {
  disks: DiskConfig[];
  volume_groups?: VolumeGroup[];
  wipe_all_disks?: boolean;
};

export type Role = {
  id?: number;
  name: string;
  description?: string;
  osm_module: string;
  os_name: string;
  os_release: string;
  os_arch: string;
  disk_layout: DiskLayout;
  cmdline_args?: string;
  config_template?: any;
  firmware_mode?: FirmwareMode;
  created_at?: string;
  updated_at?: string;
}

export type CreateRoleRequest = {
  name: string;
  description?: string;
  osm_module: string;
  os_name: string;
  os_release: string;
  os_arch: string;
  disk_layout: DiskLayout;
  cmdline_args?: string;
  config_template?: any;
  firmware_mode?: FirmwareMode;
}

export type UpdateRoleRequest = {
  name?: string;
  description?: string;
  osm_module?: string;
  os_name?: string;
  os_release?: string;
  os_arch?: string;
  disk_layout?: DiskLayout;
  cmdline_args?: string;
  config_template?: any;
  firmware_mode?: FirmwareMode;
  clear_firmware_mode?: boolean;
}

export type AssignRoleRequest = {
  role_id: number;
}

// Roles API

export async function getRoles(): Promise<Role[]> {
  return fetch('/ui/roles').then((response) => {
    if (response.ok) {
      return response.json();
    } else {
      console.error('Error getting roles:', response.statusText);
      throw new Error('Failed to fetch roles');
    }
  });
}

export async function getRole(id: number): Promise<Role> {
  return fetch(`/ui/roles/${id}`).then((response) => {
    if (response.ok) {
      return response.json();
    } else {
      console.error('Error getting role:', response.statusText);
      throw new Error('Failed to fetch role');
    }
  });
}

export async function createRole(data: CreateRoleRequest): Promise<Role> {
  const response = await fetch('/ui/roles', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(data)
  });
  if (response.ok) {
    return response.json();
  }
  return handleApiError(response, 'Failed to create role');
}

export async function updateRole(id: number, data: UpdateRoleRequest): Promise<Role> {
  const response = await fetch(`/ui/roles/${id}`, {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(data)
  });
  if (response.ok) {
    return response.json();
  }
  return handleApiError(response, 'Failed to update role');
}

export async function deleteRole(id: number): Promise<void> {
  return fetch(`/ui/roles/${id}`, {
    method: 'DELETE'
  }).then((response) => {
    if (!response.ok) {
      console.error('Error deleting role:', response.statusText);
      throw new Error('Failed to delete role');
    }
  });
}

export async function getRoleDevices(id: number): Promise<string[]> {
  return fetch(`/ui/roles/${id}/devices`).then((response) => {
    if (response.ok) {
      return response.json();
    } else {
      console.error('Error getting role devices:', response.statusText);
      throw new Error('Failed to fetch role devices');
    }
  });
}

export async function assignRoleToDevice(deviceUuid: string, roleId: number): Promise<void> {
  return fetch(`/ui/devices/${deviceUuid}/role`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ role_id: roleId })
  }).then((response) => {
    if (!response.ok) {
      console.error('Error assigning role to device:', response.statusText);
      throw new Error('Failed to assign role to device');
    }
  });
}

export async function getDeviceRole(deviceUuid: string): Promise<Role | null> {
  return fetch(`/ui/devices/${deviceUuid}/role`).then((response) => {
    if (response.ok) {
      return response.json();
    } else {
      console.error('Error getting device role:', response.statusText);
      throw new Error('Failed to fetch device role');
    }
  });
}

// Devices API

export async function getDevice(uuid: string): Promise<Device> {
  return fetch(`/ui/devices/${uuid}`).then((response) => {
    if (response.ok) {
      return response.json();
    } else {
      console.error('Error getting device:', response.statusText);
      throw new Error('Failed to fetch device');
    }
  });
}

export async function updateDeviceAttributes(uuid: string, attributes: Record<string, any>): Promise<void> {
  const response = await fetch(`/ui/devices/${uuid}/attributes`, {
    method: 'PATCH',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ attributes })
  });

  if (!response.ok) {
    await handleApiError(response, 'Failed to update device attributes');
  }
}

export async function deleteDevice(uuid: string): Promise<void> {
  return fetch(`/ui/devices/${uuid}`, {
    method: 'DELETE'
  }).then((response) => {
    if (!response.ok) {
      throw new Error('Failed to delete device');
    }
  });
}

export async function getDeviceStatus(uuid: string): Promise<DeviceStatus> {
  return fetch(`/ui/devices/${uuid}/status`).then((response) => {
    if (response.ok) {
      return response.json();
    } else {
      console.error('Error getting device status:', response.statusText);
      throw new Error('Failed to fetch device status');
    }
  });
}

export async function getDeviceLifecycle(uuid: string): Promise<DeviceLifecycle | null> {
  return fetch(`/ui/devices/${uuid}/lifecycle`).then((response) => {
    if (response.ok) {
      return response.json();
    } else {
      console.error('Error getting device lifecycle:', response.statusText);
      throw new Error('Failed to fetch device lifecycle');
    }
  });
}

export async function transitionDeviceLifecycle(uuid: string, toState: DeviceLifecycle): Promise<{ transition_id: number; message: string }> {
  const response = await fetch(`/ui/devices/${uuid}/lifecycle/transition`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ to_state: toState })
  });
  if (response.ok) {
    return response.json();
  }
  return handleApiError(response, 'Failed to transition device lifecycle');
}

export async function cancelDeviceTransition(uuid: string): Promise<void> {
  const response = await fetch(`/ui/devices/${uuid}/lifecycle/cancel`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
  });
  if (!response.ok) {
    return handleApiError(response, 'Failed to cancel transition');
  }
}

// Power Types

export type PowerState = "on" | "off" | "unknown";
export type PowerAction = "on" | "off" | "cycle";

export type DevicePowerStatus = {
  state: PowerState;
  driver: string | null;
};

// Power API

export async function getDevicePower(uuid: string): Promise<DevicePowerStatus> {
  const response = await fetch(`/ui/devices/${uuid}/power`);
  if (response.ok) {
    return response.json();
  }
  // The backend never returns 500 for this endpoint; degrade gracefully on any error
  console.error('Error getting device power status:', response.statusText);
  return { state: "unknown", driver: null };
}

export async function setDevicePower(uuid: string, action: PowerAction): Promise<{ message: string }> {
  const response = await fetch(`/ui/devices/${uuid}/power`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ action }),
  });
  if (response.ok) {
    return response.json();
  }
  return handleApiError(response, 'Failed to execute power action');
}

export async function getDeviceTransitions(uuid: string, includeCompleted: boolean = false): Promise<LifecycleTransition[]> {
  const params = new URLSearchParams();
  if (includeCompleted) {
    params.append('include_completed', 'true');
  }
  const url = `/ui/devices/${uuid}/transitions${params.toString() ? '?' + params.toString() : ''}`;

  return fetch(url).then((response) => {
    if (response.ok) {
      return response.json();
    } else {
      console.error('Error getting device transitions:', response.statusText);
      throw new Error('Failed to fetch device transitions');
    }
  });
}

export async function getDhcpLeases(): Promise<DhcpLease[]> {
  return fetch('/ui/dhcp/leases').then((response) => {
    if (response.ok) {
      return response.json();
    } else {
      console.error('Error getting DHCP leases:', response.statusText);
      throw new Error('Failed to fetch DHCP leases');
    }
  });
}

export async function getDhcpLeaseByMac(mac: string): Promise<DhcpLease | null> {
  return fetch(`/ui/dhcp/leases/${mac}`).then((response) => {
    if (response.ok) {
      return response.json();
    } else if (response.status === 404) {
      return null;
    } else {
      console.error('Error getting DHCP lease:', response.statusText);
      throw new Error('Failed to fetch DHCP lease');
    }
  });
}

export async function createPendingDevice(data: CreatePendingDeviceRequest): Promise<PendingDevice> {
  const response = await fetch('/ui/devices/pending', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(data)
  });

  if (response.ok) {
    return response.json();
  }

  return handleApiError(response, 'Failed to create pending device');
}

export async function getPendingDevices(): Promise<PendingDevice[]> {
  return fetch('/ui/devices/pending').then((response) => {
    if (response.ok) {
      return response.json();
    } else {
      console.error('Error getting pending devices:', response.statusText);
      throw new Error('Failed to fetch pending devices');
    }
  });
}

export async function deletePendingDevice(id: number): Promise<void> {
  return fetch(`/ui/devices/pending/${id}`, {
    method: 'DELETE'
  }).then((response) => {
    if (!response.ok) {
      console.error('Error deleting pending device:', response.statusText);
      throw new Error('Failed to delete pending device');
    }
  });
}

export const ActionConsole: Action = {
  "type": "console"
};

export type ActionType = "console";
export type Action = {
  "type": ActionType
};

export async function postDevicePlan(uuid: string, plan: Action[]): Promise<void> {
  let data = { plan };
  return fetch(`/ui/devices/${uuid}/plan`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(data)
  }).then((response) => {
    if (!response.ok) {
      console.error('Error creating device plan:', response.statusText);
      return handleApiError(response, 'Failed to create device plan');
    }
  })
}

// DHCP Networks API

export async function getNetworks(): Promise<DhcpNetwork[]> {
  return fetch('/ui/dhcp/networks').then((response) => {
    if (response.ok) {
      return response.json();
    } else {
      console.error('Error getting networks:', response.statusText);
      throw new Error('Failed to fetch networks');
    }
  });
}

export async function getNetwork(id: number): Promise<DhcpNetwork> {
  return fetch(`/ui/dhcp/networks/${id}`).then((response) => {
    if (response.ok) {
      return response.json();
    } else {
      console.error('Error getting network:', response.statusText);
      throw new Error('Failed to fetch network');
    }
  });
}

export async function createNetwork(data: CreateDhcpNetworkRequest): Promise<DhcpNetwork> {
  const response = await fetch('/ui/dhcp/networks', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(data)
  });

  if (response.ok) {
    return response.json();
  }

  return handleApiError(response, 'Failed to create network');
}

export async function updateNetwork(id: number, data: UpdateDhcpNetworkRequest): Promise<DhcpNetwork> {
  return fetch(`/ui/dhcp/networks/${id}`, {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(data)
  }).then((response) => {
    if (response.ok) {
      return response.json();
    }
    return handleApiError(response, 'Failed to update network');
  });
}

export async function deleteNetwork(id: number): Promise<void> {
  return fetch(`/ui/dhcp/networks/${id}`, {
    method: 'DELETE'
  }).then((response) => {
    if (!response.ok) {
      console.error('Error deleting network:', response.statusText);
      throw new Error('Failed to delete network');
    }
  });
}

export async function getPoolsForNetwork(networkId: number): Promise<DhcpPool[]> {
  return fetch(`/ui/dhcp/networks/${networkId}/pools`).then((response) => {
    if (response.ok) {
      return response.json();
    } else {
      console.error('Error getting pools:', response.statusText);
      throw new Error('Failed to fetch pools');
    }
  });
}

export async function createPool(networkId: number, data: CreateDhcpPoolRequest): Promise<DhcpPool> {
  return fetch(`/ui/dhcp/networks/${networkId}/pools`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(data)
  }).then((response) => {
    if (response.ok) {
      return response.json();
    } else {
      console.error('Error creating pool:', response.statusText);
      throw new Error('Failed to create pool');
    }
  });
}

export async function updatePool(id: number, data: CreateDhcpPoolRequest): Promise<DhcpPool> {
  return fetch(`/ui/dhcp/pools/${id}`, {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(data)
  }).then((response) => {
    if (response.ok) {
      return response.json();
    } else {
      console.error('Error updating pool:', response.statusText);
      throw new Error('Failed to update pool');
    }
  });
}

export async function deletePool(id: number): Promise<void> {
  return fetch(`/ui/dhcp/pools/${id}`, {
    method: 'DELETE'
  }).then((response) => {
    if (!response.ok) {
      console.error('Error deleting pool:', response.statusText);
      throw new Error('Failed to delete pool');
    }
  });
}

export async function getStaticReservations(networkId: number): Promise<StaticReservation[]> {
  return fetch(`/ui/dhcp/networks/${networkId}/static-reservations`).then((response) => {
    if (response.ok) {
      return response.json();
    } else {
      console.error('Error getting static reservations:', response.statusText);
      throw new Error('Failed to fetch static reservations');
    }
  });
}

export async function createStaticReservation(networkId: number, data: CreateStaticReservationRequest): Promise<StaticReservation> {
  return fetch(`/ui/dhcp/networks/${networkId}/static-reservations`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(data)
  }).then((response) => {
    if (response.ok) {
      return response.json();
    } else {
      console.error('Error creating static reservation:', response.statusText);
      throw new Error('Failed to create static reservation');
    }
  });
}

export async function deleteStaticReservation(id: number): Promise<void> {
  return fetch(`/ui/dhcp/static-reservations/${id}`, {
    method: 'DELETE'
  }).then((response) => {
    if (!response.ok) {
      console.error('Error deleting static reservation:', response.statusText);
      throw new Error('Failed to delete static reservation');
    }
  });
}

export async function getLeasesForNetwork(networkId: number): Promise<DhcpLease[]> {
  return fetch(`/ui/dhcp/networks/${networkId}/leases`).then((response) => {
    if (response.ok) {
      return response.json();
    } else {
      console.error('Error getting leases for network:', response.statusText);
      throw new Error('Failed to fetch leases for network');
    }
  });
}

export async function makeLeaseStatic(
  leaseId: number,
  data: MakeStaticRequest
): Promise<StaticReservation> {
  return fetch(`/ui/dhcp/leases/${leaseId}/make-static`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(data)
  }).then((response) => {
    if (response.ok) {
      return response.json();
    } else {
      console.error('Error making lease static:', response.statusText);
      throw new Error('Failed to make lease static');
    }
  });
}

export async function getStaticReservationByMac(
  networkId: number,
  mac: string
): Promise<StaticReservation | null> {
  const reservations = await getStaticReservations(networkId);
  return reservations.find((r) => r.mac_address === mac) || null;
}

// Platforms Types

export type DiskType = "nvme" | "ssd" | "hdd";

export type PlatformDisk = {
  path: string;
  size_gb: number;
  disk_type: DiskType;
  label?: string;
}

export type PlatformNic = {
  logical: string;
  speed_gbps?: number;
  label?: string;
}

export type PlatformCpu = {
  brand: string;
  model: string;
  cores: number;
}

export type PlatformAttributes = {
  disks: PlatformDisk[];
  nics: PlatformNic[];
  cpus: PlatformCpu[];
  memory_gib: number;
}

export type Platform = {
  id?: number;
  name: string;
  description?: string;
  attributes: PlatformAttributes;
  created_at?: string;
  updated_at?: string;
}

export type CreatePlatformRequest = {
  name: string;
  description?: string;
  attributes: PlatformAttributes;
}

export type UpdatePlatformRequest = {
  name?: string;
  description?: string;
  attributes?: PlatformAttributes;
}

export type AssignPlatformRequest = {
  platform_id: number;
}

// Platforms API

export async function getPlatforms(): Promise<Platform[]> {
  return fetch('/ui/platforms').then((response) => {
    if (response.ok) {
      return response.json();
    } else {
      console.error('Error getting platforms:', response.statusText);
      throw new Error('Failed to fetch platforms');
    }
  });
}

export async function getPlatform(id: number): Promise<Platform> {
  return fetch(`/ui/platforms/${id}`).then((response) => {
    if (response.ok) {
      return response.json();
    } else {
      console.error('Error getting platform:', response.statusText);
      throw new Error('Failed to fetch platform');
    }
  });
}

export async function createPlatform(data: CreatePlatformRequest): Promise<Platform> {
  const response = await fetch('/ui/platforms', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(data)
  });

  if (response.ok) {
    return response.json();
  }

  return handleApiError(response, 'Failed to create platform');
}

export async function updatePlatform(id: number, data: UpdatePlatformRequest): Promise<Platform> {
  const response = await fetch(`/ui/platforms/${id}`, {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(data)
  });

  if (response.ok) {
    return response.json();
  }

  return handleApiError(response, 'Failed to update platform');
}

export async function deletePlatform(id: number): Promise<void> {
  const response = await fetch(`/ui/platforms/${id}`, {
    method: 'DELETE'
  });

  if (!response.ok) {
    // Extract detailed error message from response body
    const errorText = await response.text().catch(() => '');
    const message = errorText || 'Failed to delete platform';
    throw new Error(message);
  }
}

export async function getPlatformDevices(id: number): Promise<string[]> {
  return fetch(`/ui/platforms/${id}/devices`).then((response) => {
    if (response.ok) {
      return response.json();
    } else {
      console.error('Error getting platform devices:', response.statusText);
      throw new Error('Failed to fetch platform devices');
    }
  });
}

export async function getDevicePlatform(uuid: string): Promise<Platform | null> {
  return fetch(`/ui/devices/${uuid}/platform`).then((response) => {
    if (response.ok) {
      return response.json();
    } else if (response.status === 404) {
      return null;
    } else {
      console.error('Error getting device platform:', response.statusText);
      throw new Error('Failed to fetch device platform');
    }
  });
}

export async function assignDevicePlatform(uuid: string, platformId: number): Promise<void> {
  return fetch(`/ui/devices/${uuid}/platform`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ platform_id: platformId })
  }).then((response) => {
    if (!response.ok) {
      console.error('Error assigning platform to device:', response.statusText);
      throw new Error('Failed to assign platform to device');
    }
  });
}

export type PlatformDeviceInfo = {
  uuid: string;
  hostname?: string;
  lifecycle?: string;
}

export async function getPlatformDevicesWithDetails(
  id: number
): Promise<PlatformDeviceInfo[]> {
  const response = await fetch(`/ui/platforms/${id}/devices/details`);
  if (response.ok) {
    return response.json();
  } else {
    throw new Error('Failed to fetch platform devices');
  }
}

// Device Warnings API

export type DeviceWarning = {
  id: number;
  code: string;
  message: string;
  created_at?: string;
}

export async function getDeviceWarnings(uuid: string): Promise<DeviceWarning[]> {
  const response = await fetch(`/ui/devices/${uuid}/warnings`);
  if (response.ok) {
    return response.json();
  } else {
    console.error('Error getting device warnings:', response.statusText);
    throw new Error('Failed to fetch device warnings');
  }
}

export async function dismissDeviceWarning(uuid: string, warningId: number): Promise<void> {
  const response = await fetch(`/ui/devices/${uuid}/warnings/${warningId}`, {
    method: 'DELETE',
  });
  if (!response.ok) {
    console.error('Error dismissing device warning:', response.statusText);
    throw new Error('Failed to dismiss device warning');
  }
}

// Device Label Overrides API

export type LabelOverrideRequest = {
  label: string;
  path: string;
}

export async function putDeviceLabelOverride(uuid: string, data: LabelOverrideRequest): Promise<void> {
  const response = await fetch(`/ui/devices/${uuid}/label-overrides`, {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(data),
  });
  if (!response.ok) {
    console.error('Error setting label override:', response.statusText);
    throw new Error('Failed to set label override');
  }
}

export async function deleteDeviceLabelOverride(uuid: string, label: string): Promise<void> {
  const response = await fetch(`/ui/devices/${uuid}/label-overrides/${encodeURIComponent(label)}`, {
    method: 'DELETE',
  });
  if (!response.ok) {
    console.error('Error removing label override:', response.statusText);
    throw new Error('Failed to remove label override');
  }
}

// Platform Disk Label API

export async function updatePlatformDiskLabel(
  platformId: number,
  diskIndex: number,
  label: string | null
): Promise<void> {
  const response = await fetch(`/ui/platforms/${platformId}/disks/${diskIndex}/label`, {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ label }),
  });
  if (!response.ok) {
    const errorData = await response.json().catch(() => null);
    if (response.status === 422 && errorData && errorData.error) {
      throw new Error(errorData.error);
    }
    console.error('Error updating platform disk label:', response.statusText);
    throw new Error('Failed to update disk label');
  }
}

// --- OSM Types ---

export type OsmModule = {
  id: number;
  name: string;
  version: string;
  author: string;
  description: string;
  source: string;
  is_default: boolean;
  storage_prefix: string;
  archive_path?: string;
  os_count: number;
  created_at?: string;
  updated_at?: string;
}

export type OsmOperatingSystem = {
  id: number;
  module_id: number;
  dir_name: string;
  name: string;
  release: string;
  config: OsmOperatingSystemConfig;
  disabled: boolean;
  created_at?: string;
  updated_at?: string;
}

export type OsmOperatingSystemConfig = {
  name: string;
  release: string;
  architectures: OsmArchitectureConfig[];
  template_variables: OsmTemplateVariable[];
}

export type OsmArchitectureConfig = {
  arch: string;
  kernel: string;
  initramfs: string;
  modules: string[];
  cmdline: string;
  install_template: string;
}

export type OsmTemplateVariable = {
  name: string;
  type: "string" | "list" | "boolean" | "integer";
  description: string;
  required: boolean;
  default: unknown;
}

export type OsmUpload = {
  id: number;
  filename: string;
  status: "uploading" | "validating" | "extracting" | "complete" | "failed";
  error_message?: string;
  module_id?: number;
  total_bytes?: number;
  received_bytes: number;
  created_at?: string;
  updated_at?: string;
}

// --- OSM API ---

export async function getOsmModules(): Promise<OsmModule[]> {
  const response = await fetch("/ui/osm/modules");
  if (response.ok) return response.json();
  throw new Error("Failed to fetch OSM modules");
}

export async function getOsmModule(id: number): Promise<OsmModule> {
  const response = await fetch(`/ui/osm/modules/${id}`);
  if (response.ok) return response.json();
  throw new Error("Failed to fetch OSM module");
}

export async function deleteOsmModule(id: number): Promise<void> {
  const response = await fetch(`/ui/osm/modules/${id}`, { method: "DELETE" });
  if (!response.ok) {
    const body = await response.text().catch(() => "");
    throw new Error(body || "Failed to delete module");
  }
}

export async function getOsmModuleOperatingSystems(moduleId: number): Promise<OsmOperatingSystem[]> {
  const response = await fetch(`/ui/osm/modules/${moduleId}/operating-systems`);
  if (response.ok) return response.json();
  throw new Error("Failed to fetch module operating systems");
}

export async function uploadOsm(file: File): Promise<OsmUpload> {
  const response = await fetch("/ui/osm/upload", {
    method: "POST",
    body: file,
    headers: { "Content-Type": "application/octet-stream" },
  });
  if (response.ok || response.status === 202) return response.json();
  throw new Error("Failed to upload OSM archive");
}

export async function getOsmUploads(): Promise<OsmUpload[]> {
  const response = await fetch("/ui/osm/uploads");
  if (response.ok) return response.json();
  throw new Error("Failed to fetch OSM uploads");
}

export async function getOsmUpload(id: number): Promise<OsmUpload> {
  const response = await fetch(`/ui/osm/uploads/${id}`);
  if (response.ok) return response.json();
  throw new Error("Failed to fetch OSM upload");
}

export async function getAllOsmOperatingSystems(): Promise<OsmOperatingSystem[]> {
  const response = await fetch("/ui/osm/operating-systems");
  if (response.ok) return response.json();
  throw new Error("Failed to fetch OSM operating systems");
}

export async function disableOsmOs(osId: number): Promise<void> {
  const response = await fetch(`/ui/osm/operating-systems/${osId}/disable`, { method: "POST" });
  if (!response.ok) throw new Error("Failed to disable OS");
}

export async function enableOsmOs(osId: number): Promise<void> {
  const response = await fetch(`/ui/osm/operating-systems/${osId}/enable`, { method: "POST" });
  if (!response.ok) throw new Error("Failed to enable OS");
}

export function getOsmModuleExportUrl(id: number): string {
  return `/ui/osm/modules/${id}/export`;
}
