# CI image: builds supvan-printer-app and runs it under CUPS + cups-browsed +
# avahi-daemon in mock mode, then runs an integration test that exercises CUPS
# discovery + a print round-trip into a dump directory.

# -------- build stage --------
FROM rust:1-bookworm AS build

RUN apt-get update && apt-get install -y --no-install-recommends \
        pkg-config \
        libdbus-1-dev \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY data ./data
RUN cargo build --release -p supvan-app

# -------- runtime stage --------
FROM debian:trixie-slim

ENV DEBIAN_FRONTEND=noninteractive

RUN apt-get update && apt-get install -y --no-install-recommends \
        cups \
        cups-client \
        cups-bsd \
        cups-browsed \
        cups-filters \
        cups-ipp-utils \
        avahi-daemon \
        avahi-utils \
        libnss-mdns \
        dbus \
        libdbus-1-3 \
        ca-certificates \
        curl \
        ghostscript \
        procps \
        netcat-openbsd \
    && rm -rf /var/lib/apt/lists/*

COPY --from=build /build/target/release/supvan-printer-app /usr/local/bin/supvan-printer-app
COPY data/models.toml /usr/local/share/supvan-printer-app/models.toml

COPY ci/docker-entrypoint.sh /usr/local/bin/docker-entrypoint.sh
COPY ci/run-integration-test.sh /usr/local/bin/run-integration-test.sh
COPY ci/print-job.test /usr/local/bin/print-job.test
RUN chmod +x /usr/local/bin/docker-entrypoint.sh /usr/local/bin/run-integration-test.sh

# CUPS in a Linux container often lacks IPv6 ::1; force IPv4-only listen.
RUN sed -i 's|^Listen localhost:631|Listen 127.0.0.1:631|' /etc/cups/cupsd.conf \
    && mkdir -p /run/cups /run/dbus

ENV SUPVAN_MOCK=1 \
    SUPVAN_MODELS=/usr/local/share/supvan-printer-app/models.toml \
    SUPVAN_DUMP_DIR=/var/lib/supvan/dumps \
    RUST_LOG=info \
    IPP_PORT=8631

EXPOSE 8631 631 5353/udp

ENTRYPOINT ["/usr/local/bin/docker-entrypoint.sh"]
CMD ["run-integration-test.sh"]
