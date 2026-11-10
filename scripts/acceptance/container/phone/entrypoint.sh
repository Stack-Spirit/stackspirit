#!/usr/bin/env bash
# scripts/acceptance/container/phone/entrypoint.sh — prepare the phone client.
#
# The rig mounts one generated private key and an ssh_config at
# /run/vowp-rig/phone/. They are copied into the unprivileged phone user's home
# and then the container idles, waiting for `docker exec ... ssh home ...`.

set -Eeuo pipefail

if [ ! -f /run/vowp-rig/phone/id_ed25519 ]; then
    printf '%s\n' "phone-entrypoint: /run/vowp-rig/phone/id_ed25519 is missing" >&2
    exit 1
fi
if [ ! -f /run/vowp-rig/phone/config ]; then
    printf '%s\n' "phone-entrypoint: /run/vowp-rig/phone/config is missing" >&2
    exit 1
fi

install -d -m 0700 -o phone -g phone /home/phone/.ssh
install -m 0600 -o phone -g phone /run/vowp-rig/phone/id_ed25519 /home/phone/.ssh/id_ed25519
install -m 0644 -o phone -g phone /run/vowp-rig/phone/config /home/phone/.ssh/config
install -m 0600 -o phone -g phone /dev/null /home/phone/.ssh/known_hosts

printf '%s\n' "acceptance-phone: ready; try: ssh home 'uname -a'" >&2
exec su -s /bin/bash phone -c 'exec sleep infinity'
