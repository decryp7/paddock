# paddock-ci: the shared CI image for the Paddock family (plan X53).
#
# It builds on the garage image (Rust toolchain with clippy, rustfmt and llvm-tools; cargo-nextest, cargo-llvm-cov and
# cargo-deny; Tauri's Linux libraries; Xvfb and D-Bus; Node; Playwright's Chromium and WebKit with their dependencies; sccache)
# and adds what the jobs still fetched on every run: cargo-hakari, cargo-machete, libduckdb, a pinned Chrome for Testing and
# sccache at the exact pin. Every binary added here is downloaded with a pinned SHA-256 (ci-image/fetch-verified.sh), and the
# last step (ci-image/verify.sh) fails the build when anything a job needs is missing or off its pin, then writes
# /etc/paddock-ci-release. Jobs test `[ -f /etc/paddock-ci-release ]` to skip their own installs.
#
# Built by .gitea/workflows/ci-image.yml (manual dispatch) for linux/amd64 and pushed to the Gitea registry from CI only.
# Bump ci-image/VERSION with any change here; ci.yml names the image by that number.

ARG GARAGE_IMAGE=dev.decryptology.net/decryp7/garage:ubuntu-24.04-20261023-r5

# ---- Stage 1: fetch and verify. Nothing from here reaches the final image except /out. ----------------------------------------
FROM ${GARAGE_IMAGE} AS fetch
ARG TARGETARCH

# Versions and checksums. Renovate bumps the version pins marked `renovate:`; a bump needs the new checksums
# (`ci-image/README.md` says how to get them), so the build fails loudly rather than trusting an unverified file.
# renovate: datasource=github-releases depName=mozilla/sccache
ARG SCCACHE_VERSION=0.19.0
ARG SCCACHE_SHA256_AMD64=aa73c7270c997894038c0d8f0d297865f57569e1687883322bc431713d4bb9dd
ARG SCCACHE_SHA256_ARM64=3e47fc4a71b1ddd1b4294a0de9d34498922fdf41e3cd0bda5df3954e3d6deb9d
# renovate: datasource=github-releases depName=guppy-rs/guppy extractVersion=^cargo-hakari-(?<version>.+)$
ARG HAKARI_VERSION=0.9.39
ARG HAKARI_SHA256_AMD64=699a3193a9bd1a8d914b970d3f50f3385f24fbd8b1294d0c0ff31fbe7c2c9f48
ARG HAKARI_SHA256_ARM64=c92f6c556e84f00da1ea0a26217eea5be3d701f1ee9d24a04066a46b65886fbc
# renovate: datasource=github-releases depName=bnjbvr/cargo-machete
ARG MACHETE_VERSION=0.9.2
ARG MACHETE_SHA256_AMD64=48200087f54c55aabcd4db4af1e25742b49846c02a1b1bfa134711945b35b2e9
ARG MACHETE_SHA256_ARM64=6f96c3e6026a5bdd241b6ae600c6fb86c9197c6e189a894f91371baa01fd10f5
# libduckdb must match the `duckdb` crate the apps build (duckdb-rs 1.10506.x = DuckDB 1.5.6).
# renovate: datasource=github-releases depName=duckdb/duckdb
ARG DUCKDB_VERSION=1.5.6
ARG DUCKDB_SHA256_AMD64=b845005f5132a7d8180057c35e14a7626632258782f871a90861b19c1c03841b
ARG DUCKDB_SHA256_ARM64=b72ed9f05003f5e9d2015f7ceada6416b377d9dd33169cdde3c9e33897856eee
# renovate: datasource=github-releases depName=koalaman/shellcheck
ARG SHELLCHECK_VERSION=0.11.0
ARG SHELLCHECK_SHA256_AMD64=8c3be12b05d5c177a04c29e3c78ce89ac86f1595681cab149b65b97c4e227198
ARG SHELLCHECK_SHA256_ARM64=12b331c1d2db6b9eb13cfca64306b1b157a86eb69db83023e261eaa7e7c14588
# Chrome for Testing: the build the apps download (crates/paddock-chromium/src/chromium.toml, linux-x86_64; scripts/ci-image.test.mjs
# checks they agree), so tests drive what users get. Playwright keeps its own bundled Chromium beside it. Google publishes no
# Linux arm64 build; the image is amd64 only for Chrome.
ARG PLAYWRIGHT_VERSION=1.64.1
ARG CHROME_VERSION=156.0.8078.47
ARG CHROME_SHA256_AMD64=e6c94490f0b6b337225aeb2d2a5034858516b7017356980ba94f5176d8774a50

