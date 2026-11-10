# scripts/acceptance/container/phone/Dockerfile — the second SSH client ("phone").
#
# This is only an SSH client. It joins the acceptance network, receives the
# generated phone key, and reaches the home container as `home`. It never sees
# the owner's keys, host files or host tmux server. Running commands here is how
# the acceptance tests play the phone surface over a separate SSH connection.
#
# Build context is this directory (scripts/acceptance/container/phone).

FROM ubuntu:24.04

ENV DEBIAN_FRONTEND=noninteractive \
    LANG=C.UTF-8 \
    LC_ALL=C.UTF-8

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        bash \
        ca-certificates \
        curl \
        jq \
        less \
        nano \
        openssh-client \
        tmux \
        vim-tiny \
    && rm -rf /var/lib/apt/lists/*

RUN groupadd --gid 1001 phone \
    && useradd --create-home --uid 1001 --gid phone --shell /bin/bash phone \
    && install -d -m 0700 -o phone -g phone /home/phone/.ssh

COPY entrypoint.sh /usr/local/bin/phone-entrypoint
RUN chmod 0755 /usr/local/bin/phone-entrypoint

ENTRYPOINT ["/usr/local/bin/phone-entrypoint"]
