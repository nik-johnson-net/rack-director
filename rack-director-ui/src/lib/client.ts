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
  is_primary: boolean;
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
  username?: string;
  password?: string;
}

export type Device = {
  uuid: string;
  architecture: Architecture;
  lifecycle?: DeviceLifecycle;
  role_id?: number;
  attributes: {
    hostname?: string;
    network_interfaces?: NetworkInterface[];
    bmc?: BmcInfo;
    bmc_config?: BmcConfig;
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
  expires_at?: string;
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

export type OperatingSystem = {
  id?: number;
  name: string;
  version: string;
  description?: string;
  created_at?: string;
  updated_at?: string;
}

export type OsArchitecture = {
  id?: number;
  os_id: number;
  architecture: Architecture;
  kernel_path: string;
  initramfs_path: string;
  modules: string[];
  cmdline_args?: string;
  install_script_path?: string;
  kernel_filename?: string;
  initramfs_filename?: string;
  install_script_filename?: string;
  created_at?: string;
  updated_at?: string;
}

export type OperatingSystemWithArchitectures = OperatingSystem & {
  architectures: OsArchitecture[];
}

export type CreateOperatingSystemRequest = {
  name: string;
  version: string;
  description?: string;
}

export type UpdateOperatingSystemRequest = {
  name?: string;
  version?: string;
  description?: string;
}

export type CreateOsArchitectureRequest = {
  architecture: Architecture;
  kernel_path?: string;
  initramfs_path?: string;
  modules?: string[];
  cmdline_args?: string;
  install_script_path?: string;
}

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

// Operating Systems API

export async function getOperatingSystems(): Promise<OperatingSystem[]> {
  return fetch('/ui/operating_systems').then((response) => {
    if (response.ok) {
      return response.json();
    } else {
      console.error('Error getting operating systems:', response.statusText);
      throw new Error('Failed to fetch operating systems');
    }
  });
}

export async function getOperatingSystem(id: number): Promise<OperatingSystemWithArchitectures> {
  return fetch(`/ui/operating_systems/${id}`).then((response) => {
    if (response.ok) {
      return response.json();
    } else {
      console.error('Error getting operating system:', response.statusText);
      throw new Error('Failed to fetch operating system');
    }
  });
}

export async function createOperatingSystem(data: CreateOperatingSystemRequest): Promise<OperatingSystem> {
  return fetch('/ui/operating_systems', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(data)
  }).then((response) => {
    if (response.ok) {
      return response.json();
    } else {
      console.error('Error creating operating system:', response.statusText);
      throw new Error('Failed to create operating system');
    }
  });
}

export async function updateOperatingSystem(id: number, data: UpdateOperatingSystemRequest): Promise<OperatingSystem> {
  return fetch(`/ui/operating_systems/${id}`, {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(data)
  }).then((response) => {
    if (response.ok) {
      return response.json();
    } else {
      console.error('Error updating operating system:', response.statusText);
      throw new Error('Failed to update operating system');
    }
  });
}

export async function deleteOperatingSystem(id: number): Promise<void> {
  return fetch(`/ui/operating_systems/${id}`, {
    method: 'DELETE'
  }).then((response) => {
    if (!response.ok) {
      console.error('Error deleting operating system:', response.statusText);
      throw new Error('Failed to delete operating system');
    }
  });
}

export async function createOsArchitecture(osId: number, data: CreateOsArchitectureRequest): Promise<OsArchitecture> {
  return fetch(`/ui/operating_systems/${osId}/architectures`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(data)
  }).then((response) => {
    if (response.ok) {
      return response.json();
    } else {
      console.error('Error creating architecture:', response.statusText);
      throw new Error('Failed to create architecture');
    }
  });
}

export async function deleteOsArchitecture(osId: number, arch: Architecture): Promise<void> {
  return fetch(`/ui/operating_systems/${osId}/architectures/${arch}`, {
    method: 'DELETE'
  }).then((response) => {
    if (!response.ok) {
      console.error('Error deleting architecture:', response.statusText);
      throw new Error('Failed to delete architecture');
    }
  });
}

export async function uploadKernel(osId: number, arch: Architecture, file: File): Promise<OsArchitecture> {
  const arrayBuffer = await file.arrayBuffer();
  return fetch(`/ui/operating_systems/${osId}/architectures/${arch}/kernel?filename=${encodeURIComponent(file.name)}`, {
    method: 'POST',
    body: arrayBuffer
  }).then((response) => {
    if (response.ok) {
      return response.json();
    } else {
      console.error('Error uploading kernel:', response.statusText);
      throw new Error('Failed to upload kernel');
    }
  });
}

