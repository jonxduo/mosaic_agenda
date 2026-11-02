# Build from the repository root: docker build -f server/push-service/Dockerfile .
FROM rust:1-bookworm AS build
WORKDIR /src
COPY . .
RUN cargo build --locked --release -p push-service

FROM debian:bookworm-slim
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates \
 && rm -rf /var/lib/apt/lists/* \
 && useradd --system --uid 10001 push \
 && mkdir /data && chown push /data
COPY --from=build /src/target/release/push-service /usr/local/bin/push-service
USER push
EXPOSE 8080
ENTRYPOINT ["/usr/local/bin/push-service"]
