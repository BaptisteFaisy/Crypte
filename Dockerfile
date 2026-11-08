# kiro-horror-tui (Crypte) — image Linux portable (amd64 + arm64).
#
# Build (cette machine) :
#   docker build -t crypte .
# Build multi-arch Windows/Mac/Linux (plugin buildx requis) :
#   docker buildx build --platform linux/amd64,linux/arm64 -t crypte .
# Run Linux/Mac :
#   docker run -it --rm -e KH_API_KEY=... -e KH_MODEL=... -v "$(pwd):/work" crypte
# Run Windows (PowerShell) :
#   docker run -it --rm -e KH_API_KEY=... -e KH_MODEL=... -v "${PWD}:/work" crypte
# Sans cle ni reseau (controle) : docker run --rm crypte --smoke
# Le TUI exige un terminal >= 80x24 ; les cles/vault survivent via les volumes.

FROM rust:1-slim-bookworm AS builder
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
RUN mkdir -p src && echo 'fn main() {}' > src/main.rs && cargo fetch --locked
COPY src ./src
COPY assets ./assets
RUN cargo build --release --locked

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates tzdata \
    && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/kiro-horror-tui /usr/local/bin/kiro-horror-tui
RUN useradd -m -u 1000 crypte \
    && mkdir -p /work /var/log/crypte \
        /home/crypte/.config/kiro-horror-tui /home/crypte/.local/share/kiro-horror-tui \
    && chown -R crypte:crypte /work /var/log/crypte /home/crypte
VOLUME ["/home/crypte/.config/kiro-horror-tui", "/home/crypte/.local/share/kiro-horror-tui"]
USER crypte
WORKDIR /work
ENV TERM=xterm-256color COLORTERM=truecolor
ENTRYPOINT ["kiro-horror-tui"]
HEALTHCHECK --interval=5m --timeout=10s --start-period=5s CMD ["kiro-horror-tui", "--smoke"]
LABEL org.opencontainers.image.title="kiro-horror-tui" \
      org.opencontainers.image.description="LLM chat in a haunted crypt (TUI)" \
      org.opencontainers.image.source="https://github.com/BaptisteFaisy/Crypte"