export async function uploadInitramfs(osId: number, arch: Architecture, file: File): Promise<OsArchitecture> {
  const arrayBuffer = await file.arrayBuffer();
  return fetch(`/ui/operating_systems/${osId}/architectures/${arch}/initramfs?filename=${encodeURIComponent(file.name)}`, {
    method: 'POST',
    body: arrayBuffer
  }).then((response) => {
    if (response.ok) {
      return response.json();
    } else {
      console.error('Error uploading initramfs:', response.statusText);
      throw new Error('Failed to upload initramfs');
    }
  });
}

export async function uploadModule(osId: number, arch: Architecture, file: File, moduleName: string): Promise<OsArchitecture> {
  const arrayBuffer = await file.arrayBuffer();
  return fetch(`/ui/operating_systems/${osId}/architectures/${arch}/modules?name=${encodeURIComponent(moduleName)}`, {
    method: 'POST',
    body: arrayBuffer
  }).then((response) => {
    if (response.ok) {
      return response.json();
    } else {
      console.error('Error uploading module:', response.statusText);
      throw new Error('Failed to upload module');
    }
  });
}

export async function uploadInstallScript(osId: number, arch: Architecture, file: File): Promise<OsArchitecture> {
  const arrayBuffer = await file.arrayBuffer();
  return fetch(`/ui/operating_systems/${osId}/architectures/${arch}/install_script?filename=${encodeURIComponent(file.name)}`, {
    method: 'POST',
    body: arrayBuffer
  }).then((response) => {
    if (response.ok) {
      return response.json();
    } else {
      console.error('Error uploading install script:', response.statusText);
      throw new Error('Failed to upload install script');
    }
  });
}

export function getDownloadUrl(osId: number, arch: Architecture, component: string): string {
  return `/ui/operating_systems/${osId}/architectures/${arch}/download/${component}`;
}

// Roles Types

export type Partition = {
  device: string;
  size: string;
  filesystem: string;
  mount_point?: string;
  flags: string[];
}

export type DiskLayout = {
  partitions: Partition[];
}

export type Role = {
  id?: number;
  name: string;
  description?: string;
  os_id: number;
  disk_layout: DiskLayout;
  config_template?: any;
  created_at?: string;
  updated_at?: string;
}

export type RoleWithOs = Role & {
  os_name: string;
  os_version: string;
}

export type CreateRoleRequest = {
  name: string;
  description?: string;
  os_id: number;
  disk_layout: DiskLayout;
  config_template?: any;
}

export type UpdateRoleRequest = {
  name?: string;
  description?: string;
  os_id?: number;
  disk_layout?: DiskLayout;
  config_template?: any;
}

export type AssignRoleRequest = {
  role_id: number;
}

// Roles API

export async function getRoles(): Promise<RoleWithOs[]> {
  return fetch('/ui/roles').then((response) => {
    if (response.ok) {
      return response.json();
    } else {
      console.error('Error getting roles:', response.statusText);
      throw new Error('Failed to fetch roles');
    }
  });
}

export async function getRole(id: number): Promise<RoleWithOs> {
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
  return fetch('/ui/roles', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(data)
  }).then((response) => {
    if (response.ok) {
      return response.json();
    } else {
      console.error('Error creating role:', response.statusText);
      throw new Error('Failed to create role');
    }
  });
}

export async function updateRole(id: number, data: UpdateRoleRequest): Promise<Role> {
  return fetch(`/ui/roles/${id}`, {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(data)
  }).then((response) => {
    if (response.ok) {
      return response.json();
    } else {
      console.error('Error updating role:', response.statusText);
      throw new Error('Failed to update role');
    }
  });
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
  return fetch(`/ui/devices/${uuid}/lifecycle/transition`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ to_state: toState })
  }).then((response) => {
    if (response.ok) {
      return response.json();
    } else {
      console.error('Error transitioning device lifecycle:', response.statusText);
      throw new Error('Failed to transition device lifecycle');
    }
  });
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
  return fetch('/ui/devices/pending', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(data)
  }).then((response) => {
    if (response.ok) {
      return response.json();
    } else {
      return response.json().then((err) => {
        throw new Error(err.error || 'Failed to create pending device');
      });
    }
  });
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
