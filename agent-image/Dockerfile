FROM redhat/ubi10:10.0 AS builder
ARG REPO_NAME="centos10-x86_64-baseos-rpms"
ARG RELEASEVER="10"

COPY agent.repo /etc/yum.repos.d/agent.repo

RUN dnf --noplugins -y --repo ${REPO_NAME} install binutils dracut squashfs-tools
RUN dnf --noplugins -y --releasever ${RELEASEVER} --repo ${REPO_NAME} --installroot /agent-image install \
  redhat-release systemd passwd rootfiles sudo dracut dracut-network dnf lvm2 mdadm
RUN dnf --noplugins -y --releasever ${RELEASEVER} --installroot /agent-image install kernel NetworkManager
COPY build-agent.sh /usr/local/bin/build-agent.sh
COPY rack-agent.service /agent-image/etc/systemd/system/rack-agent.service
COPY --from=target rack-agent /agent-image/usr/local/sbin/rack-agent
RUN chmod +x /usr/local/bin/build-agent.sh
RUN /usr/local/bin/build-agent.sh

FROM scratch
COPY --from=builder /output/ /
