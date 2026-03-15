# Build Stage
FROM rust:1.77-slim-bullseye AS builder
WORKDIR /app

# Install build dependencies (Tantivy might need some basic C/C++ compilation tools)
RUN apt-get update && apt-get install -y pkg-config libssl-dev build-essential && rm -rf /var/lib/apt/lists/*

# Copy source code
COPY . .

# Build for release
RUN cargo build --release

# Runtime Stage (Minimal)
FROM debian:bullseye-slim
WORKDIR /root/

# Install runtime dependencies if needed (e.g., ca-certificates, libssl)
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*

# Copy the compiled binary from the builder stage
COPY --from=builder /app/target/release/larch /usr/local/bin/larch

# Initialize the vault directory so it exists when mounting
RUN mkdir -p /root/.larch

# Expose the default REST API port
EXPOSE 3000

# Set the default command to start the server
CMD ["larch", "serve", "--port", "3000"]
