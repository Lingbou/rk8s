#!/bin/bash

# Standalone no-auth rkforge + distribution smoke test.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
RKFORGE_BIN="${PROJECT_DIR}/target/debug/rkforge"

REGISTRY_HOST="${REGISTRY_HOST:-127.0.0.1:8968}"
REGISTRY_URL="http://${REGISTRY_HOST}"
API_URL="${REGISTRY_URL}/api/v1"
AUTH_URL="${REGISTRY_URL}/auth/token"
TIMESTAMP="$(date +%s)"
TEST_REPO="${TEST_REPO:-admin/rkforge-smoke-${TIMESTAMP}}"
TEST_TAG="${TEST_TAG:-v1}"
TEST_IMAGE="${TEST_REPO}:${TEST_TAG}"
TMP_ROOT=""

GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[0;33m'
NC='\033[0m'

info() {
    echo -e "${YELLOW}[INFO] $1${NC}"
}

success() {
    echo -e "${GREEN}[SUCCESS] $1${NC}"
}

fail() {
    echo -e "${RED}[FAIL] $1${NC}"
    cleanup
    exit 1
}

cleanup() {
    if [ -n "${TMP_ROOT:-}" ] && [ -d "$TMP_ROOT" ]; then
        rm -rf "$TMP_ROOT"
    fi
}

trap cleanup EXIT

require_cmd() {
    command -v "$1" >/dev/null 2>&1 || fail "$1 is required"
}

file_size() {
    wc -c < "$1" | tr -d ' '
}

sha256_file() {
    sha256sum "$1" | awk '{print $1}'
}

create_minimal_oci_layout() {
    local layout_dir="$1"
    local tag="$2"
    local work_dir="${TMP_ROOT}/layout-work"
    local blob_dir="${layout_dir}/blobs/sha256"

    rm -rf "$layout_dir" "$work_dir"
    mkdir -p "$blob_dir" "$work_dir"

    printf '{"imageLayoutVersion":"1.0.0"}\n' > "${layout_dir}/oci-layout"

    local layer_file="${work_dir}/layer.tar"
    tar -cf "$layer_file" -T /dev/null
    local layer_digest
    layer_digest="$(sha256_file "$layer_file")"
    local layer_size
    layer_size="$(file_size "$layer_file")"
    cp "$layer_file" "${blob_dir}/${layer_digest}"

    local config_file="${work_dir}/config.json"
    cat > "$config_file" <<EOF
{"architecture":"amd64","os":"linux","rootfs":{"type":"layers","diff_ids":["sha256:${layer_digest}"]},"config":{}}
EOF
    local config_digest
    config_digest="$(sha256_file "$config_file")"
    local config_size
    config_size="$(file_size "$config_file")"
    cp "$config_file" "${blob_dir}/${config_digest}"

    local manifest_file="${work_dir}/manifest.json"
    cat > "$manifest_file" <<EOF
{"schemaVersion":2,"mediaType":"application/vnd.oci.image.manifest.v1+json","config":{"mediaType":"application/vnd.oci.image.config.v1+json","digest":"sha256:${config_digest}","size":${config_size}},"layers":[{"mediaType":"application/vnd.oci.image.layer.v1.tar","digest":"sha256:${layer_digest}","size":${layer_size}}]}
EOF
    local manifest_digest
    manifest_digest="$(sha256_file "$manifest_file")"
    local manifest_size
    manifest_size="$(file_size "$manifest_file")"
    cp "$manifest_file" "${blob_dir}/${manifest_digest}"

    cat > "${layout_dir}/index.json" <<EOF
{"schemaVersion":2,"mediaType":"application/vnd.oci.image.index.v1+json","manifests":[{"mediaType":"application/vnd.oci.image.manifest.v1+json","digest":"sha256:${manifest_digest}","size":${manifest_size},"annotations":{"org.opencontainers.image.ref.name":"${tag}"}}]}
EOF
}

info "Starting standalone rkforge registry smoke test"

require_cmd awk
require_cmd curl
require_cmd jq
require_cmd sha256sum
require_cmd tar
require_cmd wc

info "Checking distribution health"
curl -fsS "${REGISTRY_URL}/healthz" >/dev/null || fail "distribution is not healthy at ${REGISTRY_URL}"

if [ ! -x "$RKFORGE_BIN" ]; then
    info "Building rkforge binary"
    (cd "$PROJECT_DIR" && cargo build -p rkforge --bin rkforge) || fail "failed to build rkforge"
fi

TMP_ROOT="$(mktemp -d)"
LAYOUT_DIR="${TMP_ROOT}/image-layout"

info "Creating minimal OCI layout for ${TEST_IMAGE}"
create_minimal_oci_layout "$LAYOUT_DIR" "$TEST_TAG"

info "Checking compatibility token endpoint without credentials"
TOKEN="$(curl -fsS "$AUTH_URL" | jq -r .token)"
if [ -z "$TOKEN" ] || [ "$TOKEN" = "null" ]; then
    fail "auth token endpoint did not return a token"
fi
success "Compatibility token endpoint works without Basic Auth"

info "Pushing ${TEST_IMAGE} with rkforge without login"
"$RKFORGE_BIN" push --url "$REGISTRY_HOST" --path "$LAYOUT_DIR" "$TEST_IMAGE" >/dev/null || fail "rkforge anonymous push failed"
success "rkforge anonymous push succeeded"

info "Listing repositories with rkforge without login"
REPO_LIST="$("$RKFORGE_BIN" repo --url "$REGISTRY_HOST" list 2>&1)"
echo "$REPO_LIST" | grep -q "$TEST_REPO" || fail "repo list did not include ${TEST_REPO}: ${REPO_LIST}"
success "rkforge repo list includes ${TEST_REPO}"

info "Changing repository visibility with rkforge without login"
"$RKFORGE_BIN" repo --url "$REGISTRY_HOST" vis "$TEST_REPO" public >/dev/null || fail "rkforge repo vis failed"
REPO_LIST_AFTER_VIS="$("$RKFORGE_BIN" repo --url "$REGISTRY_HOST" list 2>&1)"
echo "$REPO_LIST_AFTER_VIS" | grep -q "${TEST_REPO}.*public" || fail "visibility did not change to public: ${REPO_LIST_AFTER_VIS}"
success "rkforge repo vis succeeded"

info "Pulling ${TEST_IMAGE} with rkforge without login"
"$RKFORGE_BIN" pull --url "$REGISTRY_HOST" "$TEST_IMAGE" >/dev/null || fail "rkforge anonymous pull failed"
success "rkforge anonymous pull succeeded"

echo
success "Standalone rkforge registry smoke test passed"
