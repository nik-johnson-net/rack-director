VERSION := $(shell cargo metadata --frozen --no-deps --format-version 1 | jq -r '.packages | map(select(.name == "rack-director"))[0] | .version')
BUNDLE_DIR := target/bundle/rack-director-$(VERSION)
.PHONY: build_release bundle

default: bundle

build_release:
	@echo "Building rack-director release version $(VERSION)"
	@cargo build --release

$(BUNDLE_DIR)/tftp/undionly.kpxe:
	@echo "Downloading undionly.kpxe"
	@mkdir -p $(BUNDLE_DIR)/tftp
	@wget -O $(BUNDLE_DIR)/tftp/undionly.kpxe https://boot.ipxe.org/undionly.kpxe

$(BUNDLE_DIR)/tftp/ipxe.efi:
	@echo "Downloading ipxe.efi"
	@mkdir -p $(BUNDLE_DIR)/tftp
	@wget -O $(BUNDLE_DIR)/tftp/ipxe.efi https://boot.ipxe.org/ipxe.efi

$(BUNDLE_DIR)/agent-image/vmlinuz $(BUNDLE_DIR)/agent-image/initramfs.img $(BUNDLE_DIR)/agent-image/agent-image.sqfs:
	@echo "Building agent image"
	@docker build --output=$(BUNDLE_DIR)/agent-image agent-image/

bundle: build_release $(BUNDLE_DIR)/tftp/undionly.kpxe $(BUNDLE_DIR)/tftp/ipxe.efi $(BUNDLE_DIR)/agent-image/vmlinuz $(BUNDLE_DIR)/agent-image/initramfs.img $(BUNDLE_DIR)/agent-image/agent-image.sqfs
	@echo "Bundling rack-director version $(VERSION)"
	@mkdir -p $(BUNDLE_DIR)
	@cp target/release/rack-director $(BUNDLE_DIR)/rack-director
	@tar -czf target/bundle/rack-director-$(VERSION).tar.gz -C target/bundle rack-director-$(VERSION)
	@echo "Bundle created at target/bundle/rack-director-$(VERSION).tar.gz"
