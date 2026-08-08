FROM mcr.microsoft.com/devcontainers/base:ubuntu-24.04

ARG UBUNTU_MIRROR=http://mirrors.edge.kernel.org/ubuntu/

ENV DEBIAN_FRONTEND=noninteractive
ENV RUSTUP_HOME=/usr/local/rustup
ENV CARGO_HOME=/usr/local/cargo
ENV BUN_INSTALL=/usr/local/bun
ENV PATH=/usr/local/cargo/bin:/usr/local/bun/bin:${PATH}

RUN printf '%s\n' \
      'Acquire::Retries "5";' \
      'Acquire::ForceIPv4 "true";' \
      'Acquire::http::Timeout "30";' \
      'Acquire::https::Timeout "30";' \
      > /etc/apt/apt.conf.d/99fun-asr-retries

RUN if [ -f /etc/apt/sources.list.d/ubuntu.sources ]; then \
      sed -i \
        -e "s|http://archive.ubuntu.com/ubuntu/|${UBUNTU_MIRROR}|g" \
        -e "s|http://security.ubuntu.com/ubuntu/|${UBUNTU_MIRROR}|g" \
        /etc/apt/sources.list.d/ubuntu.sources; \
    fi

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
      build-essential \
      ca-certificates \
      clang \
      cmake \
      curl \
      file \
      git \
      libayatana-appindicator3-dev \
      libfuse2 \
      libgtk-3-dev \
      librsvg2-dev \
      libsoup-3.0-dev \
      libssl-dev \
      libwebkit2gtk-4.1-dev \
      patchelf \
      pkg-config \
      wget \
      xz-utils \
    && rm -rf /var/lib/apt/lists/*

RUN curl --proto '=https' --tlsv1.2 -fsSL https://sh.rustup.rs \
      | sh -s -- -y --profile minimal --default-toolchain stable \
    && rustup component add clippy rustfmt \
    && chmod -R a+rwx /usr/local/rustup /usr/local/cargo

RUN curl -fsSL https://bun.sh/install | bash \
    && ln -sf /usr/local/bun/bin/bun /usr/local/bin/bun \
    && ln -sf /usr/local/bun/bin/bunx /usr/local/bin/bunx \
    && chmod -R a+rwx /usr/local/bun

ENV NO_STRIP=true
ENV WEBKIT_DISABLE_DMABUF_RENDERER=1
