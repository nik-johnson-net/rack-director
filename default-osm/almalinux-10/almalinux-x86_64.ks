cmdline

repo --name=baseos --mirrorlist=https://mirrors.almalinux.org/mirrorlist/10/baseos
repo --name=appstream --mirrorlist=https://mirrors.almalinux.org/mirrorlist/10/appstream
url --url="https://repo.almalinux.org/almalinux/10/BaseOS/x86_64/kickstart/"

lang en_US.UTF-8
keyboard --vckeymap=us --xlayouts='us'
timezone UTC --utc

# To create an encrypted password, you can use python:
# `python -c 'import crypt,getpass;pw=getpass.getpass();print(crypt.crypt(pw) if (pw==getpass.getpass("Confirm: ")) else exit())'`
rootpw --iscrypted "{{ config.rootpw }}"

clearpart --none
{{#if device.is_bios}}zerombr
{{/if}}ignoredisk --only-use={{#each partitions}}{{this.disk}}{{#unless @last}},{{/unless}}{{/each}}

# Network
network --bootproto=static --ip={{ device.ip_address }} --netmask={{ device.netmask }} --gateway={{ device.gateway }} --nameserver={{ device.dns_servers_csv }} --hostname={{ device.hostname }} --device={{ device.mac_address }}

# Every partition on the target disk, declared with --onpart --noformat.
# Anaconda requires ALL partitions to be declared; omitting any causes the
# interactive Installation Destination dialog to appear.
{{#each partitions}}{{#if this.volume_group}}
part pv.{{@index}} --fstype=lvmpv --onpart={{this.device_name}} --noformat
{{else if this.is_bios_grub}}
part biosboot --fstype=biosboot --onpart={{this.device_name}} --noformat
{{else if this.mount_point}}
part {{this.mount_point}} --fstype="{{this.filesystem}}" --onpart={{this.device_name}} --noformat
{{else if this.is_esp}}
part /boot/efi --fstype=vfat --onpart={{this.device_name}} --noformat
{{/if}}{{/each}}

# LVM: declare pre-existing VGs and LVs.
# Each unique VG is declared once; logvol entries reference pre-existing LVs.
# Note: member PV names must NOT appear on volgroup --useexisting lines (RHEL >= 6.3).
{{#each volume_groups}}
volgroup {{this}} --useexisting
{{/each}}
{{#each logical_volumes}}{{#if this.mount_point}}
logvol {{this.mount_point}} --vgname={{this.vg_name}} --name={{this.lv_name}} --fstype={{this.filesystem}} --noformat
{{/if}}{{/each}}

bootloader --boot-drive={{partitions.[0].disk_name}}

{{#if config.halt}}halt{{else}}reboot{{/if}}

%packages
@^minimal-environment
{{#each config.packages}}
{{this}}
{{/each}}
%end

%post --erroronfail --log=/var/log/postinstall.log
{{config.postinstall}}
%end

%post --erroronfail --nochroot --log=/mnt/sysroot/var/log/rack-director-post.log
UUID=$(dmidecode -s system-uuid 2>/dev/null)
UUID=$(echo "$UUID" | tr '[:upper:]' '[:lower:]' | tr -d '[:space:]')

echo "rack-director post: system UUID=${UUID}"
if [ -n "$UUID" ]; then
    /usr/bin/curl -sf --max-time 30 --retry 5 --retry-delay 5 \
        -X POST {{rack_director_url}}/cnc/action_success \
        -H "Content-Type: application/json" \
        -d "{\"uuid\":\"${UUID}\"}"
    echo "rack-director post: action_success status=$?"
else
    echo "rack-director post: WARNING: could not read SMBIOS UUID; skipping action_success"
fi
%end

# TODO: In the future, the logs should be sent to rack-director
%onerror
UUID=$(dmidecode -s system-uuid 2>/dev/null)
UUID=$(echo "$UUID" | tr '[:upper:]' '[:lower:]' | tr -d '[:space:]')

echo "rack-director onerror: system UUID=${UUID}"
if [ -n "$UUID" ]; then
    /usr/bin/curl -sf --max-time 30 --retry 5 --retry-delay 5 \
        -X POST {{rack_director_url}}/cnc/action_failed \
        -H "Content-Type: application/json" \
        -d "{\"uuid\":\"${UUID}\", \"error_message\": \"Installation failed.\"}"
    echo "rack-director onerror: action_failed status=$?"
else
    echo "rack-director onerror: WARNING: could not read SMBIOS UUID; skipping action_failed"
fi
%end
