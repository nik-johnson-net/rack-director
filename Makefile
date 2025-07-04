VERSION := $(shell cargo metadata --frozen --no-deps --format-version 1 | jq -r '.packages | map(select(.name == "rack-director"))[0] | .version')
BUNDLE_DIR := target/bundle/rack-director-$(VERSION)
.PHONY: build_release bundle

default: bundle

build_release:
	@echo "Building release version $(VERSION)"
	@cargo build --release

target/bundle/rack-director/tftp/undionly.kpxe:
	@echo "Downloading undionly.kpxe"
	@mkdir -p $(BUNDLE_DIR)/tftp
	@wget -O $(BUNDLE_DIR)/tftp/undionly.kpxe https://boot.ipxe.org/undionly.kpxe

target/bundle/rack-director/tftp/ipxe.efi:
	@echo "Downloading ipxe.efi"
	@mkdir -p $(BUNDLE_DIR)/tftp
	@wget -O $(BUNDLE_DIR)/tftp/ipxe.efi https://boot.ipxe.org/ipxe.efi

bundle: build_release target/bundle/rack-director/tftp/undionly.kpxe target/bundle/rack-director/tftp/ipxe.efi
	@mkdir -p $(BUNDLE_DIR)
	@cp target/release/rack-director $(BUNDLE_DIR)/rack-director
	@tar -czf target/bundle/rack-director-$(VERSION).tar.gz -C target/bundle rack-director-$(VERSION)
