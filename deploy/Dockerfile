FROM rust:1.99-bookworm AS builder
WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
RUN cargo build --release --locked -p schisaciaccole-server

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --no-create-home --shell /usr/sbin/nologin schisaciaccole
COPY --from=builder /build/target/release/schisaciaccole-server /usr/local/bin/schisaciaccole-server
USER schisaciaccole
EXPOSE 8080
ENTRYPOINT ["schisaciaccole-server"]
