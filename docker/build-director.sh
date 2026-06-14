#!/bin/bash
set -ex

RELEASEVER=10

mkdir -p /output
# Upgrade is pinned to the 10.1 vault repos to keep the image reproducible and
# prevent inadvertent upgrades to a newer AlmaLinux minor release.
# reposdir is forced to the builder's /etc/yum.repos.d (where agent.repo lives):
# with --installroot, dnf otherwise reads repo configs from the now-populated
# /director-image/etc/yum.repos.d (the stock almalinux.repo), where our pinned
# repo IDs are undefined.
dnf --noplugins -y --setopt=reposdir=/etc/yum.repos.d --installroot /director-image --repo almalinux10-x86_64-baseos-rpms --repo almalinux10-x86_64-appstream-rpms upgrade

KERVERSION=$(chroot /director-image ls /usr/lib/modules | tail -n 1)
echo "kernel version: $KERVERSION"

# Copy the kernel to output before any cleanup
cp "/director-image/usr/lib/modules/$KERVERSION/vmlinuz" /output/vmlinuz-director

chroot /director-image ln -s /usr/lib /lib

# ============================================
# Install rack-director into the director-image
# ============================================

# Install rack-director binary
install -m 755 /rack-director /director-image/usr/local/sbin/rack-director

# Create runtime data directories
mkdir -p /director-image/var/lib/rack-director/data

# Create install directories (paths baked into binary at /opt/rack-director)
mkdir -p /director-image/opt/rack-director/firmware
mkdir -p /director-image/opt/rack-director/agent
mkdir -p /director-image/opt/rack-director/ui

# Copy agent images so rack-director can serve them via TFTP/HTTP
install -m 644 /agent-vmlinuz /director-image/opt/rack-director/agent/vmlinuz
install -m 644 /agent-initramfs.img /director-image/opt/rack-director/agent/initramfs.img

# Copy iPXE firmware so rack-director can serve it to PXE-booting devices
install -m 644 /firmware/undionly.kpxe /director-image/opt/rack-director/firmware/undionly.kpxe
install -m 644 /firmware/snponly.efi /director-image/opt/rack-director/firmware/snponly.efi

# Copy UI static files
cp -r /ui/. /director-image/opt/rack-director/ui/

# Install systemd service
install -m 644 /rack-director.service \
    /director-image/etc/systemd/system/rack-director.service

# Enable rack-director service
chroot /director-image systemctl enable rack-director.service

# Mask serial-getty: nothing in the director VM needs an interactive console,
# and serial-getty@ttyS0.service has a hard dependency on dev-ttyS0.device which
# udev may not create quickly (or at all) in a live-boot overlayfs environment.
# Without masking, systemd waits 90s for the device unit, blocking boot and
# silencing console output during that window.
chroot /director-image systemctl mask serial-getty@ttyS0.service

# Forward journald output to the serial console so boot progress is visible
# in the serial log even without a getty. /dev/console always works because
# the kernel already bound ttyS0 via console=ttyS0 in the kernel cmdline.
mkdir -p /director-image/etc/systemd/journald.conf.d
cat > /director-image/etc/systemd/journald.conf.d/serial-console.conf << 'EOF'
[Journal]
ForwardToConsole=yes
MaxLevelConsole=info
EOF

# Install systemd-networkd configuration
mkdir -p /director-image/etc/systemd/network
install -m 644 /networkd-rack.network \
    /director-image/etc/systemd/network/10-rack.network
install -m 644 /networkd-control.network \
    /director-image/etc/systemd/network/20-control.network

# Enable networkd services
chroot /director-image systemctl enable systemd-networkd.service
chroot /director-image systemctl enable systemd-networkd-wait-online.service

# Mask NetworkManager - it is pulled in as a dependency of almalinux-release but
# conflicts with systemd-networkd. Without masking, NM auto-creates DHCP profiles
# for both NICs and eventually removes the static 10.0.0.1 address by timing out
# its DHCP requests on the rack interface.
chroot /director-image systemctl mask NetworkManager.service
chroot /director-image systemctl mask NetworkManager-dispatcher.service
chroot /director-image systemctl mask NetworkManager-wait-online.service

