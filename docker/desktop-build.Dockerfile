# syntax=docker/dockerfile:1

# Pinned Linux toolchain for the StackSpirit Tauri desktop host.
#
# This image exists because the D4 worker host has no GTK/WebKit/Soup development
# packages and no passwordless sudo. It installs tools only; it never copies the
# repository. scripts/desktop-build-container.sh bind-mounts the worktree at /work
# and runs the build as the host UID/GID so the worktree stays host-owned.
#
# Base images:
#   rust:1.90-bookworm          Rust 1.90 (edition 2024) on Debian 12 bookworm.
#                               The workspace declares rust-version 1.85, but the
#                               locked Tauri 2.12 tree requires rustc 1.90.0, so a
#                               1.85 image cannot compile the host.
#   node:22.23.5-bookworm-slim  exact Node 22 LTS and npm, copied from the official
#                               pinned image instead of running an unpinned
#                               NodeSource setup script. Same Debian 12 glibc.
#
# Version tags of official images are immutable; for a fully digest-pinned build,
# replace the tags below with image@sha256:<index-digest> and record the digest in
# docs/DEV-SETUP.md.
FROM node:22.23.5-bookworm-slim AS node

FROM rust:1.90-bookworm

LABEL org.opencontainers.image.title="stackspirit-desktop-build" \
      org.opencontainers.image.description="Linux Rust 1.90 + Node 22 + GTK/WebKit toolchain for checking the StackSpirit Tauri desktop host" \
      org.opencontainers.image.source="https://example.invalid/stackspirit" \
      org.opencontainers.image.licenses="MIT"

ENV DEBIAN_FRONTEND=noninteractive

# rustfmt and clippy are not part of the rust image, but the check script runs
# `cargo fmt` and the repository toolchain file requests them.
RUN rustup component add rustfmt clippy

# Tauri 2 Linux prerequisites plus the plain build/network tools the check script
# needs. Keep this list in sync with docs/DEV-SETUP.md and the D4 report.
RUN apt-get update \
    && apt-get install --yes --no-install-recommends \
        build-essential \
        ca-certificates \
        curl \
        file \
        git \
        libayatana-appindicator3-dev \
        libgtk-3-dev \
        libjavascriptcoregtk-4.1-dev \
        libssl-dev \
        libsoup-3.0-dev \
        librsvg2-dev \
        libwebkit2gtk-4.1-dev \
        libxdo-dev \
        patchelf \
        pkg-config \
        tar \
        unzip \
        wget \
        xz-utils \
    && rm -rf /var/lib/apt/lists/*

# Node 22 from the pinned official image: binary plus npm. No setup script and no
# network installer runs at build time. Docker's COPY follows the npm/npx symlinks
# in the source image, so recreate them here instead of copying broken shims.
COPY --from=node /usr/local/bin/node /usr/local/bin/node
COPY --from=node /usr/local/lib/node_modules /usr/local/lib/node_modules
RUN ln -sf ../lib/node_modules/npm/bin/npm-cli.js /usr/local/bin/npm \
    && ln -sf ../lib/node_modules/npm/bin/npx-cli.js /usr/local/bin/npx

# RUSTUP_TOOLCHAIN overrides the repository rust-toolchain.toml channel ("stable")
# so the container uses the pinned 1.90.0 toolchain instead of downloading current
# stable on every build. The script overrides CARGO_HOME/CARGO_TARGET_DIR to a host
# cache outside the worktree; rustup still finds the toolchain through RUSTUP_HOME.
ENV RUSTUP_HOME=/usr/local/rustup \
    RUSTUP_TOOLCHAIN=1.90.0 \
    CARGO_HOME=/usr/local/cargo \
    CARGO_TERM_COLOR=never \
    PATH=/usr/local/cargo/bin:/usr/local/bin:/usr/local/sbin:/usr/sbin:/usr/bin:/sbin:/bin

# Fail the image build, not the first check run, if a tool or pkg-config module is
# missing. This is the same list the worker host was missing in the D4 report.
# Debian's libxdo-dev ships no pkg-config file, so its header is asserted instead.
RUN set -eu; \
    rustc --version; \
    cargo --version; \
    rustfmt --version; \
    cargo clippy --version; \
    node --version; \
    npm --version; \
    git --version; \
    pkg-config --version; \
    missing=""; \
    for module in \
        gobject-2.0 \
        gtk+-3.0 \
        webkit2gtk-4.1 \
        javascriptcoregtk-4.1 \
        libsoup-3.0 \
        ayatana-appindicator3-0.1 \
        librsvg-2.0; do \
        if ! pkg-config --exists "$module"; then missing="$missing $module"; fi; \
    done; \
    if [ -n "$missing" ]; then \
        echo "missing pkg-config modules:$missing" >&2; \
        pkg-config --list-all | sort >&2; \
        exit 1; \
    fi; \
    test -f /usr/include/xdo.h; \
    echo "stackspirit desktop build image: toolchain ok"

WORKDIR /work
CMD ["bash"]
