ARG REPO_ARCH

FROM asonix/rust-runner:$REPO_ARCH-latest

COPY relay /usr/local/bin/relay

USER app
EXPOSE 8080
VOLUME /mnt
ENTRYPOINT ["/sbin/tini", "--"]
CMD ["/usr/local/bin/relay"]
