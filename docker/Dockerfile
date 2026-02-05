FROM redhat/ubi10 AS bin-builder
RUN dnf -y install glibc-devel gcc openssl-devel
RUN mkdir -p /build/target && mkdir -p /root/.cargo && mkdir -p /root/.nvm
RUN --mount=type=cache,target=/root/.cargo curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
RUN --mount=type=cache,target=/root/.nvm <<EOT
  curl -o- https://raw.githubusercontent.com/nvm-sh/nvm/v0.40.3/install.sh | bash
  source /root/.nvm/nvm.sh
  nvm install --lts
EOT
COPY . /build
RUN  --mount=type=cache,target=/build/target --mount=type=cache,target=/root/.cargo <<EOT
  set -ex
  source $HOME/.cargo/env
  cd build
  cargo build --release --bin rack-director
  cargo build --release --bin rack-agent
  mkdir /output/
  cp /build/target/release/rack-director /output/
  cp /build/target/release/rack-agent /output/
EOT
RUN --mount=type=cache,target=/build/rack-director-ui/node_modules --mount=type=cache,target=/root/.nvm <<EOT
  set -ex
  source /root/.nvm/nvm.sh
  cd /build/rack-director-ui
  npm install
  npx vite build
  mkdir /output/ui
  cp -r dist/* /output/ui
EOT

FROM redhat/ubi10 AS agent-image-builder
ARG REPO_NAME="centos10-x86_64-baseos-rpms"
ARG RELEASEVER="10"

# Configure the build environment
COPY docker/agent.repo /etc/yum.repos.d/agent.repo

RUN dnf --noplugins -y --repo ${REPO_NAME} install binutils dracut
RUN dnf --noplugins -y --releasever ${RELEASEVER} --repo ${REPO_NAME} install https://dl.fedoraproject.org/pub/epel/epel-release-latest-10.noarch.rpm
RUN dnf --noplugins -y --releasever ${RELEASEVER} --repo ${REPO_NAME} --repo epel --installroot /agent-image install \
  redhat-release systemd passwd rootfiles sudo dracut dracut-network dnf lvm2 mdadm systemd-networkd btrfs-progs gdisk
RUN dnf --noplugins -y --releasever ${RELEASEVER} --installroot /agent-image install \
  kernel kernel-devel \
  ipmitool parted util-linux \
  e2fsprogs xfsprogs dosfstools \
  dmidecode grep gawk sed

# Copy build dependencies
COPY docker/build-agent.sh /usr/local/bin/build-agent.sh
COPY docker/dracut-modules /dracut-modules
COPY --from=bin-builder /output/rack-agent /rack-agent
# Build the boot images
RUN chmod +x "/usr/local/bin/build-agent.sh" && "/usr/local/bin/build-agent.sh"

FROM redhat/ubi10
ADD https://boot.ipxe.org/x86_64-pcbios/undionly.kpxe /opt/rack-director/firmware/undionly.kpxe
ADD https://boot.ipxe.org/x86_64-efi/ipxe.efi /opt/rack-director/firmware/ipxe.efi
COPY --from=bin-builder /output/rack-director /opt/rack-director/bin/rack-director
COPY --from=bin-builder /output/ui /opt/rack-director/ui
COPY --from=agent-image-builder /output/vmlinuz /opt/rack-director/agent/vmlinuz
COPY --from=agent-image-builder /output/initramfs.img /opt/rack-director/agent/initramfs.img
VOLUME [ "/var/lib/rack-director" ]
ENTRYPOINT [ "/opt/rack-director/bin/rack-director" ]