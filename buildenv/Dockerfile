FROM randomdude/gcc-cross-x86_64-elf

ENV RUSTUP_HOME=/usr/local/rustup \
    CARGO_HOME=/usr/local/cargo \
    PATH=/usr/local/cargo/bin:$PATH

RUN apt-get update \
    && apt-get upgrade -y \
    && apt-get install -y nasm xorriso grub-pc-bin grub-common mtools curl \
    && rm -rf /var/lib/apt/lists/*

RUN curl https://sh.rustup.rs -sSf | sh -s -- -y --profile minimal \
    && rustup target add x86_64-unknown-none

# Ensure non-root users (e.g. --user 1000) can access the toolchain
RUN chmod -R a+rwX /usr/local/rustup /usr/local/cargo

VOLUME /build
WORKDIR /build
