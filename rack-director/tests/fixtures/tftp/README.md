# Test Fixtures - TFTP Boot Files

This directory contains mock boot files used for integration testing.

## Files

- `undionly.kpxe`: Mock iPXE bootloader for BIOS systems
  - Used in PXE boot integration tests
  - Not a real bootloader - just a placeholder for testing

These files are intentionally small and simple since we only need to verify:
1. TFTP server can serve files
2. Correct filename is returned in DHCP options
3. File transfer completes successfully

For production use, real iPXE bootloader files should be placed in `/usr/lib/rack-director/tftp/`.