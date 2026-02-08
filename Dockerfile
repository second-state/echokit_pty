# Stage 1: Build the Rust binary
FROM rust:bookworm AS builder

WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY src/ src/

RUN cargo build --release --bin echokit_cc

# Stage 2: Runtime
# node:lts-bookworm already includes Node.js, npm, git, curl, and ca-certificates
FROM node:lts-bookworm

# Install Claude Code globally
RUN npm install -g @anthropic-ai/claude-code

# Copy the built binary
COPY --from=builder /build/target/release/echokit_cc /usr/local/bin/echokit_cc

# Copy static assets
COPY static/ /app/static/

WORKDIR /app

RUN mkdir -p /workspace

# Default bind address — 0.0.0.0 so the port is reachable from outside the container
ENV ECHOKIT_CC_BIND_ADDR="0.0.0.0:3000"
ENV ECHOKIT_WORKING_DIR="/workspace"

EXPOSE 3000

ENTRYPOINT ["echokit_cc"]
