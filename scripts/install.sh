#!/bin/sh

set -eu

VERSION="${CLEANRS_VERSION:-0.1.5}"
INSTALL_DIR="${CLEANRS_INSTALL_DIR:-$HOME/.local/bin}"
REPOSITORY="${CLEANRS_REPOSITORY:-ho-doan/my-cleaner}"

case "$(uname -s)" in
  Darwin)
    ;;
  *)
    echo "cleanrs currently supports macOS only." >&2
    exit 1
    ;;
esac

case "$(uname -m)" in
  arm64|aarch64)
    TARGET="aarch64-apple-darwin"
    ;;
  x86_64|amd64)
    TARGET="x86_64-apple-darwin"
    ;;
  *)
    echo "Unsupported macOS architecture: $(uname -m)" >&2
    exit 1
    ;;
esac

for command in curl shasum tar install mkdir mktemp grep sleep awk; do
  if ! command -v "$command" >/dev/null 2>&1; then
    echo "Required command not found: ${command}" >&2
    exit 1
  fi
done

# Keep the checksum map explicit so a release cannot be installed unless its
# artifact has been reviewed and added here.
case "${VERSION}:${TARGET}" in
  0.1.5:aarch64-apple-darwin)
    EXPECTED_SHA256="2e152e8ea0166ec73904a25504f37aa1965815f00950b397fc92ef3a7001eb5a"
    ;;
  0.1.5:x86_64-apple-darwin)
    EXPECTED_SHA256="c7e34a828eb66e7074b4d4d4a6f444d6cc496731d8f9d9ba35a837384b32d642"
    ;;
  0.1.4:aarch64-apple-darwin)
    EXPECTED_SHA256="8990b85b1e61ad61f06c1c334a8942a09abd71e0f65f482f45c4c670ed7b7449"
    ;;
  0.1.4:x86_64-apple-darwin)
    EXPECTED_SHA256="bb4ba3ba319a99f1c7132dc834995f6ddbfd78cbe0f923c7e4e3bd8f0e2d174d"
    ;;
  0.1.3:aarch64-apple-darwin)
    EXPECTED_SHA256="fb71f116f771bf17c89d5e53a7f65cc515b4fa98b4836eccae9dc88982207bd7"
    ;;
  0.1.3:x86_64-apple-darwin)
    EXPECTED_SHA256="559fab41ec179eb26aeb31875f7c4cc6f8ecba686e8e1454d0d81b3c307a9206"
    ;;
  *)
    RETRY_DIR="$(mktemp -d "${TMPDIR:-/tmp}/cleanrs-installer-refresh.XXXXXX")"
    RETRY_ATTEMPTS=6
    retry_attempt=1
    while [ "$retry_attempt" -le "$RETRY_ATTEMPTS" ]; do
      case "$retry_attempt" in
        1) retry_delay=2 ;;
        2) retry_delay=4 ;;
        *) retry_delay=8 ;;
      esac
      echo "Waiting for installer CDN to publish cleanrs ${VERSION} checksum (${retry_attempt}/${RETRY_ATTEMPTS})..." >&2
      sleep "$retry_delay"
      refreshed_script="${RETRY_DIR}/install.sh"
      if curl --fail --silent --show-error --location --retry 2 \
        --proto '=https' --tlsv1.2 \
        -H 'Cache-Control: no-cache' -H 'Pragma: no-cache' \
        --output "$refreshed_script" \
        "https://raw.githubusercontent.com/${REPOSITORY}/master/scripts/install.sh?version=${VERSION}&attempt=${retry_attempt}" \
        && grep -A1 -Fq "  ${VERSION}:${TARGET})" "$refreshed_script" \
        && grep -A1 -F "  ${VERSION}:${TARGET})" "$refreshed_script" | grep -Eq 'EXPECTED_SHA256="[^"]+"'
      then
        if CLEANRS_VERSION="$VERSION" CLEANRS_INSTALL_DIR="$INSTALL_DIR" sh "$refreshed_script"; then
          refreshed_status=0
        else
          refreshed_status=$?
        fi
        rm -rf "$RETRY_DIR"
        exit "$refreshed_status"
      fi
      retry_attempt=$((retry_attempt + 1))
    done
    rm -rf "$RETRY_DIR"
    echo "No verified checksum is available for cleanrs ${VERSION} (${TARGET}) after waiting for the installer CDN." >&2
    echo "Use a released version or set CLEANRS_VERSION after its checksum is published." >&2
    exit 1
    ;;
esac

ARCHIVE="cleanrs-v${VERSION}-${TARGET}.tar.gz"
DOWNLOAD_URL="https://github.com/${REPOSITORY}/releases/download/v${VERSION}/${ARCHIVE}"
TEMP_DIR="$(mktemp -d "${TMPDIR:-/tmp}/cleanrs-install.XXXXXX")"

cleanup() {
  rm -rf "$TEMP_DIR"
}
trap cleanup EXIT HUP INT TERM

ARCHIVE_PATH="${TEMP_DIR}/${ARCHIVE}"
echo "Downloading cleanrs ${VERSION} for ${TARGET}..."
curl --fail --silent --show-error --location --retry 3 \
  --proto '=https' --tlsv1.2 \
  --output "$ARCHIVE_PATH" "$DOWNLOAD_URL"

ACTUAL_SHA256="$(shasum -a 256 "$ARCHIVE_PATH" | awk '{print $1}')"
if [ "$ACTUAL_SHA256" != "$EXPECTED_SHA256" ]; then
  echo "Checksum verification failed." >&2
  echo "Expected: ${EXPECTED_SHA256}" >&2
  echo "Actual:   ${ACTUAL_SHA256}" >&2
  exit 1
fi

tar -xzf "$ARCHIVE_PATH" -C "$TEMP_DIR"
if [ ! -f "${TEMP_DIR}/cleanrs" ]; then
  echo "The release archive does not contain the cleanrs binary." >&2
  exit 1
fi

mkdir -p "$INSTALL_DIR"
install -m 0755 "${TEMP_DIR}/cleanrs" "${INSTALL_DIR}/cleanrs"

echo "Installed cleanrs ${VERSION} to ${INSTALL_DIR}/cleanrs"
case ":${PATH:-}:" in
  *:"${INSTALL_DIR}":*)
    ;;
  *)
    echo "Add it to PATH with: export PATH=\"${INSTALL_DIR}:\$PATH\""
    ;;
esac
echo "Run: cleanrs tui"
