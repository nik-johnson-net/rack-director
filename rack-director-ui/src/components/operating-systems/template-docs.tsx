import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";

interface TemplateDocsProps {
  type: 'install-script' | 'cmdline';
}

export default function TemplateDocs({ type }: TemplateDocsProps) {
  if (type === 'cmdline') {
    return (
      <Card className="bg-blue-50 border-blue-200">
        <CardHeader>
          <CardTitle className="text-sm">Available Template Variables</CardTitle>
          <CardDescription className="text-xs">
            Use Handlebars syntax to reference these variables
          </CardDescription>
        </CardHeader>
        <CardContent className="text-sm space-y-2">
          <div>
            <Badge variant="secondary" className="font-mono text-xs">
              {"{{ install_script_url }}"}
            </Badge>
            <span className="ml-2 text-gray-600">- URL to fetch the install script</span>
          </div>
          <div className="text-xs text-gray-500 mt-3">
            <strong>Example:</strong>
            <pre className="mt-1 bg-white p-2 rounded border text-xs overflow-x-auto">
              autoinstall ds=nocloud-net;s={"{{ install_script_url }}"} console=ttyS0
            </pre>
          </div>
        </CardContent>
      </Card>
    );
  }

  // install-script type
  return (
    <Card className="bg-blue-50 border-blue-200">
      <CardHeader>
        <CardTitle className="text-sm">Available Template Variables</CardTitle>
        <CardDescription className="text-xs">
          Use Handlebars syntax ({"{{ variable }}"}) to reference these in your install script
        </CardDescription>
      </CardHeader>
      <CardContent className="text-sm space-y-3">
        <div>
          <div className="font-semibold text-gray-700 mb-1">Device Information:</div>
          <div className="space-y-1 ml-2">
            <div>
              <Badge variant="secondary" className="font-mono text-xs">{"{{ device.uuid }}"}</Badge>
              <span className="ml-2 text-gray-600">- Device UUID</span>
            </div>
            <div>
              <Badge variant="secondary" className="font-mono text-xs">{"{{ device.hostname }}"}</Badge>
              <span className="ml-2 text-gray-600">- Device hostname</span>
            </div>
            <div>
              <Badge variant="secondary" className="font-mono text-xs">{"{{ device.mac_address }}"}</Badge>
              <span className="ml-2 text-gray-600">- Primary MAC address</span>
            </div>
            <div>
              <Badge variant="secondary" className="font-mono text-xs">{"{{ device.ip_address }}"}</Badge>
              <span className="ml-2 text-gray-600">- IP address (DHCP lease)</span>
            </div>
            <div>
              <Badge variant="secondary" className="font-mono text-xs">{"{{ device.gateway }}"}</Badge>
              <span className="ml-2 text-gray-600">- Network gateway</span>
            </div>
            <div>
              <Badge variant="secondary" className="font-mono text-xs">{"{{ device.dns_servers }}"}</Badge>
              <span className="ml-2 text-gray-600">- DNS servers (space-separated)</span>
            </div>
            <div>
              <Badge variant="secondary" className="font-mono text-xs">{"{{ device.netmask }}"}</Badge>
              <span className="ml-2 text-gray-600">- Network netmask</span>
            </div>
          </div>
        </div>

        <div>
          <div className="font-semibold text-gray-700 mb-1">Role Information:</div>
          <div className="space-y-1 ml-2">
            <div>
              <Badge variant="secondary" className="font-mono text-xs">{"{{ role.name }}"}</Badge>
              <span className="ml-2 text-gray-600">- Role name</span>
            </div>
            <div>
              <Badge variant="secondary" className="font-mono text-xs">{"{{ role.disk_layout }}"}</Badge>
              <span className="ml-2 text-gray-600">- Disk layout as JSON</span>
            </div>
          </div>
        </div>

        <div>
          <div className="font-semibold text-gray-700 mb-1">Operating System:</div>
          <div className="space-y-1 ml-2">
            <div>
              <Badge variant="secondary" className="font-mono text-xs">{"{{ os.name }}"}</Badge>
              <span className="ml-2 text-gray-600">- OS name</span>
            </div>
            <div>
              <Badge variant="secondary" className="font-mono text-xs">{"{{ os.version }}"}</Badge>
              <span className="ml-2 text-gray-600">- OS version</span>
            </div>
          </div>
        </div>

        <div>
          <div className="font-semibold text-gray-700 mb-1">Custom Configuration:</div>
          <div className="space-y-1 ml-2">
            <div>
              <Badge variant="secondary" className="font-mono text-xs">{"{{ config.* }}"}</Badge>
              <span className="ml-2 text-gray-600">- Any custom config from role.config_template</span>
            </div>
          </div>
        </div>

        <div>
          <div className="font-semibold text-gray-700 mb-1">Disk Layout (resolved, post-partitioning):</div>
          <div className="text-xs text-gray-500 mb-2 ml-2">
            These variables are populated from the device's resolved disk layout, with platform labels already resolved to actual device paths. Iterate using {"{{#each}}"}.
          </div>
          <div className="space-y-3 ml-2">
            <div>
              <div className="text-xs font-medium text-gray-600 mb-1">{"{{ partitions }}"} — list of all partitions across all disks:</div>
              <div className="space-y-1 ml-2">
                <div>
                  <Badge variant="secondary" className="font-mono text-xs">{"{{ this.disk }}"}</Badge>
                  <span className="ml-2 text-gray-600">- Disk device path (e.g., <code className="bg-gray-100 px-1 rounded">/dev/disk/by-path/pci-0000:00:03.0-nvme-1</code>)</span>
                </div>
                <div>
                  <Badge variant="secondary" className="font-mono text-xs">{"{{ this.device }}"}</Badge>
                  <span className="ml-2 text-gray-600">- Partition device path including <code className="bg-gray-100 px-1 rounded">/dev/</code> prefix</span>
                </div>
                <div>
                  <Badge variant="secondary" className="font-mono text-xs">{"{{ this.device_name }}"}</Badge>
                  <span className="ml-2 text-gray-600">- Partition path without <code className="bg-gray-100 px-1 rounded">/dev/</code> prefix (for Kickstart <code className="bg-gray-100 px-1 rounded">--onpart=</code>)</span>
                </div>
                <div>
                  <Badge variant="secondary" className="font-mono text-xs">{"{{ this.label }}"}</Badge>
                  <span className="ml-2 text-gray-600">- GPT partition label</span>
                </div>
                <div>
                  <Badge variant="secondary" className="font-mono text-xs">{"{{ this.size }}"}</Badge>
                  <span className="ml-2 text-gray-600">- Partition size string</span>
                </div>
                <div>
                  <Badge variant="secondary" className="font-mono text-xs">{"{{ this.filesystem }}"}</Badge>
                  <span className="ml-2 text-gray-600">- Filesystem type, null for LVM/ZFS partitions</span>
                </div>
                <div>
                  <Badge variant="secondary" className="font-mono text-xs">{"{{ this.mount_point }}"}</Badge>
                  <span className="ml-2 text-gray-600">- Mount point, null if not mounted directly</span>
                </div>
                <div>
                  <Badge variant="secondary" className="font-mono text-xs">{"{{ this.flags }}"}</Badge>
                  <span className="ml-2 text-gray-600">- Array of flags (e.g., <code className="bg-gray-100 px-1 rounded">["esp"]</code>)</span>
                </div>
                <div>
                  <Badge variant="secondary" className="font-mono text-xs">{"{{ this.volume_group }}"}</Badge>
                  <span className="ml-2 text-gray-600">- LVM volume group name, null for regular partitions</span>
                </div>
              </div>
            </div>

            <div>
              <div className="text-xs font-medium text-gray-600 mb-1">{"{{ logical_volumes }}"} — list of LVM logical volumes:</div>
              <div className="space-y-1 ml-2">
                <div>
                  <Badge variant="secondary" className="font-mono text-xs">{"{{ this.device }}"}</Badge>
                  <span className="ml-2 text-gray-600">- LV device path (e.g., <code className="bg-gray-100 px-1 rounded">/dev/vg0/root</code>)</span>
                </div>
                <div>
                  <Badge variant="secondary" className="font-mono text-xs">{"{{ this.device_name }}"}</Badge>
                  <span className="ml-2 text-gray-600">- LV path without <code className="bg-gray-100 px-1 rounded">/dev/</code> prefix (e.g., <code className="bg-gray-100 px-1 rounded">vg0/root</code>)</span>
                </div>
                <div>
                  <Badge variant="secondary" className="font-mono text-xs">{"{{ this.vg_name }}"}</Badge>
                  <span className="ml-2 text-gray-600">- Volume group name</span>
                </div>
                <div>
                  <Badge variant="secondary" className="font-mono text-xs">{"{{ this.lv_name }}"}</Badge>
                  <span className="ml-2 text-gray-600">- Logical volume name</span>
                </div>
                <div>
                  <Badge variant="secondary" className="font-mono text-xs">{"{{ this.size }}"}</Badge>
                  <span className="ml-2 text-gray-600">- LV size string</span>
                </div>
                <div>
                  <Badge variant="secondary" className="font-mono text-xs">{"{{ this.filesystem }}"}</Badge>
                  <span className="ml-2 text-gray-600">- Filesystem type</span>
                </div>
                <div>
                  <Badge variant="secondary" className="font-mono text-xs">{"{{ this.mount_point }}"}</Badge>
                  <span className="ml-2 text-gray-600">- Mount point, null if no mount</span>
                </div>
              </div>
            </div>
          </div>
        </div>

        <div className="text-xs text-gray-500 mt-3">
          <strong>Example (Debian Preseed):</strong>
          <pre className="mt-1 bg-white p-2 rounded border text-xs overflow-x-auto">
{`d-i netcfg/get_hostname string {{ device.hostname }}
d-i netcfg/get_ipaddress string {{ device.ip_address }}
d-i netcfg/get_netmask string {{ device.netmask }}
d-i netcfg/get_gateway string {{ device.gateway }}
d-i netcfg/get_nameservers string {{ device.dns_servers }}`}
          </pre>
        </div>

        <div className="text-xs text-gray-500 mt-3">
          <strong>Example (Kickstart / RHEL / Anaconda):</strong>
          <pre className="mt-1 bg-white p-2 rounded border text-xs overflow-x-auto">
{`{{#each partitions}}{{#if this.mount_point}}{{#unless this.volume_group}}
part {{this.mount_point}} --fstype="{{this.filesystem}}" --onpart={{this.device_name}}
{{/unless}}{{/if}}{{/each}}
{{#each logical_volumes}}{{#if this.mount_point}}
logvol {{this.mount_point}} --vgname={{this.vg_name}} --name={{this.lv_name}} --fstype={{this.filesystem}}
{{/if}}{{/each}}`}
          </pre>
        </div>
      </CardContent>
    </Card>
  );
}
