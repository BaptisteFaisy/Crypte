# Crypte (Tauri v2 + Svelte) — image Linux portable (amd64 + arm64).
#
# Build (cette machine) :
#   docker build -t crypte .
# Build multi-arch (plugin buildx requis) :
#   docker buildx build --platform linux/amd64,linux/arm64 -t crypte .
# Sans cle ni reseau (controle) : docker run --rm crypte --smoke
# Le binaire embarque un CLI headless (--smoke/--sheet/--apercu/--mbox) ;
# la fenêtre Tauri exige un affichage (pas dans ce conteneur).
# Les cles/vault survivent via les volumes (nouveaux chemins com.crypte.app ;
# migration auto depuis kiro-horror-tui au premier lancement).

FROM node:22-bookworm-slim AS frontend
WORKDIR /app
COPY package.json package-lock.json ./
RUN npm ci
COPY . .
RUN npm run build

FROM rust:1-slim-bookworm AS builder
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
       libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev \
       librsvg2-dev pkg-config \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY src-tauri/Cargo.toml src-tauri/Cargo.lock ./src-tauri/
RUN mkdir -p src-tauri/src build \
    && echo 'fn main() {}' > src-tauri/src/main.rs \
    && cd src-tauri && cargo fetch --locked
COPY src-tauri ./src-tauri
COPY --from=frontend /app/build ./build
COPY assets ./assets
RUN cd src-tauri && cargo build --release --locked

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates tzdata \
       libwebkit2gtk-4.1-0 libgtk-3-0 libayatana-appindicator3-1 \
       librsvg2-2 \
    && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/src-tauri/target/release/crypte /usr/local/bin/crypte
RUN useradd -m -u 1000 crypte \
    && mkdir -p /work /var/log/crypte \
        /home/crypte/.config/com.crypte.app /home/crypte/.local/share/com.crypte.app \
    && chown -R crypte:crypte /work /var/log/crypte /home/crypte
VOLUME ["/home/crypte/.config/com.crypte.app", "/home/crypte/.local/share/com.crypte.app"]
USER crypte
WORKDIR /work
ENTRYPOINT ["crypte"]
HEALTHCHECK --interval=5m --timeout=10s --start-period=5s CMD ["crypte", "--smoke"]
LABEL org.opencontainers.image.title="crypte" \
      org.opencontainers.image.description="LLM chat in a haunted crypt (Tauri desktop)" \
      org.opencontainers.image.source="https://github.com/BaptisteFaisy/Crypte"
