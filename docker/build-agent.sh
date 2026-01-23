#!/bin/bash
set -ex

RELEASEVER=10
REPO_NAME="centos10-x86_64-baseos-rpms"

mkdir -p /output
dnf --noplugins -y --releasever "$RELEASEVER" --installroot /agent-image upgrade

KERVERSION=$(chroot /agent-image ls /usr/lib/modules)
echo "kernel version: $KERVERSION"

chroot /agent-image ln -s /usr/lib /lib

# Install custom dracut module
mkdir -p /agent-image/usr/lib/dracut/modules.d/99rack-agent
cp /dracut-modules/99rack-agent/module-setup.sh /agent-image/usr/lib/dracut/modules.d/99rack-agent/
cp /dracut-modules/99rack-agent/rack-agent.service /agent-image/usr/lib/dracut/modules.d/99rack-agent/
cp /dracut-modules/99rack-agent/networkd-dhcp.network /agent-image/usr/lib/dracut/modules.d/99rack-agent/
cp /rack-agent /agent-image/usr/lib/dracut/modules.d/99rack-agent/
chmod +x /agent-image/usr/lib/dracut/modules.d/99rack-agent/module-setup.sh

# Build the initramfs
chroot /agent-image dracut --kver "$KERVERSION" --no-hostonly --force --reproducible --xz \
  --add "systemd-networkd rack-agent" \
  --omit "bluetooth resume nfs" \
  /initramfs.img
mv /agent-image/initramfs.img /output/initramfs.img

# Check if dracut succeeded
[[ -f '/output/initramfs.img' ]] || exit 1

# Copy the kernel to the root image
cp "/agent-image/usr/lib/modules/$KERVERSION/vmlinuz" /output/vmlinuz
