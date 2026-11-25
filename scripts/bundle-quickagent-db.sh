#!/usr/bin/env bash
set -euo pipefail
repo=$(cd -- "$(dirname -- "$0")/.." && pwd -P)
dest=${1:-${QUICKAGENT_DB_DEST:-/home/goodtime/Documents/project/codex-workspace-mcp/bundled_plugins/db}}
if [[ $dest != /* || $(basename -- "$dest") != db ]]; then
  printf 'Expected an absolute destination directory named db.\n' >&2
  exit 2
fi
parent=$(cd -- "$(dirname -- "$dest")" && pwd -P)
dest=$parent/db
check_dest() {
  if [[ -L $dest || ( -e $dest && ( ! -d $dest || ! -f $dest/plugin.json || -L $dest/plugin.json ) ) ]]; then
    printf 'Refusing to replace an unrecognized or symlink destination: %s\n' "$dest" >&2
    exit 2
  fi
  if [[ -f $dest/plugin.json ]]; then
    node -e 'const m=JSON.parse(require("node:fs").readFileSync(process.argv[1],"utf8")); if(m.id!=="db" || m.library!=="db") process.exit(1)' "$dest/plugin.json"
  fi
}
check_dest
cd -- "$repo"
if [[ ! -x node_modules/.bin/vite ]]; then npm ci --no-audit --no-fund; fi
npm run build --workspace db-studio
cargo build --release --locked --manifest-path rust-db/Cargo.toml --bin db-server
stage=$(mktemp -d "$parent/.db-stage.XXXXXXXX")
trap 'rm -rf -- "$stage"' EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
chmod 755 "$stage"
install -d "$stage/bin" "$stage/page"
install -m 755 rust-db/target/release/db-server "$stage/bin/db-server"
cp -R packages/db-studio/dist/. "$stage/page/"
install -m 644 scripts/quickagent-db/SKILL.md "$stage/SKILL.md"
node scripts/quickagent-db/manifest.mjs "$stage/plugin.json"
node scripts/quickagent-db/verify.mjs "$stage"
check_dest
if [[ -e $dest ]]; then
  # Fail without moving either directory on platforms that cannot atomically exchange them.
  mv --exchange --no-copy -T -- "$stage" "$dest"
else
  mv --no-copy -T -- "$stage" "$dest"
fi
printf 'Installed DB bundle: %s\n' "$dest"
