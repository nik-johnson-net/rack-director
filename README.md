# Rack Director

Rack Director provides low level machine inventory and control for a rack using netboot techniques and IPMI. It is stateful; allowing for runtime configuration.

## System Design

Rack Director can be configured in conjunction with an external DHCP server, or use its internal one. Both require a static IP address to be allocated to the Rack Director.

Rack Director commands machines in its control by netbooting a control image. At first boot, a machine's information is recorded by Rack Director, IPMI can be reset and configured, and the machine can automatically boot into a net installer.

Rack Director is not highly-available. It supports either an S3-like interface, or local file storage for images.
