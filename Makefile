# Cross-compilation Makefile for aichat
# Requires: rustup, cargo, zig (for Linux/Windows cross-compilation)

BINARY_NAME := aichat
VERSION := $(shell grep '^version' Cargo.toml | head -1 | cut -d'"' -f2)
RELEASE_DIR := target/release
DIST_DIR := dist

# macOS targets
MACOS_X86 := x86_64-apple-darwin
MACOS_ARM := aarch64-apple-darwin

# Linux targets
LINUX_X86 := x86_64-unknown-linux-gnu
LINUX_ARM := aarch64-unknown-linux-gnu
LINUX_MUSL_X86 := x86_64-unknown-linux-musl
LINUX_MUSL_ARM := aarch64-unknown-linux-musl

# Windows targets
WINDOWS_X86 := x86_64-pc-windows-gnu
WINDOWS_MSVC := x86_64-pc-windows-msvc

.PHONY: all clean setup setup-targets setup-zigbuild \
        build build-release \
        macos macos-x86 macos-arm macos-universal \
        linux linux-x86 linux-arm linux-musl-x86 linux-musl-arm \
        windows windows-x86 \
        dist dist-macos dist-linux dist-windows

# Default target
all: build-release

# Build for current platform
build:
	cargo build

build-release:
	cargo build --release

# ============================================================================
# Setup targets
# ============================================================================

setup: setup-targets setup-zigbuild

setup-targets:
	@echo "Installing Rust cross-compilation targets..."
	# macOS
	rustup target add $(MACOS_X86)
	rustup target add $(MACOS_ARM)
	# Linux
	rustup target add $(LINUX_X86)
	rustup target add $(LINUX_ARM)
	rustup target add $(LINUX_MUSL_X86)
	rustup target add $(LINUX_MUSL_ARM)
	# Windows
	rustup target add $(WINDOWS_X86)
	@echo "Targets installed successfully!"

setup-zigbuild:
	@echo "Installing cargo-zigbuild for cross-compilation..."
	@command -v zig >/dev/null 2>&1 || { echo "Installing zig via brew..."; brew install zig; }
	cargo install cargo-zigbuild
	@echo "cargo-zigbuild installed successfully!"

# ============================================================================
# macOS builds
# ============================================================================

macos: macos-x86 macos-arm

macos-x86:
	cargo build --release --target $(MACOS_X86)

macos-arm:
	cargo build --release --target $(MACOS_ARM)

# Create universal binary (fat binary) for macOS
macos-universal: macos-x86 macos-arm
	@mkdir -p $(DIST_DIR)
	lipo -create \
		target/$(MACOS_X86)/release/$(BINARY_NAME) \
		target/$(MACOS_ARM)/release/$(BINARY_NAME) \
		-output $(DIST_DIR)/$(BINARY_NAME)-universal-apple-darwin

# ============================================================================
# Linux builds (using cargo-zigbuild)
# ============================================================================

linux: linux-x86 linux-arm

linux-x86:
	cargo zigbuild --release --target $(LINUX_X86)

linux-arm:
	cargo zigbuild --release --target $(LINUX_ARM)

linux-musl-x86:
	cargo zigbuild --release --target $(LINUX_MUSL_X86)

linux-musl-arm:
	cargo zigbuild --release --target $(LINUX_MUSL_ARM)

linux-all: linux linux-musl-x86 linux-musl-arm

# ============================================================================
# Windows builds (using cargo-zigbuild)
# ============================================================================

windows: windows-x86

windows-x86:
	cargo zigbuild --release --target $(WINDOWS_X86)

# ============================================================================
# Distribution packages
# ============================================================================

dist: dist-macos dist-linux dist-windows

dist-macos: macos-x86 macos-arm macos-universal
	@mkdir -p $(DIST_DIR)
	@echo "Creating macOS distribution packages..."
	tar -czvf $(DIST_DIR)/$(BINARY_NAME)-$(VERSION)-$(MACOS_X86).tar.gz \
		-C target/$(MACOS_X86)/release $(BINARY_NAME)
	tar -czvf $(DIST_DIR)/$(BINARY_NAME)-$(VERSION)-$(MACOS_ARM).tar.gz \
		-C target/$(MACOS_ARM)/release $(BINARY_NAME)
	tar -czvf $(DIST_DIR)/$(BINARY_NAME)-$(VERSION)-universal-apple-darwin.tar.gz \
		-C $(DIST_DIR) $(BINARY_NAME)-universal-apple-darwin

