// Standalone Android CDP runner: drives the phone's Chrome through the existing e2e
// gates over the Chrome DevTools Protocol. NOT part of test:e2e — it needs a device,
// an adb forward (tcp:9222 -> localabstract:chrome_devtools_remote) and a vite DEV
// server reachable from the phone (adb reverse tcp:5173 tcp:5173). The gates import
// /src/... dev modules, so a dev server (not a dist preview) is required.
//
//   node tests/android-cdp.mjs [gates] [url] [docsDir]
//     gates    comma list, default "selprune,rename,regiondelete"
//     url      default "http://localhost:5173/"  (phone localhost -> adb reverse -> host)
//     docsDir  default "/tmp/android-verify"     (keeps canonical desktop reports untouched)
//
// browser.close() only disconnects CDP; it leaves the phone's Chrome running.
import { chromium } from 'playwright';
import { selpruneTest } from './selprune.mjs';
import { renameTest } from './rename.mjs';
import { regiondeleteTest } from './regiondelete.mjs';
import { sketchstartTest } from './sketchstart.mjs';
import { sketchcutTest } from './sketchcut.mjs';

const GATES = { selprune: selpruneTest, rename: renameTest, regiondelete: regiondeleteTest, sketchstart: sketchstartTest, sketchcut: sketchcutTest };
const names = (process.argv[2] || 'selprune,rename,regiondelete').split(',').map((s) => s.trim()).filter(Boolean);
const url = process.argv[3] || 'http://localhost:5173/';
const docsDir = process.argv[4] || '/tmp/android-verify';

for (const n of names) {
  if (!GATES[n]) {
    console.error(`unknown gate: ${n}`);
    process.exit(2);
  }
}

// Hard cap so a hung CDP page cannot stall the run past the gates' own 120s waits.
// Three UI-click gates on phone CPU can be slow; 480s leaves room without masking a hang.
const watchdog = setTimeout(() => {
  console.error('ANDROID_WATCHDOG: run exceeded 480s, aborting');
  process.exit(3);
}, 480000);

// Android Chrome keeps a persistent new-tab/prerender target with no committed frame
// (url:'', title:''). connectOverCDP waits for EVERY target to initialise, so that
// frameless target stalls the connect to its 60s timeout even though raw CDP works.
// Target.closeTarget does not stick (Chrome re-creates the NTP target); navigating it to
// about:blank commits a frame and unblocks the connect. Best-effort and idempotent: a
// missing forward/device falls through to connectOverCDP's own error, and real-content
// tabs (a url or a title) are never matched, so the user's tabs are left alone.
async function commitFramelessTargets() {
  try {
    const ver = await (await fetch('http://localhost:9222/json/version')).json();
    const list = await (await fetch('http://localhost:9222/json/list')).json();
    const blanks = list.filter((t) => t.type === 'page' && (!t.url || t.url === 'about:blank') && !t.title);
    if (blanks.length === 0) return;
    const ws = new WebSocket(ver.webSocketDebuggerUrl);
    await new Promise((res, rej) => { ws.onopen = res; ws.onerror = () => rej(new Error('ws error')); });
    let id = 0;
    const pending = new Map();
    ws.onmessage = (ev) => { const m = JSON.parse(ev.data); if (m.id && pending.has(m.id)) { pending.get(m.id)(m); pending.delete(m.id); } };
    const send = (method, params, sessionId) => new Promise((res) => {
      const myId = ++id;
      const timer = setTimeout(() => { if (pending.has(myId)) { pending.delete(myId); res({}); } }, 8000);
      pending.set(myId, (m) => { clearTimeout(timer); res(m); });
      ws.send(JSON.stringify({ id: myId, method, params, ...(sessionId ? { sessionId } : {}) }));
    });
    try {
      for (const t of blanks) {
        const att = await send('Target.attachToTarget', { targetId: t.id, flatten: true });
        const sid = att.result && att.result.sessionId;
        if (!sid) continue;
        await send('Page.enable', {}, sid);
        await send('Page.navigate', { url: 'about:blank' }, sid);
        console.log(`android-cdp: committed frame on frameless target ${t.id}`);
      }
      await new Promise((r) => setTimeout(r, 1500));
    } finally {
      ws.close();
    }
  } catch (e) {
    console.error(`android-cdp: pre-connect frameless-target fix skipped (${e.message}); continuing`);
  }
}

await commitFramelessTargets();
const cdp = await chromium.connectOverCDP('http://localhost:9222/', { timeout: 60000 });
// Android Chrome rejects Browser.setWindowBounds, so force viewport:null (use the
// real window, no emulation) on every newPage the gates request. All other Browser
// methods delegate to the CDP connection unchanged.
const browser = new Proxy(cdp, {
  get(target, prop) {
    if (prop === 'newPage') return async (opts = {}) => {
      const page = await target.newPage({ ...opts, viewport: null, screen: undefined });
      // Android Chrome rejects Browser.setWindowBounds (a real window resize), but the
      // Emulation domain's device-metrics override IS supported. Give the desktop-shaped
      // gates a 1600x1000 emulated viewport so their UI clicks land, on the phone's engine.
      await page.setViewportSize({ width: 1600, height: 1000 });
      return page;
    };
    const v = target[prop];
    return typeof v === 'function' ? v.bind(target) : v;
  },
});
let allOk = true;
try {
  for (const n of names) {
    const ok = await GATES[n](browser, url, { docsDir });
    console.log(`android ${n} ${ok ? 'PASS' : 'FAIL'}`);
    if (!ok) allOk = false;
  }
} finally {
  clearTimeout(watchdog);
  await browser.close();
}
console.log(`ANDROID_EXIT=${allOk ? 0 : 1}`);
process.exit(allOk ? 0 : 1);