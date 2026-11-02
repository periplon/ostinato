# syntax=docker/dockerfile:1
FROM rust:1-bookworm AS build
WORKDIR /src
COPY . .
RUN cargo build --release -p ostinato

FROM debian:bookworm-slim
ARG CUE_VERSION=v0.17.1
ARG TARGETARCH
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/* \
    && case "${TARGETARCH:-amd64}" in \
         arm64) ARCH=arm64 ;; \
         *) ARCH=amd64 ;; \
       esac \
    && curl -fsSL "https://github.com/cue-lang/cue/releases/download/${CUE_VERSION}/cue_${CUE_VERSION}_linux_${ARCH}.tar.gz" \
       | tar -xz -C /usr/local/bin cue \
    && cue version

COPY --from=build /src/target/release/ostinato /usr/local/bin/ostinato
WORKDIR /work
ENTRYPOINT ["ostinato"]
CMD ["--help"]