dist-linux: linux
	@mkdir -p $(DIST_DIR)
	@echo "Creating Linux distribution packages..."
	tar -czvf $(DIST_DIR)/$(BINARY_NAME)-$(VERSION)-$(LINUX_X86).tar.gz \
		-C target/$(LINUX_X86)/release $(BINARY_NAME)
	tar -czvf $(DIST_DIR)/$(BINARY_NAME)-$(VERSION)-$(LINUX_ARM).tar.gz \
		-C target/$(LINUX_ARM)/release $(BINARY_NAME)

dist-linux-musl: linux-musl-x86 linux-musl-arm
	@mkdir -p $(DIST_DIR)
	@echo "Creating Linux musl distribution packages..."
	tar -czvf $(DIST_DIR)/$(BINARY_NAME)-$(VERSION)-$(LINUX_MUSL_X86).tar.gz \
		-C target/$(LINUX_MUSL_X86)/release $(BINARY_NAME)
	tar -czvf $(DIST_DIR)/$(BINARY_NAME)-$(VERSION)-$(LINUX_MUSL_ARM).tar.gz \
		-C target/$(LINUX_MUSL_ARM)/release $(BINARY_NAME)

dist-windows: windows
	@mkdir -p $(DIST_DIR)
	@echo "Creating Windows distribution packages..."
	zip -j $(DIST_DIR)/$(BINARY_NAME)-$(VERSION)-$(WINDOWS_X86).zip \
		target/$(WINDOWS_X86)/release/$(BINARY_NAME).exe

# ============================================================================
# Utilities
# ============================================================================

clean:
	cargo clean
	rm -rf $(DIST_DIR)

# Show available targets
list-targets:
	@echo "Available targets:"
	@echo "  macOS:"
	@echo "    - $(MACOS_X86)"
	@echo "    - $(MACOS_ARM)"
	@echo "  Linux:"
	@echo "    - $(LINUX_X86)"
	@echo "    - $(LINUX_ARM)"
	@echo "    - $(LINUX_MUSL_X86)"
	@echo "    - $(LINUX_MUSL_ARM)"
	@echo "  Windows:"
	@echo "    - $(WINDOWS_X86)"

# Show help
help:
	@echo "aichat Cross-Compilation Makefile"
	@echo ""
	@echo "Setup:"
	@echo "  make setup          - Install all cross-compilation dependencies"
	@echo "  make setup-targets  - Install Rust cross-compilation targets"
	@echo "  make setup-zigbuild - Install zig and cargo-zigbuild"
	@echo ""
	@echo "Build:"
	@echo "  make build          - Build for current platform (debug)"
	@echo "  make build-release  - Build for current platform (release)"
	@echo ""
	@echo "macOS:"
	@echo "  make macos          - Build for all macOS architectures"
	@echo "  make macos-x86      - Build for Intel Macs"
	@echo "  make macos-arm      - Build for Apple Silicon Macs"
	@echo "  make macos-universal - Create universal binary"
	@echo ""
	@echo "Linux:"
	@echo "  make linux          - Build for Linux (x86_64 + ARM64)"
	@echo "  make linux-x86      - Build for Linux x86_64"
	@echo "  make linux-arm      - Build for Linux ARM64"
	@echo "  make linux-musl-x86 - Build for Linux x86_64 (musl)"
	@echo "  make linux-musl-arm - Build for Linux ARM64 (musl)"
	@echo ""
	@echo "Windows:"
	@echo "  make windows        - Build for Windows x86_64"
	@echo ""
	@echo "Distribution:"
	@echo "  make dist           - Create all distribution packages"
	@echo "  make dist-macos     - Create macOS packages"
	@echo "  make dist-linux     - Create Linux packages"
	@echo "  make dist-windows   - Create Windows packages"
	@echo ""
	@echo "Other:"
	@echo "  make clean          - Clean build artifacts"
	@echo "  make list-targets   - List available targets"
	@echo "  make help           - Show this help"
