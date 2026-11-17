FROM docker.io/library/ubuntu@sha256:77eb101c607d6e3d4f5ad0c8a02c2136c689b98aba1f359e6502ba517e5c57ec

# Bootstrap HTTPS for the snapshot service with a fixed Mozilla CA bundle.
ADD --chmod=0644 --checksum=sha256:427c064216761a1e72a8b30f0d954d08cb6e4435a82091fb865afd20c8497285 \
    https://curl.se/ca/cacert-2026-11-05.pem /etc/ssl/certs/ca-certificates.crt

LABEL org.opencontainers.image.source="https://github.com/szdytom/markview"
LABEL org.opencontainers.image.description="Markview's Linux amd64 rendering baseline environment"

RUN printf '%s\n' \
        'Types: deb' \
        'URIs: https://snapshot.ubuntu.com/ubuntu/20261116T000000Z/' \
        'Suites: noble noble-updates noble-security' \
        'Components: main universe' \
        'Signed-By: /usr/share/keyrings/ubuntu-archive-keyring.gpg' \
        > /etc/apt/sources.list.d/ubuntu.sources \
    && apt-get update --error-on=any \
    && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
        build-essential ca-certificates curl git pkg-config xz-utils \
        libfontconfig1-dev libxkbcommon-dev libwayland-dev libvulkan1 vulkan-tools \
        poppler-utils=24.02.0-1ubuntu9.11 libpoppler134=24.02.0-1ubuntu9.11 \
        mesa-vulkan-drivers=25.2.8-0ubuntu0.24.04.4 libllvm20=1:20.1.2-0ubuntu1~24.04.3 \
    && rm -rf /var/lib/apt/lists/*

RUN curl --fail --location --retry 3 \
        https://static.rust-lang.org/dist/rust-1.100.0-x86_64-unknown-linux-gnu.tar.xz -o /tmp/rust.tar.xz \
    && echo '98ee6bec240e1cfe944f17e7092412dc5ef6e008484bb668c6936d35a7d15acd  /tmp/rust.tar.xz' | sha256sum --check \
    && tar -xJf /tmp/rust.tar.xz -C /tmp \
    && /tmp/rust-1.100.0-x86_64-unknown-linux-gnu/install.sh \
        --prefix=/opt/rust --disable-ldconfig --without=rust-docs \
    && rm -rf /tmp/rust* \
    && driver=$(find /usr/share/vulkan/icd.d -name 'lvp_icd*.json' -print -quit) \
    && test -n "$driver" \
    && ln -s "$driver" /opt/lavapipe.json

ENV PATH="/opt/rust/bin:${PATH}" \
    HOME=/tmp \
    CARGO_HOME=/cargo \
    CARGO_TARGET_DIR=/target \
    CARGO_INCREMENTAL=0 \
    CARGO_PROFILE_DEV_DEBUG=0 \
    CARGO_PROFILE_TEST_DEBUG=0 \
    WGPU_BACKEND=vulkan \
    VK_DRIVER_FILES=/opt/lavapipe.json \
    LP_NUM_THREADS=1 \
    FONTCONFIG_FILE=/workspace/tests/fontconfig.conf \
    LANG=C.UTF-8 \
    TZ=UTC

COPY run.sh /opt/render-tests.sh
WORKDIR /workspace
ENTRYPOINT ["bash", "/opt/render-tests.sh"]
