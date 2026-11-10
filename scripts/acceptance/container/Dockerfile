# scripts/acceptance/container/Dockerfile — a disposable "fresh VPS" with sshd.
#
# The image is deliberately empty of VOWP: acceptance tests install the
# published binary the same way a new owner does. It provides the things a
# fresh Ubuntu VPS has (sshd, git, tmux, curl, python3) and one unprivileged
# `vowp` account.
#
# Build with:
#   docker build --build-arg WITH_SUDO=1 -t vowp-acceptance-vps:sudo .
#   docker build --build-arg WITH_SUDO=0 -t vowp-acceptance-vps:no-sudo .
#
# WITH_SUDO=0 leaves no `sudo` binary and no sudoers entry, for the no-sudo
# setup path. Both variants are otherwise identical so a failure is a sudo
# failure, not an image difference.
#
# Everything runs inside the container. The rig mounts only read-only test
# scripts and generated public keys; no host home, tmux socket or VOWP home is
# visible.

FROM ubuntu:24.04

ARG WITH_SUDO=1
ENV DEBIAN_FRONTEND=noninteractive \
    LANG=C.UTF-8 \
    LC_ALL=C.UTF-8 \
    CARGO_TERM_COLOR=never

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        bash \
        ca-certificates \
        curl \
        git \
        iproute2 \
        jq \
        less \
        nano \
        openssh-client \
        openssh-server \
        procps \
        python3 \
        rsync \
        sqlite3 \
        tmux \
        vim-tiny \
    && rm -rf /var/lib/apt/lists/* \
    && if [ "$WITH_SUDO" = "1" ]; then \
        apt-get update \
        && apt-get install -y --no-install-recommends sudo \
        && rm -rf /var/lib/apt/lists/* \
        && printf '%s\n' 'vowp ALL=(ALL) NOPASSWD:ALL' >/etc/sudoers.d/vowp \
        && chmod 0440 /etc/sudoers.d/vowp; \
    fi

RUN groupadd --gid 1001 vowp \
    && useradd --create-home --uid 1001 --gid vowp --shell /bin/bash vowp \
    && install -d -m 0700 -o vowp -g vowp /home/vowp/.ssh \
    && install -d -m 0755 /run/sshd

COPY tmux-wrapper.sh /usr/local/bin/tmux
COPY entrypoint.sh /usr/local/bin/vowp-rig-entrypoint
RUN chmod 0755 /usr/local/bin/tmux /usr/local/bin/vowp-rig-entrypoint

EXPOSE 22
STOPSIGNAL SIGTERM
ENTRYPOINT ["/usr/local/bin/vowp-rig-entrypoint"]
