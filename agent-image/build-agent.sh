#!/bin/bash
set -x
set -e

RELEASEVER=10
REPO_NAME="centos10-x86_64-baseos-rpms"

mkdir -p /output
dnf --noplugins -y --releasever "$RELEASEVER" --installroot /agent-image upgrade

KERVERSION=$(chroot /agent-image ls /usr/lib/modules)
echo "kernel version: $KERVERSION"

# Build the initramfs
dracut --sysroot /agent-image --kver "$KERVERSION" --no-hostonly --force --reproducible --xz \
  --add "overlayfs" \
  --omit "bluetooth" \
  --omit "resume" \
  --omit "nfs" \
  /output/initramfs.img

# Copy the kernel to the root image
cp "/agent-image/usr/lib/modules/$KERVERSION/vmlinuz" /output/vmlinuz

# Build the root image with squashfs
mksquashfs /agent-image /output/agentfs.sqfs -comp xz -noappend