# ============================================
# Enable IP forwarding and NAT masquerade so agent VMs can reach the internet.
#
# The agent VM's default gateway is 10.0.0.1 (this director VM, NIC0).
# NIC1 (user-networking) has internet access via QEMU's NAT (10.0.2.x / 10.0.2.2).
# nftables masquerades 10.0.0.0/24 traffic out through NIC1 so agents can reach
# external mirrors (e.g. dl.rockylinux.org) during OS installation.
# ============================================

# Enable IP forwarding persistently
cat > /director-image/etc/sysctl.d/10-forwarding.conf << 'EOF'
net.ipv4.ip_forward=1
EOF

# NAT masquerade: apply nft rules via a shell script to avoid escaping issues
# with braces/semicolons in systemd ExecStart lines.
# Runs after both NICs are up so the masquerade target can resolve its egress IP.
cat > /director-image/usr/local/sbin/rack-nat-start.sh << 'EOF'
#!/bin/bash
set -e
/sbin/nft add table ip nat
/sbin/nft add chain ip nat postrouting '{ type nat hook postrouting priority srcnat ; policy accept ; }'
/sbin/nft add rule ip nat postrouting ip saddr 10.0.0.0/24 masquerade
EOF
chmod +x /director-image/usr/local/sbin/rack-nat-start.sh

cat > /director-image/etc/systemd/system/rack-nat.service << 'EOF'
[Unit]
Description=NAT masquerade for rack agent subnet
After=network-online.target
Wants=network-online.target

[Service]
Type=oneshot
RemainAfterExit=yes
ExecStart=/usr/local/sbin/rack-nat-start.sh
ExecStop=/sbin/nft delete table ip nat

[Install]
WantedBy=multi-user.target
EOF
chroot /director-image systemctl enable rack-nat.service

# Diagnostic oneshot: log routes + nftables rules + ip_forward to serial after
# the network and NAT are both up.  Output flows via journald ForwardToConsole.
cat > /director-image/etc/systemd/system/rack-nat-diag.service << 'EOF'
[Unit]
Description=NAT routing diagnostics (rack-director e2e)
After=rack-nat.service

[Service]
Type=oneshot
RemainAfterExit=yes
ExecStart=/bin/sh -c 'echo "=NAT-DIAG ip_forward=" && cat /proc/sys/net/ipv4/ip_forward && echo "=NAT-DIAG ip route=" && ip route && echo "=NAT-DIAG nft ruleset=" && nft list ruleset && echo "=NAT-DIAG end="'

[Install]
WantedBy=multi-user.target
EOF
chroot /director-image systemctl enable rack-nat-diag.service

# Configure networkd-wait-online to succeed as soon as ANY one interface is
# routable (--any). The rack NIC uses a static address and comes up in ~5s;
# we don't need to wait for the DHCP control NIC before starting rack-director.
# Also set --timeout=60 so it never hangs forever in edge cases.
mkdir -p /director-image/etc/systemd/system/systemd-networkd-wait-online.service.d
cat > /director-image/etc/systemd/system/systemd-networkd-wait-online.service.d/any.conf << 'EOF'
[Service]
ExecStart=
ExecStart=/usr/lib/systemd/systemd-networkd-wait-online --timeout=60 --any
EOF

# ============================================
# Clean up unnecessary files to reduce size
# ============================================

# Remove dnf/yum and RPM database
rm -rf /director-image/var/lib/dnf
rm -rf /director-image/var/lib/rpm
rm -rf /director-image/var/cache/dnf
rm -rf /director-image/var/cache/yum
rm -rf /director-image/usr/lib/sysimage/rpm

# Remove documentation
rm -rf /director-image/usr/share/doc
rm -rf /director-image/usr/share/man
rm -rf /director-image/usr/share/info
rm -rf /director-image/usr/share/gtk-doc

# Remove locales except en_US
find /director-image/usr/share/locale -mindepth 1 -maxdepth 1 -type d ! -name 'en_US' -exec rm -rf {} + 2>/dev/null || true

# Remove development files
rm -rf /director-image/usr/include
rm -rf /director-image/usr/lib/gcc
rm -rf /director-image/usr/src/kernels

# Remove unnecessary /usr/share items
rm -rf /director-image/usr/share/cracklib
rm -rf /director-image/usr/share/X11
rm -rf /director-image/usr/share/groff
rm -rf /director-image/usr/share/bison
rm -rf /director-image/usr/share/perl5
rm -rf /director-image/usr/share/python3-wheels

