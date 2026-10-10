#!/usr/bin/env bash
set -euo pipefail
repo="$(cd "$(dirname "$0")/../.." && pwd)"
root="$(mktemp -d /tmp/limo-cad-viewport-fixture-tests.XXXXXX)"
cleanup() {
  [[ "$root" == /tmp/limo-cad-viewport-fixture-tests.* ]]
  rm -rf -- "$root"
}
trap cleanup EXIT
mkdir "$root/bin" "$root/shared-runner"
chmod 777 "$root/shared-runner"
export RUNNER_TEMP="$root/shared-runner"
export PATH="$root/bin:$PATH"
cat >"$root/model.AppImage" <<'MOCK'
#!/usr/bin/env bash
set -euo pipefail
[[ "$1" == --appimage-extract ]]
mkdir squashfs-root
printf '#!/bin/sh\nexit 0\n' >squashfs-root/AppRun
printf '[Desktop Entry]\nMimeType=x-scheme-handler/limo-cad;\n' >squashfs-root/limo-cad.desktop
MOCK
cat >"$root/bin/desktop-file-validate" <<'MOCK'
#!/usr/bin/env bash
test -f "$1"
MOCK
cat >"$root/bin/find" <<'MOCK'
#!/usr/bin/env bash
printf '/test/lvp_icd.json\n'
MOCK
cat >"$root/bin/dbus-run-session" <<'MOCK'
#!/usr/bin/env bash
[[ "$1" == -- ]]
shift
exec "$@"
MOCK
cat >"$root/bin/xvfb-run" <<'MOCK'
#!/usr/bin/env bash
[[ "$1" == -a && "$2" == -s ]]
shift 3
exec "$@"
MOCK
cat >"$root/bin/cargo" <<'MOCK'
#!/usr/bin/env bash
set -euo pipefail
[[ "$1" == xtask ]]
command="$2"
shift 2
if [[ "$command" == test-mcp ]]; then
  [[ "$1" == native-platform ]]
  while [[ "$1" != --out ]]; do shift; done
  out="$2"
  [[ "$out" == /tmp/limo-cad-package-display.*/evidence/native-platform ]]
  [[ "$out" != "$RUNNER_TEMP"/* ]]
  if [[ "$OSTYPE" != msys* && "$OSTYPE" != cygwin* ]]; then
    [[ "$(stat -c '%a' "$(dirname "$(dirname "$out")")")" == 700 ]]
  fi
  mkdir -p "$out"
  printf '{"fixture":"isolated","failure_requested":%s}\n' "$FAIL_FIXTURE" >"$out/report.json"
  printf '%s\n' "$out" >"$out/owned-path.txt"
  echo 'owned fixture diagnostic'
  if [[ "$FAIL_FIXTURE" == true ]]; then exit 19; fi
elif [[ "$command" == verify-linux-recipe-handler ]]; then
  [[ "$1" == --evidence ]]
  printf '{"passed":true}\n' >"$2/recipe-handler.json"
else
  exit 2
fi
MOCK
chmod +x "$root/model.AppImage" "$root/bin/"*
for failure in false true; do
  evidence="$RUNNER_TEMP/case-$failure"
  export FAIL_FIXTURE="$failure"
  if bash "$repo/scripts/verify-linux-viewport.sh" "$root/model.AppImage" x11 "$evidence"; then
    [[ "$failure" == false ]]
  else
    if [[ "$failure" != true ]]; then
      cat "$evidence/fixture.log" >&2
      exit 1
    fi
  fi
  grep -q 'owned fixture diagnostic' "$evidence/fixture.log"
  grep -q '"fixture":"isolated"' "$evidence/native-platform/report.json"
  owned="$(cat "$evidence/native-platform/owned-path.txt")"
  [[ ! -e "$owned" ]]
  if [[ "$failure" == false ]]; then
    test -f "$evidence/recipe-handler.json"
  else
    test ! -e "$evidence/recipe-handler.json"
  fi
done
echo 'PASS private package fixture, successful verification, and failed-run diagnostics'
