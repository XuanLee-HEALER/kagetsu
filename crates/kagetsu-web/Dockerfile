# kagetsu-web 自部署镜像 —— 多阶段: rust 编译 → debian-slim 运行。
#
# 构建 context 必须是 workspace 根 (kagetsu-web 是 workspace 成员):
#   docker build -f crates/kagetsu-web/Dockerfile -t kagetsu-web .
# 运行:
#   docker run --rm -p 8080:8080 kagetsu-web
# 或在仓库根用 compose: docker compose up

FROM rust:1-bookworm AS builder
WORKDIR /src
COPY . .
# 只构建 web 节点 (及其依赖 kagetsu-core), 不连带编 TUI crate。
RUN cargo build --release -p kagetsu-web

FROM debian:bookworm-slim AS runtime
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=builder /src/target/release/kagetsu-web /usr/local/bin/kagetsu-web
COPY --from=builder /src/crates/kagetsu-web/static /app/static
# 注: 环境变量沿用 web crate 现有命名 (WEB_MAJO_ 前缀), 后续可随品牌统一改名。
ENV WEB_MAJO_ADDR=0.0.0.0:8080 \
    WEB_MAJO_STATIC=/app/static \
    RUST_LOG=info,tower_http=info
EXPOSE 8080
ENTRYPOINT ["kagetsu-web"]