# Keep only UTC timezone
find /director-image/usr/share/zoneinfo -mindepth 1 -maxdepth 1 -type d ! -name 'UTC' -exec rm -rf {} + 2>/dev/null || true

# Remove GPU firmware
rm -rf /director-image/usr/lib/firmware/nvidia
rm -rf /director-image/usr/lib/firmware/amdgpu
rm -rf /director-image/usr/lib/firmware/radeon
rm -rf /director-image/usr/lib/firmware/i915
rm -rf /director-image/usr/lib/firmware/amd-ucode
rm -rf /director-image/usr/lib/firmware/amd

# Remove WiFi firmware
rm -rf /director-image/usr/lib/firmware/ath10k
rm -rf /director-image/usr/lib/firmware/ath11k
rm -rf /director-image/usr/lib/firmware/ath12k
rm -rf /director-image/usr/lib/firmware/mediatek
rm -rf /director-image/usr/lib/firmware/rtw88
rm -rf /director-image/usr/lib/firmware/rtw89
rm -rf /director-image/usr/lib/firmware/rtl_bt
rm -rf /director-image/usr/lib/firmware/rtl_nic
rm -rf /director-image/usr/lib/firmware/iwlwifi*
rm -rf /director-image/usr/lib/firmware/ti-connectivity
rm -rf /director-image/usr/lib/firmware/cypress

# Remove Bluetooth firmware
rm -rf /director-image/usr/lib/firmware/intel/*bt*
rm -rf /director-image/usr/lib/firmware/qca

# Remove Python cache
find /director-image -type d -name __pycache__ -exec rm -rf {} + 2>/dev/null || true
find /director-image -name "*.pyc" -delete 2>/dev/null || true

# Remove other unnecessary files
rm -rf /director-image/var/log/*
rm -rf /director-image/tmp/*
rm -rf /director-image/boot/* # kernel is separate

# ============================================
# Build squashfs WITHOUT kernel modules, embed in dracut initramfs.
#
# Strategy: temporarily move modules out of director-image so the squashfs
# is small (~461 MiB), then restore them for dracut so it can include the
# required drivers (virtio-net, squashfs, overlay, etc.) in the initramfs.
# Final initramfs = ~48 MiB base + 461 MiB squashfs = ~509 MiB, just under
# QEMU's 511.9 MiB direct-kernel-boot limit.
# ============================================

# Move modules out temporarily so they're excluded from the squashfs
mv "/director-image/usr/lib/modules" /tmp/director-modules

# Build the small squashfs (no kernel modules)
mksquashfs /director-image /output/squashfs-director.img -comp xz -noappend

# Restore modules so dracut can build the initramfs with required drivers
mv /tmp/director-modules "/director-image/usr/lib/modules"

# Copy squashfs into the image's /tmp for dracut to embed
mkdir -p /director-image/tmp
cp /output/squashfs-director.img /director-image/tmp/squashfs-director.img

# Write dracut live boot configuration
cat > /director-image/tmp/99-live.conf << 'EOF'
root=live:/squashfs-director.img
rd.live.overlay.overlayfs=1
EOF

# Build initramfs with dmsquash-live + embedded squashfs.
# Expected size: ~48 MiB (modules) + 461 MiB (squashfs) = ~509 MiB < 511.9 MiB limit.
#
# --force-drivers: The squashfs root has no kernel modules (excluded to save space), so
# modprobe cannot load anything after switch_root.  nf_tables and its NAT/masquerade
# dependencies must be force-loaded here (before root switch) so that nftables.service
# can open its Netlink socket after the squashfs environment starts.
chroot /director-image dracut --kver "$KERVERSION" --no-hostonly --force --reproducible --xz \
    --add "dmsquash-live" \
    --force-drivers "nf_tables nf_conntrack nf_nat nft_chain_nat nft_masq" \
    --omit "bluetooth resume nfs" \
    --include /tmp/squashfs-director.img /squashfs-director.img \
    --include /tmp/99-live.conf /etc/cmdline.d/99-live.conf \
    /initramfs-director.img

mv /director-image/initramfs-director.img /output/director-initramfs.img

# Check that both outputs were produced
[[ -f '/output/director-initramfs.img' ]] || exit 1

echo "Build complete. Outputs:"
ls -lh /output/
