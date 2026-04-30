# Rack Director

Rack Director provides PXEBoot services for automated server installations and web service for viewing and provisioning servers. Rack Director makes it easy to install Operating Systems to new servers or reinstall your existing ones.

## Building

The build system is fully docker based. To build, just run:

```
docker build -f docker/Dockerfile .
```

## Quick Start

First decide how Rack Director will be deployed and what IP address assignment to Rack Director will look like. Rack Director can be deployed to serve the local network segment (L2 Network) or remote network segments via DHCP Relay services, or both simultaneously. DHCP works on broadcast packets and thus Rack Director doesn't need a dedicated IP address, but DHCP Relay will forward based on IP addresses and Rack Director will need to be assigned an address that the DHCP Relay services know about.

The second decision is how Rack Director's persistent storage will work. Rack Director uses a persistent volume for its database at `/var/lib/rack-director`. This directory may also be used for uploaded OSMs.

Then deploy Rack Director's container on the platform of your choice. This container will require NET_ADMIN priviledges and the `host` network mode, and will require traffic on ports UDP 67 and 68 for DHCP, UDP 69 and many high level ports for TFTP, and TCP 3000 for web.

Now Rack Director is up and running. It won't be doing anything useful, since no networks are configured. Navigate to the web interface and then to the Networks page. Create a new Network, and then the address pool for that network. Rack Director will now be responding to DHCP requests for that network, though requests through a relay will require the relay agent to be configured as well.

Devices should be configured to network boot first. When a device boots on the configured network, Rack Director will respond to its DHCP requests. If Autodiscovery is enabled, Rack Director will also provide PXEBoot instructions to load its agent and run hardware discovery on the device. If autodiscovery is disabled, you will need to add the device by MAC address through the web interface either through the Devices page or through the Networks page by viewing active leases. The discovered device will first be assigned a "New" state, and after going through discovery will be assigned the "Unprovisioned" state.

You will notice the Device was also assigned a Platform during discovery. Platforms are a way to deal with hardware quirks across like machines, you do not need to worry about them for now.

Provisioning a Device requires assigning it to a Role, which defines how the machine should be configured. Navigate to the Roles page and click Create Role. You will be presented with a form to select its Operating System and Disk Layout. Disk Layouts can be quite complex, but a sane default is presented for you. With a Role created, you can now navigate to the Devices page, and provision a device to that role. This will kick off the automated installation process.
