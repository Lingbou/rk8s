#!/bin/bash

# Standalone no-auth registry smoke test.
set -euo pipefail

REGISTRY_HOST="${REGISTRY_HOST:-127.0.0.1:8968}"
REGISTRY_URL="http://${REGISTRY_HOST}"
API_URL="${REGISTRY_URL}/api/v1"
AUTH_URL="${REGISTRY_URL}/auth/token"
BASE_IMAGE="${BASE_IMAGE:-hello-world}"
TEST_REPO="${TEST_REPO:-admin/distribution-smoke}"
TEST_TAG="${TEST_TAG:-v1}"
REMOTE_IMAGE="${REGISTRY_HOST}/${TEST_REPO}:${TEST_TAG}"

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
    docker logout "$REGISTRY_HOST" >/dev/null 2>&1 || true
    exit 1
}

require_cmd() {
    command -v "$1" >/dev/null 2>&1 || fail "$1 is required"
}

info "Starting standalone distribution registry smoke test"

require_cmd curl
require_cmd docker
require_cmd jq

info "Checking registry health"
curl -fsS "${REGISTRY_URL}/healthz" >/dev/null || fail "registry is not healthy at ${REGISTRY_URL}"

info "Ensuring no Docker credentials are active for ${REGISTRY_HOST}"
docker logout "$REGISTRY_HOST" >/dev/null 2>&1 || true

info "Fetching compatibility token without credentials"
TOKEN="$(curl -fsS "$AUTH_URL" | jq -r .token)"
if [ -z "$TOKEN" ] || [ "$TOKEN" = "null" ]; then
    fail "auth token endpoint did not return a token"
fi
success "Compatibility token endpoint works without Basic Auth"

info "Ensuring local copy of ${BASE_IMAGE} exists"
docker pull "$BASE_IMAGE" >/dev/null || fail "failed to pull ${BASE_IMAGE}"

info "Pushing ${REMOTE_IMAGE} without login"
docker tag "$BASE_IMAGE" "$REMOTE_IMAGE"
docker push "$REMOTE_IMAGE" >/dev/null || fail "anonymous push failed"
success "Anonymous push succeeded"

info "Listing repositories without Authorization"
REPO_LIST="$(curl -fsS "${API_URL}/repo")"
echo "$REPO_LIST" | jq -e --arg repo "$TEST_REPO" '
  (.repositories // .data // .) as $root
  | tostring
  | contains($repo)
' >/dev/null || fail "repo list did not include ${TEST_REPO}: ${REPO_LIST}"
success "Repository list includes ${TEST_REPO}"

info "Changing visibility without Authorization"
curl -fsS -X PUT \
  -H "Content-Type: application/json" \
  -d '{"visibility": "public"}' \
  "${API_URL}/${TEST_REPO}/visibility" >/dev/null || fail "visibility update failed"
success "Visibility update succeeded"

info "Pulling ${REMOTE_IMAGE} without login"
docker rmi "$REMOTE_IMAGE" >/dev/null 2>&1 || true
docker pull "$REMOTE_IMAGE" >/dev/null || fail "anonymous pull failed"
success "Anonymous pull succeeded"

echo
success "Standalone distribution registry smoke test passed"
