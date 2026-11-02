FROM rust:1.99.0-bookworm AS build
WORKDIR /build
COPY Cargo.toml Cargo.lock rust-toolchain.toml build.rs ./
COPY src ./src
COPY migrations ./migrations
COPY docs/api/mcp ./docs/api/mcp
RUN cargo build --locked --release --features mcp && mkdir /data && chown 65532:65532 /data

FROM gcr.io/distroless/cc-debian12:nonroot
COPY --from=build /build/target/release/pactling /usr/local/bin/pactling
COPY --from=build --chown=65532:65532 /data /data
COPY LICENSE /usr/share/licenses/pactling/LICENSE
ENV PACTLING_DATABASE_URL=sqlite:///data/pactling.db?mode=rwc
VOLUME ["/data"]
EXPOSE 9292
USER 65532:65532
ENTRYPOINT ["/usr/local/bin/pactling"]