# draw.io desktop (the CLI export of the SVG drift check, scripts/svg-drift.mjs). Source: GitHub release jgraph/drawio-desktop
# v32.6.10 (2026-11-04), drawio-amd64-32.6.10.deb; the SHA-256 is the release's own digest and was recomputed on 2026-11-12.
# The version is the one that exported the committed diagrams (the kit writes mxfile version 32.5.5; the export script is unchanged).
# renovate: datasource=github-releases depName=jgraph/drawio-desktop
ARG DRAWIO_VERSION=32.6.10
ARG DRAWIO_SHA256_AMD64=fc96faafe68cb4e06c0b7116779891592b20fce2cde7907adec504f23c5af83c

COPY fetch-verified.sh /usr/local/bin/fetch-verified
RUN set -eu; \
    case "${TARGETARCH:-amd64}" in \
      amd64) A=amd64; GNU=x86_64-unknown-linux-gnu; MUSL=x86_64-unknown-linux-musl ;; \
      arm64) A=arm64; GNU=aarch64-unknown-linux-gnu; MUSL=aarch64-unknown-linux-gnu ;; \
      *) echo "unsupported architecture ${TARGETARCH}" >&2; exit 1 ;; \
    esac; \
    pick() { if [ "$A" = amd64 ]; then echo "$1"; else echo "$2"; fi; }; \
    mkdir -p /out/bin /tmp/dl; \
    # sccache
    fetch-verified "https://github.com/mozilla/sccache/releases/download/v${SCCACHE_VERSION}/sccache-v${SCCACHE_VERSION}-$(pick x86_64-unknown-linux-musl aarch64-unknown-linux-musl).tar.gz" \
      "$(pick "$SCCACHE_SHA256_AMD64" "$SCCACHE_SHA256_ARM64")" /tmp/dl/sccache.tar.gz; \
    tar -xzf /tmp/dl/sccache.tar.gz -C /tmp/dl --strip-components=1 --wildcards '*/sccache'; \
    install -m 0755 /tmp/dl/sccache /out/bin/sccache; \
    # cargo-hakari (one file in the archive)
    fetch-verified "https://github.com/guppy-rs/guppy/releases/download/cargo-hakari-${HAKARI_VERSION}/cargo-hakari-${HAKARI_VERSION}-${GNU}.tar.gz" \
      "$(pick "$HAKARI_SHA256_AMD64" "$HAKARI_SHA256_ARM64")" /tmp/dl/hakari.tar.gz; \
    tar -xzf /tmp/dl/hakari.tar.gz -C /tmp/dl cargo-hakari; \
    install -m 0755 /tmp/dl/cargo-hakari /out/bin/cargo-hakari; \
    # cargo-machete
    fetch-verified "https://github.com/bnjbvr/cargo-machete/releases/download/v${MACHETE_VERSION}/cargo-machete-v${MACHETE_VERSION}-${MUSL}.tar.gz" \
      "$(pick "$MACHETE_SHA256_AMD64" "$MACHETE_SHA256_ARM64")" /tmp/dl/machete.tar.gz; \
    tar -xzf /tmp/dl/machete.tar.gz -C /tmp/dl --strip-components=1 --wildcards '*/cargo-machete'; \
    install -m 0755 /tmp/dl/cargo-machete /out/bin/cargo-machete; \
    # shellcheck (the same pin as engineering-rules' ci.yml)
    fetch-verified "https://github.com/koalaman/shellcheck/releases/download/v${SHELLCHECK_VERSION}/shellcheck-v${SHELLCHECK_VERSION}.linux.$(pick x86_64 aarch64).tar.xz" \
      "$(pick "$SHELLCHECK_SHA256_AMD64" "$SHELLCHECK_SHA256_ARM64")" /tmp/dl/shellcheck.tar.xz; \
    tar -xJf /tmp/dl/shellcheck.tar.xz -C /tmp/dl --strip-components=1 "shellcheck-v${SHELLCHECK_VERSION}/shellcheck"; \
    install -m 0755 /tmp/dl/shellcheck /out/bin/shellcheck; \
    # libduckdb (library and header only)
    fetch-verified "https://github.com/duckdb/duckdb/releases/download/v${DUCKDB_VERSION}/libduckdb-linux-${A}.zip" \
      "$(pick "$DUCKDB_SHA256_AMD64" "$DUCKDB_SHA256_ARM64")" /tmp/dl/libduckdb.zip; \
    mkdir -p /out/libduckdb && unzip -q -o /tmp/dl/libduckdb.zip -d /out/libduckdb; \
    # Chrome for Testing (amd64 only)
    mkdir -p /out/chrome-for-testing; \
    if [ "$A" = amd64 ]; then \
      fetch-verified "https://storage.googleapis.com/chrome-for-testing-public/${CHROME_VERSION}/linux64/chrome-linux64.zip" \
        "$CHROME_SHA256_AMD64" /tmp/dl/chrome.zip; \
      unzip -q -o /tmp/dl/chrome.zip -d /out/chrome-for-testing; \
    else \
      echo "no Chrome for Testing for linux/${A}: Playwright's own Chromium is used" > /out/chrome-for-testing/README; \
    fi; \
    # draw.io desktop (amd64 only; installed with apt in the final stage)
    mkdir -p /out/drawio; \
    if [ "$A" = amd64 ]; then \
      fetch-verified "https://github.com/jgraph/drawio-desktop/releases/download/v${DRAWIO_VERSION}/drawio-amd64-${DRAWIO_VERSION}.deb" \
        "$DRAWIO_SHA256_AMD64" /out/drawio/drawio.deb; \
    else \
      echo "no draw.io package installed for linux/${A}" > /out/drawio/README; \
    fi; \
    rm -rf /tmp/dl

