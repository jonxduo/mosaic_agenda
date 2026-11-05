# Build from the repository root: docker build -f server/blind-node/Dockerfile .
FROM rust:1-bookworm AS build
WORKDIR /src
COPY . .
RUN cargo build --locked --release -p blind-node

FROM debian:bookworm-slim
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates \
 && rm -rf /var/lib/apt/lists/* \
 && useradd --system --uid 10002 blind \
 && mkdir /data && chown blind /data
COPY --from=build /src/target/release/blind-node /usr/local/bin/blind-node
USER blind
ENTRYPOINT ["/usr/local/bin/blind-node"]
