# A clean build environment for the fresh-user test.
#
# This image intentionally contains no VOWP binary and no build cache: the test
# compiles the checkout itself, exactly as a new user would. Only the base Rust
# toolchain, git and tmux are provided (the README quickstart prerequisites).
FROM rust:1-slim-bookworm

ENV DEBIAN_FRONTEND=noninteractive \
    CARGO_TERM_COLOR=never \
    RUST_BACKTRACE=1 \
    RUSTUP_AUTO_INSTALL=0

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        ca-certificates \
        git \
        tmux \
    && rm -rf /var/lib/apt/lists/*

# The rolling rust image installs a numbered toolchain, while rust-toolchain.toml
# asks for the `stable` channel. Install that channel (and the components the
# repository asks for) while building the image, so the test never downloads a
# toolchain at run time.
RUN rustup toolchain install stable --profile minimal \
        --component rustfmt --component clippy \
    && rustup default stable

# A normal unprivileged user, so the test does not accidentally depend on root
# or on a writable system directory.
RUN useradd --create-home --uid 1000 --shell /bin/bash fresh

# The checkout to test. run.sh builds this context from the worktree and leaves
# target/ and .git out; the container starts with no build artifacts.
COPY repo /work/vowp
COPY fresh-user-plan.sh /work/fresh-user-plan.sh
COPY fake-bin /work/fake-bin
RUN chmod 0755 /work/fresh-user-plan.sh /work/fake-bin/claude /work/fake-bin/opencode \
    && chown -R fresh:fresh /work

USER fresh
WORKDIR /work/vowp
