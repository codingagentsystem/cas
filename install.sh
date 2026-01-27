#!/usr/bin/env bash
# CAS Installation Script
# Usage: curl -fsSL https://cas.dev/install.sh | bash
#
# Environment variables:
#   CAS_VERSION    - Specific version to install (default: latest)
#   CAS_INSTALL_DIR - Installation directory (default: /usr/local/bin or ~/.local/bin)

set -euo pipefail

REPO="codingagentsystem/cas"
BINARY_NAME="cas"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

info() {
    echo -e "${BLUE}==>${NC} $1"
}

success() {
    echo -e "${GREEN}==>${NC} $1"
}

warn() {
    echo -e "${YELLOW}Warning:${NC} $1"
}

error() {
    echo -e "${RED}Error:${NC} $1" >&2
    exit 1
}

# Detect OS
detect_os() {
    local os
    os=$(uname -s)
    case "$os" in
        Darwin) echo "darwin" ;;
        Linux) echo "linux" ;;
        *) error "Unsupported operating system: $os" ;;
    esac
}

# Detect architecture
detect_arch() {
    local arch
    arch=$(uname -m)
    case "$arch" in
        x86_64|amd64) echo "x86_64" ;;
        arm64|aarch64) echo "aarch64" ;;
        *) error "Unsupported architecture: $arch" ;;
    esac
}

# Get the target triple for the current platform
get_target() {
    local os arch
    os=$(detect_os)
    arch=$(detect_arch)

    case "${os}-${arch}" in
        darwin-aarch64) echo "aarch64-apple-darwin" ;;
        darwin-x86_64) error "Intel macOS is not currently supported. Please use an Apple Silicon Mac or Linux." ;;
        linux-x86_64) echo "x86_64-unknown-linux-gnu" ;;
        linux-aarch64) error "ARM64 Linux is not currently supported." ;;
        *) error "Unsupported platform: ${os}-${arch}" ;;
    esac
}

# Get the latest version from GitHub releases
get_latest_version() {
    local version
    version=$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" | grep '"tag_name"' | sed -E 's/.*"([^"]+)".*/\1/')
    if [[ -z "$version" ]]; then
        error "Failed to fetch latest version from GitHub"
    fi
    echo "$version"
}

# Determine installation directory
get_install_dir() {
    if [[ -n "${CAS_INSTALL_DIR:-}" ]]; then
        echo "$CAS_INSTALL_DIR"
    elif [[ -w /usr/local/bin ]]; then
        echo "/usr/local/bin"
    else
        echo "$HOME/.local/bin"
    fi
}

# Main installation logic
main() {
    info "Installing CAS - Coding Agent System"
    echo ""

    # Detect platform
    local target
    target=$(get_target)
    info "Detected platform: $target"

    # Get version
    local version
    version="${CAS_VERSION:-$(get_latest_version)}"
    info "Version: $version"

    # Determine install directory
    local install_dir
    install_dir=$(get_install_dir)
    info "Install directory: $install_dir"

    # Create install directory if needed
    if [[ ! -d "$install_dir" ]]; then
        info "Creating directory: $install_dir"
        mkdir -p "$install_dir"
    fi

    # Download URL
    local download_url="https://github.com/${REPO}/releases/download/${version}/${BINARY_NAME}-${target}.tar.gz"
    info "Downloading from: $download_url"

    # Create temp directory
    local tmp_dir
    tmp_dir=$(mktemp -d)
    trap "rm -rf '$tmp_dir'" EXIT

    # Download and extract
    if ! curl -fsSL "$download_url" | tar -xz -C "$tmp_dir"; then
        error "Failed to download CAS. Please check if version '$version' exists for your platform."
    fi

    # Install binary
    local binary_path="$tmp_dir/$BINARY_NAME"
    if [[ ! -f "$binary_path" ]]; then
        error "Binary not found in archive"
    fi

    chmod +x "$binary_path"

    # Move to install directory (may need sudo)
    if [[ -w "$install_dir" ]]; then
        mv "$binary_path" "$install_dir/$BINARY_NAME"
    else
        info "Elevated permissions required to install to $install_dir"
        sudo mv "$binary_path" "$install_dir/$BINARY_NAME"
    fi

    echo ""
    success "CAS $version installed successfully!"
    echo ""

    # Check if install directory is in PATH
    if [[ ":$PATH:" != *":$install_dir:"* ]]; then
        warn "$install_dir is not in your PATH"
        echo ""
        echo "Add it to your shell profile:"
        echo ""
        echo "  # For bash (~/.bashrc or ~/.bash_profile)"
        echo "  export PATH=\"$install_dir:\$PATH\""
        echo ""
        echo "  # For zsh (~/.zshrc)"
        echo "  export PATH=\"$install_dir:\$PATH\""
        echo ""
        echo "  # For fish (~/.config/fish/config.fish)"
        echo "  set -gx PATH $install_dir \$PATH"
        echo ""
    fi

    # Verify installation
    if command -v cas &> /dev/null; then
        echo "Run 'cas --help' to get started."
    else
        echo "Run '$install_dir/cas --help' to get started."
    fi

    echo ""
    echo "To update CAS in the future, simply run:"
    echo "  cas update"
}

main "$@"
