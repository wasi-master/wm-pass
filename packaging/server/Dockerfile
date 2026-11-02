# Build from the repository root:
#   docker build -f packaging/server/Dockerfile -t wmpass-server .
FROM rust:1.99-slim AS build
WORKDIR /src
COPY . .
RUN cargo build --release -p wmpass-server

FROM debian:stable-slim
RUN useradd --system --create-home --uid 10001 wmpass && mkdir /data && chown wmpass /data
COPY --from=build /src/target/release/wmpass-server /usr/local/bin/wmpass-server
USER wmpass
VOLUME /data
ENV WMPASS_DATA=/data WMPASS_BIND=0.0.0.0:8222 WMPASS_BEHIND_PROXY=1
EXPOSE 8222
ENTRYPOINT ["wmpass-server"]
