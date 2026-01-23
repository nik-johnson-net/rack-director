#!/bin/bash

check() {
    return 0
}

depends() {
    echo "systemd-networkd systemd"
}

install() {
    # Install rack-agent binary
    inst_simple "$moddir/rack-agent" "/usr/local/sbin/rack-agent"
    chmod 755 "${initdir}/usr/local/sbin/rack-agent"

    # Install systemd service
    inst_simple "$moddir/rack-agent.service" "${systemdsystemunitdir}/rack-agent.service"

    # Enable the service in initrd.target
    mkdir -p "${initdir}${systemdsystemunitdir}/initrd.target.wants"
    ln -sf "../rack-agent.service" \
        "${initdir}${systemdsystemunitdir}/initrd.target.wants/rack-agent.service"

    # Install systemd-networkd configuration for DHCP
    mkdir -p "${initdir}/etc/systemd/network"
    inst_simple "$moddir/networkd-dhcp.network" "/etc/systemd/network/10-dhcp.network"

    # Install required CLI tools for rack-agent operations
    inst_multiple ipmitool parted lsblk \
                  mkfs.ext2 mkfs.ext3 mkfs.ext4 \
                  mkfs.xfs mkfs.btrfs mkswap mkfs.vfat \
                  blkid partprobe sfdisk gdisk \
                  dmidecode grep awk sed

    return 0
}