# ---- Stage 2: the image. -------------------------------------------------------------------------------------------------------
FROM ${GARAGE_IMAGE}
ARG TARGETARCH
ARG NODE_MAJOR=24
ARG PLAYWRIGHT_VERSION=1.64.1
ARG CHROME_VERSION=156.0.8078.47
ARG DUCKDB_VERSION=1.5.6
ARG SCCACHE_VERSION=0.19.0
ARG SHELLCHECK_VERSION=0.11.0
ARG DRAWIO_VERSION=32.6.10
# The Rust of the family: the `channel` of the root rust-toolchain.toml (scripts/ci-image.test.mjs checks they agree).
ARG RUST_VERSION=1.100.0

# Rust: the pinned version is installed under its own name, with the components the jobs use, and made the default; the base's
# `stable` stays beside it. setup-rust finds the toolchain named like rust-toolchain.toml's channel and downloads nothing, and a
# rebuild after a newer Rust release does not move the image (bump the pin and this ARG together; the verification below
# fails the build when the default rustc is not RUST_VERSION).
RUN rustup toolchain install "${RUST_VERSION}" --profile minimal -c rustfmt -c clippy -c llvm-tools --no-self-update \
 && rustup default "${RUST_VERSION}"

# sccache at the exact pin. Every binary added here is downloaded with a pinned SHA-256 (ci-image/fetch-verified.sh), and the
# last step (ci-image/verify.sh) fails the build when anything a job needs is missing or off its pin, then writes
# /etc/paddock-ci-release. Jobs test `[ -f /etc/paddock-ci-release ]` to skip their own installs.
#
# Built by .gitea/workflows/ci-image.yml (manual dispatch) for linux/amd64 and pushed to the Gitea registry from CI only.
# Bump ci-image/VERSION with any change here; ci.yml names the image by that number.

ARG GARAGE_IMAGE=dev.decryptology.net/decryp7/garage:ubuntu-24.04-20261023-r5

# ---- Stage 1: fetch and verify. Nothing from here reaches the final image except /out. ----------------------------------------
FROM ${GARAGE_IMAGE} AS fetch
ARG TARGETARCH

