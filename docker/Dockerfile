FROM rust:1-bookworm AS build

RUN apt-get update \
    && apt-get install -y --no-install-recommends pkg-config libssl-dev \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /work
COPY . .

RUN cargo build --release


FROM ghcr.io/charmbracelet/vhs:latest

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        bash \
        ca-certificates \
        zsh \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /work

COPY --from=build /work/target/release/twatch /usr/local/bin/twatch

ENV TERM=xterm-256color

ENTRYPOINT []
CMD ["bash"]
