# Build
FROM rust:1.85-slim AS build
RUN apt-get update && apt-get install -y pkg-config libssl-dev && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY src ./src
# binário otimizado
RUN cargo build --release

# Run - imagem mínima
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
ENV RUST_LOG=info
COPY --from=build /app/target/release/p99 /p99
EXPOSE 9999
ENTRYPOINT ["/p99"]