# Versions and checksums. Renovate bumps the version pins marked `renovate:`; a bump needs the new checksums
# (`ci-image/README.md` says how to get them), so the build fails loudly rather than trusting an unverified file.
# renovate: datasource=github-releases depName=mozilla/sccache
ARG SCCACHE_VERSION=0.19.0
ARG SCCACHE_SHA256_AMD64=aa73c7270c997894038c0d8f0d297865f57569e1687883322bc431713d4bb9dd
ARG SCCACHE_SHA256_ARM64=3e47fc4a71b1ddd1b4294a0de9d34498922fdf41e3cd0bda5df3954e3d6deb9d
# renovate: datasource=github-releases depName=guppy-rs/guppy extractVersion=^cargo-hakari-(?<version>.+)$
ARG HAKARI_VERSION=0.9.39
ARG HAKARI_SHA256_AMD64=699a3193a9bd1a8d914b970d3f50f3385f24fbd8b1294d0c0ff31fbe7c2c9f48
ARG HAKARI_SHA256_ARM64=c92f6c556e84f00da1ea0a26217eea5be3d701f1ee9d24a04066a46b65886fbc
# renovate: datasource=github-releases depName=bnjbvr/cargo-machete
ARG MACHETE_VERSION=0.9.2
ARG MACHETE_SHA256_AMD64=48200087f54c55aabcd4db4af1e25742b49846c02a1b1bfa134711945b35b2e9
ARG MACHETE_SHA256_ARM64=6f96c3e6026a5bdd241b6ae600c6fb86c9197c6e189a894f91371baa01fd10f5
# libduckdb must match the `duckdb` crate the apps build (duckdb-rs 1.10506.x = DuckDB 1.5.6).
# renovate: datasource=github-releases depName=duckdb/duckdb
ARG DUCKDB_VERSION=1.5.6
ARG DUCKDB_SHA256_AMD64=b845005f5132a7d8180057c35e14a7626632258782f871a90861b19c1c03841b
ARG DUCKDB_SHA256_ARM64=b72ed9f05003f5e9d2015f7ceada6416b377d9dd33169cdde3c9e33897856eee
# renovate: datasource=github-releases depName=koalaman/shellcheck
ARG SHELLCHECK_VERSION=0.11.0
ARG SHELLCHECK_SHA256_AMD64=8c3be12b05d5c177a04c29e3c78ce89ac86f1595681cab149b65b97c4e227198
ARG SHELLCHECK_SHA256_ARM64=12b331c1d2db6b9eb13cfca64306b1b157a86eb69db83023e261eaa7e7c14588
# Chrome for Testing: the build the apps download (crates/paddock-chromium/src/chromium.toml, linux-x86_64; scripts/ci-image.test.mjs
# checks they agree), so tests drive what users get. Playwright keeps its own bundled Chromium beside it. Google publishes no
# Linux arm64 build; the image is amd64 only for Chrome.
ARG PLAYWRIGHT_VERSION=1.64.1
ARG CHROME_VERSION=156.0.8078.47
ARG CHROME_SHA256_AMD64=e6c94490f0b6b337225aeb2d2a5034858516b7017356980ba94f5176d8774a50

# draw.io desktop (the CLI export of the SVG drift check, scripts/svg-drift.mjs). Source: GitHub release jgraph/drawio-desktop
# v32.6.10 (2026-11-04), drawio-amd64-32.6.10.deb; the SHA-256 is the release's own digest and was recomputed on 2026-11-12.
# The version is the one that exported the committed diagrams (the kit writes mxfile version 32.5.5; the export script is unchanged).
# renovate: datasource=github-releases depName=jgraph/drawio-desktop
ARG DRAWIO_VERSION=32.6.10
ARG DRAWIO_SHA256_AMD64=fc96faafe68cb4e06c0b7116779891592b20fce2cde7907adec504f23c5af83c

