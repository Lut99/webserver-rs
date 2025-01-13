# DOCKERFILE for the `webserver` binary
#   by Lut99


##### BUILD #####
# Start with the Rust image
FROM rust:1-alpine3.19 AS build

# Add additional dependencies
RUN apk add --no-cache \
    musl-dev

# Copy over the source
RUN mkdir -p /source/target
COPY Cargo.toml /source/Cargo.toml
COPY Cargo.lock /source/Cargo.lock
COPY src /source/src

# Build it
WORKDIR /source
RUN --mount=type=cache,id=cargoidx,target=/usr/local/cargo/registry \
    --mount=type=cache,id=webserver,target=/source/target \
    cargo build --release --bin webserver \
 && cp /source/target/release/webserver /source/webserver



##### RELEASE #####
# The release is alpine-based for quickness
FROM alpine:3.19 AS release

# Define some build args
ARG UID=1000
ARG GID=1000

# Setup a user mirroring the main one
RUN addgroup -g $GID webserver
RUN adduser -u $UID -G webserver -g "Webserver-rs" -D webserver

# Copy the binary from the build
COPY --from=build --chown=webserver:webserver /source/webserver /webserver

# Alrighty define the entrypoint and be done with it
USER webserver
ENTRYPOINT [ "/webserver" ]
