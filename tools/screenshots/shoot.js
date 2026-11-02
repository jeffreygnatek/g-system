// Screenshot the real app frontend with the Tauri backend mocked from the
// factory-set JSON (gsys json output). No values are invented: slots, names
// and params come straight from the decoded dump.
const { chromium } = require("playwright");
const fs = require("fs");
const path = require("path");
const http = require("http");

const DIST = process.argv[2];
const FACTORY = process.argv[3];
const OUT = process.argv[4];
const FONTS = path.join(__dirname, "node_modules/@fontsource");

const factory = JSON.parse(fs.readFileSync(FACTORY, "utf8"));
const fontCss = (port) => [
  ["Inter", 400, "inter/files/inter-latin-400-normal.woff2"],
  ["Inter", 500, "inter/files/inter-latin-500-normal.woff2"],
  ["Inter", 600, "inter/files/inter-latin-600-normal.woff2"],
  ["JetBrains Mono", 400, "jetbrains-mono/files/jetbrains-mono-latin-400-normal.woff2"],
  ["JetBrains Mono", 500, "jetbrains-mono/files/jetbrains-mono-latin-500-normal.woff2"],
].map(([f, w, p]) => `@font-face{font-family:"${f}";font-weight:${w};src:url(http://localhost:${port}/__fonts/${p}) format("woff2")}`).join("\n");

const server = http.createServer((req, res) => {
  let p = decodeURIComponent(req.url.split("?")[0]);
  let file = p.startsWith("/__fonts/") ? path.join(FONTS, p.slice(9)) : path.join(DIST, p === "/" ? "index.html" : p);
  if (!fs.existsSync(file)) { res.writeHead(404); return res.end(); }
  const type = file.endsWith(".js") ? "text/javascript" : file.endsWith(".css") ? "text/css" : file.endsWith(".woff2") ? "font/woff2" : "text/html";
  res.writeHead(200, { "content-type": type, "access-control-allow-origin": "*" });
  fs.createReadStream(file).pipe(res);
});

(async () => {
  await new Promise((r) => server.listen(0, r));
  const port = server.address().port;
  const browser = await chromium.launch();
  const page = await browser.newPage({ viewport: { width: 1360, height: 860 }, deviceScaleFactor: 2 });
  await page.route("https://fonts.googleapis.com/**", (r) => r.fulfill({ contentType: "text/css", body: fontCss(port) }));
  await page.addInitScript((factory) => {
    const label = (i) => `${String(Math.floor(i / 5)).padStart(2, "0")}-${(i % 5) + 1}`;
    const slots = Array.from({ length: 200 }, (_, i) => ({ index: i, label: label(i), name: null }));
    for (const p of factory) slots[p.slot].name = p.name;
    // Before "connect" the bank is empty, as on a real first launch. After the
    // (mocked) read, two slots are marked edited to show the changed indicator.
    const empty = { path: null, occupied: 0, dirty: [], slots: slots.map((s) => ({ ...s, name: null })) };
    const loaded = { path: "/Users/jeff/G-System/Factory_Set.SQS", occupied: factory.length, dirty: [32, 61], slots };
    let bank = empty;
    window.__TAURI_INTERNALS__ = {
      transformCallback: () => 0,
      invoke: async (cmd, args) => {
        switch (cmd) {
          case "get_bank": return bank;
          case "slot_detail": {
            const p = factory.find((x) => x.slot === args.index);
            return { index: p.slot, label: p.label, name: p.name, params_i24: p.params_i24, raw_hex: p.raw_hex };
          }
          case "midi_ports": return { inputs: ["G-System"], outputs: ["G-System"] };
          case "midi_identify": return "01 00 04 02";
          case "midi_backup": bank = loaded; return factory.map((p) => p.slot);
          default: return null;
        }
      },
    };
  }, factory);

  await page.goto(`http://localhost:${port}/`);
  await page.waitForTimeout(600);
  await page.screenshot({ path: path.join(OUT, "connect.png") });

  // connect → banks, select 02-3
  await page.click("#connect");
  await page.waitForSelector(".cell");
  await page.click('.cell[data-i="12"]');
  await page.waitForTimeout(400);
  await page.screenshot({ path: path.join(OUT, "banks.png") });

  await page.click('[data-view="preset"]');
  await page.waitForTimeout(400);
  await page.screenshot({ path: path.join(OUT, "preset.png") });

  await page.click('[data-view="global"]');
  await page.waitForTimeout(400);
  await page.screenshot({ path: path.join(OUT, "global.png") });

  await browser.close();
  server.close();
})();
