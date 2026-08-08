#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
APP_VERSION="$(grep -m1 '"version"' "${ROOT_DIR}/src-tauri/tauri.conf.json" | sed -E 's/.*"version": "([^"]+)".*/\1/')"
APPIMAGE_DIR="${ROOT_DIR}/src-tauri/target/release/bundle/appimage"
APP_DIR="${APPIMAGE_DIR}/Fun ASR Desktop.AppDir"
OUTPUT_NAME="Fun_ASR_Desktop-${APP_VERSION}-x86_64.AppImage"
APPIMAGE_PLUGIN="${TAURI_LINUXDEPLOY_APPIMAGE_PLUGIN:-${HOME}/.cache/tauri/linuxdeploy-plugin-appimage.AppImage}"
LOG_DIR="${ROOT_DIR}/target"
LOG_FILE="${LOG_DIR}/host-appimage-build.log"

cd "${ROOT_DIR}"
mkdir -p "${APPIMAGE_DIR}" "${LOG_DIR}"
rm -f "${ROOT_DIR}"/Fun_ASR_Desktop-*.AppImage "${APPIMAGE_DIR}"/Fun_ASR_Desktop-*.AppImage

set +e
NO_STRIP=true bunx tauri build --bundles appimage --verbose >"${LOG_FILE}" 2>&1
TAURI_STATUS=$?
set -e

if [[ "${TAURI_STATUS}" -eq 0 ]]; then
  FOUND_APPIMAGE="$(find "${APPIMAGE_DIR}" "${ROOT_DIR}" -maxdepth 1 -type f -name '*.AppImage' -print -quit)"
  if [[ -n "${FOUND_APPIMAGE}" ]]; then
    chmod +x "${FOUND_APPIMAGE}"
    echo "AppImage: ${FOUND_APPIMAGE}"
    exit 0
  fi

  echo "Tauri reported success, but no AppImage artifact was found." >&2
  exit 1
fi

if [[ ! -d "${APP_DIR}" ]]; then
  echo "Tauri AppImage build failed before creating AppDir." >&2
  echo "Log: ${LOG_FILE}" >&2
  tail -120 "${LOG_FILE}" >&2 || true
  exit "${TAURI_STATUS}"
fi

if ! grep -Eq 'Failed to run plugin: gtk|globusconnectpersonal|libpng15|Strip call failed|\\.relr\\.dyn' "${LOG_FILE}"; then
  echo "Tauri AppImage build failed for an unexpected reason." >&2
  echo "Log: ${LOG_FILE}" >&2
  tail -160 "${LOG_FILE}" >&2 || true
  exit "${TAURI_STATUS}"
fi

if [[ ! -x "${APPIMAGE_PLUGIN}" ]]; then
  echo "linuxdeploy AppImage output plugin was not found at ${APPIMAGE_PLUGIN}." >&2
  echo "Run bunx tauri build --bundles appimage once, or set TAURI_LINUXDEPLOY_APPIMAGE_PLUGIN." >&2
  exit 1
fi

echo "Tauri populated AppDir but host linuxdeploy failed during packaging."
echo "Retrying direct AppImage packaging with NO_STRIP=true."
echo "Original log: ${LOG_FILE}"

DESKTOP_FILE="${APP_DIR}/usr/share/applications/Fun ASR Desktop.desktop"
if [[ -f "${DESKTOP_FILE}" ]]; then
  ln -sfn "usr/share/applications/Fun ASR Desktop.desktop" "${APP_DIR}/Fun ASR Desktop.desktop"
fi

for APPDIR_DESKTOP_FILE in \
  "${APP_DIR}/Fun ASR Desktop.desktop" \
  "${APP_DIR}/usr/share/applications/Fun ASR Desktop.desktop"; do
  if [[ -f "${APPDIR_DESKTOP_FILE}" ]]; then
    if grep -q '^X-AppImage-Version=' "${APPDIR_DESKTOP_FILE}"; then
      sed -i -E "s/^X-AppImage-Version=.*/X-AppImage-Version=${APP_VERSION}/" "${APPDIR_DESKTOP_FILE}"
    else
      printf '\nX-AppImage-Version=%s\n' "${APP_VERSION}" >>"${APPDIR_DESKTOP_FILE}"
    fi
  fi
done

ICON_SOURCE=""
for ICON_CANDIDATE in \
  "${APP_DIR}/usr/share/icons/hicolor/512x512/apps/fun-asr-desktop.png" \
  "${APP_DIR}/usr/share/icons/hicolor/256x256@2/apps/fun-asr-desktop.png" \
  "${APP_DIR}/usr/share/icons/hicolor/128x128/apps/fun-asr-desktop.png" \
  "${APP_DIR}/usr/share/icons/hicolor/32x32/apps/fun-asr-desktop.png"; do
  if [[ -f "${ICON_CANDIDATE}" ]]; then
    ICON_SOURCE="${ICON_CANDIDATE}"
    break
  fi
done

if [[ -n "${ICON_SOURCE}" ]]; then
  cp -f "${ICON_SOURCE}" "${APP_DIR}/fun-asr-desktop.png"
fi

NO_STRIP=true \
ARCH=x86_64 \
VERSION="${APP_VERSION}" \
LINUXDEPLOY_OUTPUT_VERSION="${APP_VERSION}" \
"${APPIMAGE_PLUGIN}" \
  --appimage-extract-and-run \
  --appdir "${APP_DIR}"

if [[ ! -f "${ROOT_DIR}/${OUTPUT_NAME}" ]]; then
  echo "linuxdeploy finished, but ${OUTPUT_NAME} was not produced." >&2
  exit 1
fi

mv -f "${ROOT_DIR}/${OUTPUT_NAME}" "${APPIMAGE_DIR}/${OUTPUT_NAME}"
chmod +x "${APPIMAGE_DIR}/${OUTPUT_NAME}"
echo "AppImage: ${APPIMAGE_DIR}/${OUTPUT_NAME}"
