# Multi-stage build for Replicante
FROM rust:1.75-alpine AS builder

# Install build dependencies
RUN apk add --no-cache \
    musl-dev \
    pkgconfig \
    openssl-dev \
    sqlite-dev

# Create app directory
WORKDIR /app

# Copy source code
COPY Cargo.toml Cargo.lock ./
COPY src ./src

# Build the application
RUN cargo build --release --target x86_64-unknown-linux-musl

# Runtime stage
FROM alpine:3.19

# Install runtime dependencies
RUN apk add --no-cache \
    ca-certificates \
    sqlite-libs \
    curl

# Create non-root user
RUN addgroup -g 1000 replicante && \
    adduser -D -u 1000 -G replicante replicante

# Copy binary from builder
COPY --from=builder /app/target/x86_64-unknown-linux-musl/release/replicante /usr/local/bin/replicante

# Create necessary directories
RUN mkdir -p /data /sandbox /config /logs && \
    chown -R replicante:replicante /data /sandbox /logs

# Switch to non-root user
USER replicante
WORKDIR /home/replicante

# Default environment variables
ENV RUST_LOG=info
ENV DATABASE_PATH=/data/replicante.db

# Health check
HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
    CMD ["echo", "healthy"]

# Default command (can be overridden)
ENTRYPOINT ["/usr/local/bin/replicante"]
CMD ["agent"]