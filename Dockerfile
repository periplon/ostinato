# syntax=docker/dockerfile:1
FROM rust:1-bookworm AS build
WORKDIR /src
COPY . .
RUN cargo build --release -p ostinato

FROM debian:bookworm-slim
ARG CUE_VERSION=v0.17.1
ARG TARGETARCH
RUN apt-get update \
    && apt-get install -y --no-install-recommends bash ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*

COPY scripts/install-cue.sh /tmp/install-cue.sh
RUN bash /tmp/install-cue.sh "${CUE_VERSION}" /usr/local/bin \
    && rm /tmp/install-cue.sh

COPY --from=build /src/target/release/ostinato /usr/local/bin/ostinato
WORKDIR /work
ENTRYPOINT ["ostinato"]
CMD ["--help"]
