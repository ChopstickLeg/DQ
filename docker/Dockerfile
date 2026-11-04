FROM --platform=$BUILDPLATFORM ubuntu:24.04 AS filesystem

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && install -d -o 10001 -g 10001 -m 0750 /data \
    && rm -rf /var/lib/apt/lists/*

FROM ubuntu:24.04

ARG TARGETARCH
COPY --from=filesystem /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/ca-certificates.crt
COPY --from=filesystem --chown=10001:10001 /data /data
COPY --chmod=755 ${TARGETARCH}/dq /usr/local/bin/dq

USER 10001:10001
ENV LISTEN_ADDR=0.0.0.0:50051 \
    DATABASE_URL=sqlite:///data/dq.db
EXPOSE 50051

ENTRYPOINT ["/usr/local/bin/dq"]
