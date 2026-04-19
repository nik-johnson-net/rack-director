#!/bin/bash
set -ex

RELEASEVER=10

mkdir -p /output
dnf --noplugins -y --releasever "$RELEASEVER" --installroot /agent-image upgrade

KERVERSION=$(chroot /agent-image ls /usr/lib/modules | tail -n 1)
echo "kernel version: $KERVERSION"

chroot /agent-image ln -s /usr/lib /lib

# ============================================
# Install rack-agent into the agent-image
# (it will be included in the squashfs)
# ============================================

# Install rack-agent binary
install -m 755 /rack-agent /agent-image/usr/local/sbin/rack-agent

# Install systemd target and services
install -m 644 /rack-agent.target \
    /agent-image/etc/systemd/system/rack-agent.target
install -m 644 /rack-agent.service \
    /agent-image/etc/systemd/system/rack-agent.service
install -m 644 /rack-agent-reboot.service \
    /agent-image/etc/systemd/system/rack-agent-reboot.service

# Enable rack-agent services and set default target
chroot /agent-image systemctl enable rack-agent.service
chroot /agent-image systemctl enable rack-agent-reboot.service
chroot /agent-image systemctl set-default rack-agent.target

# Install systemd-networkd configuration for DHCP
mkdir -p /agent-image/etc/systemd/network
install -m 644 /networkd-dhcp.network \
    /agent-image/etc/systemd/network/10-dhcp.network

# networkd-wait-online: wait up to 60s for any interface to get online
mkdir -p /agent-image/etc/systemd/system/systemd-networkd-wait-online.service.d
cat > /agent-image/etc/systemd/system/systemd-networkd-wait-online.service.d/timeout.conf << 'EOF'
[Service]
ExecStart=
ExecStart=/usr/lib/systemd/systemd-networkd-wait-online --timeout=60 --any
EOF

# Enable networkd services
chroot /agent-image systemctl enable systemd-networkd.service
chroot /agent-image systemctl enable systemd-networkd-wait-online.service

# Mask lvm2 so we don't autoload pvs
chroot /agent-image systemctl mask lvm2-monitor.service
chroot /agent-image systemctl mask lvm-activate-boot.service

# ============================================
# Clean up unnecessary files to reduce size
# ============================================

# Remove dnf/yum and RPM database (not needed at runtime)
rm -rf /agent-image/var/lib/dnf
rm -rf /agent-image/var/lib/rpm
rm -rf /agent-image/var/cache/dnf
rm -rf /agent-image/var/cache/yum
rm -rf /agent-image/usr/lib/sysimage/rpm

# Remove documentation
rm -rf /agent-image/usr/share/doc
rm -rf /agent-image/usr/share/man
rm -rf /agent-image/usr/share/info
rm -rf /agent-image/usr/share/gtk-doc

# Remove locales except en_US
find /agent-image/usr/share/locale -mindepth 1 -maxdepth 1 -type d ! -name 'en_US' -exec rm -rf {} + 2>/dev/null || true

# Remove development files (not needed at runtime)
rm -rf /agent-image/usr/include
rm -rf /agent-image/usr/lib/gcc
rm -rf /agent-image/usr/src/kernels

# Remove unnecessary /usr/share items
rm -rf /agent-image/usr/share/cracklib
rm -rf /agent-image/usr/share/X11
rm -rf /agent-image/usr/share/groff
rm -rf /agent-image/usr/share/bison
rm -rf /agent-image/usr/share/perl5
rm -rf /agent-image/usr/share/python3-wheels

# Keep only UTC timezone
find /agent-image/usr/share/zoneinfo -mindepth 1 -maxdepth 1 -type d ! -name 'UTC' -exec rm -rf {} + 2>/dev/null || true

# Remove GPU firmware (servers use BMC/IPMI, not displays)
rm -rf /agent-image/usr/lib/firmware/nvidia
rm -rf /agent-image/usr/lib/firmware/amdgpu
rm -rf /agent-image/usr/lib/firmware/radeon
rm -rf /agent-image/usr/lib/firmware/i915
rm -rf /agent-image/usr/lib/firmware/amd-ucode
rm -rf /agent-image/usr/lib/firmware/amd

