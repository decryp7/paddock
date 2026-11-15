# garage-arm64: a local linux/arm64 stand-in for the garage base image, so ci-image/Dockerfile builds natively on an Apple
# Silicon Mac for the local Linux gate (tools/linux-container.sh --ci-image; never pushed, never used by CI).
#
# Why: garage (dev.decryptology.net/decryp7/garage) and paddock-ci are linux/amd64 only. On an arm64 Mac, amd64 runs under
# emulation (Rosetta or qemu-user), where a failed exec is not reported to the parent: missing-program tests fail there and pass
# on CI's native amd64 runners (f08362a29 on 2026-10-30, feat/heavy-priority-lane on 2026-11-13, train 10 on 2026-11-14).
# Docker and OrbStack document native arm64 images as the way to run Linux containers on Apple Silicon.
#
# It carries only what ci-image/Dockerfile's final stage and verify.sh expect from garage: garage's base tag (its arm64 manifest),
# Tauri's Linux libraries, Xvfb and D-Bus, and garage's versions of Node, cargo-nextest, cargo-llvm-cov and cargo-deny, as
# recorded in ci-image/garage-release.txt (scripts/ci-image.test.mjs checks they agree; the EXPECT_* variables below make
# verify.sh check the built tools). ci-image/Dockerfile then adds its own pins on top and verify.sh checks them, as for the CI
# image. Every download is SHA-256 checked (fetch-verified.sh); no installer is piped into a shell.
#
# Where it differs from garage (it is local only, never a CI result): the apt packages float to the archive of the day of the
# build (garage's did too, on 2026-10-23); garage's `stable` Rust (1.99.1) beside the pinned one, Go, pnpm, Bun, the bundlers
# and the other cargo tools are absent; and ci-image/Dockerfile installs no Chrome for Testing or draw.io on arm64 (Google and
# jgraph publish none), so this image runs no e2e against Chrome for Testing and no SVG drift check.
#
#   docker build --platform linux/arm64 -f ci-image/garage-arm64.Dockerfile -t <tag> ci-image

# garage-base: catthehacker/ubuntu:act-24.04-20260815 (Ubuntu 24.04.4, garage's base: ci-image/garage-release.txt), its
# linux/arm64 manifest from the tag's index on Docker Hub (looked up 2026-11-15).
FROM catthehacker/ubuntu@sha256:84e94c96278dd26b8feb71226a521d259f8160cf6b1dd08e51fd42be43a82e56

# Node: the exact version paddock-ci:v3 carries (the .nvmrc major). Checksum: nodejs.org/dist/v24.19.0/SHASUMS256.txt.
ARG NODE_VERSION=24.19.0
ARG NODE_SHA256_ARM64=01443c1e1a29e531ccad5a46fefa6df490d2189c49f7955904aecdbb0fe86fdc
# Rust: the `channel` of the root rust-toolchain.toml (ci-image/Dockerfile installs the same and verify.sh checks it).
ARG RUST_VERSION=1.100.0
# rustup-init itself, not the piped installer. Checksum: static.rust-lang.org/rustup/archive/1.29.3/aarch64-unknown-linux-gnu/rustup-init.sha256.
ARG RUSTUP_VERSION=1.29.3
ARG RUSTUP_INIT_SHA256_ARM64=9f608c142e0e38a78957d0661ba1dfaf0b6fb37184cfaa4b9a0aa57af6184282
# Cargo tools at garage's versions. Checksums: each equals the GitHub release asset's `digest` (nextest, llvm-cov and deny,
# checked 2026-11-14 and 2026-11-15); deny also publishes a .sha256 file, which agrees.
ARG CARGO_NEXTEST_VERSION=0.9.149
ARG CARGO_NEXTEST_SHA256_ARM64=6f82e375187cf3a1f3e336879a51940c315a5d0ce7253f7e517ad28295b74189
ARG CARGO_LLVM_COV_VERSION=0.9.1
ARG CARGO_LLVM_COV_SHA256_ARM64=abf5f13c1520f8756d2192bfaaeadb0208f5b7eaa8cadc15bf847befa42b7360
ARG CARGO_DENY_VERSION=0.20.2
ARG CARGO_DENY_SHA256_ARM64=995c82be0defc7a025cae49a2aa2644ce8245c9a3318fc4103907c6a285e8c7d

