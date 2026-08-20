VERSION := $(shell sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n1)
TARGET_WIN := x86_64-pc-windows-gnu
WIN_EXE := target/$(TARGET_WIN)/release/ha-desktop-agent.exe
LINUX_BIN := target/release/ha-desktop-agent
DIST := dist
DEB_ROOT := $(DIST)/deb/ha-desktop-agent

.PHONY: linux windows all test dist deb windows-installer release clean

linux:
	cargo build --release

windows:
	cargo build --release --target $(TARGET_WIN)

all: linux windows

test:
	cargo test

dist: linux windows
	mkdir -p $(DIST)
	cp -f $(LINUX_BIN) $(DIST)/ha-desktop-agent
	cp -f $(WIN_EXE) $(DIST)/ha-desktop-agent.exe

deb: linux
	rm -rf $(DEB_ROOT)
	mkdir -p $(DEB_ROOT)/DEBIAN
	mkdir -p $(DEB_ROOT)/usr/bin
	mkdir -p $(DEB_ROOT)/usr/lib/systemd/user
	mkdir -p $(DEB_ROOT)/usr/share/ha-desktop-agent
	mkdir -p $(DEB_ROOT)/usr/share/doc/ha-desktop-agent
	mkdir -p $(DEB_ROOT)/usr/share/polkit-1/rules.d
	install -m755 $(LINUX_BIN) $(DEB_ROOT)/usr/bin/ha-desktop-agent
	install -m644 packaging/debian/ha-desktop-agent.service $(DEB_ROOT)/usr/lib/systemd/user/ha-desktop-agent.service
	install -m644 config.example.yaml $(DEB_ROOT)/usr/share/ha-desktop-agent/config.example.yaml
	install -m644 README.md $(DEB_ROOT)/usr/share/doc/ha-desktop-agent/README.md
	install -m644 LICENSE $(DEB_ROOT)/usr/share/doc/ha-desktop-agent/LICENSE
	install -m644 packaging/debian/polkit-rules/50-ha-desktop-agent-update.rules $(DEB_ROOT)/usr/share/polkit-1/rules.d/50-ha-desktop-agent-update.rules
	sed 's/@VERSION@/$(VERSION)/' packaging/debian/control > $(DEB_ROOT)/DEBIAN/control
	install -m755 packaging/debian/postinst $(DEB_ROOT)/DEBIAN/postinst
	mkdir -p $(DIST)
	dpkg-deb --root-owner-group --build $(DEB_ROOT) $(DIST)/ha-desktop-agent_$(VERSION)_amd64.deb

windows-installer: windows
	mkdir -p $(DIST)
	makensis -DVERSION=$(VERSION) -DEXE_PATH=$(CURDIR)/$(WIN_EXE) installer/windows/ha-desktop-agent.nsi

# Build artifacts, checksums, and signature. Then: git tag + gh release create.
release: deb windows-installer
	mkdir -p $(DIST)
	cp -f $(LINUX_BIN) $(DIST)/ha-desktop-agent-linux-x86_64
	cd $(DIST) && sha256sum \
		ha-desktop-agent_$(VERSION)_amd64.deb \
		ha-desktop-agent-linux-x86_64 \
		ha-desktop-agent-setup.exe \
		> SHA256SUMS
	python3 scripts/sign-sha256sums.py $(DIST)/SHA256SUMS
	@echo "Artifacts ready in $(DIST)/. Next:"
	@echo "  git tag -a v$(VERSION) -m 'v$(VERSION)' && git push origin v$(VERSION)"
	@echo "  gh release create v$(VERSION) \\"
	@echo "    $(DIST)/ha-desktop-agent_$(VERSION)_amd64.deb \\"
	@echo "    $(DIST)/ha-desktop-agent-linux-x86_64 \\"
	@echo "    $(DIST)/ha-desktop-agent-setup.exe \\"
	@echo "    $(DIST)/SHA256SUMS $(DIST)/SHA256SUMS.sig"

clean:
	cargo clean
	rm -rf $(DIST)
