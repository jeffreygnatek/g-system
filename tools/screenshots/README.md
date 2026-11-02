# README screenshots

Renders the built frontend in headless Chromium with the Tauri backend mocked
from the decoded factory set, so every name and slot shown is real data.

```sh
(cd app && npm run build)
cargo run -q -p gsys -- json testdata/Factory_Set.SQS > /tmp/factory.json
cd tools/screenshots && npm install
node shoot.js ../../app/dist /tmp/factory.json ../../docs/screenshots
```