COPY fetch-verified.sh /usr/local/bin/fetch-verified
RUN set -eu; \
    case "${TARGETARCH:-amd64}" in \
      amd64) A=amd64; GNU=x86_64-unknown-linux-gnu; MUSL=x86_64-unknown-linux-musl ;; \
      arm64) A=arm64; GNU=aarch64-unknown-linux-gnu; MUSL=aarch64-unknown-linux-gnu ;; \
      *) echo "unsupported architecture ${TARGETARCH}" >&2; exit 1 ;; \
    esac; \
    pick() { if [ "$A" = amd64 ]; then echo "$1"; else echo "$2"; fi; }; \
    mkdir -p /out/bin /tmp/dl; \
    # sccache
    fetch-verified "https://github.com/mozilla/sccache/releases/download/v${SCCACHE_VERSION}/sccache-v${SCCACHE_VERSION}-$(pick x86_64-unknown-linux-musl aarch64-unknown-linux-musl).tar.gz" \
      "$(pick "$SCCACHE_SHA256_AMD64" "$SCCACHE_SHA256_ARM64")" /tmp/dl/sccache.tar.gz; \
    tar -xzf /tmp/dl/sccache.tar.gz -C /tmp/dl --strip-components=1 --wildcards '*/sccache'; \
    install -m 0755 /tmp/dl/sccache /out/bin/sccache; \
    # cargo-hakari (one file in the archive)
    fetch-verified "https://github.com/guppy-rs/guppy/releases/download/cargo-hakari-${HAKARI_VERSION}/cargo-hakari-${HAKARI_VERSION}-${GNU}.tar.gz" \
      "$(pick "$HAKARI_SHA256_AMD64" "$HAKARI_SHA256_ARM64")" /tmp/dl/hakari.tar.gz; \
    tar -xzf /tmp/dl/hakari.tar.gz -C /tmp/dl cargo-hakari; \
    install -m 0755 /tmp/dl/cargo-hakari /out/bin/cargo-hakari; \
    # cargo-machete
    fetch-verified "https://github.com/bnjbvr/cargo-machete/releases/download/v${MACHETE_VERSION}/cargo-machete-v${MACHETE_VERSION}-${MUSL}.tar.gz" \
      "$(pick "$MACHETE_SHA256_AMD64" "$MACHETE_SHA256_ARM64")" /tmp/dl/machete.tar.gz; \
    tar -xzf /tmp/dl/machete.tar.gz -C /tmp/dl --strip-components=1 --wildcards '*/cargo-machete'; \
    install -m 0755 /tmp/dl/cargo-machete /out/bin/cargo-machete; \
    # shellcheck (the same pin as engineering-rules' ci.yml)
    fetch-verified "https://github.com/koalaman/shellcheck/releases/download/v${SHELLCHECK_VERSION}/shellcheck-v${SHELLCHECK_VERSION}.linux.$(pick x86_64 aarch64).tar.xz" \
      "$(pick "$SHELLCHECK_SHA256_AMD64" "$SHELLCHECK_SHA256_ARM64")" /tmp/dl/shellcheck.tar.xz; \
    tar -xJf /tmp/dl/shellcheck.tar.xz -C /tmp/dl --strip-components=1 "shellcheck-v${SHELLCHECK_VERSION}/shellcheck"; \
    install -m 0755 /tmp/dl/shellcheck /out/bin/shellcheck; \
    # libduckdb (library and header only)
    fetch-verified "https://github.com/duckdb/duckdb/releases/download/v${DUCKDB_VERSION}/libduckdb-linux-${A}.zip" \
      "$(pick "$DUCKDB_SHA256_AMD64" "$DUCKDB_SHA256_ARM64")" /tmp/dl/libduckdb.zip; \
    mkdir -p /out/libduckdb && unzip -q -o /tmp/dl/libduckdb.zip -d /out/libduckdb; \
    # Chrome for Testing (amd64 only)
    mkdir -p /out/chrome-for-testing; \
    if [ "$A" = amd64 ]; then \
      fetch-verified "https://storage.googleapis.com/chrome-for-testing-public/${CHROME_VERSION}/linux64/chrome-linux64.zip" \
        "$CHROME_SHA256_AMD64" /tmp/dl/chrome.zip; \
      unzip -q -o /tmp/dl/chrome.zip -d /out/chrome-for-testing; \
    else \
      echo "no Chrome for Testing for linux/${A}: Playwright's own Chromium is used" > /out/chrome-for-testing/README; \
    fi; \
    # draw.io desktop (amd64 only; installed with apt in the final stage)
    mkdir -p /out/drawio; \
    if [ "$A" = amd64 ]; then \
      fetch-verified "https://github.com/jgraph/drawio-desktop/releases/download/v${DRAWIO_VERSION}/drawio-amd64-${DRAWIO_VERSION}.deb" \
        "$DRAWIO_SHA256_AMD64" /out/drawio/drawio.deb; \
    else \
      echo "no draw.io package installed for linux/${A}" > /out/drawio/README; \
    fi; \
    rm -rf /tmp/dl

