// Screenshot the real app frontend with the Tauri backend mocked from real
// data: the user bank is examples/vh4-starter.syx, the Factory tab is TC's
// factory set (both decoded by `gsys json`), and the Global tab shows
// globals.json, read from a real unit with `gsys globals`. No values are
// invented; the mock only replays them, including the slot-by-slot read.
const { chromium } = require("playwright");
const fs = require("fs");
const path = require("path");
const http = require("http");

const [DIST, FACTORY, USER, OUT] = process.argv.slice(2);
if (!OUT) {
  console.error("usage: node shoot.js <app/dist> <factory.json> <user.json> <out dir>");
  process.exit(1);
}
const factory = JSON.parse(fs.readFileSync(FACTORY, "utf8"));
const user = JSON.parse(fs.readFileSync(USER, "utf8"));
const globals = JSON.parse(fs.readFileSync(path.join(__dirname, "globals.json"), "utf8"));
// Fonts are bundled into the build (@fontsource), so the page needs no network.

const server = http.createServer((req, res) => {
  const p = decodeURIComponent(req.url.split("?")[0]);
  const file = path.join(DIST, p === "/" ? "index.html" : p);
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
  await page.addInitScript(({ factory, user, globals }) => {
    const label = (i) => `${String(Math.floor(i / 5)).padStart(2, "0")}-${(i % 5) + 1}`;
    const slots = Array.from({ length: 200 }, (_, i) => ({ index: i, label: label(i), name: null }));
    for (const p of user) slots[p.slot].name = p.name;
    // Before "connect" the bank is empty, as on a real first launch. The read
    // replays slot by slot: the presets, and "empty" for never-written slots,
    // as a unit with only these presets stored answers. One slot is marked
    // edited afterwards to show the changed indicator.
    const empty = { path: null, occupied: 0, dirty: [], slots: slots.map((s) => ({ ...s, name: null })) };
    const loaded = { path: null, occupied: user.length, dirty: [7], slots };
    let bank = empty;
    // Minimal event plugin: remember listeners so the read can emit progress.
    const callbacks = new Map();
    const listeners = [];
    let nextId = 1;
    window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => {} };
    window.__TAURI_INTERNALS__ = {
      transformCallback: (cb) => { const id = nextId++; callbacks.set(id, cb); return id; },
      invoke: async (cmd, args) => {
        switch (cmd) {
          case "plugin:event|listen": listeners.push(args); return listeners.length;
          case "plugin:event|unlisten": return null;
          case "get_bank": return bank;
          case "preset_view": return user.find((x) => x.slot === args.index).view;
          case "edit_preset": {
            // Only routing is simulated, to show the signal flow redraw.
            const v = user.find((x) => x.slot === args.index).view;
            if (args.edit.kind === "routing") v.routing = args.edit.value;
            return v;
          }
          case "factory_bank": return Array.from({ length: 100 }, (_, i) => ({
            index: i, label: `${i < 50 ? "A" : "B"}${Math.floor(i / 5) % 10}-${(i % 5) + 1}`,
            name: factory.find((x) => x.slot === i)?.name ?? null }));
          case "factory_view": return { ...factory.find((x) => x.slot === args.index).view, label: "A0-" + (args.index + 1) };
          case "midi_ports": return { inputs: ["G-System"], outputs: ["G-System"] };
          case "midi_identify": return "01 00 04 02";
          case "midi_globals": return globals;
          case "midi_backup": {
            const total = args.to - args.from + 1;
            const emit = (payload) => listeners
              .filter((l) => l.event === "backup-progress")
              .forEach((l) => callbacks.get(l.handler)?.({ event: "backup-progress", id: 0, payload }));
            const received = [];
            const damaged = [];
            for (let i = args.from; i <= args.to; i++) {
              const p = user.find((x) => x.slot === i);
              if (p) received.push(i); else damaged.push(i);
              emit({ slot: i, done: i - args.from + 1, total, found: received.length, name: p ? p.name : null, damaged: !p });
              await new Promise((r) => setTimeout(r, window.__readDelay ?? 0));
            }
            bank = loaded;
            return { received, damaged };
          }
          default: return null;
        }
      },
    };
  }, { factory, user, globals });

  await page.emulateMedia({ colorScheme: "dark" });
  await page.goto(`http://localhost:${port}/`);
  await page.waitForTimeout(600);
  await page.screenshot({ path: path.join(OUT, "connect.png") });

  // Connect: the bank fills in as the unit answers. Shoot it mid-read.
  await page.evaluate(() => (window.__readDelay = 25));
  await page.click("#connect");
  await page.waitForFunction(() => document.querySelector("[data-progress]")?.textContent.includes("slot 1"));
  await page.waitForFunction(() => /slot (3[0-9]|4[0-9]) of/.test(document.querySelector("[data-progress]")?.textContent ?? ""));
  await page.screenshot({ path: path.join(OUT, "reading.png") });
  await page.waitForFunction(() => !document.querySelector(".readbar"), null, { timeout: 30000 });

  // Banks: back to the first page (the read followed onto 20–39), select 00-5 (Ch1 Ambient)
  await page.click('[data-half="0"]');
  await page.click('.cell[data-i="4"]');
  await page.waitForTimeout(400);
  await page.screenshot({ path: path.join(OUT, "banks.png") });

  await page.setViewportSize({ width: 1360, height: 1500 });
  await page.click('[data-view="preset"]');
  await page.click('g.node[data-block="delay"]');
  await page.waitForTimeout(400);
  await page.screenshot({ path: path.join(OUT, "preset.png") });
  for (const [i, name] of [[2, "semi"], [1, "parallel"], [0, null]]) {
    await page.click(`[data-routing-set="${i}"]`);
    await page.waitForTimeout(300);
    if (name) await page.screenshot({ path: path.join(OUT, `preset-${name}.png`) });
  }
  await page.emulateMedia({ colorScheme: "light" });
  await page.waitForTimeout(300);
  await page.screenshot({ path: path.join(OUT, "preset-light.png") });
  await page.setViewportSize({ width: 1360, height: 860 });
  await page.click('[data-view="banks"]');
  await page.waitForTimeout(300);
  await page.screenshot({ path: path.join(OUT, "banks-light.png") });
  await page.emulateMedia({ colorScheme: "dark" });

  await page.click('[data-view="factory"]');
  await page.waitForSelector('.cell[data-f="3"]');
  await page.click('.cell[data-f="3"]');
  await page.mouse.move(5, 5);
  await page.waitForTimeout(400);
  await page.screenshot({ path: path.join(OUT, "factory.png") });

  await page.click('[data-view="global"]');
  await page.click("#readglobals");
  await page.waitForSelector(".gcols");
  await page.waitForTimeout(300);
  await page.screenshot({ path: path.join(OUT, "global.png") });

  await browser.close();
  server.close();
})();
