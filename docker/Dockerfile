FROM redhat/ubi10 AS web-builder
RUN mkdir /output && mkdir -p /root/.nvm && mkdir -p /build
RUN --mount=type=cache,target=/root/.nvm <<EOT
  curl -o- https://raw.githubusercontent.com/nvm-sh/nvm/v0.40.3/install.sh | bash
  source /root/.nvm/nvm.sh
  nvm install --lts
EOT
COPY rack-director-ui /build
RUN --mount=type=cache,target=/build/rack-director-ui/node_modules --mount=type=cache,target=/root/.nvm <<EOT
  set -ex
  source /root/.nvm/nvm.sh
  cd /build
  npm install
  npx vite build
  mkdir /output/ui
  cp -r dist/* /output/ui
EOT

FROM redhat/ubi10 AS bin-builder
RUN dnf -y install glibc-devel gcc openssl-devel zstd
RUN mkdir -p /build/target && mkdir -p /root/.cargo
RUN --mount=type=cache,target=/root/.cargo curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
COPY --exclude=default-osm --exclude=rack-director-ui . /build
RUN  --mount=type=cache,target=/build/target --mount=type=cache,target=/root/.cargo <<EOT
  set -ex
  source $HOME/.cargo/env
  cd build
  cargo build --release --bin rack-director
  cargo build --release --bin rack-director-osm
  cargo build --release --bin rack-agent
  mkdir /output/
  cp /build/target/release/rack-director /output/
  cp /build/target/release/rack-director-osm /output/
  cp /build/target/release/rack-agent /output/
EOT

FROM redhat/ubi10 AS agent-image-builder
ARG REPO_NAME="centos10-x86_64-baseos-rpms"
ARG RELEASEVER="10"

# Configure the build environment
COPY docker/agent.repo /etc/yum.repos.d/agent.repo

RUN dnf --noplugins -y --repo ${REPO_NAME} install binutils dracut squashfs-tools
RUN dnf --noplugins -y --releasever ${RELEASEVER} --repo ${REPO_NAME} install https://dl.fedoraproject.org/pub/epel/epel-release-latest-10.noarch.rpm
RUN dnf --noplugins -y --releasever ${RELEASEVER} --repo ${REPO_NAME} --repo centos10-x86_64-appstream-rpms --repo epel --installroot /agent-image install \
  redhat-release systemd passwd rootfiles sudo dracut dracut-network dracut-live lvm2 mdadm systemd-networkd btrfs-progs gdisk
RUN dnf --noplugins -y --releasever ${RELEASEVER} --installroot /agent-image install \
  kernel kernel-devel \
  ipmitool parted util-linux \
  e2fsprogs xfsprogs dosfstools \
  dmidecode grep gawk sed

# Copy build dependencies
COPY docker/build-agent.sh /usr/local/bin/build-agent.sh
COPY docker/rack-agent.target /rack-agent.target
COPY docker/rack-agent.service /rack-agent.service
COPY docker/rack-agent-reboot.service /rack-agent-reboot.service
COPY docker/networkd-dhcp.network /networkd-dhcp.network
COPY --from=bin-builder /output/rack-agent /rack-agent
# Build the boot images
RUN chmod +x "/usr/local/bin/build-agent.sh" && "/usr/local/bin/build-agent.sh"

FROM redhat/ubi10 AS director-image-builder
ARG REPO_NAME="centos10-x86_64-baseos-rpms"
ARG RELEASEVER="10"

# Configure the build environment
COPY docker/agent.repo /etc/yum.repos.d/agent.repo

RUN dnf --noplugins -y --repo ${REPO_NAME} install binutils dracut squashfs-tools
RUN dnf --noplugins -y --releasever ${RELEASEVER} --repo ${REPO_NAME} install \
  https://dl.fedoraproject.org/pub/epel/epel-release-latest-10.noarch.rpm
RUN dnf --noplugins -y --releasever ${RELEASEVER} --repo ${REPO_NAME} \
  --repo centos10-x86_64-appstream-rpms --repo epel --installroot /director-image install \
  redhat-release systemd passwd rootfiles sudo dracut dracut-network dracut-live systemd-networkd nftables

RUN dnf --noplugins -y --releasever ${RELEASEVER} --installroot /director-image install \
  kernel

# Copy build dependencies
ADD https://boot.ipxe.org/x86_64-pcbios/undionly.kpxe /firmware/undionly.kpxe
ADD https://boot.ipxe.org/x86_64-efi/snponly.efi /firmware/snponly.efi
COPY docker/build-director.sh /usr/local/bin/build-director.sh
COPY docker/rack-director.service /rack-director.service
COPY docker/networkd-rack.network /networkd-rack.network
COPY docker/networkd-control.network /networkd-control.network
COPY --from=bin-builder /output/rack-director /rack-director
COPY --from=web-builder /output/ui /ui
COPY --from=agent-image-builder /output/vmlinuz /agent-vmlinuz
COPY --from=agent-image-builder /output/initramfs.img /agent-initramfs.img
# Build the director boot images
RUN chmod +x "/usr/local/bin/build-director.sh" && "/usr/local/bin/build-director.sh"

FROM scratch AS agent-image-export
COPY --from=agent-image-builder /output/vmlinuz /vmlinuz
COPY --from=agent-image-builder /output/initramfs.img /initramfs.img

FROM scratch AS rack-director-e2e-export
COPY --from=director-image-builder /output/vmlinuz-director /vmlinuz-director
COPY --from=director-image-builder /output/director-initramfs.img /director-initramfs.img

FROM redhat/ubi10 AS default-osm-builder
RUN dnf -y install zstd
COPY default-osm/ /build/
COPY --from=bin-builder /output/rack-director-osm /bin/rack-director-osm
RUN  --mount=type=cache,target=/build/target --mount=type=cache,target=/root/.cargo <<EOT
  set -ex
  cd /build
  /bin/rack-director-osm build
  mkdir /output/
  tar -C /output -I /usr/bin/unzstd -xf Default.osm
EOT

FROM redhat/ubi10
ADD https://boot.ipxe.org/x86_64-pcbios/undionly.kpxe /opt/rack-director/firmware/undionly.kpxe
ADD https://boot.ipxe.org/x86_64-efi/snponly.efi /opt/rack-director/firmware/snponly.efi
COPY --from=bin-builder /output/rack-director /opt/rack-director/bin/rack-director
COPY --from=web-builder /output/ui /opt/rack-director/ui
COPY --from=agent-image-builder /output/vmlinuz /opt/rack-director/agent/vmlinuz
COPY --from=agent-image-builder /output/initramfs.img /opt/rack-director/agent/initramfs.img
RUN mkdir -p /opt/rack-director/osm/default && mkdir -p /var/lib/rack-director/images
COPY --from=default-osm-builder /output/ /opt/rack-director/osm/default/
VOLUME [ "/var/lib/rack-director" ]
ENTRYPOINT [ "/opt/rack-director/bin/rack-director" ]
