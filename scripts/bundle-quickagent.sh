#!/usr/bin/env bash
set -euo pipefail

repo=$(cd -- "$(dirname -- "$0")/.." && pwd -P)
dest=${1:-${QUICKAGENT_EXCEL_DEST:-/home/goodtime/Documents/project/codex-workspace-mcp/bundled_plugins/excel}}
if [[ $dest != /* || $dest == / || $(basename -- "$dest") != excel ]]; then
  printf 'Expected an absolute destination directory named excel.\n' >&2
  exit 2
fi
parent=$(dirname -- "$dest")
if [[ ! -d $parent ]]; then
  printf 'QuickAgent bundled_plugins directory not found: %s\n' "$parent" >&2
  exit 2
fi
parent=$(cd -- "$parent" && pwd -P)
dest=$parent/excel
if [[ -L $dest || ( -e $dest && ! -f $dest/plugin.json ) ]]; then
  printf 'Refusing to replace a symlink or unrecognized directory: %s\n' "$dest" >&2
  exit 2
fi
if [[ -f $dest/plugin.json ]]; then
  node -e 'const m = JSON.parse(require("node:fs").readFileSync(process.argv[1], "utf8")); if (m.id !== "excel") process.exit(1)' "$dest/plugin.json"
fi

cd -- "$repo"
if [[ ! -x node_modules/.bin/vite ]]; then npm ci --no-audit --no-fund; fi
npm run build
cargo build --release --locked --manifest-path rust-excel/Cargo.toml --bin excel-server

stage=$(mktemp -d "$parent/.excel-stage.XXXXXXXX")
backup=''
cleanup() {
  if [[ -n $backup && -d $backup/old && ! -e $dest ]]; then mv -- "$backup/old" "$dest"; fi
  if [[ -n $backup ]]; then rm -rf -- "$backup"; fi
  if [[ -n $stage ]]; then rm -rf -- "$stage"; fi
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
chmod 755 "$stage"
install -d "$stage/bin" "$stage/page"
install -m 755 rust-excel/target/release/excel-server "$stage/bin/excel-server"
if command -v strip >/dev/null 2>&1; then strip "$stage/bin/excel-server"; fi
cp -R packages/excel-studio/dist/. "$stage/page/"
install -m 644 scripts/quickagent/SKILL.md "$stage/SKILL.md"
node scripts/quickagent/manifest.mjs "$stage/plugin.json"
node scripts/quickagent/verify.mjs "$stage"

if [[ -e $dest ]]; then
  backup=$(mktemp -d "$parent/.excel-backup.XXXXXXXX")
  mv -- "$dest" "$backup/old"
fi
mv -- "$stage" "$dest"
stage=''
printf 'Installed Excel bundle: %s\n' "$dest"