# ---- Stage 2: the image. -------------------------------------------------------------------------------------------------------
FROM ${GARAGE_IMAGE}
ARG TARGETARCH
ARG NODE_MAJOR=24
ARG PLAYWRIGHT_VERSION=1.64.1
ARG CHROME_VERSION=156.0.8078.47
ARG DUCKDB_VERSION=1.5.6
ARG SCCACHE_VERSION=0.19.0
ARG SHELLCHECK_VERSION=0.11.0
ARG DRAWIO_VERSION=32.6.10
# The Rust of the family: the `channel` of the root rust-toolchain.toml (scripts/ci-image.test.mjs checks they agree).
ARG RUST_VERSION=1.100.0

# Rust: the base's `stable` is moved to the pin, with the components it has (rustfmt, clippy, llvm-tools), so setup-rust finds
# `stable` equal to rust-toolchain.toml and downloads nothing. The release of Rust after the pin is not taken by a rebuild:
# the verification below fails the build when `stable` is not RUST_VERSION (bump the pin and this ARG together).
RUN rustup update stable --no-self-update \
 && rustup component add rustfmt clippy llvm-tools --toolchain stable

# sccache at the exact pin replaces whatever the base carries; the cargo tools sit beside the base's.
COPY --from=fetch /out/bin/ /usr/local/bin/
COPY --from=fetch /out/libduckdb /opt/libduckdb
COPY --from=fetch /out/chrome-for-testing /opt/chrome-for-testing
COPY --from=fetch /out/drawio /tmp/drawio

# draw.io desktop for scripts/svg-drift.mjs (it runs headless under xvfb-run with --no-sandbox; the check passes both).
RUN if [ -f /tmp/drawio/drawio.deb ]; then apt-get update && apt-get install -y --no-install-recommends /tmp/drawio/drawio.deb; fi \
 && rm -rf /tmp/drawio /var/lib/apt/lists/*

# Playwright's browsers at the version the apps pin: a no-op when the base already has them (its pin is checked below).
RUN npx -y "playwright@${PLAYWRIGHT_VERSION}" install --with-deps chromium webkit \
 && chmod -R a+rx "${PLAYWRIGHT_BROWSERS_PATH:-/ms-playwright}" /opt/libduckdb /opt/chrome-for-testing \
 && echo /opt/libduckdb > /etc/ld.so.conf.d/duckdb.conf && ldconfig \
 && rm -rf /root/.npm /var/lib/apt/lists/*

# jq: paddock-dev's hook tests run the old shell hooks (src/hook/fixtures/*.sh) as a live oracle, and those hooks parse their input with it.
RUN apt-get update && apt-get install -y --no-install-recommends jq \
 && rm -rf /var/lib/apt/lists/*

# Where jobs find things. CHROMIUM_TEST_PATH is the variable @decryp7/paddock-e2e reads (rule 7: never an installed Chrome).
ENV DUCKDB_LIB_DIR=/opt/libduckdb \
    DUCKDB_INCLUDE_DIR=/opt/libduckdb \
    CHROMIUM_TEST_PATH=/opt/chrome-for-testing/chrome-linux64/chrome \
    PADDOCK_CI_IMAGE=1

COPY verify.sh /usr/local/bin/paddock-ci-verify

# The build fails here when the image is not what the jobs expect; on success the manifest is /etc/paddock-ci-release.
# (Chrome is checked on amd64 only: Google publishes no Linux arm64 build.)
RUN EXPECT_RUST="${RUST_VERSION}" EXPECT_NODE_MAJOR="${NODE_MAJOR}" EXPECT_SCCACHE="${SCCACHE_VERSION}" EXPECT_PLAYWRIGHT="${PLAYWRIGHT_VERSION}" \
    EXPECT_DUCKDB="${DUCKDB_VERSION}" EXPECT_SHELLCHECK="${SHELLCHECK_VERSION}" EXPECT_DRAWIO="$([ "${TARGETARCH:-amd64}" = amd64 ] && echo "${DRAWIO_VERSION}")" EXPECT_CHROME="$([ "${TARGETARCH:-amd64}" = amd64 ] && echo "${CHROME_VERSION}")" \
    paddock-ci-verify

LABEL org.opencontainers.image.title="paddock-ci" \
      org.opencontainers.image.description="Shared CI image for the Paddock family: Rust and cargo tools, Tauri libraries, Node, Playwright, Chrome for Testing, libduckdb" \
      org.opencontainers.image.source="https://dev.decryptology.net/decryp7/paddock"
