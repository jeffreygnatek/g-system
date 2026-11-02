# Cross-check the ported parameter tables

`crates/gsystem-protocol/src/params.rs` is a Rust port of
davidavalanches/gsystem-editor's `params.ts`. This compares the two:
every block, effect type and parameter (names, ranges, value lists, slot sizes).

```sh
cd tools/xcheck
git clone --depth 1 https://github.com/davidavalanches/gsystem-editor
npx esbuild dump.ts --bundle --platform=node --outfile=dump.js && node dump.js > ts.json
cargo run -q -p gsystem-protocol --example dump_tables > rs.json
python3 -c "import json; a=json.load(open('ts.json')); b=json.load(open('rs.json')); print('match' if a==b else 'DIFFERENT')"
```

Last run (port of commit 9b50ef8): 8 blocks, 40 types, 0 differences.
Display formats aren't compared; the EQ width display intentionally differs (see params.rs).
