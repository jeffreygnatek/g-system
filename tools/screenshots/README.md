# README screenshots

Renders the built frontend in headless Chromium with the Tauri backend mocked
from real data, so every name and value shown comes from a decoded dump:

- the user bank is `examples/vh4-starter.syx` (the bank read is replayed slot
  by slot, with never-written slots answering as empty);
- the Factory tab is `testdata/Factory_Set.SQS`;
- the Global tab is `globals.json`, read from a real unit with `gsys globals`.

```sh
(cd app && npm run build)
cargo run -q -p gsys -- json testdata/Factory_Set.SQS > /tmp/factory.json
cargo run -q -p gsys -- json examples/vh4-starter.syx > /tmp/user.json
cd tools/screenshots && npm install
node shoot.js ../../app/dist /tmp/factory.json /tmp/user.json ../../docs/screenshots
```
