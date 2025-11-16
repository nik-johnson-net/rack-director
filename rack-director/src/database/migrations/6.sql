-- Add original filename columns to os_architectures table
ALTER TABLE os_architectures ADD COLUMN kernel_filename TEXT;
ALTER TABLE os_architectures ADD COLUMN initramfs_filename TEXT;
ALTER TABLE os_architectures ADD COLUMN install_script_filename TEXT;
