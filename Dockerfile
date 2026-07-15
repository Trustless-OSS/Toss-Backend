# Build stage
FROM rust:latest as builder

WORKDIR /app

# Copy manifests
COPY Cargo.toml Cargo.lock* ./
COPY src ./src
COPY migrations ./migrations

# Build for release
RUN cargo build --release

# Runtime stage
FROM debian:bookworm-slim

# Install runtime dependencies
RUN apt-get update && \
    apt-get install -y ca-certificates && \
    rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Copy the binary from builder
COPY --from=builder /app/target/release/toss-backend /usr/local/bin/toss-backend

# Expose port
EXPOSE 5000

# Run the binary
CMD ["toss-backend"]
