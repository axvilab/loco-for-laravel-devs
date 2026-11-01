#!/usr/bin/env bash
# Builds, lints and tests the blog locally in Docker (no Rust needed on the host),
# then starts the production stack from step 13 with docker compose and
# checks that it answers.
#
#   ./scripts/check.sh                              # main
#   ./scripts/check.sh main step-01-new step-07-uploads
#
# Each ref is checked from a clean `git archive` copy, so uncommitted changes
# are not included. Ports 5150 and 8025 must be free for the compose check.
# The cargo registry is cached in the Docker volume `lfld-cargo`;
# remove it with `docker volume rm lfld-cargo`.
set -uo pipefail

cd "$(dirname "$0")/.."
refs=("${@:-main}")
image=lfld-check-rust
work="$(mktemp -d)"
project=lfld-check

cleanup() {
    if [ -f "$work/compose/blog/docker-compose.yml" ]; then
        (cd "$work/compose/blog" && docker compose -p "$project" down -v --rmi local >/dev/null 2>&1)
    fi
    # files written by the container belong to the same user, so this works
    rm -rf "$work"
}
trap cleanup EXIT

echo "==> preparing Docker image"
printf 'FROM rust:1-bookworm\nRUN rustup component add rustfmt clippy\n' \
    | docker build -q -t "$image" - >/dev/null || exit 1

# The volume starts empty: copy the toolchain's cargo home into it once.
docker run --rm -v lfld-cargo:/cargo "$image" \
    bash -c "cp -rn /usr/local/cargo/. /cargo/ && chown -R $(id -u):$(id -g) /cargo"

failed=()
run() {
    local name="$1"; shift
    echo; echo "==> $name"
    if "$@"; then echo "OK    $name"; else echo "FAIL  $name"; failed+=("$name"); fi
}

test_ref() {
    local dir="$work/$1"
    mkdir -p "$dir"
    git archive "$1" blog | tar -x -C "$dir" || return 1
    docker run --rm -u "$(id -u):$(id -g)" -e CARGO_HOME=/cargo \
        -v lfld-cargo:/cargo -v "$dir/blog":/w -w /w "$image" \
        bash -c "cargo clippy -- -D warnings && cargo test"
}

compose_up() (  # subshell: the cd stays inside
    local dir="$work/compose"
    mkdir -p "$dir"
    git archive main blog | tar -x -C "$dir" || return 1
    cd "$dir/blog" || return 1
    cp .env.example .env
    sed -i "s|^JWT_SECRET=.*|JWT_SECRET=$(openssl rand -base64 48 | tr -d '\n')|" .env
    docker compose -p "$project" up -d --build --wait || return 1
    local ping home
    ping=$(curl -s -o /dev/null -w '%{http_code}' http://localhost:5150/_ping)
    home=$(curl -s -o /dev/null -w '%{http_code}' http://localhost:5150/)
    echo "/_ping: $ping, /: $home"
    [ "$ping" = 200 ] && [ "$home" = 200 ]
)

for ref in "${refs[@]}"; do
    run "$ref: clippy + test" test_ref "$ref"
done
run "docker compose (main)" compose_up

echo
if [ ${#failed[@]} -eq 0 ]; then
    echo "All checks passed."
else
    echo "Failed: ${failed[*]}"
    exit 1
fi
