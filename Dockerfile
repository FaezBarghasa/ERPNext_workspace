# 1. Build environment (Debian-based has better bindgen compatibility)
FROM rust:1.80-bullseye as builder
WORKDIR /app

# Install dependencies for quiche and boringssl
RUN apt-get update && apt-get install -y \
    cmake \
    clang \
    llvm \
    pkg-config \
    libssl-dev

# Copy workspaces
COPY Cargo.toml Cargo.lock ./
COPY frappe-framework frappe-framework
COPY erpnext-domain erpnext-domain

# Build the frappe-net binary
WORKDIR /app/frappe-framework
# Use target dir explicitly for build cache
RUN cargo build --release --bin frappe-net

# 2. Runtime environment
FROM debian:bullseye-slim
WORKDIR /app

RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/frappe-framework/target/release/frappe-net /usr/local/bin/frappe-net

EXPOSE 8080
EXPOSE 4433/udp

CMD ["frappe-net"]
