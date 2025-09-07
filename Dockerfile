# Build
FROM rust:1.85-alpine AS build
RUN apk add --no-cache musl-dev pkgconfig
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY src ./src
# binário estático (musl)
RUN RUSTFLAGS="-C target-feature=+crt-static" cargo build --release

# Run - imagem mínima
FROM scratch
ENV RUST_LOG=info
COPY --from=build /app/target/release/p99 /p99
EXPOSE 9999
ENTRYPOINT ["/p99"]