# Remove WiFi firmware (servers use wired Ethernet)
rm -rf /agent-image/usr/lib/firmware/ath10k
rm -rf /agent-image/usr/lib/firmware/ath11k
rm -rf /agent-image/usr/lib/firmware/ath12k
rm -rf /agent-image/usr/lib/firmware/mediatek
rm -rf /agent-image/usr/lib/firmware/rtw88
rm -rf /agent-image/usr/lib/firmware/rtw89
rm -rf /agent-image/usr/lib/firmware/rtl_bt
rm -rf /agent-image/usr/lib/firmware/rtl_nic
rm -rf /agent-image/usr/lib/firmware/iwlwifi*
rm -rf /agent-image/usr/lib/firmware/ti-connectivity
rm -rf /agent-image/usr/lib/firmware/cypress
rm -rf /agent-image/usr/lib/firmware/brcm/*-pcie.* # Keep brcm server NIC, remove WiFi

# Remove Bluetooth firmware
rm -rf /agent-image/usr/lib/firmware/intel/*bt*
rm -rf /agent-image/usr/lib/firmware/qca

# Remove unnecessary kernel modules
rm -rf /agent-image/usr/lib/modules/*/kernel/drivers/gpu
rm -rf /agent-image/usr/lib/modules/*/kernel/drivers/media
rm -rf /agent-image/usr/lib/modules/*/kernel/drivers/staging
rm -rf /agent-image/usr/lib/modules/*/kernel/sound
rm -rf /agent-image/usr/lib/modules/*/kernel/drivers/infiniband
rm -rf /agent-image/usr/lib/modules/*/kernel/drivers/isdn
rm -rf /agent-image/usr/lib/modules/*/kernel/drivers/bluetooth
rm -rf /agent-image/usr/lib/modules/*/kernel/drivers/nfc
rm -rf /agent-image/usr/lib/modules/*/kernel/drivers/usb/gadget
rm -rf /agent-image/usr/lib/modules/*/kernel/drivers/usb/serial
rm -rf /agent-image/usr/lib/modules/*/kernel/net/wireless
rm -rf /agent-image/usr/lib/modules/*/kernel/net/mac80211
rm -rf /agent-image/usr/lib/modules/*/kernel/net/bluetooth

# Remove Python cache
find /agent-image -type d -name __pycache__ -exec rm -rf {} + 2>/dev/null || true
find /agent-image -name "*.pyc" -delete 2>/dev/null || true

# Remove other unnecessary files
rm -rf /agent-image/var/log/*
rm -rf /agent-image/tmp/*
rm -rf /agent-image/boot/* # kernel is separate

# Rebuild module dependencies after cleanup
chroot /agent-image depmod -a "$KERVERSION" 2>/dev/null || true

# ============================================
# Create squashfs of the agent-image
# ============================================

mksquashfs /agent-image /output/squashfs.img -comp xz -noappend

# Copy squashfs into agent-image for dracut --include
cp /output/squashfs.img /agent-image/tmp/squashfs.img

# ============================================
# Create embedded kernel cmdline
# ============================================

cat > /agent-image/tmp/99-live.conf << 'EOF'
root=live:/squashfs.img
rd.live.overlay.overlayfs=1
EOF

# ============================================
# Build the initramfs with dmsquash-live
# ============================================

chroot /agent-image dracut --kver "$KERVERSION" --no-hostonly --force --reproducible --xz \
    --add "dmsquash-live" \
    --omit "bluetooth resume nfs" \
    --include /tmp/squashfs.img /squashfs.img \
    --include /tmp/99-live.conf /etc/cmdline.d/99-live.conf \
    /initramfs.img

mv /agent-image/initramfs.img /output/initramfs.img

# Check if dracut succeeded
[[ -f '/output/initramfs.img' ]] || exit 1

# Copy the kernel
cp "/agent-image/usr/lib/modules/$KERVERSION/vmlinuz" /output/vmlinuz

echo "Build complete. Outputs:"
ls -lh /output/
