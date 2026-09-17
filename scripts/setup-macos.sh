#!/usr/bin/env bash
#
# setup-macos.sh — install the ESP-IDF v5.5.2 toolchain required by the
# EmotionDisplay firmware and the GIF Replacer Tool.
#
# Safe to re-run: every step checks current state before acting, so a second
# run is a fast no-op and a partially-failed run can be resumed by re-running.
#
# On success this script prints a line containing PASS and exits 0.
# On failure it prints a line containing FAIL and exits non-zero.
#
# VERIFICATION STATUS (2026-09-17)
#   Exercised: the idempotent path only — every prerequisite was already present
#   on the machine this was written on, so all six steps took their "already done"
#   branch. Result: PASS: ESP-IDF v5.5.2, exit 0. `bash -n` and `shellcheck` clean.
#   NOT exercised: the clean-machine path — the Xcode CLT install prompt, the
#   Homebrew-missing abort, `brew install` of missing tools, the ESP-IDF clone,
#   and install.sh. These branches are unverified. Test on a fresh Mac before
#   telling a non-technical user to run this.

set -euo pipefail

IDF_VERSION="v5.5.2"
IDF_DIR="${IDF_DIR:-$HOME/esp/esp-idf-v5.5.2}"
TARGET="esp32s3"

step() { printf '\n\033[1;34m==> %s\033[0m\n' "$1"; }
ok()   { printf '\033[1;32m  ok  %s\033[0m\n' "$1"; }
skip() { printf '\033[0;90m  --  %s (already done)\033[0m\n' "$1"; }
die()  { printf '\n\033[1;31mFAIL: %s\033[0m\n' "$1" >&2; exit 1; }

# Run a command inside a fully-initialised ESP-IDF environment.
#
# Two subtleties, both of which bite when run by hand:
#   1. Sourcing export.sh is not `set -u` safe, so nounset is disabled for it.
#   2. A stale IDF_PYTHON_ENV_PATH inherited from a previous ESP-IDF install
#      makes idf.py run under the wrong Python and halt with "Run 'idf.py
#      fullclean'". export.sh overrides IDF_PATH but *respects* an already-set
#      IDF_PYTHON_ENV_PATH, so these three must be unset first.
# The whole thing runs in a subshell so this script's own environment is never
# polluted by the ESP-IDF variables.
idf_run() {
  (
    unset IDF_PATH IDF_PYTHON_ENV_PATH VIRTUAL_ENV
    bash -c "set +u; source '$IDF_DIR/export.sh' >/dev/null 2>&1; $1"
  ) 2>/dev/null
}

printf '\033[1mESP-IDF %s setup for macOS\033[0m\n' "$IDF_VERSION"
printf 'This takes 15-30 minutes and downloads about 3 GB. It is safe to re-run.\n'

# ---------------------------------------------------------------- 1. Xcode CLT
step "Xcode Command Line Tools"
if xcode-select -p >/dev/null 2>&1; then
  skip "Command Line Tools present"
else
  printf '  A macOS dialog will now open. Click "Install", wait for it to finish,\n'
  printf '  then run this script again.\n'
  xcode-select --install >/dev/null 2>&1 || true
  die "Command Line Tools not installed yet. Finish the installer, then re-run this script."
fi

# ----------------------------------------------------------------- 2. Homebrew
step "Homebrew"
if command -v brew >/dev/null 2>&1; then
  skip "Homebrew present"
else
  die "Homebrew is not installed. Install it from https://brew.sh and re-run this script."
fi

# --------------------------------------------------------------- 3. Build tools
step "Build tools (cmake, ninja, dfu-util)"
missing=""
for tool in cmake ninja dfu-util; do
  command -v "$tool" >/dev/null 2>&1 || missing="$missing $tool"
done
if [ -z "${missing# }" ]; then
  skip "cmake, ninja, dfu-util present"
else
  # shellcheck disable=SC2086  # word splitting is intentional: brew wants separate args
  printf '  Installing:%s\n' "$missing"
  # shellcheck disable=SC2086
  brew install $missing || die "brew install failed for:$missing"
  ok "Installed:$missing"
fi

# ----------------------------------------------------------------- 4. ESP-IDF
step "ESP-IDF $IDF_VERSION"
if [ -f "$IDF_DIR/export.sh" ]; then
  skip "ESP-IDF already at $IDF_DIR"
else
  if [ -d "$IDF_DIR" ]; then
    die "$IDF_DIR exists but is incomplete. Delete it and re-run this script."
  fi
  mkdir -p "$(dirname "$IDF_DIR")"
  printf '  Cloning %s into %s (this is the slow part)...\n' "$IDF_VERSION" "$IDF_DIR"
  git clone -b "$IDF_VERSION" --recursive \
    https://github.com/espressif/esp-idf.git "$IDF_DIR" \
    || die "Failed to clone ESP-IDF. Check your network and try again."
  ok "Cloned ESP-IDF $IDF_VERSION"
fi

# ------------------------------------------------------- 5. Install the tools
step "Installing ESP-IDF tools for $TARGET"
if [ -d "$HOME/.espressif/tools" ] || [ -d "$HOME/.espressif/python_env" ]; then
  skip "ESP-IDF tools directory already populated"
else
  printf '  Compiling and downloading toolchains. Progress output follows.\n'
  ( cd "$IDF_DIR" && ./install.sh "$TARGET" ) \
    || die "ESP-IDF install.sh failed. Scroll up for the first error, then re-run."
  ok "Installed ESP-IDF tools for $TARGET"
fi

# -------------------------------------------------------------- 6. Verification
step "Verifying installation"
if [ ! -f "$IDF_DIR/export.sh" ]; then
  die "export.sh missing from $IDF_DIR — the install did not complete."
fi

reported="$(idf_run 'idf.py --version' | head -1)"

case "$reported" in
  *"$IDF_VERSION"*)
    printf '\n\033[1;32mPASS: %s\033[0m\n' "$reported"
    printf '\nESP-IDF is installed at %s\n' "$IDF_DIR"
    printf 'You can now open the GIF Replacer Tool.\n'
    exit 0
    ;;
  *)
    printf '\n\033[1;31mFAIL: expected ESP-IDF %s, got: %s\033[0m\n' \
      "$IDF_VERSION" "${reported:-<no output>}" >&2
    printf 'Try deleting %s and re-running this script.\n' "$IDF_DIR" >&2
    exit 1
    ;;
esac
