# Stage 1: Build the Rust binary
FROM rust:bookworm AS builder

WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY src/ src/

RUN cargo build --release --bin echokit_cc

# Stage 2: Runtime
FROM node:lts-bookworm

# Install Claude Code globally
RUN npm install -g @anthropic-ai/claude-code

# Copy the built binary
COPY --from=builder /build/target/release/echokit_cc /usr/local/bin/echokit_cc

# Create a non-root user (required for --dangerously-skip-permissions)
RUN useradd -m -s /bin/bash echokit

# Copy static assets and run_cc.sh
COPY static/ /app/static/
COPY run_cc.sh /app/run_cc.sh
RUN chmod +x /app/run_cc.sh && \
    sed -i 's/claude --resume/claude --dangerously-skip-permissions --resume/g; s/claude --session-id/claude --dangerously-skip-permissions --session-id/g' /app/run_cc.sh

WORKDIR /app

RUN mkdir -p /workspace && chown echokit:echokit /workspace /app

ENV ECHOKIT_CLAUDE_COMMAND="./run_cc.sh"
ENV ECHOKIT_CC_BIND_ADDR="0.0.0.0:3000"
ENV ECHOKIT_WORKING_PATH="/workspace"

EXPOSE 3000

USER echokit

ENTRYPOINT ["echokit_cc"]
