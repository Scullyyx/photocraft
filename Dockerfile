# syntax=docker/dockerfile:1
ARG RUST_VERSION=1.95
ARG NGINX_VERSION=1.28-alpine

FROM rust:${RUST_VERSION}-slim-bookworm AS builder
# Keep aligned with .github/workflows/release.yml.
ARG TRUNK_VERSION=0.21.14
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/* \
    && rustup target add wasm32-unknown-unknown
RUN set -eu; \
    case "$(uname -m)" in \
        x86_64) arch=x86_64 ;; \
        aarch64) arch=aarch64 ;; \
        *) echo 'The web builder supports amd64 and arm64' >&2; exit 1 ;; \
    esac; \
    asset="trunk-${arch}-unknown-linux-musl.tar.gz"; \
    url="https://github.com/trunk-rs/trunk/releases/download/v${TRUNK_VERSION}"; \
    cd /tmp; \
    curl -fsSL --retry 3 -A Photocraft-dev "$url/$asset" -o "$asset"; \
    curl -fsSL --retry 3 -A Photocraft-dev "$url/$asset.sha256" -o "$asset.sha256"; \
    printf '%s  %s\n' "$(cat "$asset.sha256")" "$asset" | sha256sum -c -; \
    tar -xzf "$asset" -C /usr/local/bin trunk; \
    rm "$asset" "$asset.sha256"; \
    trunk --version

WORKDIR /src
COPY . .
WORKDIR /src/apps/photocraft-web
# Docker serving has no static-host per-file size cap. Thin LTO uses less build
# memory than the release zip's fat LTO; retain the profile's other optimizations.
ARG CARGO_PROFILE_WASM_RELEASE_LTO=thin
ARG CARGO_BUILD_JOBS=1
# index.html selects the wasm-release profile, HEIF support and wasm-opt -Oz.
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    --mount=type=cache,target=/root/.cache/trunk \
    trunk build --release --locked \
    && test -s /src/dist/web/index.html \
    && gzip -9 -k /src/dist/web/*.wasm /src/dist/web/*.js /src/dist/web/index.html

FROM nginx:${NGINX_VERSION} AS runtime
COPY packaging/web/nginx.conf /etc/nginx/nginx.conf
COPY --from=builder /src/dist/web/ /usr/share/nginx/html/
COPY LICENSE-MIT LICENSE-APACHE NOTICE ATTRIBUTION.md /usr/share/nginx/html/
COPY assets/fonts/OFL-*.txt assets/icons/LICENSE-*.txt assets/dict/LICENSE-*.txt \
    crates/ui-egui/src/i18n/LICENSE-translations.txt /usr/share/nginx/html/licenses/
# Bypass the stock entrypoint: this configuration needs no runtime rewriting.
USER nginx
EXPOSE 8080
ENTRYPOINT []
CMD ["nginx", "-g", "daemon off;"]
HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
    CMD wget -q -O /dev/null http://127.0.0.1:8080/healthz || exit 1
