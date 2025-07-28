# Rack Director

Rack Director provides low level machine inventory and control for a rack using netboot techniques and IPMI. It is stateful; allowing for runtime configuration.

## System Design

Rack Director can be configured in conjunction with an external DHCP server, or use its internal one. Both require a static IP address to be allocated to the Rack Director.

Rack Director commands machines in its control by netbooting a control image. At first boot, a machine's information is recorded by Rack Director, IPMI can be reset and configured, and the machine can automatically boot into a net installer.

Rack Director is not highly-available. It supports either an S3-like interface, or local file storage for images.

## Devices

A Device is any networked chassis that can be booted by rack-director. Devices may be uniquely identified by MAC Address, Rack Position, or UUID, though a device may have multiple NICs and thus multiple MAC addresses.

## Lifecycle

Devices can be in one of several states. Devices move between states by executing a series a steps, or plans. A Device that has been created in rack-director but not seen on the network is "new". A device then seen on the network, or auto-discovered, are then moved to "Unprovisioned" by running steps such as memtest, part enumeration, firmware updates, BMC configuration, etc. At this point the node is ready to be provisioned in an Infrastructure-as-a-Service (IaaS) manner.

Provisioning a node moves it to the Provisioned state, which is accomplished by configuring NICs, disks, and installing an operating system. Unprovisioning a node moves it back to the Unprovisioned state, which can include wiping the disks. Finally, a machine can be "Removed", keeping its history but not allowing more actions.

Hardware is, well, hard. Failures can happen at any point. Failures in a transition are handled by moving the device to a "Broken" state, requiring intervention to debug and fix issues. Devices can then be moved back to "Unprovisioned", which will re-run discovery, disk-wipes, etc.

Lifecycle transitions are tracked in the lifecycle_transition table.

## Actions

Actions are the underlying instructions for a device to take some action, like reboot, install an OS, or wipe disks. Actions can take parameters, useful for configuring login details or what OS to install. Actions are organized into Plans, useful for linking back to Lifecycles.

A table called plans is used to store a list of actions, their parameters, and the current step.
