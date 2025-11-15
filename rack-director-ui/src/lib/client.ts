import { redirect } from "react-router";

export type Plan = {

}

export type DeviceLifecycle = "new" | "unprovisioned" | "provisioned" | "removed" | "broken";

export type Device = {
  uuid: string;
  architecture: Architecture;
  lifecycle?: DeviceLifecycle;
  role_id?: number;
  attributes: {
    hostname?: string;
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
  mac_address: string;
  ip_address: string;
  device_uuid?: string;
  expires_at?: string;
}

export type LifecycleTransition = {
  id: number;
  device_uuid: string;
  from_state: DeviceLifecycle;
  to_state: DeviceLifecycle;
  started_at: string;
  completed_at?: string;
  plan_id?: number;
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
  return fetch('/api/operating_systems').then((response) => {
    if (response.ok) {
      return response.json();
    } else {
      console.error('Error getting operating systems:', response.statusText);
      throw new Error('Failed to fetch operating systems');
    }
  });
}

export async function getOperatingSystem(id: number): Promise<OperatingSystemWithArchitectures> {
  return fetch(`/api/operating_systems/${id}`).then((response) => {
    if (response.ok) {
      return response.json();
    } else {
      console.error('Error getting operating system:', response.statusText);
      throw new Error('Failed to fetch operating system');
    }
  });
}

export async function createOperatingSystem(data: CreateOperatingSystemRequest): Promise<OperatingSystem> {
  return fetch('/api/operating_systems', {
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
  return fetch(`/api/operating_systems/${id}`, {
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
  return fetch(`/api/operating_systems/${id}`, {
    method: 'DELETE'
  }).then((response) => {
    if (!response.ok) {
      console.error('Error deleting operating system:', response.statusText);
      throw new Error('Failed to delete operating system');
    }
  });
}

export async function createOsArchitecture(osId: number, data: CreateOsArchitectureRequest): Promise<OsArchitecture> {
  return fetch(`/api/operating_systems/${osId}/architectures`, {
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
  return fetch(`/api/operating_systems/${osId}/architectures/${arch}`, {
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
  return fetch(`/api/operating_systems/${osId}/architectures/${arch}/kernel`, {
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
  return fetch(`/api/operating_systems/${osId}/architectures/${arch}/initramfs`, {
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
  return fetch(`/api/operating_systems/${osId}/architectures/${arch}/modules?name=${encodeURIComponent(moduleName)}`, {
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
  return fetch(`/api/operating_systems/${osId}/architectures/${arch}/install_script`, {
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
  return `/api/operating_systems/${osId}/architectures/${arch}/download/${component}`;
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
  return fetch('/api/roles').then((response) => {
    if (response.ok) {
      return response.json();
    } else {
      console.error('Error getting roles:', response.statusText);
      throw new Error('Failed to fetch roles');
    }
  });
}

export async function getRole(id: number): Promise<RoleWithOs> {
  return fetch(`/api/roles/${id}`).then((response) => {
    if (response.ok) {
      return response.json();
    } else {
      console.error('Error getting role:', response.statusText);
      throw new Error('Failed to fetch role');
    }
  });
}

export async function createRole(data: CreateRoleRequest): Promise<Role> {
  return fetch('/api/roles', {
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
  return fetch(`/api/roles/${id}`, {
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
  return fetch(`/api/roles/${id}`, {
    method: 'DELETE'
  }).then((response) => {
    if (!response.ok) {
      console.error('Error deleting role:', response.statusText);
      throw new Error('Failed to delete role');
    }
  });
}

export async function getRoleDevices(id: number): Promise<string[]> {
  return fetch(`/api/roles/${id}/devices`).then((response) => {
    if (response.ok) {
      return response.json();
    } else {
      console.error('Error getting role devices:', response.statusText);
      throw new Error('Failed to fetch role devices');
    }
  });
}

export async function assignRoleToDevice(deviceUuid: string, roleId: number): Promise<void> {
  return fetch(`/api/devices/${deviceUuid}/role`, {
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
  return fetch(`/api/devices/${deviceUuid}/role`).then((response) => {
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
  return fetch(`/api/devices/${uuid}`).then((response) => {
    if (response.ok) {
      return response.json();
    } else {
      console.error('Error getting device:', response.statusText);
      throw new Error('Failed to fetch device');
    }
  });
}

export async function getDeviceStatus(uuid: string): Promise<DeviceStatus> {
  return fetch(`/api/devices/${uuid}/status`).then((response) => {
    if (response.ok) {
      return response.json();
    } else {
      console.error('Error getting device status:', response.statusText);
      throw new Error('Failed to fetch device status');
    }
  });
}

export async function getDeviceLifecycle(uuid: string): Promise<DeviceLifecycle | null> {
  return fetch(`/api/devices/${uuid}/lifecycle`).then((response) => {
    if (response.ok) {
      return response.json();
    } else {
      console.error('Error getting device lifecycle:', response.statusText);
      throw new Error('Failed to fetch device lifecycle');
    }
  });
}

export async function transitionDeviceLifecycle(uuid: string, toState: DeviceLifecycle): Promise<{ transition_id: number; message: string }> {
  return fetch(`/api/devices/${uuid}/lifecycle/transition`, {
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
  const url = `/api/devices/${uuid}/transitions${params.toString() ? '?' + params.toString() : ''}`;

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
  return fetch('/api/dhcp/leases').then((response) => {
    if (response.ok) {
      return response.json();
    } else {
      console.error('Error getting DHCP leases:', response.statusText);
      throw new Error('Failed to fetch DHCP leases');
    }
  });
}

export async function getDhcpLeaseByMac(mac: string): Promise<DhcpLease | null> {
  return fetch(`/api/dhcp/leases/${mac}`).then((response) => {
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