ENV DEBIAN_FRONTEND=noninteractive \
    RUSTUP_HOME=/usr/local/rustup \
    CARGO_HOME=/usr/local/cargo \
    PLAYWRIGHT_BROWSERS_PATH=/ms-playwright \
    PATH=/usr/local/cargo/bin:/usr/local/node/bin:$PATH \
    CARGO_INCREMENTAL=0 \
    SCCACHE_DIR=/sccache \
    GARAGE_IMAGE=1 \
    EXPECT_NEXTEST=${CARGO_NEXTEST_VERSION} \
    EXPECT_LLVM_COV=${CARGO_LLVM_COV_VERSION} \
    EXPECT_DENY=${CARGO_DENY_VERSION}

SHELL ["/bin/bash", "-o", "pipefail", "-c"]

# Tauri 2's Linux libraries, Xvfb and D-Bus (garage's list without the bundlers and Go, which the local gate does not run).
RUN apt-get update \
 && apt-get install -y --no-install-recommends \
      libwebkit2gtk-4.1-dev libgtk-3-dev librsvg2-dev libayatana-appindicator3-dev \
      patchelf file xdg-utils binutils cmake xvfb dbus dbus-x11 \
 && rm -rf /var/lib/apt/lists/*

COPY fetch-verified.sh /usr/local/bin/fetch-verified

RUN set -eu; mkdir -p /tmp/dl /usr/local/node; \
    fetch-verified "https://nodejs.org/dist/v${NODE_VERSION}/node-v${NODE_VERSION}-linux-arm64.tar.xz" "$NODE_SHA256_ARM64" /tmp/dl/node.tar.xz; \
    tar -xJf /tmp/dl/node.tar.xz -C /usr/local/node --strip-components=1; \
    ln -sf /usr/local/node/bin/node /usr/local/node/bin/npm /usr/local/node/bin/npx /usr/local/bin/; \
    rm -rf /tmp/dl; node --version

RUN set -eu; \
    fetch-verified "https://static.rust-lang.org/rustup/archive/${RUSTUP_VERSION}/aarch64-unknown-linux-gnu/rustup-init" "$RUSTUP_INIT_SHA256_ARM64" /tmp/rustup-init; \
    chmod +x /tmp/rustup-init; \
    /tmp/rustup-init -y --no-modify-path --profile minimal --default-toolchain "${RUST_VERSION}" --component clippy,rustfmt,llvm-tools; \
    rm /tmp/rustup-init; chmod -R a+w "$RUSTUP_HOME" "$CARGO_HOME"

RUN set -eu; mkdir -p /tmp/dl; \
    fetch-verified "https://github.com/nextest-rs/nextest/releases/download/cargo-nextest-${CARGO_NEXTEST_VERSION}/cargo-nextest-${CARGO_NEXTEST_VERSION}-aarch64-unknown-linux-gnu.tar.gz" \
      "$CARGO_NEXTEST_SHA256_ARM64" /tmp/dl/nextest.tar.gz; \
    tar -xzf /tmp/dl/nextest.tar.gz -C "$CARGO_HOME/bin" cargo-nextest; \
    fetch-verified "https://github.com/taiki-e/cargo-llvm-cov/releases/download/v${CARGO_LLVM_COV_VERSION}/cargo-llvm-cov-aarch64-unknown-linux-gnu.tar.gz" \
      "$CARGO_LLVM_COV_SHA256_ARM64" /tmp/dl/llvm-cov.tar.gz; \
    tar -xzf /tmp/dl/llvm-cov.tar.gz -C "$CARGO_HOME/bin" cargo-llvm-cov; \
    fetch-verified "https://github.com/EmbarkStudios/cargo-deny/releases/download/${CARGO_DENY_VERSION}/cargo-deny-${CARGO_DENY_VERSION}-aarch64-unknown-linux-musl.tar.gz" \
      "$CARGO_DENY_SHA256_ARM64" /tmp/dl/deny.tar.gz; \
    tar -xzf /tmp/dl/deny.tar.gz -C /tmp/dl --strip-components=1; install -m 0755 /tmp/dl/cargo-deny "$CARGO_HOME/bin/cargo-deny"; \
    rm -rf /tmp/dl; mkdir -p /sccache && chmod 1777 /sccache; \
    cargo nextest --version && cargo llvm-cov --version && cargo deny --version

LABEL org.opencontainers.image.title="garage-arm64 (local)" \
      org.opencontainers.image.description="Local linux/arm64 stand-in for garage, the base of paddock-ci, for the local Linux gate on Apple Silicon"
