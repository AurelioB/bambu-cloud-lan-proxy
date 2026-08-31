# syntax=docker/dockerfile:1
FROM alpine:3.23

RUN apk add --no-cache mosquitto openssl
COPY docker/bootstrap.sh /usr/local/bin/bootstrap
ENTRYPOINT ["/usr/local/bin/bootstrap"]
