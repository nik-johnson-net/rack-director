interface TemplateDocsProps {
  type: "install-script" | "cmdline";
}

const varCls =
  "inline-flex items-center px-1.5 py-0.5 rounded-sm bg-bg-overlay border border-border text-xs font-mono text-accent";

export default function TemplateDocs({ type }: TemplateDocsProps) {
  if (type === "cmdline") {
    return (
      <div className="bg-bg-overlay border border-border p-3 text-xs">
        <p className="font-semibold text-text-primary mb-2">Available Template Variables</p>
        <p className="text-text-muted mb-2">Use Handlebars syntax to reference these variables</p>
        <div className="flex items-center gap-2 mb-3">
          <code className={varCls}>{"{{ install_script_url }}"}</code>
          <span className="text-text-secondary">URL to fetch the install script</span>
        </div>
        <p className="text-text-muted mb-1">Example:</p>
        <pre className="bg-bg-base border border-border p-2 text-xs text-text-secondary overflow-x-auto font-mono whitespace-pre-wrap">
          {"autoinstall ds=nocloud-net;s={{ install_script_url }} console=ttyS0"}
        </pre>
      </div>
    );
  }

  // install-script type
  return (
    <div className="bg-bg-overlay border border-border p-3 text-xs">
      <p className="font-semibold text-text-primary mb-1">Available Template Variables</p>
      <p className="text-text-muted mb-3">
        Use Handlebars syntax ({"{{ variable }}"}) to reference these in your install script
      </p>

      {/* Device Information */}
      <Section title="Device Information">
        <Var name="device.uuid" desc="Device UUID" />
        <Var name="device.hostname" desc="Device hostname" />
        <Var name="device.mac_address" desc="Primary MAC address" />
        <Var name="device.ip_address" desc="IP address (DHCP lease)" />
        <Var name="device.gateway" desc="Network gateway" />
        <Var name="device.dns_servers" desc="DNS servers (space-separated)" />
        <Var name="device.netmask" desc="Network netmask" />
      </Section>

      {/* Role Information */}
      <Section title="Role Information">
        <Var name="role.name" desc="Role name" />
        <Var name="role.disk_layout" desc="Disk layout as JSON" />
      </Section>

      {/* Operating System */}
      <Section title="Operating System">
        <Var name="os.name" desc="OS name" />
        <Var name="os.version" desc="OS version" />
      </Section>

      {/* Custom Configuration */}
      <Section title="Custom Configuration">
        <Var name="config.*" desc="Any custom config from role.config_template" />
      </Section>

      {/* Disk Layout */}
      <Section title="Disk Layout (resolved, post-partitioning)">
        <p className="text-text-muted mb-2">
          Populated from the device&apos;s resolved disk layout. Iterate using{" "}
          <code className="font-mono text-accent">{"{{#each}}"}</code>.
        </p>
        <p className="font-medium text-text-secondary mb-1">
          {"{{ partitions }}"} — all partitions across all disks:
        </p>
        <div className="ml-2 mb-2">
          <Var name="this.disk" desc="Disk device path" />
          <Var name="this.device" desc="Partition device path including /dev/ prefix" />
          <Var name="this.device_name" desc="Partition path without /dev/ prefix" />
          <Var name="this.label" desc="GPT partition label" />
          <Var name="this.size" desc="Partition size string" />
          <Var name="this.filesystem" desc="Filesystem type, null for LVM/ZFS partitions" />
          <Var name="this.mount_point" desc="Mount point, null if not mounted directly" />
          <Var name="this.flags" desc='Array of flags (e.g., ["esp"])' />
          <Var name="this.volume_group" desc="LVM volume group name, null for regular partitions" />
        </div>

        <p className="font-medium text-text-secondary mb-1">
          {"{{ logical_volumes }}"} — LVM logical volumes:
        </p>
        <div className="ml-2">
          <Var name="this.device" desc="LV device path (e.g., /dev/vg0/root)" />
          <Var name="this.device_name" desc="LV path without /dev/ prefix" />
          <Var name="this.vg_name" desc="Volume group name" />
          <Var name="this.lv_name" desc="Logical volume name" />
          <Var name="this.size" desc="LV size string" />
          <Var name="this.filesystem" desc="Filesystem type" />
          <Var name="this.mount_point" desc="Mount point, null if no mount" />
        </div>
      </Section>

      {/* Examples */}
      <p className="text-text-muted mt-3 mb-1 font-medium">Example (Debian Preseed):</p>
      <pre className="bg-bg-base border border-border p-2 text-xs text-text-secondary overflow-x-auto font-mono mb-3">
{`d-i netcfg/get_hostname string {{ device.hostname }}
d-i netcfg/get_ipaddress string {{ device.ip_address }}
d-i netcfg/get_netmask string {{ device.netmask }}
d-i netcfg/get_gateway string {{ device.gateway }}
d-i netcfg/get_nameservers string {{ device.dns_servers }}`}
      </pre>

      <p className="text-text-muted mb-1 font-medium">Example (Kickstart / RHEL / Anaconda):</p>
      <pre className="bg-bg-base border border-border p-2 text-xs text-text-secondary overflow-x-auto font-mono">
{`{{#each partitions}}{{#if this.mount_point}}{{#unless this.volume_group}}
part {{this.mount_point}} --fstype="{{this.filesystem}}" --onpart={{this.device_name}}
{{/unless}}{{/if}}{{/each}}
{{#each logical_volumes}}{{#if this.mount_point}}
logvol {{this.mount_point}} --vgname={{this.vg_name}} --name={{this.lv_name}} --fstype={{this.filesystem}}
{{/if}}{{/each}}`}
      </pre>
    </div>
  );
}

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <div className="mb-3">
      <p className="font-semibold text-text-primary mb-1">{title}:</p>
      <div className="ml-2 space-y-1">{children}</div>
    </div>
  );
}

function Var({ name, desc }: { name: string; desc: string }) {
  return (
    <div className="flex items-start gap-2">
      <code className="inline-flex items-center px-1.5 py-0.5 rounded-sm bg-bg-overlay border border-border text-xs font-mono text-accent shrink-0">
        {"{{ "}
        {name}
        {" }}"}
      </code>
      <span className="text-text-secondary">{desc}</span>
    </div>
  );
}
